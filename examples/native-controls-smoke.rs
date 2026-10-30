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
                let keys = ShortcutSettings {
                    capture: "Super+Alt+Shift+F16".into(),
                    pause_resume: "Super+Alt+Shift+F17".into(),
                    stop: "Super+Alt+Shift+F18".into(),
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
