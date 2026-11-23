//! Exercise the production receipt from a background app using synthetic pixels.
//! Only the receipt is visible. This fixture does not activate a window,
//! capture the screen, access the pasteboard or inspect the foreground app's contents.
#[cfg(all(target_os = "macos", feature = "ui"))]
mod mac {
    use framecraft::{
        capture::CaptureMode,
        editor,
        native_handoff::{CaptureThumbnail, EditorWindow, ThumbnailAction, ThumbnailPlacement},
    };
    use objc2::{rc::Retained, MainThreadMarker, MainThreadOnly};
    use objc2_app_kit::{
        NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSBitmapImageFileType,
        NSButton, NSColorSpace, NSScreen, NSTextView, NSView, NSWindow, NSWindowStyleMask,
        NSWorkspace,
    };
    use objc2_foundation::{ns_string, NSDictionary, NSNumber, NSPoint, NSRange, NSRect, NSSize};
    use raw_window_handle::{AppKitWindowHandle, HandleError, HasWindowHandle, WindowHandle};
    use std::{error::Error, path::Path, ptr::NonNull};

    struct FixtureView<'a>(&'a NSView);
    impl HasWindowHandle for FixtureView<'_> {
        fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
            let raw = AppKitWindowHandle::new(NonNull::from(self.0).cast());
            // SAFETY: the borrowed fixture view outlives this handle.
            Ok(unsafe { WindowHandle::borrow_raw(raw.into()) })
        }
    }

    fn window(mtm: MainThreadMarker, frame: NSRect) -> Retained<NSWindow> {
        // SAFETY: disable auto-release on close before handing out ownership.
        unsafe {
            let window = NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                frame,
                NSWindowStyleMask::Titled,
                NSBackingStoreType::Buffered,
                false,
            );
            window.setReleasedWhenClosed(false);
            window
        }
    }

    fn export_own_view(panel: &NSWindow, view: &NSView, path: &Path) -> Result<(), Box<dyn Error>> {
        view.displayIfNeeded();
        let rect = view.bounds();
        let rep = view
            .bitmapImageRepForCachingDisplayInRect(rect)
            .ok_or("Missing view bitmap")?;
        view.cacheDisplayInRect_toBitmapImageRep(rect, &rep);
        // SAFETY: PNG requires no options. The data is the panel's own view.
        let png = unsafe {
            rep.representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new())
        }
        .ok_or("Could not encode the panel view")?;
        // AppKit caches the subviews on transparency. Composite those actual
        // pixels onto this panel's own resolved background for a standalone
        // preview. This excludes desktop pixels and the WindowServer shadow.
        let bg = panel
            .backgroundColor()
            .colorUsingColorSpace(&NSColorSpace::sRGBColorSpace())
            .ok_or("Could not resolve the panel's native background colour")?;
        let channel = |value: f64| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
        let layer = image::load_from_memory(&png.to_vec())?.to_rgba8();
        let mut frame = image::RgbaImage::from_pixel(
            layer.width(),
            layer.height(),
            image::Rgba([
                channel(bg.redComponent()),
                channel(bg.greenComponent()),
                channel(bg.blueComponent()),
                255,
            ]),
        );
        image::imageops::overlay(&mut frame, &layer, 0, 0);
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)?;
        frame.write_to(&mut file, image::ImageFormat::Png)?;
        Ok(())
    }

    pub fn run() -> Result<(), Box<dyn Error>> {
        let args: Vec<_> = std::env::args_os().skip(1).collect();
        let output = match args.as_slice() {
            [] => None,
            [flag, path] if flag == "--export-panel" => Some(Path::new(path)),
            _ => return Err("Use native-handoff-smoke [--export-panel NEW.png]".into()),
        };
        let mtm = MainThreadMarker::new().ok_or("AppKit main thread required")?;
        let app = NSApplication::sharedApplication(mtm);
        app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
        app.finishLaunching();
        let front_pid = || {
            NSWorkspace::sharedWorkspace()
                .frontmostApplication()
                .map(|a| a.processIdentifier())
        };
        let front = front_pid().ok_or("No logged-in foreground app")?;
        assert_ne!(
            front as u32,
            std::process::id(),
            "Start the fixture in the background"
        );
        assert!(!app.isActive());

        let frame = NSRect::new(NSPoint::new(40.0, 40.0), NSSize::new(240.0, 100.0));
        let editor_window = window(mtm, frame);
        editor_window.setAlphaValue(0.625);
        let editor_view = editor_window.contentView().ok_or("Missing editor view")?;
        let handle = EditorWindow::from_handle(&FixtureView(&editor_view))?;
        let source = window(mtm, frame);
        let text = NSTextView::initWithFrame(NSTextView::alloc(mtm), frame);
        text.setString(ns_string!("beforeafter"));
        source.setContentView(Some(&text));
        assert!(source.makeFirstResponder(Some(&text)));
        assert!(!source.isVisible());
        let caret = NSRange::new(6, 0);
        text.setSelectedRange(caret);
        let key = app.keyWindow();
        let responder = source
            .firstResponder()
            .ok_or("Missing source first responder")?;

        // A background editor stays out of view throughout this full guard cycle.
        {
            let _guard = handle.begin_capture();
            assert_eq!(editor_window.alphaValue(), 0.0);
            assert!(editor_window.ignoresMouseEvents());
        }
        assert_eq!(editor_window.alphaValue(), 0.625);
        assert!(!editor_window.ignoresMouseEvents());
        assert!(!editor_window.isVisible());

        let pixels = editor::demo_image();
        let ctx = eframe::egui::Context::default();
        let observed = ThumbnailPlacement::before_selection(CaptureMode::Screen)
            .confirm_after_selection(CaptureMode::Screen);
        // This fixture owns synthetic pixels and explicitly chooses their
        // destination. An online display can be asleep during unattended CI.
        let placement = match observed {
            ThumbnailPlacement::Unavailable => {
                let target = NSScreen::screens(mtm)
                    .firstObject()
                    .ok_or("AppKit has no screen for the synthetic fixture")?;
                let id = target
                    .deviceDescription()
                    .objectForKey(ns_string!("NSScreenNumber"))
                    .and_then(|v| v.downcast::<NSNumber>().ok())
                    .ok_or("The fixture screen has no ID")?;
                println!("The selector would suppress a receipt. Probing the explicitly chosen synthetic destination.");
                ThumbnailPlacement::Display(id.unsignedIntValue())
            }
            confirmed => confirmed,
        };
        let mut thumbnail = CaptureThumbnail::default();
        for (title, expected) in [
            ("Edit", ThumbnailAction::Edit),
            ("Clear", ThumbnailAction::Clear),
        ] {
            thumbnail.show(&pixels, placement, "Copied on this Mac", &ctx)?;
            let panel = app
                .windows()
                .iter()
                .find(|w| w.isVisible() && *w.title() == *ns_string!("Last capture"))
                .ok_or_else(|| format!("The synthetic thumbnail did not appear on the primary display ({placement:?}, {} fixture windows)", app.windows().count()))?;
            assert!(panel
                .styleMask()
                .contains(NSWindowStyleMask::NonactivatingPanel));
            assert!(!panel.isKeyWindow());
            assert!(!panel.isMainWindow());
            assert_eq!(app.keyWindow(), key);
            assert!(!app.isActive());
            assert_eq!(source.firstResponder().as_deref(), Some(&*responder));
            assert_eq!(text.selectedRange(), caret);
            assert_eq!(front_pid(), Some(front));
            let view = panel.contentView().ok_or("Missing panel content")?;
            if expected == ThumbnailAction::Edit {
                if let Some(path) = output {
                    export_own_view(&panel, &view, path)?;
                }
            }
            let button = view
                .subviews()
                .iter()
                .find_map(|subview| {
                    subview
                        .downcast::<NSButton>()
                        .ok()
                        .filter(|b| b.title().to_string() == title)
                })
                .ok_or("Missing thumbnail action")?;
            // SAFETY: this is an actual push button in the fixture-owned panel.
            unsafe { button.performClick(None) };
            assert_eq!(thumbnail.take_action(), Some(expected));
            assert!(!panel.isVisible());
            assert_eq!(front_pid(), Some(front));
            assert_eq!(text.selectedRange(), caret);
        }
        thumbnail.show(
            &pixels,
            ThumbnailPlacement::Unavailable,
            "Saved to file",
            &ctx,
        )?;
        assert!(!app
            .windows()
            .iter()
            .any(|w| w.isVisible() && *w.title() == *ns_string!("Last capture")));
        assert_eq!(front_pid(), Some(front));
        source.close();
        editor_window.close();
        println!("Passed non-key native panel, unchanged foreground app and offscreen caret, Edit/Clear, display suppression and editor mask restoration.");
        Ok(())
    }
}

#[cfg(all(target_os = "macos", feature = "ui"))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    mac::run()
}

#[cfg(not(all(target_os = "macos", feature = "ui")))]
fn main() {
    println!("Requires a macOS desktop and the ui feature.");
}
