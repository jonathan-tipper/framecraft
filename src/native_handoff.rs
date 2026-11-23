//! Native, local-only output for one-off Mac captures.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThumbnailAction {
    Edit,
    Clear,
}

/// The optional thumbnail is shown only on the confirmed capture display.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThumbnailPlacement {
    Display(u32),
    Unavailable,
}

impl ThumbnailPlacement {
    pub fn before_selection(mode: crate::capture::CaptureMode) -> Self {
        #[cfg(target_os = "macos")]
        {
            mac::capture_display(mode)
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = mode;
            Self::Unavailable
        }
    }

    pub fn confirm_after_selection(self, mode: crate::capture::CaptureMode) -> Self {
        self.confirmed_with(Self::before_selection(mode))
    }

    fn confirmed_with(self, other: Self) -> Self {
        if self == other {
            self
        } else {
            Self::Unavailable
        }
    }
}

#[cfg(target_os = "macos")]
mod mac {
    use super::{ThumbnailAction, ThumbnailPlacement};
    use crate::capture::CaptureMode;
    use eframe::egui::Context;
    use image::{
        codecs::png::{CompressionType, FilterType, PngEncoder},
        ExtendedColorType, ImageEncoder, RgbaImage,
    };
    use objc2::{
        define_class, msg_send, rc::Retained, runtime::ProtocolObject, sel, AnyThread, ClassType,
        DefinedClass, MainThreadMarker, MainThreadOnly,
    };
    use objc2_app_kit::{
        NSAccessibility, NSApplicationDidChangeScreenParametersNotification, NSBackingStoreType,
        NSButton, NSFloatingWindowLevel, NSImage, NSImageScaling, NSImageView, NSPanel,
        NSPasteboard, NSPasteboardContentsOptions, NSPasteboardItem, NSPasteboardTypePNG,
        NSPasteboardWriting, NSScreen, NSTextField, NSView, NSWindow, NSWindowAnimationBehavior,
        NSWindowCollectionBehavior, NSWindowStyleMask, NSWindowTitleVisibility,
    };
    use objc2_foundation::{
        ns_string, NSArray, NSData, NSNotificationCenter, NSNumber, NSObject, NSObjectProtocol,
        NSPoint, NSRect, NSSize, NSString,
    };
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use std::{
        cell::Cell,
        time::{Duration, Instant},
    };

    fn encode_png(pixels: &RgbaImage) -> Result<Vec<u8>, String> {
        if pixels.width() == 0 || pixels.height() == 0 {
            return Err("The capture contains no pixels.".into());
        }
        let mut png = Vec::new();
        PngEncoder::new_with_quality(&mut png, CompressionType::Fast, FilterType::Sub)
            .write_image(
                pixels.as_raw(),
                pixels.width(),
                pixels.height(),
                ExtendedColorType::Rgba8,
            )
            .map_err(|e| format!("Could not encode the screenshot for handoff: {e}"))?;
        Ok(png)
    }

    fn preview_pixels(source: &RgbaImage) -> RgbaImage {
        let (w, h) = source.dimensions();
        if w <= 400 && h <= 220 {
            return source.clone();
        }
        let scale = (400.0 / f64::from(w)).min(220.0 / f64::from(h));
        let width = (f64::from(w) * scale).round().max(1.0) as u32;
        let height = (f64::from(h) * scale).round().max(1.0) as u32;
        // Sampling only the display-sized preview keeps work on the AppKit
        // thread bounded even for a large Retina screenshot.
        RgbaImage::from_fn(width, height, |x, y| {
            let sx = u64::from(x) * u64::from(w) / u64::from(width);
            let sy = u64::from(y) * u64::from(h) / u64::from(height);
            source[(sx as u32, sy as u32)]
        })
    }

    fn put_image(
        board: &NSPasteboard,
        pixels: &RgbaImage,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<bool, String> {
        if cancelled() {
            return Ok(false);
        }
        let data = NSData::with_bytes(&encode_png(pixels)?);
        let item = NSPasteboardItem::new();
        // SAFETY: this immutable AppKit constant is available on macOS 13+.
        if !item.setData_forType(&data, unsafe { NSPasteboardTypePNG }) {
            return Err("Could not prepare screenshot data for the clipboard.".into());
        }
        let objects: Retained<NSArray<ProtocolObject<dyn NSPasteboardWriting>>> =
            NSArray::arrayWithObject(ProtocolObject::from_ref(&*item));
        // The publication boundary is immediately before prepare, which
        // clears the previous contents and sets the scope for this write.
        if cancelled() {
            return Ok(false);
        }
        board.prepareForNewContentsWithOptions(NSPasteboardContentsOptions::CurrentHostOnly);
        if board.writeObjects(&objects) {
            Ok(true)
        } else {
            Err("macOS could not copy the screenshot. Retry or save the captured image.".into())
        }
    }

    pub fn copy_image_local(
        pixels: &RgbaImage,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<bool, String> {
        objc2::rc::autoreleasepool(|_| {
            // The native service can return nil when unavailable, even
            // though the SDK declares this convenience accessor non-null.
            // SAFETY: the accessor returns an autoreleased NSPasteboard.
            let board: Option<Retained<NSPasteboard>> =
                unsafe { msg_send![NSPasteboard::class(), generalPasteboard] };
            let board = board
                .ok_or("The macOS clipboard is unavailable. Retry or save the captured image.")?;
            put_image(&board, pixels, cancelled)
        })
    }

    pub struct EditorWindow(Retained<NSWindow>);

    impl EditorWindow {
        pub fn from_handle(frame: &impl HasWindowHandle) -> Result<Self, String> {
            let _main_thread =
                MainThreadMarker::new().ok_or("The editor window requires the main thread.")?;
            let handle = frame.window_handle().map_err(|e| e.to_string())?;
            let RawWindowHandle::AppKit(appkit) = handle.as_raw() else {
                return Err("Cannot locate the Mac editor window.".into());
            };
            // SAFETY: the borrowed eframe handle keeps this AppKit NSView alive.
            let view = unsafe { appkit.ns_view.cast::<NSView>().as_ref() };
            view.window()
                .map(Self)
                .ok_or_else(|| "The editor view is not attached to a Mac window.".into())
        }

        /// Temporarily remove visible editor pixels and mouse interception.
        /// Keeping the window in place avoids changing the source app's key
        /// window, window order, active Space or original minimised state.
        pub fn begin_capture(&self) -> CaptureWindowGuard {
            let guard = CaptureWindowGuard {
                window: self.0.clone(),
                alpha: self.0.alphaValue(),
                ignores_mouse: self.0.ignoresMouseEvents(),
            };
            self.0.setIgnoresMouseEvents(true);
            self.0.setAlphaValue(0.0);
            guard
        }
    }

    pub struct CaptureWindowGuard {
        window: Retained<NSWindow>,
        alpha: f64,
        ignores_mouse: bool,
    }

    impl Drop for CaptureWindowGuard {
        fn drop(&mut self) {
            self.window.setAlphaValue(self.alpha);
            self.window.setIgnoresMouseEvents(self.ignores_mouse);
        }
    }

    struct ButtonState {
        action: Cell<Option<ThumbnailAction>>,
        wake: Context,
    }

    define_class!(
        // SAFETY: NSObject has no additional subclass invariants. Actions
        // and the AppKit target are confined to the main thread.
        #[unsafe(super = NSObject)]
        #[thread_kind = MainThreadOnly]
        #[ivars = ButtonState]
        #[name = "FramecraftCaptureThumbnailButtons"]
        struct ThumbnailButtons;

        unsafe impl NSObjectProtocol for ThumbnailButtons {}
        impl ThumbnailButtons {
            #[unsafe(method(editCapture:))]
            fn edit(&self, _sender: &NSButton) {
                self.ivars().action.set(Some(ThumbnailAction::Edit));
                self.ivars().wake.request_repaint();
            }
            #[unsafe(method(clearCapture:))]
            fn clear(&self, _sender: &NSButton) {
                self.ivars().action.set(Some(ThumbnailAction::Clear));
                self.ivars().wake.request_repaint();
            }
        }
    );

    struct VisibleThumbnail {
        panel: Retained<NSPanel>,
        buttons: Retained<ThumbnailButtons>,
        expires: Instant,
    }

    impl Drop for VisibleThumbnail {
        fn drop(&mut self) {
            // SAFETY: unregister the exact panel/notification pair registered
            // in show before dropping the retained observer.
            unsafe {
                NSNotificationCenter::defaultCenter().removeObserver_name_object(
                    &self.panel,
                    Some(NSApplicationDidChangeScreenParametersNotification),
                    None,
                );
            }
            self.panel.orderOut(None);
            self.panel.setContentView(None);
            self.panel.close();
        }
    }

    #[derive(Default)]
    pub struct CaptureThumbnail {
        visible: Option<VisibleThumbnail>,
    }

    fn rect(x: f64, y: f64, w: f64, h: f64) -> NSRect {
        NSRect::new(NSPoint::new(x, y), NSSize::new(w, h))
    }

    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGGetActiveDisplayList(limit: u32, displays: *mut u32, count: *mut u32) -> i32;
    }

    pub(super) fn capture_display(mode: CaptureMode) -> ThumbnailPlacement {
        let mut active = [0; 2];
        let mut count = 0;
        // SAFETY: CoreGraphics writes at most two display IDs and one count.
        let error = unsafe { CGGetActiveDisplayList(2, active.as_mut_ptr(), &mut count) };
        if error != 0 {
            return ThumbnailPlacement::Unavailable;
        }
        choose_display(mode, &active[..(count as usize).min(active.len())])
    }

    fn choose_display(mode: CaptureMode, active: &[u32]) -> ThumbnailPlacement {
        match (mode, active) {
            // The -m flag fixes a screen capture to the primary display,
            // which CGGetActiveDisplayList documents as its first result.
            (CaptureMode::Screen, [primary, ..]) if *primary != 0 => {
                ThumbnailPlacement::Display(*primary)
            }
            // Interactive selection returns no display ID. Two active monitors
            // already make its source ambiguous, regardless of pointer position.
            (CaptureMode::Area | CaptureMode::Window, [display]) if *display != 0 => {
                ThumbnailPlacement::Display(*display)
            }
            _ => ThumbnailPlacement::Unavailable,
        }
    }

    fn panel_screen(
        placement: ThumbnailPlacement,
        mtm: MainThreadMarker,
    ) -> Option<Retained<NSScreen>> {
        let ThumbnailPlacement::Display(expected) = placement else {
            return None;
        };
        NSScreen::screens(mtm).iter().find(|screen| {
            screen
                .deviceDescription()
                .objectForKey(ns_string!("NSScreenNumber"))
                .and_then(|value| value.downcast::<NSNumber>().ok())
                .is_some_and(|id| id.unsignedIntValue() == expected)
        })
    }

    impl CaptureThumbnail {
        pub fn show(
            &mut self,
            pixels: &RgbaImage,
            placement: ThumbnailPlacement,
            message: &str,
            ctx: &Context,
        ) -> Result<(), String> {
            self.hide();
            let mtm =
                MainThreadMarker::new().ok_or("The capture thumbnail requires the main thread.")?;
            if pixels.width() == 0 || pixels.height() == 0 {
                return Err("The capture thumbnail contains no pixels.".into());
            }
            let Some(screen) = panel_screen(placement, mtm) else {
                // The menu receipt and Edit Last Capture remain available.
                return Ok(());
            };
            let area = screen.visibleFrame();
            let width = 228.0;
            let height = 188.0;
            let frame = rect(
                area.origin.x + area.size.width - width - 16.0,
                area.origin.y + 16.0,
                width,
                height,
            );
            if area.size.width < width + 32.0 || area.size.height < height + 32.0 {
                return Err("The display is too small to show the capture thumbnail.".into());
            }

            let data = NSData::with_bytes(&encode_png(&preview_pixels(pixels))?);
            let image = NSImage::initWithData(NSImage::alloc(), &data)
                .ok_or("Could not display the captured image.")?;
            let panel = NSPanel::initWithContentRect_styleMask_backing_defer(
                NSPanel::alloc(mtm),
                frame,
                NSWindowStyleMask::Titled
                    | NSWindowStyleMask::FullSizeContentView
                    | NSWindowStyleMask::NonactivatingPanel,
                NSBackingStoreType::Buffered,
                false,
            );
            // SAFETY: this panel is strongly owned by VisibleThumbnail.
            unsafe { panel.setReleasedWhenClosed(false) };
            panel.setTitle(ns_string!("Last capture"));
            panel.setTitleVisibility(NSWindowTitleVisibility::Hidden);
            panel.setTitlebarAppearsTransparent(true);
            panel.setFloatingPanel(true);
            panel.setBecomesKeyOnlyIfNeeded(true);
            panel.setHidesOnDeactivate(false);
            panel.setMovable(false);
            panel.setExcludedFromWindowsMenu(true);
            panel.setAnimationBehavior(NSWindowAnimationBehavior::None);
            panel.setLevel(NSFloatingWindowLevel);
            panel.setCollectionBehavior(
                NSWindowCollectionBehavior::MoveToActiveSpace
                    | NSWindowCollectionBehavior::FullScreenAuxiliary
                    | NSWindowCollectionBehavior::IgnoresCycle,
            );
            panel.setFrame_display(frame, false);
            let content = panel
                .contentView()
                .ok_or("The thumbnail has no content view.")?;
            let preview = NSImageView::imageViewWithImage(&image, mtm);
            preview.setFrame(rect(14.0, 68.0, 200.0, 110.0));
            preview.setImageScaling(NSImageScaling::ScaleProportionallyUpOrDown);
            preview.setAccessibilityLabel(Some(ns_string!("Last screenshot preview")));
            content.addSubview(&preview);
            let label = NSTextField::wrappingLabelWithString(&NSString::from_str(message), mtm);
            label.setFrame(rect(12.0, 35.0, 204.0, 33.0));
            label.setToolTip(Some(&NSString::from_str(message)));
            content.addSubview(&label);

            let allocated = ThumbnailButtons::alloc(mtm).set_ivars(ButtonState {
                action: Cell::new(None),
                wake: ctx.clone(),
            });
            // SAFETY: NSObject's designated initialiser takes no arguments.
            let buttons: Retained<ThumbnailButtons> = unsafe { msg_send![super(allocated), init] };
            for (title, description, action, x) in [
                ("Edit", "Edit last capture", sel!(editCapture:), 10.0),
                (
                    "Clear",
                    "Clear last capture from Framecraft",
                    sel!(clearCapture:),
                    119.0,
                ),
            ] {
                // SAFETY: both selectors take the sending NSButton; the target
                // outlives the panel's controls and callbacks.
                let button = unsafe {
                    NSButton::buttonWithTitle_target_action(
                        &NSString::from_str(title),
                        Some(&buttons),
                        Some(action),
                        mtm,
                    )
                };
                button.setAccessibilityLabel(Some(&NSString::from_str(description)));
                button.setRefusesFirstResponder(true);
                button.setFrame(rect(x, 7.0, 99.0, 28.0));
                content.addSubview(&button);
            }

            const LIFETIME: Duration = Duration::from_secs(8);
            // AppKit may move windows when a monitor disconnects. Dismiss the
            // sensitive thumbnail when display parameters change.
            // SAFETY: NSPanel's orderOut: accepts any object as its sender.
            // VisibleThumbnail retains the observer until unregistration.
            unsafe {
                NSNotificationCenter::defaultCenter().addObserver_selector_name_object(
                    &panel,
                    sel!(orderOut:),
                    Some(NSApplicationDidChangeScreenParametersNotification),
                    None,
                );
            }
            // Only an explicit Edit later opens and activates the editor.
            panel.orderFrontRegardless();
            self.visible = Some(VisibleThumbnail {
                panel,
                buttons,
                expires: Instant::now() + LIFETIME,
            });
            ctx.request_repaint_after(LIFETIME);
            Ok(())
        }

        pub fn take_action(&mut self) -> Option<ThumbnailAction> {
            let visible = self.visible.as_ref()?;
            let action = visible.buttons.ivars().action.take();
            if action.is_some() || Instant::now() >= visible.expires {
                self.hide();
            }
            action
        }

        pub fn hide(&mut self) {
            self.visible = None;
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use image::Rgba;
        use objc2_app_kit::NSPasteboardTypePNG;

        #[test]
        fn receipt_only_appears_on_a_confirmed_capture_display() {
            use ThumbnailPlacement::{Display, Unavailable};
            assert_eq!(choose_display(CaptureMode::Screen, &[5, 8]), Display(5));
            assert_eq!(choose_display(CaptureMode::Area, &[5]), Display(5));
            assert_eq!(choose_display(CaptureMode::Window, &[5]), Display(5));
            assert_eq!(choose_display(CaptureMode::Area, &[5, 8]), Unavailable);
            assert_eq!(choose_display(CaptureMode::Window, &[5, 8]), Unavailable);
            assert_eq!(choose_display(CaptureMode::Area, &[]), Unavailable);
            assert_eq!(Display(5).confirmed_with(Display(8)), Unavailable);
            assert_eq!(Display(5).confirmed_with(Unavailable), Unavailable);
            assert_eq!(Unavailable.confirmed_with(Display(5)), Unavailable);
            assert_eq!(Display(5).confirmed_with(Display(5)), Display(5));
        }

        #[test]
        fn preview_preserves_source_aspect_and_stays_small_for_retina_captures() {
            let image = RgbaImage::from_pixel(1000, 500, Rgba([7, 19, 211, 255]));
            let thumb = preview_pixels(&image);
            assert_eq!(thumb.dimensions(), (400, 200));
            assert!(thumb.pixels().all(|p| *p == image[(0, 0)]));
            let small = RgbaImage::from_pixel(5, 3, Rgba([180, 2, 3, 128]));
            assert_eq!(preview_pixels(&small), small);
        }

        #[test]
        #[ignore = "requires a logged-in macOS pasteboard server; uses only a private pasteboard"]
        fn private_pasteboard_round_trips_rgba_and_cancel_preserves_previous_item() {
            objc2::rc::autoreleasepool(|_| {
                let board = NSPasteboard::pasteboardWithUniqueName();
                let image = RgbaImage::from_fn(7, 3, |x, y| Rgba([x as u8, y as u8, 78, 130]));
                assert_eq!(put_image(&board, &image, &|| false), Ok(true));
                let count = board.changeCount();
                let already_encoded = Cell::new(false);
                assert_eq!(
                    put_image(&board, &RgbaImage::new(12, 8), &|| already_encoded
                        .replace(true)),
                    Ok(false)
                );
                assert!(put_image(&board, &RgbaImage::new(0, 0), &|| false).is_err());
                assert_eq!(board.changeCount(), count);
                // SAFETY: this immutable AppKit constant is available on all supported macOS versions.
                let data = board.dataForType(unsafe { NSPasteboardTypePNG }).unwrap();
                assert_eq!(
                    image::load_from_memory(&data.to_vec()).unwrap().to_rgba8(),
                    image
                );
                // A second process exercises a real NSTextView paste receiver
                // from its main thread, without accessing the general clipboard.
                let cache = tempfile::tempdir().unwrap();
                let result = std::process::Command::new("/usr/bin/xcrun")
                    .args(["swift", "-module-cache-path"])
                    .arg(cache.path())
                    .arg(concat!(
                        env!("CARGO_MANIFEST_DIR"),
                        "/tests/private-paste-receiver.swift"
                    ))
                    .arg(board.name().to_string())
                    .output()
                    .unwrap();
                // SAFETY: releaseGlobally takes no arguments. This is our
                // unique fixture pasteboard, never the system clipboard.
                unsafe {
                    let _: () = msg_send![&*board, releaseGlobally];
                }
                assert!(
                    result.status.success(),
                    "private paste receiver: {}",
                    String::from_utf8_lossy(&result.stderr)
                );
            });
        }
    }
}

#[cfg(target_os = "macos")]
pub use mac::{copy_image_local, CaptureThumbnail, CaptureWindowGuard, EditorWindow};

#[cfg(not(target_os = "macos"))]
mod unavailable {
    use super::{ThumbnailAction, ThumbnailPlacement};
    use eframe::egui::Context;
    use image::RgbaImage;
    use raw_window_handle::HasWindowHandle;

    pub fn copy_image_local(_: &RgbaImage, _: &dyn Fn() -> bool) -> Result<bool, String> {
        Err("Mac-only clipboard capture requires macOS.".into())
    }

    pub struct EditorWindow;
    pub struct CaptureWindowGuard;
    impl EditorWindow {
        pub fn from_handle(_: &impl HasWindowHandle) -> Result<Self, String> {
            Err("The native capture window requires macOS.".into())
        }
        pub fn begin_capture(&self) -> CaptureWindowGuard {
            CaptureWindowGuard
        }
    }

    #[derive(Default)]
    pub struct CaptureThumbnail {
        _private: (),
    }
    impl CaptureThumbnail {
        pub fn show(
            &mut self,
            _: &RgbaImage,
            _: ThumbnailPlacement,
            _: &str,
            _: &Context,
        ) -> Result<(), String> {
            Err("The native thumbnail requires macOS.".into())
        }
        pub fn take_action(&mut self) -> Option<ThumbnailAction> {
            None
        }
        pub fn hide(&mut self) {}
    }
}

#[cfg(not(target_os = "macos"))]
pub use unavailable::{copy_image_local, CaptureThumbnail, CaptureWindowGuard, EditorWindow};
