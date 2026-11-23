//! Main-thread macOS controls. The runner remains the state authority.
use crate::settings::ShortcutSettings;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DesktopAction {
    CaptureArea,
    CaptureWindow,
    CaptureScreen,
    CancelCapture,
    EditLastCapture,
    RetryLastOutput,
    ClearLastCapture,
    CloseScreenshot,
    Settings,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LastCaptureStatus {
    Copied,
    Saved,
    CopyFailed,
    SaveFailed,
}

impl LastCaptureStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Copied => "Copied on this Mac",
            Self::Saved => "Saved to file",
            Self::CopyFailed => "Copy failed",
            Self::SaveFailed => "Save failed",
        }
    }

    pub fn status_item_title(self) -> Option<&'static str> {
        match self {
            Self::CopyFailed | Self::SaveFailed => Some(self.label()),
            Self::Copied | Self::Saved => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum QuickShortcutState {
    #[default]
    Off,
    Active,
    Unavailable,
}
impl QuickShortcutState {
    pub fn description(self, binding: &str) -> String {
        match self {
            Self::Active => format!("{binding} is active while Framecraft is running."),
            Self::Off => "Everyday shortcut is off. Use Capture Area in the menu bar.".into(),
            Self::Unavailable => format!("Could not register {binding}. Use Capture Area or choose another shortcut in Capture setup."),
        }
    }
    #[cfg(target_os = "macos")]
    fn menu_title(self, binding: &str) -> String {
        match self {
            Self::Off => "Capture Area…  (shortcut off)".into(),
            Self::Active => format!("Capture Area…  {binding}"),
            Self::Unavailable => format!("Capture Area…  ({binding} unavailable)"),
        }
    }
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
mod exclusive_shortcut;

#[cfg(target_os = "macos")]
mod native {
    use super::exclusive_shortcut::ExclusiveShortcuts;
    use super::*;
    use global_hotkey::{hotkey::HotKey, GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
    use objc2::{
        runtime::{AnyClass, AnyObject, Bool, Imp, Sel},
        sel, MainThreadMarker,
    };
    use objc2_app_kit::{NSAccessibility, NSApplication, NSApplicationTerminateReply};
    use objc2_foundation::NSString;
    use std::sync::{mpsc, Arc, Mutex};
    use tray_icon::{
        menu::{
            accelerator::{Accelerator, Code, Modifiers},
            Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu,
        },
        Icon, TrayIcon, TrayIconBuilder,
    };

    fn capture_icon() -> Result<Icon, String> {
        // A monochrome framing reticle. AppKit treats it as a template and
        // supplies the correct light/dark/high-contrast menu-bar appearance.
        let size = 36_usize;
        let mut rgba = vec![0; size * size * 4];
        for y in 0..size {
            for x in 0..size {
                let corner = ((5..9).contains(&x) || (27..31).contains(&x))
                    && ((5..15).contains(&y) || (21..31).contains(&y))
                    || ((5..9).contains(&y) || (27..31).contains(&y))
                        && ((5..15).contains(&x) || (21..31).contains(&x));
                let centre = (16..20).contains(&x) && (13..23).contains(&y)
                    || (16..20).contains(&y) && (13..23).contains(&x);
                if corner || centre {
                    rgba[(y * size + x) * 4 + 3] = 255;
                }
            }
        }
        Icon::from_rgba(rgba, size as u32, size as u32).map_err(|e| e.to_string())
    }

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
        application_menu: Menu,
        capture_area: MenuItem,
        app_capture_area: MenuItem,
        capture_window: MenuItem,
        capture_screen: MenuItem,
        cancel_capture: MenuItem,
        edit_last: MenuItem,
        retry_last: MenuItem,
        clear_last: MenuItem,
        last_receipt: MenuItem,
        capture_status: Option<LastCaptureStatus>,
        close_screenshot: MenuItem,
        settings: MenuItem,
        quit: MenuItem,
        status: MenuItem,
        source: MenuItem,
        capture: MenuItem,
        pause: MenuItem,
        stop: MenuItem,
        review: MenuItem,
        manager: Result<GlobalHotKeyManager, String>,
        quick_bindings: Result<ExclusiveShortcuts, String>,
        keys: Option<[HotKey; 3]>,
        quick_key: Option<HotKey>,
        suppressed_quick_press: bool,
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
            let capture_keys = Modifiers::META | Modifiers::ALT;
            let capture_area = MenuItem::new(QuickShortcutState::Off.menu_title(""), true, None);
            let app_capture_area = MenuItem::new(
                "Capture Area…",
                true,
                Some(Accelerator::new(capture_keys, Code::Digit4)),
            );
            let capture_window = MenuItem::new(
                "Capture Window…",
                true,
                Some(Accelerator::new(capture_keys, Code::Digit5)),
            );
            let capture_screen = MenuItem::new(
                "Capture Screen",
                true,
                Some(Accelerator::new(capture_keys, Code::Digit3)),
            );
            let cancel_capture = MenuItem::new("Cancel Capture", false, None);
            let edit_last = MenuItem::new("Edit Last Capture…", false, None);
            let retry_last = MenuItem::new("Retry Last Output", false, None);
            let clear_last = MenuItem::new("Clear Last Capture", false, None);
            let last_receipt = MenuItem::new("No capture in memory", false, None);
            let last_options = Submenu::with_items(
                "Last Capture Options",
                true,
                &[&last_receipt, &retry_last, &clear_last],
            )
            .map_err(|e| e.to_string())?;
            let close_screenshot = MenuItem::new("Close Screenshot", false, None);
            let settings = MenuItem::new(
                "Capture setup…",
                true,
                Some(Accelerator::new(Modifiers::META, Code::Comma)),
            );
            let quit = MenuItem::new(
                "Quit Framecraft",
                true,
                Some(Accelerator::new(Modifiers::META, Code::KeyQ)),
            );
            let status = MenuItem::new(status_text(DesktopState::Stopped, 0), false, None);
            let source = MenuItem::new("Source: none selected", false, None);
            let capture = MenuItem::new("Capture Session Frame", false, None);
            let pause = MenuItem::new("Pause Session", false, None);
            let stop = MenuItem::new("Stop Session", false, None);
            let review = MenuItem::new("Open Framecraft", true, None);
            menu.append_items(&[
                &capture_area,
                &capture_window,
                &capture_screen,
                &cancel_capture,
                &edit_last,
                &last_options,
                &PredefinedMenuItem::separator(),
                &review,
                &settings,
                &PredefinedMenuItem::separator(),
                &status,
                &source,
                &PredefinedMenuItem::separator(),
                &capture,
                &pause,
                &stop,
                &PredefinedMenuItem::separator(),
                &quit,
            ])
            .map_err(|e| e.to_string())?;
            let tray = TrayIconBuilder::new()
                .with_icon(capture_icon()?)
                .with_icon_as_template(true)
                .with_tooltip("Framecraft: Stopped")
                .with_menu(Box::new(menu))
                .build()
                .map_err(|e| e.to_string())?;
            if let Some(button) = tray
                .ns_status_item()
                .and_then(|item| item.button(main_thread))
            {
                button.setAccessibilityLabel(Some(&NSString::from_str("Framecraft")));
                button.setAccessibilityHelp(Some(&NSString::from_str(
                    "Take a screenshot or control a capture session.",
                )));
            }
            let manager = GlobalHotKeyManager::new().map_err(|e| e.to_string());
            // Normal application menus make Framecraft visible by name when
            // focused. Menu events use the same guarded application actions as
            // the status item, including Quit; no native termination bypass.
            let app = Submenu::with_items(
                "Framecraft",
                true,
                &[
                    &settings,
                    &PredefinedMenuItem::separator(),
                    &PredefinedMenuItem::hide(Some("Hide Framecraft")),
                    &PredefinedMenuItem::hide_others(None),
                    &PredefinedMenuItem::show_all(None),
                    &PredefinedMenuItem::separator(),
                    &quit,
                ],
            )
            .map_err(|e| e.to_string())?;
            let captures = Submenu::with_items(
                "Capture",
                true,
                &[
                    &app_capture_area,
                    &capture_window,
                    &capture_screen,
                    &cancel_capture,
                    &edit_last,
                    &last_options,
                    &PredefinedMenuItem::separator(),
                    &capture,
                    &pause,
                    &stop,
                ],
            )
            .map_err(|e| e.to_string())?;
            let windows = Submenu::with_items(
                "Window",
                true,
                &[
                    &review,
                    &close_screenshot,
                    &PredefinedMenuItem::minimize(None),
                    &PredefinedMenuItem::zoom(None),
                ],
            )
            .map_err(|e| e.to_string())?;
            let application_menu =
                Menu::with_items(&[&app, &captures, &windows]).map_err(|e| e.to_string())?;
            let menu_wake = wake.clone();
            let (menu_tx, menus) = mpsc::channel();
            MenuEvent::set_event_handler(Some(move |event| {
                let _ = menu_tx.send(event);
                menu_wake();
            }));
            let (key_tx, hotkeys) = mpsc::channel();
            let quick_tx = key_tx.clone();
            let quick_wake = wake.clone();
            let quick_bindings = ExclusiveShortcuts::new(move |id| {
                let _ = quick_tx.send(GlobalHotKeyEvent {
                    id,
                    state: HotKeyState::Pressed,
                });
                quick_wake();
            });
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
            application_menu.init_for_nsapp();
            windows.set_as_windows_menu_for_nsapp();
            Ok(Self {
                tray,
                application_menu,
                capture_area,
                app_capture_area,
                capture_window,
                capture_screen,
                cancel_capture,
                edit_last,
                retry_last,
                clear_last,
                last_receipt,
                capture_status: None,
                close_screenshot,
                settings,
                quit,
                status,
                source,
                capture,
                pause,
                stop,
                review,
                manager,
                quick_bindings,
                keys: None,
                quick_key: None,
                suppressed_quick_press: false,
                menus,
                hotkeys,
                visibility,
                quits,
                last_status: String::new(),
                last_source: String::new(),
            })
        }

        pub fn poll(&mut self) -> Vec<DesktopAction> {
            self.poll_with_capture_hide(|| {})
        }
        pub fn poll_with_capture_hide(
            &mut self,
            mut hide_thumbnail: impl FnMut(),
        ) -> Vec<DesktopAction> {
            let mut actions = Vec::new();
            self.suppressed_quick_press = false;
            if self.quits.try_recv().is_ok() {
                actions.push(DesktopAction::Quit);
            }
            for (hidden, ack) in self.visibility.try_iter() {
                if hidden {
                    hide_thumbnail();
                }
                let result = self
                    .tray
                    .set_visible(!hidden)
                    .map_err(|error| error.to_string());
                if !hidden && result.is_ok() {
                    // AppKit creates a new status button when the capture
                    // controls return. Restore its accessibility name too.
                    self.update_status_button_accessibility();
                }
                let _ = ack.send(result);
            }
            for event in self.menus.try_iter() {
                if (event.id == *self.capture_area.id() || event.id == *self.app_capture_area.id())
                    && self.capture_area.is_enabled()
                {
                    actions.push(DesktopAction::CaptureArea);
                } else if event.id == *self.capture_window.id() && self.capture_window.is_enabled()
                {
                    actions.push(DesktopAction::CaptureWindow);
                } else if event.id == *self.capture_screen.id() && self.capture_screen.is_enabled()
                {
                    actions.push(DesktopAction::CaptureScreen);
                } else if event.id == *self.close_screenshot.id()
                    && self.close_screenshot.is_enabled()
                {
                    actions.push(DesktopAction::CloseScreenshot);
                } else if event.id == *self.edit_last.id() && self.edit_last.is_enabled() {
                    actions.push(DesktopAction::EditLastCapture);
                } else if event.id == *self.retry_last.id() && self.retry_last.is_enabled() {
                    actions.push(DesktopAction::RetryLastOutput);
                } else if event.id == *self.clear_last.id() && self.clear_last.is_enabled() {
                    actions.push(DesktopAction::ClearLastCapture);
                } else if event.id == *self.cancel_capture.id() && self.cancel_capture.is_enabled()
                {
                    actions.push(DesktopAction::CancelCapture);
                } else if event.id == *self.settings.id() {
                    actions.push(DesktopAction::Settings);
                } else if event.id == *self.quit.id() {
                    actions.push(DesktopAction::Quit);
                } else if event.id == *self.capture.id() {
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
                if self.quick_key.is_some_and(|key| key.id() == event.id) {
                    if self.capture_area.is_enabled() {
                        actions.push(DesktopAction::CaptureArea);
                    } else {
                        self.suppressed_quick_press = true;
                    }
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

        pub fn suppressed_quick_press(&self) -> bool {
            self.suppressed_quick_press
        }

        pub fn update(&mut self, state: DesktopState, count: usize, source: &str) {
            let text = status_text(state, count);
            if text != self.last_status {
                self.status.set_text(&text);
                let _ = self.tray.set_tooltip(Some(&text));
                self.pause.set_text(if state == DesktopState::Running {
                    "Pause Session"
                } else {
                    "Resume Session"
                });
                self.capture.set_enabled(state == DesktopState::Running);
                self.pause.set_enabled(state != DesktopState::Stopped);
                self.stop.set_enabled(state != DesktopState::Stopped);
                self.last_status = text;
                self.update_status_button_accessibility();
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

        /// Disable one-off capture during another capture, a modal decision or
        /// any state in which the application cannot safely replace its image.
        pub fn set_close_available(&mut self, available: bool) {
            self.close_screenshot.set_enabled(available);
        }
        fn update_status_button_accessibility(&self) {
            let Some(button) = MainThreadMarker::new()
                .and_then(|mtm| self.tray.ns_status_item().and_then(|item| item.button(mtm)))
            else {
                return;
            };
            let (label, help) = match self.capture_status.and_then(LastCaptureStatus::status_item_title) {
                Some(failure) => (format!("Framecraft. {failure}."), "Open Last Capture Options to retry, or choose Edit Last Capture to recover the image."),
                None => ("Framecraft".into(), "Open the menu to take a screenshot or control the session."),
            };
            button.setAccessibilityLabel(Some(&NSString::from_str(&label)));
            button.setAccessibilityHelp(Some(&NSString::from_str(&format!(
                "{}. {help}",
                self.last_status
            ))));
        }
        pub fn set_last_capture(&mut self, receipt: Option<LastCaptureStatus>, available: bool) {
            self.edit_last.set_enabled(available && receipt.is_some());
            self.clear_last.set_enabled(available && receipt.is_some());
            let failure = receipt.and_then(LastCaptureStatus::status_item_title);
            self.retry_last.set_enabled(available && failure.is_some());
            if self.capture_status != receipt {
                self.capture_status = receipt;
                self.last_receipt.set_text(
                    receipt
                        .map(LastCaptureStatus::label)
                        .unwrap_or("No capture in memory"),
                );
                self.tray.set_title(failure);
                self.update_status_button_accessibility();
            }
        }
        pub fn set_capture_available(&mut self, available: bool) {
            self.capture_area.set_enabled(available);
            self.app_capture_area.set_enabled(available);
            self.capture_window.set_enabled(available);
            self.capture_screen.set_enabled(available);
        }

        /// Only the active one-off capture owns this cancellation route. The
        /// session Stop action and its state remain separate.
        pub fn set_capture_cancellable(&mut self, cancellable: bool) {
            self.cancel_capture.set_enabled(cancellable);
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
                .map(|name| {
                    name.parse::<HotKey>().map_err(|_| {
                        "This shortcut is not supported. Choose another key combination."
                            .to_string()
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;
            let new: [HotKey; 3] = parsed.try_into().map_err(|_| "Expected three shortcuts.")?;
            if self.quick_key.is_some_and(|key| new.contains(&key)) {
                return Err("A session shortcut conflicts with your quick capture shortcut. Change it in Capture Setup first.".into());
            }
            let previous = self.keys;
            let manager = self.manager.as_ref().map_err(Clone::clone)?;
            replace_shortcuts(
                previous.as_ref().map_or(&[], |keys| keys.as_slice()),
                &new,
                |key| {
                    manager
                        .register(*key)
                        .map_err(|_| "macOS could not register this key combination".to_string())
                },
                |key| {
                    manager
                        .unregister(*key)
                        .map_err(|_| "macOS could not release this key combination".to_string())
                },
            )?;
            self.keys = Some(new);
            Ok(())
        }

        /// One-off capture stays registered independently of a running session.
        /// Register the replacement first so a conflict preserves the old key.
        pub fn configure_quick_capture(&mut self, shortcut: Option<&str>) -> Result<(), String> {
            self.commit_quick_capture(shortcut, || Ok(())).map(|_| ())
        }
        pub fn set_quick_shortcut_status(&self, status: QuickShortcutState, binding: &str) {
            self.capture_area.set_text(status.menu_title(binding));
        }
        pub fn commit_quick_capture(
            &mut self,
            shortcut: Option<&str>,
            persist: impl FnOnce() -> Result<(), String>,
        ) -> Result<Option<String>, String> {
            let new = shortcut
                .map(|value| {
                    crate::quick_capture::canonical_quick_shortcut(value)?
                        .parse::<HotKey>()
                        .map_err(|_| {
                            "This capture shortcut is not supported. Choose another combination."
                                .to_string()
                        })
                })
                .transpose()?;
            if new.is_some_and(|key| self.keys.is_some_and(|keys| keys.contains(&key))) {
                return Err("The capture shortcut is already used by a session command. Choose another combination.".into());
            }
            let warning = commit_quick_binding(
                self.quick_key,
                new,
                |key| {
                    self.quick_bindings
                        .as_ref()
                        .map_err(Clone::clone)?
                        .register(key)
                },
                |key| {
                    self.quick_bindings
                        .as_ref()
                        .map_err(Clone::clone)?
                        .release(key)
                },
                persist,
            )?;
            self.quick_key = new;
            Ok(warning)
        }

        pub fn clear_shortcuts(&mut self) {
            if let (Ok(manager), Some(keys)) = (&self.manager, self.keys.take()) {
                let _ = manager.unregister_all(&keys);
            }
            while self.hotkeys.try_recv().is_ok() {}
        }
    }
    impl Drop for DesktopControls {
        fn drop(&mut self) {
            self.application_menu.remove_for_nsapp();
            if let Ok(mut handler) = QUIT_HANDLER.lock() {
                *handler = None;
            }
            crate::capture::set_visibility_handler(None);
            self.clear_shortcuts();
            let _ = self.configure_quick_capture(None);
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

/// Keep the old key registered until both its replacement and its preference
/// file are committed. Event dispatch uses only the committed binding.
#[cfg(any(target_os = "macos", test))]
fn commit_quick_binding<T: Copy + PartialEq>(
    previous: Option<T>,
    new: Option<T>,
    mut register: impl FnMut(T) -> Result<(), String>,
    mut release: impl FnMut(T) -> Result<(), String>,
    persist: impl FnOnce() -> Result<(), String>,
) -> Result<Option<String>, String> {
    if previous == new {
        persist()?;
        return Ok(None);
    }
    if let Some(key) = new {
        register(key)?;
    }
    if let Err(error) = persist() {
        if let Some(key) = new {
            if let Err(cleanup) = release(key) {
                return Err(format!(
                    "{error}. {cleanup}. Reopen Framecraft to release the unused key."
                ));
            }
        }
        return Err(error);
    }
    let warning = previous.and_then(|key| release(key).err()).map(|error| {
        format!("Capture setup saved. {error}. Reopen Framecraft to release the previous key.")
    });
    Ok(warning)
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
    pub fn suppressed_quick_press(&self) -> bool {
        false
    }
    pub fn poll_with_capture_hide(&mut self, _: impl FnMut()) -> Vec<DesktopAction> {
        self.poll()
    }
    pub fn update(&mut self, _: DesktopState, _: usize, _: &str) {}
    pub fn set_capture_available(&mut self, _: bool) {}
    pub fn set_close_available(&mut self, _: bool) {}
    pub fn set_last_capture(&mut self, _: Option<LastCaptureStatus>, _: bool) {}
    pub fn set_capture_cancellable(&mut self, _: bool) {}
    pub fn configure_shortcuts(&mut self, _: &ShortcutSettings, _: bool) -> Result<(), String> {
        Err("Global shortcuts require macOS.".into())
    }
    pub fn configure_quick_capture(&mut self, shortcut: Option<&str>) -> Result<(), String> {
        if shortcut.is_none() {
            Ok(())
        } else {
            Err("Global shortcuts require macOS.".into())
        }
    }
    pub fn set_quick_shortcut_status(&self, _: QuickShortcutState, _: &str) {}
    pub fn commit_quick_capture(
        &mut self,
        shortcut: Option<&str>,
        persist: impl FnOnce() -> Result<(), String>,
    ) -> Result<Option<String>, String> {
        self.configure_quick_capture(shortcut)?;
        persist()?;
        Ok(None)
    }
    pub fn clear_shortcuts(&mut self) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    #[test]
    fn direct_output_failures_name_the_action_on_the_status_item_until_recovery_or_clear() {
        use LastCaptureStatus::*;
        let receipts = [
            None,
            Some(CopyFailed),
            Some(Copied),
            Some(SaveFailed),
            Some(Saved),
            Some(CopyFailed),
            None,
        ];
        assert_eq!(
            receipts.map(|receipt| receipt.and_then(LastCaptureStatus::status_item_title)),
            [
                None,
                Some("Copy failed"),
                None,
                Some("Save failed"),
                None,
                Some("Copy failed"),
                None
            ]
        );
    }
    #[test]
    fn preference_write_failure_does_not_release_the_working_quick_binding() {
        let keys = RefCell::new(vec![1]);
        let result = commit_quick_binding(
            Some(1),
            Some(2),
            |key| {
                keys.borrow_mut().push(key);
                Ok(())
            },
            |key| {
                keys.borrow_mut().retain(|value| *value != key);
                Ok(())
            },
            || {
                assert_eq!(*keys.borrow(), [1, 2]);
                Err("disk full".into())
            },
        );
        assert_eq!(result.unwrap_err(), "disk full");
        assert_eq!(*keys.borrow(), [1]);
    }
    #[test]
    fn occupied_shortcut_keeps_the_previous_key_and_preferences() {
        let result = commit_quick_binding(
            Some(1),
            Some(2),
            |_| Err("occupied".into()),
            |_| panic!("old binding must remain"),
            || panic!("must not rewrite settings"),
        );
        assert_eq!(result.unwrap_err(), "occupied");
        let same_key =
            commit_quick_binding(Some(1), Some(1), |_| panic!(), |_| panic!(), || Ok(()));
        assert_eq!(same_key.unwrap(), None);
    }
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
