use super::*;
use framecraft::desktop::LastCaptureStatus;
use image::RgbaImage;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Destination {
    Clipboard,
    File,
}

enum Receipt {
    Copied,
    Saved(PathBuf),
    Failed(Destination, String),
}

pub(super) struct LastCapture {
    pixels: Arc<RgbaImage>,
    placement: ThumbnailPlacement,
    receipt: Receipt,
}

impl LastCapture {
    pub(super) fn status(&self) -> LastCaptureStatus {
        match self.receipt {
            Receipt::Copied => LastCaptureStatus::Copied,
            Receipt::Saved(_) => LastCaptureStatus::Saved,
            Receipt::Failed(Destination::Clipboard, _) => LastCaptureStatus::CopyFailed,
            Receipt::Failed(Destination::File, _) => LastCaptureStatus::SaveFailed,
        }
    }
    pub(super) fn failed(&self) -> bool {
        self.status().status_item_title().is_some()
    }
}

/// One-off acquisition is complete. Publish exactly one chosen output, or
/// return a cancellation without disturbing the last successful capture.
pub(super) fn finish_capture(
    placement: ThumbnailPlacement,
    pixels: Option<RgbaImage>,
    settings: &QuickCaptureSettings,
    cancelled: &dyn Fn() -> bool,
    copy: impl FnOnce(&RgbaImage, &dyn Fn() -> bool) -> Result<bool, String>,
) -> Outcome {
    let Some(pixels) = pixels.filter(|_| !cancelled()) else {
        return Outcome::Captured(None);
    };
    let destination = match settings.capture_action() {
        AfterCapture::Edit => return Outcome::Captured(Some(pixels)),
        AfterCapture::Save => {
            return match quick_capture::save_capture_cancellable(&pixels, settings, cancelled) {
                Ok(Some(path)) => Outcome::AutoSaved(pixels, Ok(path)),
                Ok(None) => Outcome::Captured(None),
                Err(error) => Outcome::AutoSaved(pixels, Err(error)),
            };
        }
        AfterCapture::CopyAndReturn => Destination::Clipboard,
        AfterCapture::SaveAndReturn => Destination::File,
    };
    publish(
        Arc::new(pixels),
        placement,
        destination,
        settings,
        cancelled,
        copy,
    )
}

fn publish(
    pixels: Arc<RgbaImage>,
    placement: ThumbnailPlacement,
    destination: Destination,
    settings: &QuickCaptureSettings,
    cancelled: &dyn Fn() -> bool,
    copy: impl FnOnce(&RgbaImage, &dyn Fn() -> bool) -> Result<bool, String>,
) -> Outcome {
    let receipt = match destination {
        Destination::Clipboard => match copy(&pixels, cancelled) {
            Ok(true) => Receipt::Copied,
            Ok(false) => return Outcome::Captured(None),
            Err(error) => Receipt::Failed(destination, error),
        },
        Destination::File => {
            match quick_capture::save_capture_cancellable(&pixels, settings, cancelled) {
                Ok(Some(path)) => Receipt::Saved(path),
                Ok(None) => return Outcome::Captured(None),
                Err(error) => Receipt::Failed(destination, error),
            }
        }
    };
    Outcome::Direct(LastCapture {
        pixels,
        placement,
        receipt,
    })
}

pub(super) fn show_editor(ctx: &egui::Context) {
    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
}

impl Framecraft {
    pub(super) fn receive_direct_capture(&mut self, ctx: &egui::Context, capture: LastCapture) {
        let label = capture.status().label();
        let mut message = match &capture.receipt {
            Receipt::Copied => "Copied on this Mac. Paste into your app.".into(),
            Receipt::Saved(path) => format!("Saved {}", path.display()),
            Receipt::Failed(_, error) => format!("{label}: {error}. Choose Retry Last Output, or Edit Last Capture to save the image manually."),
        };
        if self.quick_settings.show_thumbnail {
            if let Err(error) = self
                .thumbnail
                .show(&capture.pixels, capture.placement, label, ctx)
            {
                message.push_str(&format!(
                    " Thumbnail unavailable: {error}. Edit Last Capture is in the menu bar."
                ));
            }
        }
        self.notify(message, capture.failed());
        self.last_capture = Some(capture);
    }

    pub(super) fn sync_last_capture_controls(&mut self) {
        let available = self.capture_available();
        if let Some(controls) = &mut self.desktop {
            controls.set_last_capture(
                self.last_capture.as_ref().map(LastCapture::status),
                available,
            );
        }
    }

    pub(super) fn edit_last_capture(&mut self, ctx: &egui::Context) {
        if self.capture_available() && self.last_capture.is_some() {
            self.thumbnail.hide();
            self.request(ctx, Pending::EditLastCapture);
        }
    }

    pub(super) fn open_last_capture(&mut self, ctx: &egui::Context) {
        let Some(last) = &self.last_capture else {
            return;
        };
        let path = match &last.receipt {
            Receipt::Saved(path) => Some(path.as_path()),
            _ => None,
        };
        let name = path
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "New capture".into());
        let fresh = path.is_none();
        self.load(ctx.clone(), last.pixels.as_ref().clone(), name);
        self.fresh_capture = fresh;
        show_editor(ctx);
    }

    pub(super) fn clear_last_capture(&mut self) {
        if self.capture_available() && self.last_capture.take().is_some() {
            self.thumbnail.hide();
            self.sync_last_capture_controls();
            self.notify("Cleared the last capture from Framecraft memory.", false);
        }
    }

    pub(super) fn retry_last_output(&mut self, ctx: &egui::Context) {
        if !self.capture_available() {
            return;
        }
        let Some(last) = &self.last_capture else {
            return;
        };
        let Receipt::Failed(destination, _) = last.receipt else {
            return;
        };
        let pixels = last.pixels.clone();
        let placement = last.placement;
        let settings = self.quick_settings.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        self.capture_cancel = Some(cancel.clone());
        self.thumbnail.hide();
        self.start_job(ctx, "Retrying the last output…", move || {
            Ok(publish(
                pixels,
                placement,
                destination,
                &settings,
                &|| cancel.load(Ordering::Acquire),
                framecraft::native_handoff::copy_image_local,
            ))
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn capture_pixels(value: u8) -> RgbaImage {
        RgbaImage::from_fn(64, 32, |x, y| image::Rgba([x as u8, y as u8, value, 255]))
    }

    fn fixture(
        ctx: &egui::Context,
        destination: &std::path::Path,
        action: AfterCapture,
    ) -> Framecraft {
        let mut app = Framecraft::with_context(ctx, true);
        app.fresh_capture = true;
        app.quick_settings = QuickCaptureSettings {
            destination: destination.into(),
            setup_complete: true,
            after_capture: action,
            show_thumbnail: false,
            ..Default::default()
        };
        app
    }

    // Drive the normal worker result boundary in an egui frame, including the
    // native presentation commands an unsolicited editor activation would send.
    fn complete(ctx: &egui::Context, app: &mut Framecraft, outcome: Outcome) {
        let (tx, rx) = mpsc::channel();
        app.job = Some(Job { rx });
        tx.send(Ok(outcome)).unwrap();
        let output = ctx.run(Default::default(), |ctx| app.poll(ctx));
        let commands = &output.viewport_output[&egui::ViewportId::ROOT].commands;
        assert!(!commands.iter().any(|c| matches!(
            c,
            egui::ViewportCommand::Focus
                | egui::ViewportCommand::Minimized(false)
                | egui::ViewportCommand::Visible(true)
        )));
    }

    #[test]
    fn a_dirty_document_is_guarded_only_for_capture_modes_that_will_replace_it() {
        let root = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        for mode in [
            AfterCapture::Edit,
            AfterCapture::Save,
            AfterCapture::CopyAndReturn,
            AfterCapture::SaveAndReturn,
        ] {
            let mut app = fixture(&ctx, root.path(), mode);
            // An injected permission result and no native window keep this
            // entry-point test independent of system permissions or live pixels.
            app.capture_with_permission(&ctx, CaptureMode::Area, || {
                capture::PermissionStatus::Granted
            });
            assert_eq!(
                matches!(app.pending, Some(Pending::Capture(CaptureMode::Area))),
                !mode.stays_in_source(),
                "{mode:?}"
            );
            assert!(app.fresh_capture);
            assert!(app.job.is_none());
        }
    }

    #[test]
    fn direct_copy_preserves_the_editor_and_only_the_latest_raw_selection_is_retained() {
        let root = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let mut app = fixture(&ctx, root.path(), AfterCapture::CopyAndReturn);
        let revision = app.revision;
        let name = app.name.clone();
        for i in [1, 2] {
            let pixels = capture_pixels(i);
            let out = finish_capture(
                ThumbnailPlacement::Unavailable,
                Some(pixels.clone()),
                &app.quick_settings,
                &|| false,
                |copied, _| {
                    assert_eq!(copied, &pixels);
                    Ok(true)
                },
            );
            complete(&ctx, &mut app, out);
            assert_eq!(*app.last_capture.as_ref().unwrap().pixels, pixels);
            assert!(app.status.contains("Copied"));
        }
        let fail_copy = |_: &RgbaImage, _: &dyn Fn() -> bool| -> Result<bool, String> {
            panic!("cancelled output published")
        };
        let cancelled = finish_capture(
            ThumbnailPlacement::Unavailable,
            Some(capture_pixels(3)),
            &app.quick_settings,
            &|| true,
            fail_copy,
        );
        complete(&ctx, &mut app, cancelled);
        let escape = finish_capture(
            ThumbnailPlacement::Unavailable,
            None,
            &app.quick_settings,
            &|| false,
            fail_copy,
        );
        complete(&ctx, &mut app, escape);
        assert_eq!(
            *app.last_capture.as_ref().unwrap().pixels,
            capture_pixels(2)
        );
        assert!(app.fresh_capture);
        assert_eq!(app.revision, revision);
        assert_eq!(app.name, name);
        assert!(app.pending.is_none());
        assert_eq!(root.path().read_dir().unwrap().count(), 0);
        app.edit_last_capture(&ctx);
        assert!(matches!(app.pending, Some(Pending::EditLastCapture)));
        assert_eq!(app.name, name);
        // Cancel the unsaved-document decision, then clear the RAM copy.
        app.pending = None;
        app.clear_last_capture();
        assert!(app.last_capture.is_none());
        assert_eq!(app.name, name);
        assert_eq!(app.revision, revision);
    }

    #[test]
    fn direct_save_is_exact_and_recovers_a_failed_publication_without_recapture() {
        let root = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let mut app = fixture(
            &ctx,
            &root.path().join("missing"),
            AfterCapture::SaveAndReturn,
        );
        let revision = app.revision;
        let pixels = capture_pixels(17);
        let done = finish_capture(
            ThumbnailPlacement::Unavailable,
            Some(pixels.clone()),
            &app.quick_settings,
            &|| false,
            |_, _| panic!("save wrote the clipboard"),
        );
        complete(&ctx, &mut app, done);
        assert_eq!(
            app.last_capture
                .as_ref()
                .unwrap()
                .status()
                .status_item_title(),
            Some("Save failed")
        );
        assert!(app.error && app.status.contains("Save failed"));
        app.quick_settings.destination = root.path().into();
        app.retry_last_output(&ctx);
        assert!(app.job.is_some());
        app.capture_with_permission(&ctx, CaptureMode::Window, || {
            panic!("a second capture entered an in-flight retry")
        });
        let deadline = Instant::now() + Duration::from_secs(5);
        while app.job.is_some() {
            assert!(Instant::now() < deadline, "publishing fixture timed out");
            app.poll(&ctx);
            std::thread::sleep(Duration::from_millis(1));
        }
        let Receipt::Saved(path) = &app.last_capture.as_ref().unwrap().receipt else {
            panic!("expected saved file")
        };
        let path = path.clone();
        assert_eq!(
            app.last_capture
                .as_ref()
                .unwrap()
                .status()
                .status_item_title(),
            None
        );
        assert_eq!(image::open(&path).unwrap().to_rgba8(), pixels);
        assert_eq!(root.path().read_dir().unwrap().count(), 1);
        assert_eq!(app.revision, revision);
        assert!(app.fresh_capture);
        app.edit_last_capture(&ctx);
        let pending = app.pending.take().unwrap();
        app.execute(&ctx, pending);
        assert_eq!(app.doc.as_ref().unwrap().source, pixels);
        assert!(!app.fresh_capture);
        assert_eq!(app.name, path.file_name().unwrap().to_string_lossy());
        app.clear_last_capture();
        assert!(path.is_file());
    }

    #[test]
    fn direct_clipboard_failure_retains_recoverable_pixels_and_cancellation_keeps_the_previous_receipt(
    ) {
        let root = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let mut app = fixture(&ctx, root.path(), AfterCapture::CopyAndReturn);
        let pixels = capture_pixels(22);
        let failed = finish_capture(
            ThumbnailPlacement::Unavailable,
            Some(pixels.clone()),
            &app.quick_settings,
            &|| false,
            |_, _| Err("pasteboard busy".into()),
        );
        complete(&ctx, &mut app, failed);
        assert!(app.error && app.status.contains("pasteboard busy"));
        assert_eq!(
            app.last_capture
                .as_ref()
                .unwrap()
                .status()
                .status_item_title(),
            Some("Copy failed")
        );
        let cancelled_during_encoding = finish_capture(
            ThumbnailPlacement::Unavailable,
            Some(capture_pixels(23)),
            &app.quick_settings,
            &|| false,
            |_, _| Ok(false),
        );
        complete(&ctx, &mut app, cancelled_during_encoding);
        assert!(app.last_capture.as_ref().unwrap().failed());
        app.edit_last_capture(&ctx);
        let pending = app.pending.take().unwrap();
        app.execute(&ctx, pending);
        assert_eq!(app.doc.as_ref().unwrap().source, pixels);
        assert!(app.fresh_capture);
        assert_eq!(root.path().read_dir().unwrap().count(), 0);
    }

    #[test]
    fn a_new_capture_opened_in_the_editor_retires_obsolete_raw_output() {
        let root = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let mut app = fixture(&ctx, root.path(), AfterCapture::CopyAndReturn);
        let first = finish_capture(
            ThumbnailPlacement::Unavailable,
            Some(capture_pixels(1)),
            &app.quick_settings,
            &|| false,
            |_, _| Ok(true),
        );
        complete(&ctx, &mut app, first);
        let (tx, rx) = mpsc::channel();
        app.job = Some(Job { rx });
        tx.send(Ok(Outcome::Captured(Some(capture_pixels(2)))))
            .unwrap();
        app.poll(&ctx);
        assert!(
            app.last_capture.is_none(),
            "Edit Last Capture must not point at an older raw image"
        );
        assert_eq!(app.doc.as_ref().unwrap().source, capture_pixels(2));
    }
}
