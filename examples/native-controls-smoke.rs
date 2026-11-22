//! Native controls lifecycle only: never captures a screen, invokes OCR, or
//! saves user settings. Run with `cargo run --example native-controls-smoke`.
#[cfg(all(target_os = "macos", feature = "ui"))]
fn main() -> eframe::Result {
    use framecraft::{
        desktop::{DesktopAction, DesktopControls, DesktopState, LastCaptureStatus},
        settings::ShortcutSettings,
    };
    struct Smoke {
        controls: Option<DesktopControls>,
        began: std::time::Instant,
        stage: usize,
    }
    impl eframe::App for Smoke {
        fn update(&mut self, ctx: &eframe::egui::Context, _: &mut eframe::Frame) {
            if self.controls.is_none() {
                let application = objc2_app_kit::NSApplication::sharedApplication(
                    objc2::MainThreadMarker::new().unwrap(),
                );
                let original_delegate = application.delegate().unwrap();
                let wake = ctx.clone();
                let mut controls = DesktopControls::new(move || wake.request_repaint())
                    .expect("native controls created on main thread");
                let native_menu = application.mainMenu().expect("application menu installed");
                let captures = native_menu
                    .itemWithTitle(&objc2_foundation::NSString::from_str("Capture"))
                    .unwrap()
                    .submenu()
                    .unwrap();
                for (title, expected) in [
                    ("Capture Area…", DesktopAction::CaptureArea),
                    ("Capture Window…", DesktopAction::CaptureWindow),
                    ("Capture Screen", DesktopAction::CaptureScreen),
                ] {
                    let title = objc2_foundation::NSString::from_str(title);
                    assert!(captures.itemWithTitle(&title).unwrap().isEnabled());
                    captures.performActionForItemAtIndex(captures.indexOfItemWithTitle(&title));
                    assert_eq!(controls.poll(), vec![expected]);
                }
                controls.set_capture_available(false);
                let area_title = objc2_foundation::NSString::from_str("Capture Area…");
                assert!(!captures.itemWithTitle(&area_title).unwrap().isEnabled());
                captures.performActionForItemAtIndex(captures.indexOfItemWithTitle(&area_title));
                assert!(controls.poll().is_empty(), "busy captures must not route");
                controls.set_capture_available(true);
                assert!(captures.itemWithTitle(&area_title).unwrap().isEnabled());
                let chord = "Control+Option+F18";
                controls.configure_quick_capture(Some(chord)).unwrap();
                let key: global_hotkey::hotkey::HotKey = "Control+Alt+F18".parse().unwrap();
                deliver_local_capture_press(key.id()).unwrap();
                assert_eq!(controls.poll(), vec![DesktopAction::CaptureArea]);
                assert!(controls.poll().is_empty(), "one press produces one action");
                controls.configure_quick_capture(None).unwrap();
                println!("PASS targeted Carbon callback delivered exactly one global Capture Area action");
                let edit = objc2_foundation::NSString::from_str("Edit Last Capture…");
                let options = captures
                    .itemWithTitle(&objc2_foundation::NSString::from_str(
                        "Last Capture Options",
                    ))
                    .unwrap()
                    .submenu()
                    .unwrap();
                let retry = objc2_foundation::NSString::from_str("Retry Last Output");
                let clear = objc2_foundation::NSString::from_str("Clear Last Capture");
                assert!(!captures.itemWithTitle(&edit).unwrap().isEnabled());
                controls.set_last_capture(Some(LastCaptureStatus::Copied), true);
                assert!(captures.itemWithTitle(&edit).unwrap().isEnabled());
                captures.performActionForItemAtIndex(captures.indexOfItemWithTitle(&edit));
                assert_eq!(controls.poll(), vec![DesktopAction::EditLastCapture]);
                assert!(!options.itemWithTitle(&retry).unwrap().isEnabled());
                controls.set_last_capture(Some(LastCaptureStatus::CopyFailed), true);
                for (title, action) in [
                    (&retry, DesktopAction::RetryLastOutput),
                    (&clear, DesktopAction::ClearLastCapture),
                ] {
                    assert!(options.itemWithTitle(title).unwrap().isEnabled());
                    options.performActionForItemAtIndex(options.indexOfItemWithTitle(title));
                    assert_eq!(controls.poll(), vec![action]);
                }
                controls.set_last_capture(Some(LastCaptureStatus::Saved), true);
                assert!(!options.itemWithTitle(&retry).unwrap().isEnabled());
                controls.set_last_capture(Some(LastCaptureStatus::SaveFailed), true);
                assert!(options.itemWithTitle(&retry).unwrap().isEnabled());
                controls.set_last_capture(None, true);
                assert!(!captures.itemWithTitle(&edit).unwrap().isEnabled());
                assert!(!options.itemWithTitle(&clear).unwrap().isEnabled());
                let app_menu = native_menu.itemAtIndex(0).unwrap().submenu().unwrap();
                let settings_title = objc2_foundation::NSString::from_str("Capture setup…");
                app_menu
                    .performActionForItemAtIndex(app_menu.indexOfItemWithTitle(&settings_title));
                assert_eq!(controls.poll(), vec![DesktopAction::Settings]);
                let quit_title = objc2_foundation::NSString::from_str("Quit Framecraft");
                app_menu.performActionForItemAtIndex(app_menu.indexOfItemWithTitle(&quit_title));
                assert_eq!(controls.poll(), vec![DesktopAction::Quit]);
                println!(
                    "PASS native capture, edit last, retry, clear, settings and quit commands"
                );
                let keys = ShortcutSettings {
                    capture: "Command + Option + Shift + F16".into(),
                    pause_resume: "Command + Option + Shift + F17".into(),
                    stop: "Command + Option + Shift + F18".into(),
                };
                controls
                    .configure_shortcuts(&keys, true)
                    .expect("smoke shortcuts registered");
                assert!(controls.configure_shortcuts(&keys, false).is_err());
                println!("PASS native menu creation; 3 shortcut registrations; active-session edit rejected");
                assert!(std::ptr::eq(
                    &*original_delegate,
                    &*application.delegate().unwrap()
                ));
                // This is AppKit's real ordinary-quit path, shared by menu,
                // Command-Q and Dock. A working hook returns to this function.
                application.terminate(None);
                assert_eq!(controls.poll(), vec![DesktopAction::Quit]);
                println!("PASS native terminate cancelled; Quit action received; original delegate retained; app still alive");
                self.controls = Some(controls);
                self.began = std::time::Instant::now();
            }
            let phases = [
                DesktopState::Running,
                DesktopState::Paused,
                DesktopState::Error,
                DesktopState::Stopped,
            ];
            let controls = self.controls.as_mut().unwrap();
            let _ = controls.poll();
            controls.update(phases[self.stage], 2, "Synthetic smoke fixture; no capture");
            eframe::egui::CentralPanel::default().show(ctx, |ui| {
                ui.label("Framecraft native control smoke test. No screen capture or OCR occurs.");
                ui.label(format!("{:?}", phases[self.stage]));
            });
            if self.began.elapsed() >= std::time::Duration::from_millis(300) {
                println!("PASS state update {:?}", phases[self.stage]);
                self.stage += 1;
                self.began = std::time::Instant::now();
                if self.stage == phases.len() {
                    controls.clear_shortcuts();
                    println!("PASS release registrations and close");
                    ctx.send_viewport_cmd(eframe::egui::ViewportCommand::Close);
                    self.stage = phases.len() - 1;
                }
            }
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
    }
    eframe::run_native(
        "Framecraft native control smoke",
        eframe::NativeOptions {
            viewport: eframe::egui::ViewportBuilder::default().with_inner_size([500., 150.]),
            ..Default::default()
        },
        Box::new(|_| {
            Ok(Box::new(Smoke {
                controls: None,
                began: std::time::Instant::now(),
                stage: 0,
            }))
        }),
    )
}

/// Deliver a Carbon event only to this smoke process. It never injects a
/// keyboard event into macOS, calls capture or sends input to another app.
#[cfg(all(target_os = "macos", feature = "ui"))]
fn deliver_local_capture_press(id: u32) -> Result<(), i32> {
    use std::{ffi::c_void, ptr};
    type Event = *mut c_void;
    #[link(name = "Carbon", kind = "framework")]
    extern "C" {
        fn GetApplicationEventTarget() -> Event;
        fn CreateEvent(
            allocator: Event,
            class: u32,
            kind: u32,
            time: f64,
            attributes: u32,
            event: *mut Event,
        ) -> i32;
        fn SetEventParameter(
            event: Event,
            name: u32,
            kind: u32,
            size: usize,
            data: *const c_void,
        ) -> i32;
        fn SendEventToEventTarget(event: Event, target: Event) -> i32;
        fn ReleaseEvent(event: Event);
    }
    let mut event = ptr::null_mut();
    let status = unsafe {
        CreateEvent(
            ptr::null_mut(),
            u32::from_be_bytes(*b"keyb"),
            5,
            0.,
            0,
            &mut event,
        )
    };
    if status != 0 {
        return Err(status);
    }
    let hot_key = [u32::from_be_bytes(*b"FrCp"), id];
    let mut status = unsafe {
        SetEventParameter(
            event,
            u32::from_be_bytes(*b"----"),
            u32::from_be_bytes(*b"hkid"),
            std::mem::size_of_val(&hot_key),
            hot_key.as_ptr().cast(),
        )
    };
    if status == 0 {
        status = unsafe { SendEventToEventTarget(event, GetApplicationEventTarget()) };
    }
    unsafe {
        ReleaseEvent(event);
    }
    if status == 0 {
        Ok(())
    } else {
        Err(status)
    }
}
#[cfg(not(all(target_os = "macos", feature = "ui")))]
fn main() {
    eprintln!("This smoke test requires macOS and the ui feature.");
}
