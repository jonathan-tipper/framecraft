//! Native controls lifecycle only: never captures a screen, invokes OCR, or
//! saves user settings. Run with `cargo run --example native-controls-smoke`.
#[cfg(all(target_os = "macos", feature = "ui"))]
fn main() -> eframe::Result {
    use framecraft::{
        desktop::{DesktopAction, DesktopControls, DesktopState},
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
                let app_menu = native_menu.itemAtIndex(0).unwrap().submenu().unwrap();
                let settings_title = objc2_foundation::NSString::from_str("Keyboard Shortcuts…");
                app_menu
                    .performActionForItemAtIndex(app_menu.indexOfItemWithTitle(&settings_title));
                assert_eq!(controls.poll(), vec![DesktopAction::Settings]);
                let quit_title = objc2_foundation::NSString::from_str("Quit Framecraft");
                app_menu.performActionForItemAtIndex(app_menu.indexOfItemWithTitle(&quit_title));
                assert_eq!(controls.poll(), vec![DesktopAction::Quit]);
                println!("PASS actual native Capture Area/Window/Screen, busy gating, Settings and Quit actions; no image captured");
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
#[cfg(not(all(target_os = "macos", feature = "ui")))]
fn main() {
    eprintln!("This smoke test requires macOS and the ui feature.");
}
