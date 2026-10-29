//! Main-thread macOS controls. The runner remains the state authority.
use crate::settings::ShortcutSettings;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DesktopAction {
    Capture,
    PauseResume,
    Stop,
    Review,
    Quit,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DesktopState {
    Running,
    Paused,
    Stopped,
    Error,
}

pub fn status_text(state: DesktopState, count: usize) -> String {
    let phase = match state {
        DesktopState::Running => "Running",
        DesktopState::Paused => "Paused",
        DesktopState::Stopped => "Stopped",
        DesktopState::Error => "Error: paused",
    };
    format!("Framecraft: {phase} · {count} saved")
}

#[cfg(target_os = "macos")]
mod native {
    use super::*;
    use global_hotkey::{hotkey::HotKey, GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
    use objc2::{
        runtime::{AnyClass, AnyObject, Bool, Imp, Sel},
        sel, MainThreadMarker,
    };
    use objc2_app_kit::{NSApplication, NSApplicationTerminateReply};
    use std::sync::{mpsc, Arc, Mutex};
    use tray_icon::{
        menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem},
        TrayIcon, TrayIconBuilder,
    };

    struct QuitHandler {
        sender: mpsc::SyncSender<()>,
        wake: Arc<dyn Fn() + Send + Sync>,
    }
    static QUIT_HANDLER: Mutex<Option<QuitHandler>> = Mutex::new(None);

    /// NSApplication uses this for menu, Command-Q, Dock and ordinary OS quit
    /// requests. Cancelling returns to eframe's normal event loop; the app's
    /// existing save/discard and worker-drain path closes the viewport later.
    unsafe extern "C-unwind" fn application_should_terminate(
        _delegate: *mut AnyObject,
        _selector: Sel,
        _application: *mut NSApplication,
    ) -> NSApplicationTerminateReply {
        // Do not retain the mutex while waking the event loop, and coalesce
        // repeated quit requests while the UI is showing its decision dialog.
        let wake = QUIT_HANDLER.lock().ok().and_then(|handler| {
            handler.as_ref().map(|handler| {
                let _ = handler.sender.try_send(());
                handler.wake.clone()
            })
        });
        if let Some(wake) = wake {
            wake();
        }
        // No handler also fails closed. It must never restore an unsafe native
        // termination path during teardown or a failed control initialisation.
        NSApplicationTerminateReply::TerminateCancel
    }

    fn quit_implementation() -> Imp {
        // SAFETY: IMP erases the signature. The registered encoding below and
        // callback use NSApplicationTerminateReply (NSUInteger), self, SEL, app.
        unsafe {
            std::mem::transmute::<
                unsafe extern "C-unwind" fn(
                    *mut AnyObject,
                    Sel,
                    *mut NSApplication,
                ) -> NSApplicationTerminateReply,
                Imp,
            >(application_should_terminate)
        }
    }

    fn install_quit_handler(
        main_thread: MainThreadMarker,
        wake: Arc<dyn Fn() + Send + Sync>,
    ) -> Result<mpsc::Receiver<()>, String> {
        let application = NSApplication::sharedApplication(main_thread);
        let delegate = application
            .delegate()
            .ok_or("Cannot guard native Quit: application delegate is unavailable.")?;
        let delegate_object: &AnyObject = (*delegate).as_ref();
        let class = delegate_object.class();
        // The locked winit 0.30 delegate has no applicationShouldTerminate:.
        // Adding one optional method preserves its identity, ivars and all
        // original methods. Never mutate a different framework's delegate.
        if class.name() != c"WinitApplicationDelegate" {
            return Err(format!("Cannot guard native Quit for application delegate {}. Close Framecraft before editing or starting capture.", class.name().to_string_lossy()));
        }
        let selector = sel!(applicationShouldTerminate:);
        let implementation = quit_implementation();
        if let Some(existing) = class.instance_method(selector) {
            if !std::ptr::fn_addr_eq(existing.implementation(), implementation) {
                return Err("Cannot guard native Quit: an existing termination delegate must be reviewed. No native method was replaced.".into());
            }
        } else {
            #[link(name = "objc")]
            unsafe extern "C" {
                fn class_addMethod(
                    class: *const AnyClass,
                    selector: Sel,
                    implementation: Imp,
                    types: *const std::ffi::c_char,
                ) -> Bool;
            }
            // SAFETY: macOS 13 supports only 64-bit architectures. Q is the
            // NSUInteger result, followed by object self, SEL and app object.
            // class_addMethod is atomic and cannot overwrite an existing method.
            let added =
                unsafe { class_addMethod(class, selector, implementation, c"Q@:@".as_ptr()) };
            if !added.as_bool() {
                return Err("Cannot install the native Quit guard. Close Framecraft before editing or starting capture.".into());
            }
        }
        let (sender, receiver) = mpsc::sync_channel(1);
        *QUIT_HANDLER
            .lock()
            .map_err(|_| "Native Quit guard lock was interrupted.")? =
            Some(QuitHandler { sender, wake });
        // Refresh AppKit's optional delegate-method cache without replacing
        // the delegate object or class that winit expects.
        application.setDelegate(Some(&delegate));
        Ok(receiver)
    }

    pub struct DesktopControls {
        tray: TrayIcon,
        status: MenuItem,
        source: MenuItem,
        capture: MenuItem,
        pause: MenuItem,
        stop: MenuItem,
        review: MenuItem,
        manager: GlobalHotKeyManager,
        keys: Option<[HotKey; 3]>,
        menus: mpsc::Receiver<tray_icon::menu::MenuEvent>,
        hotkeys: mpsc::Receiver<GlobalHotKeyEvent>,
        visibility: mpsc::Receiver<(bool, mpsc::SyncSender<Result<(), String>>)>,
        quits: mpsc::Receiver<()>,
        last_status: String,
        last_source: String,
    }

    impl DesktopControls {
        /// Call from the first eframe update, once the native event loop runs.
        pub fn new(wake: impl Fn() + Send + Sync + 'static) -> Result<Self, String> {
            let main_thread = MainThreadMarker::new()
                .ok_or("Desktop controls must be created on the main thread.")?;
            let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(wake);
            // Install before fallible menu/shortcut creation. If those fail,
            // the registered callback still cancels native termination; normal
            // window Close remains available through the app's save guard.
            let quits = install_quit_handler(main_thread, wake.clone())?;
            let menu = Menu::new();
            let status = MenuItem::new(status_text(DesktopState::Stopped, 0), false, None);
            let source = MenuItem::new("Source: none selected", false, None);
            let capture = MenuItem::new("Capture now", false, None);
            let pause = MenuItem::new("Pause", false, None);
            let stop = MenuItem::new("Stop", false, None);
            let review = MenuItem::new("Open review", true, None);
            menu.append_items(&[
                &status,
                &source,
                &PredefinedMenuItem::separator(),
                &capture,
                &pause,
                &stop,
                &PredefinedMenuItem::separator(),
                &review,
            ])
            .map_err(|e| e.to_string())?;
            let tray = TrayIconBuilder::new()
                .with_title("FC: Stopped")
                .with_tooltip("Framecraft: Stopped")
                .with_menu(Box::new(menu))
                .build()
                .map_err(|e| e.to_string())?;
            let manager = GlobalHotKeyManager::new().map_err(|e| e.to_string())?;
            let menu_wake = wake.clone();
            let (menu_tx, menus) = mpsc::channel();
            MenuEvent::set_event_handler(Some(move |event| {
                let _ = menu_tx.send(event);
                menu_wake();
            }));
            let (key_tx, hotkeys) = mpsc::channel();
            let key_wake = wake.clone();
            GlobalHotKeyEvent::set_event_handler(Some(move |event| {
                let _ = key_tx.send(event);
                key_wake();
            }));
            let (visibility_tx, visibility) = mpsc::channel();
            crate::capture::set_visibility_handler(Some(Arc::new(move |hidden| {
                let (ack, ready) = mpsc::sync_channel(1);
                visibility_tx
                    .send((hidden, ack))
                    .map_err(|_| "Desktop controls have closed.".to_string())?;
                wake();
                ready
                    .recv_timeout(std::time::Duration::from_secs(2))
                    .map_err(|_| {
                        "Could not hide capture controls in time. Retry the capture.".to_string()
                    })?
            })));
            Ok(Self {
                tray,
                status,
                source,
                capture,
                pause,
                stop,
                review,
                manager,
                keys: None,
                menus,
                hotkeys,
                visibility,
                quits,
                last_status: String::new(),
                last_source: String::new(),
            })
        }

        pub fn poll(&mut self) -> Vec<DesktopAction> {
            let mut actions = Vec::new();
            if self.quits.try_recv().is_ok() {
                actions.push(DesktopAction::Quit);
            }
            for (hidden, ack) in self.visibility.try_iter() {
                let result = self
                    .tray
                    .set_visible(!hidden)
                    .map_err(|error| error.to_string());
                let _ = ack.send(result);
            }
            for event in self.menus.try_iter() {
                if event.id == *self.capture.id() {
                    actions.push(DesktopAction::Capture);
                } else if event.id == *self.pause.id() {
                    actions.push(DesktopAction::PauseResume);
                } else if event.id == *self.stop.id() {
                    actions.push(DesktopAction::Stop);
                } else if event.id == *self.review.id() {
                    actions.push(DesktopAction::Review);
                }
            }
            for event in self.hotkeys.try_iter() {
                if event.state != HotKeyState::Pressed {
                    continue;
                }
                if let Some(keys) = self.keys {
                    if let Some(index) = keys.iter().position(|key| key.id() == event.id) {
                        actions.push(
                            [
                                DesktopAction::Capture,
                                DesktopAction::PauseResume,
                                DesktopAction::Stop,
                            ][index],
                        );
                    }
                }
            }
            actions
        }

        pub fn update(&mut self, state: DesktopState, count: usize, source: &str) {
            let text = status_text(state, count);
            if text != self.last_status {
                self.status.set_text(&text);
                let phase = match state {
                    DesktopState::Running => "Running",
                    DesktopState::Paused => "Paused",
                    DesktopState::Stopped => "Stopped",
                    DesktopState::Error => "Error",
                };
                self.tray.set_title(Some(format!("FC: {phase} {count}")));
                let _ = self.tray.set_tooltip(Some(&text));
                self.pause.set_text(if state == DesktopState::Running {
                    "Pause"
                } else {
                    "Resume"
                });
                self.capture.set_enabled(state == DesktopState::Running);
                self.pause.set_enabled(state != DesktopState::Stopped);
                self.stop.set_enabled(state != DesktopState::Stopped);
                self.last_status = text;
            }
            if source != self.last_source {
                // Bounded text contains only the user-selected source summary.
                self.source.set_text(format!(
                    "Source: {}",
                    source.chars().take(120).collect::<String>()
                ));
                self.last_source = source.into();
            }
        }

        /// A failed replacement rolls back newly registered keys. Retained old
        /// registrations stay intact; removed keys are released after success.
        pub fn configure_shortcuts(
            &mut self,
            settings: &ShortcutSettings,
            stopped: bool,
        ) -> Result<(), String> {
            if !stopped {
                return Err("Stop the session before changing shortcuts.".into());
            }
            let names = settings.canonical_keys()?;
            let parsed = names
                .iter()
                .map(|name| name.parse::<HotKey>().map_err(|e| e.to_string()))
                .collect::<Result<Vec<_>, _>>()?;
            let new: [HotKey; 3] = parsed.try_into().map_err(|_| "Expected three shortcuts.")?;
            let previous = self.keys;
            replace_shortcuts(
                previous.as_ref().map_or(&[], |keys| keys.as_slice()),
                &new,
                |key| self.manager.register(*key).map_err(|e| e.to_string()),
                |key| self.manager.unregister(*key).map_err(|e| e.to_string()),
            )?;
            self.keys = Some(new);
            Ok(())
        }

        pub fn clear_shortcuts(&mut self) {
            if let Some(keys) = self.keys.take() {
                let _ = self.manager.unregister_all(&keys);
            }
            while self.hotkeys.try_recv().is_ok() {}
        }
    }
    impl Drop for DesktopControls {
        fn drop(&mut self) {
            if let Ok(mut handler) = QUIT_HANDLER.lock() {
                *handler = None;
            }
            crate::capture::set_visibility_handler(None);
            self.clear_shortcuts();
            MenuEvent::set_event_handler(None::<fn(MenuEvent)>);
            GlobalHotKeyEvent::set_event_handler(None::<fn(GlobalHotKeyEvent)>);
        }
    }

    #[cfg(test)]
    mod quit_tests {
        use super::*;
        #[test]
        fn native_termination_callback_cancels_and_coalesces_requests() {
            let (sender, receiver) = mpsc::sync_channel(1);
            *QUIT_HANDLER.lock().unwrap() = Some(QuitHandler {
                sender,
                wake: Arc::new(|| {}),
            });
            for _ in 0..3 {
                // The callback never dereferences these arguments. Native
                // registration/invocation is covered by the smoke executable.
                let reply = unsafe {
                    application_should_terminate(
                        std::ptr::null_mut(),
                        sel!(applicationShouldTerminate:),
                        std::ptr::null_mut(),
                    )
                };
                assert_eq!(reply, NSApplicationTerminateReply::TerminateCancel);
            }
            assert!(receiver.try_recv().is_ok());
            assert!(receiver.try_recv().is_err());
            *QUIT_HANDLER.lock().unwrap() = None;
            assert_eq!(
                unsafe {
                    application_should_terminate(
                        std::ptr::null_mut(),
                        sel!(applicationShouldTerminate:),
                        std::ptr::null_mut(),
                    )
                },
                NSApplicationTerminateReply::TerminateCancel
            );
        }
    }
}

/// Register additions before releasing obsolete bindings. If an OS operation
/// fails, surface it explicitly and attempt rollback instead of saving new prefs.
fn replace_shortcuts<T: Copy + PartialEq>(
    previous: &[T],
    new: &[T],
    mut register: impl FnMut(&T) -> Result<(), String>,
    mut unregister: impl FnMut(&T) -> Result<(), String>,
) -> Result<(), String> {
    let mut added = Vec::new();
    for key in new.iter().filter(|key| !previous.contains(key)) {
        if let Err(error) = register(key) {
            let mut rollback_failed = false;
            for key in &added {
                rollback_failed |= unregister(key).is_err();
            }
            return Err(format!(
                "Shortcut is unavailable: {error}. {}",
                if rollback_failed {
                    "Rollback failed; quit and reopen Framecraft before retrying."
                } else {
                    "Previous shortcuts are unchanged. Choose another combination or use the menu controls."
                }
            ));
        }
        added.push(*key);
    }
    let mut removed = Vec::new();
    for key in previous.iter().filter(|key| !new.contains(key)) {
        if let Err(error) = unregister(key) {
            let mut rollback_failed = false;
            for key in &removed {
                rollback_failed |= register(key).is_err();
            }
            for key in &added {
                rollback_failed |= unregister(key).is_err();
            }
            return Err(format!(
                "Could not release the old shortcut: {error}. {}",
                if rollback_failed {
                    "Quit and reopen Framecraft before retrying."
                } else {
                    "Previous shortcuts were restored."
                }
            ));
        }
        removed.push(*key);
    }
    Ok(())
}

#[cfg(target_os = "macos")]
pub use native::DesktopControls;

#[cfg(not(target_os = "macos"))]
pub struct DesktopControls;
#[cfg(not(target_os = "macos"))]
impl DesktopControls {
    pub fn new(_: impl Fn() + Send + Sync + 'static) -> Result<Self, String> {
        Err("Menu-bar controls require macOS.".into())
    }
    pub fn poll(&mut self) -> Vec<DesktopAction> {
        Vec::new()
    }
    pub fn update(&mut self, _: DesktopState, _: usize, _: &str) {}
    pub fn configure_shortcuts(&mut self, _: &ShortcutSettings, _: bool) -> Result<(), String> {
        Err("Global shortcuts require macOS.".into())
    }
    pub fn clear_shortcuts(&mut self) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    #[test]
    fn native_state_labels_always_include_phase_and_acknowledged_count() {
        for (state, label) in [
            (DesktopState::Running, "Running"),
            (DesktopState::Paused, "Paused"),
            (DesktopState::Stopped, "Stopped"),
            (DesktopState::Error, "Error: paused"),
        ] {
            assert_eq!(
                status_text(state, 7),
                format!("Framecraft: {label} · 7 saved")
            );
        }
    }
    #[test]
    fn failed_registration_restores_previous_keys_and_releases_partial_new_keys() {
        let registered = RefCell::new(vec![1, 2, 3]);
        let result = replace_shortcuts(
            &[1, 2, 3],
            &[1, 4, 5],
            |key| {
                if *key == 5 {
                    Err("conflict".into())
                } else {
                    registered.borrow_mut().push(*key);
                    Ok(())
                }
            },
            |key| {
                registered.borrow_mut().retain(|v| v != key);
                Ok(())
            },
        );
        assert!(result
            .unwrap_err()
            .contains("Previous shortcuts are unchanged"));
        assert_eq!(*registered.borrow(), [1, 2, 3]);
    }
    #[test]
    fn successful_registration_releases_obsolete_keys() {
        let registered = RefCell::new(vec![1, 2, 3]);
        replace_shortcuts(
            &[1, 2, 3],
            &[1, 4, 5],
            |key| {
                registered.borrow_mut().push(*key);
                Ok(())
            },
            |key| {
                registered.borrow_mut().retain(|v| v != key);
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(*registered.borrow(), [1, 4, 5]);
    }
}

#[cfg(target_os = "macos")]
mod dragging {
    use objc2::{
        define_class, msg_send, rc::Retained, runtime::ProtocolObject, AllocAnyThread,
        MainThreadMarker, MainThreadOnly,
    };
    use objc2_app_kit::{
        NSApplication, NSDragOperation, NSDraggingContext, NSDraggingItem, NSDraggingSession,
        NSDraggingSource, NSEventType, NSImage, NSView,
    };
    use objc2_foundation::{NSArray, NSObject, NSObjectProtocol, NSRect, NSSize, NSString, NSURL};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use std::{cell::RefCell, path::Path};

    define_class!(
        // NSObject has no extra subclass invariants. AppKit calls this object
        // only on the main thread; dragging advertises copy and never move.
        #[unsafe(super = NSObject)]
        #[thread_kind = MainThreadOnly]
        struct FileDragSource;
        unsafe impl NSObjectProtocol for FileDragSource {}
        unsafe impl NSDraggingSource for FileDragSource {
            #[unsafe(method(draggingSession:sourceOperationMaskForDraggingContext:))]
            fn operation(
                &self,
                _session: &NSDraggingSession,
                _context: NSDraggingContext,
            ) -> NSDragOperation {
                NSDragOperation::Copy
            }
        }
    );
    thread_local! {
        // Keep the native source alive through asynchronous drag completion.
        static SOURCE: RefCell<Option<Retained<FileDragSource>>> = const { RefCell::new(None) };
    }

    pub fn start_file_drag(frame: &eframe::Frame, path: &Path) -> Result<(), String> {
        let mtm = MainThreadMarker::new().ok_or("File dragging requires the main thread.")?;
        let app = NSApplication::sharedApplication(mtm);
        let event = app
            .currentEvent()
            .ok_or("Start dragging with the mouse or use Copy image.")?;
        if !matches!(
            event.r#type(),
            NSEventType::LeftMouseDown | NSEventType::LeftMouseDragged
        ) {
            return Err("Start dragging with the mouse or use Copy image.".into());
        }
        let RawWindowHandle::AppKit(handle) = frame
            .window_handle()
            .map_err(|error| error.to_string())?
            .as_raw()
        else {
            return Err("The current window does not support native file dragging.".into());
        };
        let path = std::fs::canonicalize(path)
            .map_err(|error| format!("The prepared PNG is unavailable: {error}"))?;
        if !path.is_file() {
            return Err("The prepared PNG is unavailable.".into());
        }
        let native_path = NSString::from_str(
            path.to_str()
                .ok_or("The prepared image path is not valid Unicode.")?,
        );
        let url = NSURL::fileURLWithPath_isDirectory(&native_path, false);
        let item = NSDraggingItem::initWithPasteboardWriter(
            NSDraggingItem::alloc(),
            ProtocolObject::from_ref(&*url),
        );
        let preview = NSImage::initWithContentsOfFile(NSImage::alloc(), &native_path)
            .ok_or("Could not open the prepared PNG for dragging.")?;
        // SAFETY: eframe's borrowed AppKit window handle guarantees a live
        // NSView for this frame, and the main-thread marker is checked above.
        let view = unsafe { handle.ns_view.cast::<NSView>().as_ref() };
        let position = view.convertPoint_fromView(event.locationInWindow(), None);
        let image_size = preview.size();
        let scale = (160.0 / image_size.width)
            .min(100.0 / image_size.height)
            .min(1.0);
        // SAFETY: AppKit accepts an NSImage as dragging-frame contents.
        unsafe {
            item.setDraggingFrame_contents(
                NSRect::new(
                    position,
                    NSSize::new(image_size.width * scale, image_size.height * scale),
                ),
                Some(&preview),
            );
        }
        SOURCE.with(|source| {
            let mut source = source.borrow_mut();
            let source = source.get_or_insert_with(|| {
                // SAFETY: NSObject init is the inherited designated initialiser.
                unsafe { msg_send![FileDragSource::alloc(mtm), init] }
            });
            view.beginDraggingSessionWithItems_event_source(
                &NSArray::arrayWithObject(&*item),
                &event,
                ProtocolObject::from_ref(&**source),
            );
        });
        Ok(())
    }
}
#[cfg(target_os = "macos")]
pub use dragging::start_file_drag;
#[cfg(not(target_os = "macos"))]
pub fn start_file_drag(_: &eframe::Frame, _: &std::path::Path) -> Result<(), String> {
    Err("Native file dragging requires macOS. Use Copy image or Export PNG.".into())
}
