use crate::session_ui::{Action as SessionAction, ReviewFrame, SessionView};
use crate::setup_ui::{SetupAction, SetupDraft};
use crate::ui_icons::{self, Icon};
use eframe::egui::{self, Color32, Pos2, RichText, Sense, Stroke, Vec2};
use framecraft::desktop::{DesktopAction, DesktopControls};
use framecraft::quick_capture::{self, AfterCapture, QuickCaptureSettings};
use framecraft::{
    capture::{self, CaptureMode},
    editor::{self, Annotation, Document, Point, Rect, Style, Theme},
};
use framecraft::{
    library::CaptureVariant,
    output::{self, PackFrame, Snapshot},
    session::CaptureHandle,
    settings::ShortcutSettings,
};
use std::{
    borrow::Cow,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    time::{Duration, Instant},
};

const ACCENT: Color32 = Color32::from_rgb(39, 90, 223);
const MUTED: Color32 = Color32::from_rgb(91, 99, 112);
const INK: Color32 = Color32::from_rgb(30, 35, 43);
const SURFACE: Color32 = Color32::from_rgb(246, 248, 251);
const BORDER: Color32 = Color32::from_rgb(218, 222, 229);

#[derive(Clone, Copy, PartialEq)]
enum Tool {
    Select,
    Arrow,
    Box,
    Redact,
    Crop,
    Highlight,
    Pixelate,
    Pen,
    Text,
    Step,
}
#[derive(Clone, Copy, PartialEq)]
enum Page {
    Session,
    Editor,
}
#[derive(Clone)]
struct Edits {
    crop: Option<Rect>,
    annotations: Vec<Annotation>,
    style: Style,
}
impl Edits {
    fn read(d: &Document) -> Self {
        Self {
            crop: d.crop,
            annotations: d.annotations.clone(),
            style: d.style.clone(),
        }
    }
    fn apply(self, d: &mut Document) {
        d.crop = self.crop;
        d.annotations = self.annotations;
        d.style = self.style;
    }
}
enum Outcome {
    Loaded(image::RgbaImage, String),
    Captured(Option<image::RgbaImage>),
    AutoSaved(image::RgbaImage, Result<PathBuf, String>),
    Exported(PathBuf, u64),
    DragReady(output::DragFile, Option<u64>, image::RgbaImage, String),
    Revealed,
    PackPrepared(Arc<Vec<PackFrame>>, Vec<PathBuf>),
    PackExported(PathBuf),
    Copied(arboard::Clipboard),
    RegionSource(image::RgbaImage, u32, u64),
    SessionLoaded(image::RgbaImage, PathBuf, CaptureHandle),
}
type RenderResult = Result<(image::RgbaImage, image::RgbaImage), String>;
type RenderJob = (u64, mpsc::Receiver<RenderResult>);

struct Job {
    rx: mpsc::Receiver<Result<Outcome, String>>,
}

enum Pending {
    Open(PathBuf),
    Capture(CaptureMode),
    SelectDisplay(u32),
    Session(CaptureHandle, CaptureVariant),
    Quit,
    CloseScreenshot,
}
struct OcrJob {
    revision: u64,
    cancelled: Arc<AtomicBool>,
    rx: mpsc::Receiver<Result<String, String>>,
}
struct PackDraft {
    frames: Arc<Vec<PackFrame>>,
    roots: Vec<PathBuf>,
    title: String,
    name: String,
    previews: Vec<egui::TextureHandle>,
}

struct TextDraft {
    index: Option<usize>,
    position: Point,
    text: String,
    size: f32,
    colour: Color32,
    background: Option<[u8; 4]>,
    focus: bool,
}
impl TextDraft {
    fn validation_error(&self) -> Option<&'static str> {
        if self.text.trim().is_empty() {
            Some("Type some text before adding the label.")
        } else if self.text.chars().count() > editor::MAX_TEXT_CHARS {
            Some("Keep the label to 2,048 characters or fewer.")
        } else if self.text.split('\n').count() > editor::MAX_TEXT_LINES {
            Some("Keep the label to 32 lines or fewer.")
        } else if self.text.chars().any(|c| c.is_control() && c != '\n') {
            Some("Remove tabs and other control characters. Line breaks are supported.")
        } else if !self.size.is_finite() || !(8.0..=128.0).contains(&self.size) {
            Some("Choose a text size from 8 to 128 pixels.")
        } else {
            None
        }
    }
}

pub struct Framecraft {
    page: Page,
    session: SessionView,
    selecting_display: Option<u32>,
    source_epoch: u64,
    editing_capture: Option<PathBuf>,
    editing_handle: Option<CaptureHandle>,
    selected: Option<usize>,
    moving: Option<Annotation>,
    text_draft: Option<TextDraft>,
    text_size: f32,
    step_number: u32,
    flat_texture: Option<egui::TextureHandle>,
    fresh_capture: bool,
    highlight_colour: [u8; 3],
    pixelate_block: u32,
    block_input: String,
    block_error: Option<String>,
    colour_dialog: Option<([u8; 3], [u8; 3])>,
    colour_before: Option<Edits>,
    colour_was_saved: bool,
    colour_return: Option<egui::Id>,
    property_before: Option<Edits>,
    property_block_before: Option<u32>,
    property_was_saved: bool,
    gesture_before: Option<Edits>,
    gesture_was_saved: bool,
    capture_cancel: Option<Arc<AtomicBool>>,
    screen_access_open: bool,
    screen_access_status: capture::PermissionStatus,
    saved_revision: u64,
    rendered_revision: Option<u64>,
    pending: Option<Pending>,
    continue_after_save: bool,
    allow_close: bool,
    closing: bool,
    shortcuts_registered: bool,
    ocr_job: Option<OcrJob>,
    ocr_text: Option<(u64, String)>,
    pack: Option<PackDraft>,
    pack_cancel: Option<Arc<AtomicBool>>,
    pack_progress: Option<mpsc::Receiver<usize>>,
    drag_files: Vec<output::DragFile>,
    drag_ready: Option<usize>,
    drag_preview: Option<egui::TextureHandle>,
    drag_label: String,
    desktop: Option<DesktopControls>,
    desktop_attempted: bool,
    shortcuts_settings: ShortcutSettings,
    quick_settings: QuickCaptureSettings,
    setup: Option<SetupDraft>,
    settings_open: bool,
    settings_error: Option<String>,
    shortcut_draft: Option<ShortcutSettings>,
    freehand: Vec<Point>,
    doc: Option<Document>,
    name: String,
    source_texture: Option<egui::TextureHandle>,
    preview_texture: Option<egui::TextureHandle>,
    tool: Tool,
    preview: bool,
    drag: Option<Point>,
    colour: Color32,
    stroke: f32,
    undo: Vec<Edits>,
    redo: Vec<Edits>,
    style_before: Option<Edits>,
    status: String,
    error: bool,
    job: Option<Job>,
    revision: u64,
    dirty: Option<Instant>,
    render_job: Option<RenderJob>,
    clipboard: Option<arboard::Clipboard>,
}

impl Framecraft {
    pub fn new(cc: &eframe::CreationContext<'_>, demo: bool) -> Self {
        let mut app = Self::with_context(&cc.egui_ctx, demo);
        match QuickCaptureSettings::load(&app.shortcuts_settings) {
            Ok(settings) => app.quick_settings = settings,
            Err(error) => {
                app.notify(&error, true);
                app.setup = Some(SetupDraft::new(app.quick_settings.clone()));
                app.setup.as_mut().unwrap().error = Some(error);
            }
        }
        if !demo && !app.quick_settings.setup_complete && app.setup.is_none() {
            app.setup = Some(SetupDraft::new(app.quick_settings.clone()));
        }
        app
    }

    fn with_context(ctx: &egui::Context, demo: bool) -> Self {
        install_mac_font(ctx);
        let mut visuals = egui::Visuals::light();
        visuals.panel_fill = SURFACE;
        visuals.window_fill = Color32::WHITE;
        visuals.override_text_color = Some(INK);
        visuals.selection.bg_fill = Color32::from_rgb(218, 234, 255);
        visuals.selection.stroke = Stroke::new(1.0_f32, ACCENT);
        for widget in [
            &mut visuals.widgets.inactive,
            &mut visuals.widgets.hovered,
            &mut visuals.widgets.active,
        ] {
            widget.corner_radius = egui::CornerRadius::same(8);
        }
        visuals.widgets.inactive.bg_fill = Color32::WHITE;
        visuals.widgets.inactive.weak_bg_fill = Color32::WHITE;
        visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, BORDER);
        visuals.widgets.hovered.bg_fill = Color32::from_rgb(232, 240, 252);
        visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(232, 240, 252);
        ctx.set_visuals(visuals);
        ctx.style_mut(|s| {
            s.spacing.item_spacing = Vec2::new(10.0, 8.0);
            s.spacing.button_padding = Vec2::new(12.0, 7.0);
            s.spacing.interact_size.y = 30.0;
            s.text_styles
                .insert(egui::TextStyle::Body, egui::FontId::proportional(14.0));
            s.text_styles
                .insert(egui::TextStyle::Button, egui::FontId::proportional(14.0));
            s.text_styles
                .insert(egui::TextStyle::Small, egui::FontId::proportional(12.0));
            s.text_styles
                .insert(egui::TextStyle::Heading, egui::FontId::proportional(22.0));
        });
        let mut app = Self {
            page: Page::Editor,
            session: SessionView::new(),
            selecting_display: None,
            source_epoch: 0,
            editing_capture: None,
            editing_handle: None,
            selected: None,
            moving: None,
            text_draft: None,
            text_size: 28.0,
            step_number: 1,
            flat_texture: None,
            fresh_capture: false,
            highlight_colour: [255, 212, 71],
            pixelate_block: 12,
            block_input: "12".into(),
            block_error: None,
            colour_dialog: None,
            colour_before: None,
            colour_was_saved: false,
            colour_return: None,
            property_before: None,
            property_block_before: None,
            property_was_saved: false,
            gesture_before: None,
            gesture_was_saved: false,
            capture_cancel: None,
            screen_access_open: false,
            screen_access_status: capture::PermissionStatus::Unknown,
            saved_revision: 0,
            rendered_revision: None,
            pending: None,
            continue_after_save: false,
            allow_close: false,
            closing: false,
            shortcuts_registered: false,
            ocr_job: None,
            ocr_text: None,
            pack: None,
            pack_cancel: None,
            pack_progress: None,
            drag_files: vec![],
            drag_ready: None,
            drag_preview: None,
            drag_label: String::new(),
            desktop: None,
            desktop_attempted: false,
            shortcuts_settings: ShortcutSettings::load().unwrap_or_default(),
            quick_settings: QuickCaptureSettings::default(),
            setup: None,
            settings_open: false,
            settings_error: None,
            shortcut_draft: None,
            freehand: vec![],
            doc: None,
            name: String::new(),
            source_texture: None,
            preview_texture: None,
            tool: Tool::Select,
            preview: false,
            drag: None,
            colour: ACCENT,
            stroke: 5.0,
            undo: vec![],
            redo: vec![],
            style_before: None,
            status: "Ready to capture. Your images stay on this Mac.".into(),
            error: false,
            job: None,
            revision: 0,
            dirty: None,
            render_job: None,
            clipboard: None,
        };
        if demo {
            app.load(ctx.clone(), editor::demo_image(), "Sample canvas".into());
        }
        app
    }
    fn notify(&mut self, text: impl Into<String>, error: bool) {
        self.status = text.into();
        self.error = error;
    }
    fn changed(&mut self) {
        self.revision += 1;
        self.preview_texture = None;
        self.flat_texture = None;
        self.rendered_revision = None;
        self.ocr_text = None;
        self.drag_ready = None;
        if let Some(job) = &self.ocr_job {
            job.cancelled.store(true, Ordering::Release);
        }
        self.dirty = Some(Instant::now());
    }
    fn checkpoint(&mut self) {
        if let Some(d) = &self.doc {
            self.undo.push(Edits::read(d));
            if self.undo.len() > 40 {
                self.undo.remove(0);
            }
        }
        self.redo.clear();
    }
    fn undo(&mut self, redo: bool) {
        let state = if redo {
            self.redo.pop()
        } else {
            self.undo.pop()
        };
        if let (Some(state), Some(d)) = (state, self.doc.as_mut()) {
            if redo {
                self.undo.push(Edits::read(d));
            } else {
                self.redo.push(Edits::read(d));
            }
            state.apply(d);
            self.selected = None;
            self.moving = None;
            self.changed();
        }
    }
    fn load(&mut self, ctx: egui::Context, pixels: image::RgbaImage, name: String) {
        self.page = Page::Editor;
        self.fresh_capture = false;
        self.colour_dialog = None;
        self.text_draft = None;
        self.property_before = None;
        self.property_block_before = None;
        self.block_error = None;
        self.gesture_before = None;
        self.colour_before = None;
        self.editing_capture = None;
        self.editing_handle = None;
        self.selected = None;
        self.moving = None;
        self.selecting_display = None;
        self.freehand.clear();
        self.flat_texture = None;
        self.source_texture = Some(texture(&ctx, "source", &pixels));
        self.doc = Some(Document {
            source: pixels,
            crop: None,
            annotations: vec![],
            style: Style {
                padding: 0,
                radius: 0,
                shadow: false,
                ..Style::default()
            },
        });
        self.name = name;
        self.undo.clear();
        self.redo.clear();
        self.style_before = None;
        self.preview_texture = None;
        self.preview = false;
        self.drag = None;
        self.changed();
        self.saved_revision = self.revision;
        self.notify(
            "Screenshot ready. Copy it, save it, or add an annotation.",
            false,
        );
    }
    fn start_job<F>(&mut self, ctx: &egui::Context, label: &str, f: F)
    where
        F: FnOnce() -> Result<Outcome, String> + Send + 'static,
    {
        if self.job.is_some() || self.closing {
            return;
        }
        let (tx, rx) = mpsc::channel();
        let repaint = ctx.clone();
        std::thread::spawn(move || {
            let result = f();
            let _ = tx.send(result);
            repaint.request_repaint();
        });
        self.job = Some(Job { rx });
        self.notify(label, false);
    }
    fn open_setup(&mut self) {
        if self.editor_mutation_allowed() && !self.session.active() {
            self.setup = Some(SetupDraft::new(self.quick_settings.clone()));
        }
    }
    fn finish_setup(&mut self, mut candidate: QuickCaptureSettings) -> Result<(), String> {
        candidate.setup_complete = true;
        candidate.validate(&self.shortcuts_settings)?;
        if candidate.after_capture == AfterCapture::Save {
            std::fs::create_dir_all(&candidate.destination)
                .map_err(|e| format!("Could not create save folder: {e}"))?;
            capture::probe_destination(&candidate.destination).map_err(|e| e.to_string())?;
        }
        let controls = self
            .desktop
            .as_mut()
            .ok_or("Menu controls are unavailable. Reopen Framecraft and retry setup.")?;
        let shortcut = candidate
            .shortcut_enabled
            .then_some(candidate.shortcut.as_str());
        controls.configure_quick_capture(shortcut)?;
        if let Err(error) = candidate.save(&self.shortcuts_settings) {
            let previous = (self.quick_settings.setup_complete
                && self.quick_settings.shortcut_enabled)
                .then_some(self.quick_settings.shortcut.as_str());
            return match controls.configure_quick_capture(previous) {
                Ok(()) => Err(error),
                Err(rollback) => Err(format!("{error}. Previous shortcut could not be restored: {rollback}. Quit and reopen Framecraft before retrying.")),
            };
        }
        self.quick_settings = candidate;
        self.notify(
            "Capture setup saved. Use your shortcut or Capture Area to begin.",
            false,
        );
        Ok(())
    }
    fn open_settings(&mut self) {
        if self.setup.is_some() || self.text_draft.is_some() {
            return;
        }
        if !self.settings_open && self.text_draft.is_none() {
            self.shortcut_draft = Some(self.shortcuts_settings.clone());
            self.settings_error = None;
            self.settings_open = true;
        }
    }
    fn capture_available(&self) -> bool {
        self.editor_mutation_allowed() && !self.session.active()
    }
    fn capture(&mut self, ctx: &egui::Context, mode: CaptureMode) {
        self.capture_with_permission(ctx, mode, capture::permission_status);
    }
    fn capture_with_permission(
        &mut self,
        ctx: &egui::Context,
        mode: CaptureMode,
        status: impl FnOnce() -> capture::PermissionStatus,
    ) {
        if !self.capture_available() {
            self.notify(
                if self.session.active() {
                    "Stop the capture session before taking a new screenshot."
                } else {
                    "Finish the current operation or close the dialog before capturing."
                },
                true,
            );
            return;
        }
        if cfg!(target_os = "macos") && !self.check_screen_access(status()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            return;
        }
        self.request(ctx, Pending::Capture(mode));
    }
    fn check_screen_access(&mut self, status: capture::PermissionStatus) -> bool {
        self.screen_access_status = status;
        if let Err(message) = capture::require_permission(status) {
            self.screen_access_open = true;
            self.notify(message, true);
            return false;
        }
        true
    }
    fn capture_now(&mut self, ctx: &egui::Context, mode: CaptureMode) {
        // Recheck after an unsaved-edit prompt: a session may have started meanwhile.
        if self.session.active() {
            self.notify(
                "Stop the capture session before taking a new screenshot.",
                true,
            );
            return;
        }
        if self.job.is_some() {
            return;
        }
        if !cfg!(target_os = "macos") {
            self.notify(
                "Screen capture requires macOS. Import an image to use the editor here.",
                true,
            );
            return;
        }
        if !self.check_screen_access(capture::permission_status()) {
            return;
        }
        let cancel = Arc::new(AtomicBool::new(false));
        self.capture_cancel = Some(cancel.clone());
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        let restore = ctx.clone();
        let preferences = self.quick_settings.clone();
        self.start_job(
            ctx,
            "Select an area or window. Escape cancels.",
            move || {
                // Give the compositor a moment to remove our editor from the desktop.
                std::thread::sleep(Duration::from_millis(350));
                let result = capture::capture_cancellable(mode, &|| cancel.load(Ordering::Acquire))
                    .map(|pixels| {
                        if cancel.load(Ordering::Acquire) {
                            return Outcome::Captured(None);
                        }
                        match pixels {
                            Some(pixels)
                                if preferences.setup_complete
                                    && preferences.after_capture == AfterCapture::Save =>
                            {
                                match quick_capture::save_capture_cancellable(
                                    &pixels,
                                    &preferences,
                                    &|| cancel.load(Ordering::Acquire),
                                ) {
                                    Ok(Some(path)) => Outcome::AutoSaved(pixels, Ok(path)),
                                    Ok(None) => Outcome::Captured(None),
                                    Err(error) => Outcome::AutoSaved(pixels, Err(error)),
                                }
                            }
                            pixels => Outcome::Captured(pixels),
                        }
                    });
                restore.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                restore.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                restore.send_viewport_cmd(egui::ViewportCommand::Focus);
                restore.request_repaint();
                result
            },
        );
    }
    fn open(&mut self, ctx: &egui::Context) {
        if self.job.is_some() {
            return;
        }
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Images", &["png", "jpg", "jpeg"])
            .pick_file()
        {
            self.open_path(ctx, path);
        }
    }
    fn open_path(&mut self, ctx: &egui::Context, path: PathBuf) {
        self.request(ctx, Pending::Open(path));
    }
    fn open_path_now(&mut self, ctx: &egui::Context, path: PathBuf) {
        self.session.pause();
        self.start_job(ctx, "Opening image...", move || {
            let name = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            editor::load_image(&path).map(|image| Outcome::Loaded(image, name))
        });
    }
    fn select_session_source(&mut self, ctx: &egui::Context, display: u32) {
        self.request(ctx, Pending::SelectDisplay(display));
    }
    fn select_session_source_now(&mut self, ctx: &egui::Context, display: u32) {
        if self.session.active() {
            self.notify("Stop the session before changing its source.", true);
            return;
        }
        if self.job.is_some() {
            return;
        }
        self.session.pause();
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        let restore = ctx.clone();
        self.source_epoch = self.source_epoch.wrapping_add(1);
        let epoch = self.source_epoch;
        self.start_job(
            ctx,
            "Preparing the display. Drag around the shared content.",
            move || {
                std::thread::sleep(Duration::from_millis(350));
                let result = capture::capture_display(display)
                    .map(|im| Outcome::RegionSource(im, display, epoch));
                restore.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                restore.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                restore.send_viewport_cmd(egui::ViewportCommand::Focus);
                restore.request_repaint();
                result
            },
        );
    }
    fn use_session_area(&mut self) {
        if let (Some(display), Some(doc)) = (self.selecting_display, self.doc.as_ref()) {
            let region = doc.crop.unwrap_or(Rect {
                x: 0,
                y: 0,
                width: doc.source.width(),
                height: doc.source.height(),
            });
            let result = self.session.set_target(capture::CaptureTarget {
                display,
                region,
                source_width: doc.source.width(),
                source_height: doc.source.height(),
            });
            if let Err(error) = result {
                self.notify(error, true);
                return;
            }
            self.saved_revision = self.revision;
            self.selecting_display = None;
            self.page = Page::Session;
        }
    }
    fn save_edited_copy(&mut self, ctx: &egui::Context) {
        if !self.handoff_ready() {
            self.notify("Wait for the current image preview before saving.", false);
            return;
        }
        if self.job.is_some() {
            return;
        }
        if let (Some(path), Some(doc)) = (self.editing_capture.clone(), self.doc.clone()) {
            let revision = self.revision;
            let handle = self.editing_handle.clone();
            self.start_job(
                ctx,
                "Saving edited copy in the session folder...",
                move || {
                    let output = Snapshot::current(&doc)?;
                    let original = handle
                        .as_ref()
                        .map(|h| h.snapshot().map(|c| c.path))
                        .transpose()?
                        .unwrap_or(path);
                    let saved = save_edit(&original, &output.pixels)?;
                    if let Some(handle) = handle {
                        handle.register_edit(&saved).map_err(|e| {
                            format!(
                                "PNG saved at {} but review metadata could not be updated: {e}",
                                saved.display()
                            )
                        })?;
                    }
                    Ok(Outcome::Exported(saved, revision))
                },
            );
        }
    }
    fn handoff_ready(&self) -> bool {
        self.doc.is_some() && self.job.is_none() && self.rendered_revision == Some(self.revision)
    }
    fn export(&mut self, ctx: &egui::Context, copy: bool) {
        if !self.handoff_ready() {
            self.notify(
                "Wait for the current image preview before copying or saving.",
                false,
            );
            return;
        }
        if self.job.is_some() || self.doc.is_none() {
            return;
        }
        let path = if copy {
            None
        } else {
            match rfd::FileDialog::new()
                .set_directory(&self.quick_settings.destination)
                .set_file_name(format!(
                    "{}-edited.png",
                    self.quick_settings.filename_prefix
                ))
                .add_filter("PNG", &["png"])
                .save_file()
            {
                // Preserve the exact path approved by the save dialog.
                // The file is encoded as PNG regardless of its chosen suffix.
                Some(path) => Some(path),
                None => {
                    self.continue_after_save = false;
                    return;
                }
            }
        };
        let doc = self.doc.as_ref().unwrap().clone();
        let revision = self.revision;
        self.start_job(
            ctx,
            if copy {
                "Rendering to clipboard..."
            } else {
                "Rendering PNG..."
            },
            move || {
                let snapshot = Snapshot::current(&doc)?;
                if let Some(path) = path {
                    snapshot.save(&path)?;
                    Ok(Outcome::Exported(path, revision))
                } else {
                    copy_snapshot(snapshot).map(Outcome::Copied)
                }
            },
        );
    }
    fn poll(&mut self, ctx: &egui::Context) {
        let completed = self.job.as_ref().and_then(|j| match j.rx.try_recv() {
            Ok(r) => Some(r),
            Err(mpsc::TryRecvError::Disconnected) => {
                Some(Err("The operation stopped unexpectedly. Try again.".into()))
            }
            _ => None,
        });
        if let Some(result) = completed {
            self.job = None;
            self.capture_cancel = None;
            match result {
                Ok(Outcome::RegionSource(pixels, display, epoch)) => {
                    if epoch != self.source_epoch || self.session.active() {
                        self.notify(
                            "The session started. Pending source selection was discarded.",
                            true,
                        );
                        return;
                    }
                    self.load(
                        ctx.clone(),
                        pixels,
                        format!("Select shared area on display {display}"),
                    );
                    self.selecting_display = Some(display);
                    self.tool = Tool::Crop;
                    if let Some(d) = self.doc.as_mut() {
                        d.style.padding = 0;
                        d.style.radius = 0;
                        d.style.shadow = false;
                    }
                    self.changed();
                }
                Ok(Outcome::SessionLoaded(pixels, path, handle)) => {
                    let name = path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned();
                    self.load(ctx.clone(), pixels, name);
                    self.editing_capture = Some(path);
                    self.editing_handle = Some(handle);
                    if let Some(d) = self.doc.as_mut() {
                        d.style.padding = 0;
                        d.style.radius = 0;
                        d.style.shadow = false;
                    }
                    self.changed();
                    self.saved_revision = self.revision;
                }
                Ok(Outcome::Loaded(pixels, name)) => self.load(ctx.clone(), pixels, name),
                Ok(Outcome::Captured(Some(pixels))) => {
                    self.load(ctx.clone(), pixels, "New capture".into());
                    self.fresh_capture = true;
                }
                Ok(Outcome::AutoSaved(pixels, saved)) => {
                    self.load(ctx.clone(), pixels, "New capture".into());
                    match saved {
                        Ok(path) => {
                            self.name = path
                                .file_name()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .into_owned();
                            self.notify(
                                format!(
                                    "Saved {}. Edits can be saved as a separate copy.",
                                    path.display()
                                ),
                                false,
                            );
                        }
                        Err(error) => {
                            self.fresh_capture = true;
                            self.notify(format!("Automatic save failed: {error} Your screenshot is open. Choose Save PNG to keep it."), true);
                        }
                    }
                }
                Ok(Outcome::Captured(None)) => self.notify("Capture cancelled.", false),
                Ok(Outcome::Exported(path, revision)) => {
                    if revision == self.revision {
                        self.saved_revision = revision;
                        self.fresh_capture = false;
                    }
                    self.session.refresh_variants();
                    self.notify(format!("Saved {}", path.display()), false);
                    if self.continue_after_save && revision == self.revision {
                        self.continue_after_save = false;
                        if let Some(action) = self.pending.take() {
                            self.execute(ctx, action);
                        }
                    }
                }
                Ok(Outcome::DragReady(file, revision, preview, label)) => {
                    if revision.is_some_and(|revision| revision != self.revision) {
                        self.notify("The image changed. Prepare the drag PNG again.", true);
                        return;
                    }
                    if self.drag_files.len() >= 32 {
                        self.notify("32 drag files are retained for delayed drops. Quit and reopen Framecraft before preparing more.",true);
                        return;
                    }
                    self.drag_preview = Some(texture(ctx, "drag-preview", &preview));
                    self.drag_label = label;
                    self.drag_files.push(file);
                    self.drag_ready = Some(self.drag_files.len() - 1);
                    self.notify(
                        "PNG prepared. Drag the file button into another app.",
                        false,
                    );
                }
                Ok(Outcome::Revealed) => self.notify("Revealed in Finder.", false),
                Ok(Outcome::PackPrepared(frames, roots)) => {
                    let previews = frames
                        .iter()
                        .enumerate()
                        .map(|(i, f)| {
                            texture(
                                ctx,
                                &format!("pack-{i}"),
                                &image::imageops::thumbnail(&f.snapshot.pixels, 320, 180),
                            )
                        })
                        .collect();
                    self.notify(
                        "Review the selected variants, captions and order before export.",
                        false,
                    );
                    self.pack = Some(PackDraft {
                        frames,
                        roots,
                        title: "Evidence pack".into(),
                        name: "Framecraft-pack".into(),
                        previews,
                    });
                }
                Ok(Outcome::PackExported(path)) => {
                    self.pack = None;
                    self.pack_cancel = None;
                    self.pack_progress = None;
                    self.notify(format!("Pack saved to {}", path.display()), false);
                }
                Ok(Outcome::Copied(clipboard)) => {
                    self.clipboard = Some(clipboard);
                    self.notify("Copied. Redactions are baked into the pixels.", false);
                }
                Err(error) => {
                    self.continue_after_save = false;
                    self.pack_cancel = None;
                    self.pack_progress = None;
                    self.notify(error, true);
                }
            }
        }
        let rendered = self
            .render_job
            .as_ref()
            .and_then(|(rev, rx)| match rx.try_recv() {
                Ok(r) => Some((*rev, r)),
                Err(mpsc::TryRecvError::Disconnected) => {
                    Some((*rev, Err("Preview worker stopped.".into())))
                }
                _ => None,
            });
        if let Some((rev, result)) = rendered {
            self.render_job = None;
            if rev == self.revision {
                match result {
                    Ok((pixels, flat)) => {
                        self.rendered_revision = Some(rev);
                        self.preview_texture = Some(texture(ctx, "preview", &pixels));
                        self.flat_texture = Some(texture(ctx, "flat-source", &flat));
                    }
                    Err(e) => self.notify(e, true),
                }
            }
        }
        if self.render_job.is_none()
            && self
                .dirty
                .is_some_and(|t| t.elapsed() > Duration::from_millis(140))
        {
            if let Some(doc) = self.doc.clone() {
                let (tx, rx) = mpsc::channel();
                let repaint = ctx.clone();
                let rev = self.revision;
                self.render_job = Some((rev, rx));
                self.dirty = None;
                std::thread::spawn(move || {
                    let result = editor::render(&doc).and_then(|output| {
                        editor::render_edit_source(&doc).map(|flat| (output, flat))
                    });
                    let _ = tx.send(result);
                    repaint.request_repaint();
                });
            }
        }
        if self.dirty.is_some() || self.job.is_some() || self.render_job.is_some() {
            ctx.request_repaint_after(Duration::from_millis(50));
        }
    }
    fn editor_mutation_allowed(&self) -> bool {
        self.job.is_none()
            && self.pending.is_none()
            && self.pack.is_none()
            && !self.closing
            && !self.settings_open
            && self.setup.is_none()
            && self.colour_dialog.is_none()
            && self.text_draft.is_none()
            && self.ocr_job.is_none()
            && self.ocr_text.is_none()
            && self.drag_ready.is_none()
    }
    fn shortcuts(&mut self, ctx: &egui::Context) {
        if self.setup.is_some() {
            return;
        }
        if !self.closing
            && self.pending.is_none()
            && self.colour_dialog.is_none()
            && self.text_draft.is_none()
            && self.job.is_none()
            && !ctx.wants_keyboard_input()
        {
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Comma)) {
                self.open_setup();
            }
            if !self.settings_open
                && ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::O))
            {
                self.open(ctx);
                return;
            }
        }
        if !self.editor_mutation_allowed()
            || self.page != Page::Editor
            || self.pending.is_some()
            || self.pack.is_some()
            || ctx.wants_keyboard_input()
        {
            return;
        }
        let cmd = egui::Modifiers::COMMAND;
        let shift = cmd | egui::Modifiers::SHIFT;
        let capture_keys = cmd | egui::Modifiers::ALT;
        let take = |modifiers, key| ctx.input_mut(|i| i.consume_key(modifiers, key));
        if !cfg!(target_os = "macos") && take(capture_keys, egui::Key::Num4) {
            self.capture(ctx, CaptureMode::Area);
        } else if !cfg!(target_os = "macos") && take(capture_keys, egui::Key::Num5) {
            self.capture(ctx, CaptureMode::Window);
        } else if !cfg!(target_os = "macos") && take(capture_keys, egui::Key::Num3) {
            self.capture(ctx, CaptureMode::Screen);
        } else if take(cmd, egui::Key::O) {
            self.open(ctx);
        } else if take(cmd, egui::Key::S) {
            if self.editing_capture.is_some() {
                self.save_edited_copy(ctx);
            } else {
                self.export(ctx, false);
            }
        } else if take(shift, egui::Key::C) {
            self.export(ctx, true);
        } else if take(shift, egui::Key::Z) {
            self.undo(true);
        } else if take(cmd, egui::Key::Z) {
            self.undo(false);
        }
        if take(egui::Modifiers::NONE, egui::Key::Delete)
            || take(egui::Modifiers::NONE, egui::Key::Backspace)
        {
            self.delete_selected();
        }
        for (key, delta) in [
            (egui::Key::ArrowLeft, Point { x: -1.0, y: 0.0 }),
            (egui::Key::ArrowRight, Point { x: 1.0, y: 0.0 }),
            (egui::Key::ArrowUp, Point { x: 0.0, y: -1.0 }),
            (egui::Key::ArrowDown, Point { x: 0.0, y: 1.0 }),
        ] {
            if take(egui::Modifiers::NONE, key) {
                self.move_selected(delta);
            }
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            self.cancel_tool();
        }
    }
    fn close_screenshot(&mut self) {
        self.page = if self.editing_capture.is_some() {
            Page::Session
        } else {
            Page::Editor
        };
        self.doc = None;
        self.source_texture = None;
        self.flat_texture = None;
        self.preview_texture = None;
        self.editing_capture = None;
        self.editing_handle = None;
        self.selected = None;
        self.moving = None;
        self.drag = None;
        self.freehand.clear();
        self.undo.clear();
        self.redo.clear();
        self.tool = Tool::Select;
        self.fresh_capture = false;
        self.colour_dialog = None;
        self.text_draft = None;
        self.property_before = None;
        self.property_block_before = None;
        self.block_error = None;
        self.gesture_before = None;
        self.colour_before = None;
        self.style_before = None;
        self.changed();
        self.notify(
            "Screenshot closed. Framecraft is ready for another capture.",
            false,
        );
    }
    fn begin_move(&mut self) {
        self.gesture_before = self.doc.as_ref().map(Edits::read);
        self.gesture_was_saved = self.saved_revision == self.revision;
    }
    fn finish_move(&mut self) {
        let changed = self
            .moving
            .as_ref()
            .zip(
                self.selected
                    .and_then(|i| self.doc.as_ref()?.annotations.get(i)),
            )
            .is_some_and(|(a, b)| a != b);
        if changed {
            if let Some(before) = self.gesture_before.take() {
                self.push_edit(before);
            }
        } else {
            self.gesture_before = None;
            if self.gesture_was_saved {
                self.saved_revision = self.revision;
            }
        }
        self.moving = None;
    }
    fn cancel_tool(&mut self) {
        if self.text_draft.is_some() {
            self.cancel_text_draft();
            return;
        }
        if let Some(before) = self.property_block_before.take() {
            if let Some(edits) = self.property_before.take() {
                if let Some(doc) = &mut self.doc {
                    edits.apply(doc);
                }
                self.changed();
                if self.property_was_saved {
                    self.saved_revision = self.revision;
                }
            } else {
                self.pixelate_block = before;
            }
            return;
        }
        if let (Some(mark), Some(index)) = (self.moving.take(), self.selected) {
            if let Some(doc) = &mut self.doc {
                doc.annotations[index] = mark;
            }
            self.gesture_before = None;
            self.changed();
            if self.gesture_was_saved {
                self.saved_revision = self.revision;
            }
        }
        self.drag = None;
        self.freehand.clear();
        self.tool = Tool::Select;
        self.notify("Tool cancelled. Completed marks are retained.", false);
    }
    fn push_edit(&mut self, before: Edits) {
        self.undo.push(before);
        if self.undo.len() > 40 {
            self.undo.remove(0);
        }
        self.redo.clear();
    }
    fn begin_colour(&mut self) {
        let rgb = self.highlight_value();
        self.colour_before = self.doc.as_ref().map(Edits::read);
        self.colour_was_saved = self.saved_revision == self.revision;
        self.colour_dialog = Some((rgb, rgb));
    }
    fn update_colour(&mut self, original: [u8; 3], draft: [u8; 3], finish: bool, commit: bool) {
        if finish {
            self.colour_dialog = None;
        }
        let value = if finish && !commit { original } else { draft };
        if self.highlight_value() != value {
            if let Some(Annotation::Highlight { colour, .. }) = self
                .selected
                .and_then(|i| self.doc.as_mut()?.annotations.get_mut(i))
            {
                *colour = [value[0], value[1], value[2], 80];
                self.changed();
            } else {
                self.highlight_colour = value;
            }
        }
        if finish {
            if commit && draft != original && self.selected.is_some() {
                if let Some(before) = self.colour_before.take() {
                    self.push_edit(before);
                }
            } else {
                self.colour_before = None;
                if self.colour_was_saved {
                    self.saved_revision = self.revision;
                }
            }
        } else {
            self.colour_dialog = Some((original, draft));
        }
    }
    fn block_value(&self) -> u32 {
        match self
            .selected
            .and_then(|i| self.doc.as_ref()?.annotations.get(i))
        {
            Some(Annotation::Pixelate { block_size, .. }) => *block_size,
            _ => self.pixelate_block,
        }
    }
    fn begin_block(&mut self) {
        if self.property_block_before.is_none() {
            self.property_block_before = Some(self.block_value());
            self.property_was_saved = self.saved_revision == self.revision;
            self.property_before = self
                .selected
                .and_then(|_| self.doc.as_ref().map(Edits::read));
        }
    }
    fn preview_block(&mut self, value: u32) {
        self.begin_block();
        if self.block_value() == value {
            return;
        }
        if let Some(Annotation::Pixelate { block_size, .. }) = self
            .selected
            .and_then(|i| self.doc.as_mut()?.annotations.get_mut(i))
        {
            *block_size = value;
            self.changed();
        } else {
            self.pixelate_block = value;
        }
    }
    fn finish_block(&mut self, commit: bool) {
        if !commit {
            self.cancel_tool();
        } else {
            let changed = self
                .property_block_before
                .take()
                .is_some_and(|v| v != self.block_value());
            if let Some(before) = self.property_before.take() {
                if changed {
                    self.push_edit(before);
                } else if self.property_was_saved {
                    self.saved_revision = self.revision;
                }
            }
        }
        self.block_input = self.block_value().to_string();
    }
    fn parse_block_input(&mut self) -> Option<u32> {
        match self.block_input.parse::<u32>() {
            Ok(v) if (2..=128).contains(&v) => {
                self.block_error = None;
                Some(v)
            }
            _ => {
                self.block_error = Some("Enter a whole number from 2 to 128.".into());
                None
            }
        }
    }
    fn highlight_value(&self) -> [u8; 3] {
        if let Some(Annotation::Highlight { colour, .. }) = self
            .selected
            .and_then(|i| self.doc.as_ref()?.annotations.get(i))
        {
            return [colour[0], colour[1], colour[2]];
        }
        self.highlight_colour
    }
    fn set_highlight_colour(&mut self, rgb: [u8; 3]) {
        if self.selected.is_some_and(|i| {
            matches!(
                self.doc.as_ref().and_then(|d| d.annotations.get(i)),
                Some(Annotation::Highlight { .. })
            )
        }) {
            if self.highlight_value() == rgb {
                return;
            }
            self.checkpoint();
            if let Some(Annotation::Highlight { colour, .. }) = self
                .selected
                .and_then(|i| self.doc.as_mut()?.annotations.get_mut(i))
            {
                *colour = [rgb[0], rgb[1], rgb[2], 80];
            }
            self.changed();
        } else {
            self.highlight_colour = rgb;
        }
    }

    fn header(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("header")
            .frame(egui::Frame::new().fill(SURFACE).inner_margin(14.0))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Framecraft").size(18.0).strong());
                    ui.add_space(16.0);
                    ui.add_enabled_ui(self.editor_mutation_allowed(),|ui|{ui.selectable_value(&mut self.page, Page::Editor, "Screenshot");ui.selectable_value(&mut self.page, Page::Session, "Sessions");});
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if icon_enabled(ui, self.capture_available(), "Capture setup", Icon::Settings).clicked() { self.open_setup(); }
                        ui.add_enabled_ui(self.capture_available(), |ui| {
                            ui.menu_button("More Capture", |ui| {
                                if ui_icons::button(ui, "Session & app shortcuts…", Icon::Keyboard, false).clicked() { self.open_settings(); ui.close_menu(); }
                                if ui.button("Screen Access…").clicked() {
                                    self.screen_access_open = true;
                                    self.screen_access_status = capture::permission_status();
                                    ui.close_menu();
                                }
                                ui.separator();
                                for (label, mode) in [("Capture Window…", CaptureMode::Window), ("Capture Screen", CaptureMode::Screen)] {
                                    if ui.button(label).clicked() { ui.close_menu(); self.capture(ctx, mode); }
                                }
                            });
                            if ui_icons::button(ui,"Capture Area…",Icon::Capture,false)
                                .on_hover_text("Draw a rectangle on screen. Escape cancels. Command + Option + 4.").clicked() {
                                self.capture(ctx, CaptureMode::Area);
                            }
                        });
                    });
                });
                if self.screen_access_open {
                    ui.separator();
                    ui.heading("Screen Access");
                    ui.label(capture::permission_guidance(self.screen_access_status));
                    ui.add_enabled_ui(self.capture_available(), |ui| {
                        ui.horizontal_wrapped(|ui| {
                            if ui.button("Request Access").clicked() {
                                self.screen_access_status = capture::request_permission();
                            }
                            if ui.button("Open System Settings").clicked() {
                                if let Err(error) = capture::open_permission_settings() { self.notify(error, true); }
                            }
                            if ui.button("Check Again").clicked() {
                                self.screen_access_status = capture::permission_status();
                            }
                            if ui.button("Close").clicked() { self.screen_access_open = false; }
                        });
                    });
                    ui.label("After allowing access, quit and reopen Framecraft if macOS requests it. Capture starts only when you explicitly retry.");
                    ui.collapsing("Which copy is running?", |ui| {
                        if let Ok(path) = std::env::current_exe() { ui.label(path.display().to_string()); }
                        ui.label("Use the copy in Applications. A Terminal or IDE launch can have a different permission identity. Updates to an ad-hoc-signed beta may require renewed approval.");
                    });
                }
                if self.session.active() {
                    ui.add_space(6.0);
                    ui.label(RichText::new("A capture session is active. Stop it in Sessions before taking a separate screenshot.").small().color(MUTED));
                }
                if self.page == Page::Editor && self.doc.is_some() {
                    ui.add_space(12.0);
                    ui.separator();
                    ui.add_space(6.0);
                    ui.horizontal_wrapped(|ui| {
                        if icon_enabled(ui,self.editor_mutation_allowed(),"Open Image…",Icon::Folder).clicked() { self.open(ctx); }
                        if self.fresh_capture || self.saved_revision != self.revision { ui.label(RichText::new("Unsaved edits").small().color(MUTED)); }
                            if icon_enabled(ui,self.editor_mutation_allowed(),"Close screenshot",Icon::Close).clicked(){self.request(ctx,Pending::CloseScreenshot);}
                            let ready = self.handoff_ready() && self.editor_mutation_allowed();
                            if icon_enabled(ui,ready,"Copy image",Icon::Copy).clicked() { self.export(ctx, true); }
                            if icon_enabled(ui,ready,"Save PNG…",Icon::Save).clicked() { self.export(ctx, false); }
                            if self.editing_capture.is_some() && ui.add_enabled(ready, egui::Button::new("Save Edited Copy")).clicked() { self.save_edited_copy(ctx); }
                            ui.add_enabled_ui(self.editor_mutation_allowed(), |ui| {
                                ui.menu_button("More Actions", |ui| {
                                    if ui.add_enabled(self.ocr_job.is_none(), egui::Button::new("Copy Text…")).clicked() { ui.close_menu(); self.recognise(ctx); }
                                    if ui.add_enabled(self.handoff_ready(), egui::Button::new("Drag Image File…")).clicked() {
                                        ui.close_menu();
                                        let doc = self.doc.clone().unwrap();
                                        let revision = self.revision;
                                        self.start_job(ctx, "Preparing image file…", move || prepare_drag(Snapshot::current(&doc)?, Some(revision), "Current edits".into()));
                                    }
                                    if let Some(path) = self.editing_capture.clone() {
                                        if ui.button("Reveal Saved Image in Finder").on_hover_text("Shows the saved file; unsaved edits are not included.").clicked() { ui.close_menu(); self.reveal(ctx, &path); }
                                    }
                                });
                            });
                    });
                }
            });
    }
    fn toolrail(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("tools")
            .exact_width(132.)
            .resizable(false)
            .frame(egui::Frame::new().fill(SURFACE).inner_margin(8.))
            .show(ctx, |ui| {
                ui.add_enabled_ui(self.editor_mutation_allowed(), |ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        section(ui, "Tools");
                        for (tool, label, icon) in [
                            (Tool::Select, "Select", Icon::Select),
                            (Tool::Highlight, "Highlight", Icon::Highlight),
                            (Tool::Pixelate, "Pixelate", Icon::Pixelate),
                            (Tool::Redact, "Redact", Icon::Redact),
                            (Tool::Arrow, "Arrow", Icon::Arrow),
                            (Tool::Box, "Rectangle", Icon::Rectangle),
                            (Tool::Pen, "Draw", Icon::Pen),
                            (Tool::Text, "Text", Icon::Text),
                            (Tool::Step, "Step", Icon::Step),
                            (Tool::Crop, "Crop", Icon::Crop),
                        ] {
                            if ui_icons::full_width(ui, label, icon, self.tool == tool).clicked() {
                                self.cancel_tool();
                                self.selected = None;
                                self.tool = tool;
                                self.preview = false;
                                self.notify(
                                    match tool {
                                        Tool::Select => "Select a mark to adjust it.".into(),
                                        Tool::Text => "Click the image where the text should go, then type your label.".into(),
                                        Tool::Step => "Click the image to place a numbered step.".into(),
                                        _ => format!(
                                            "{label}: drag to draw. Escape cancels a held gesture."
                                        ),
                                    },
                                    false,
                                );
                            }
                        }
                        ui.separator();
                        ui.add_enabled_ui(!self.undo.is_empty(), |ui| {
                            if ui_icons::full_width(ui, "Undo", Icon::Undo, false).clicked() {
                                self.undo(false);
                            }
                        });
                        ui.add_enabled_ui(!self.redo.is_empty(), |ui| {
                            if ui_icons::full_width(ui, "Redo", Icon::Redo, false).clicked() {
                                self.undo(true);
                            }
                        });
                    });
                });
            });
    }
    fn sidebar(&mut self, ctx: &egui::Context) {
        egui::SidePanel::right("properties")
            .exact_width(236.)
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_rgb(250, 251, 253))
                    .inner_margin(16.),
            )
            .show(ctx, |ui| {
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
                if !self.editor_mutation_allowed() {
                    ui.disable();
                }
                egui::ScrollArea::vertical().show(ui, |ui| {
                    self.property_controls(ui);
                    self.annotation_controls(ui);
                    if self.tool != Tool::Select {
                        ui.separator();
                        if ui_icons::button(ui, "Cancel tool", Icon::Cancel, false).clicked() {
                            self.cancel_tool();
                        }
                        ui.label(
                            RichText::new(
                                "Escape cancels a held drag. Undo reverses a completed mark.",
                            )
                            .small()
                            .color(MUTED),
                        );
                    }
                    ui.add_space(16.0);
                    let mut changed = false;
                    let before = self.doc.as_ref().map(Edits::read);
                    egui::CollapsingHeader::new("Background & framing").show(ui, |ui| {
                        if let Some(doc) = self.doc.as_mut() {
                            ui.horizontal_wrapped(|ui| {
                                for theme in
                                    [Theme::Midnight, Theme::Lagoon, Theme::Paper, Theme::Sunset]
                                {
                                    if ui
                                        .selectable_label(doc.style.theme == theme, theme.label())
                                        .clicked()
                                    {
                                        doc.style.theme = theme;
                                        changed = true;
                                    }
                                }
                            });
                            changed |= ui
                                .add(
                                    egui::Slider::new(&mut doc.style.padding, 0..=180)
                                        .text("Padding"),
                                )
                                .changed();
                            changed |= ui
                                .add(
                                    egui::Slider::new(&mut doc.style.radius, 0..=64)
                                        .text("Corners"),
                                )
                                .changed();
                            changed |= ui.checkbox(&mut doc.style.shadow, "Soft shadow").changed();
                            if ui
                                .button("Bare pixels")
                                .on_hover_text("No background, corners or shadow.")
                                .clicked()
                            {
                                doc.style.padding = 0;
                                doc.style.radius = 0;
                                doc.style.shadow = false;
                                changed = true;
                            }
                            if doc.crop.is_some() && ui.button("Reset crop").clicked() {
                                doc.crop = None;
                                changed = true;
                            }
                        } else {
                            ui.label(
                                RichText::new("Capture or open an image to set its style.")
                                    .color(MUTED),
                            );
                        }
                    });
                    if changed {
                        if self.style_before.is_none() {
                            self.style_before = before;
                        }
                        self.changed();
                    }
                    if !ctx.input(|i| i.pointer.any_down()) {
                        if let Some(before) = self.style_before.take() {
                            self.undo.push(before);
                            if self.undo.len() > 40 {
                                self.undo.remove(0);
                            }
                            self.redo.clear();
                        }
                    }
                    ui.add_space(24.0);
                    ui.separator();
                    ui.label(
                        RichText::new(
                            "Edits apply to the copy you save. Your original is retained.",
                        )
                        .small()
                        .color(MUTED),
                    );
                });
            });
    }
    fn canvas(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_rgb(232, 237, 244))
                    .inner_margin(24.0),
            )
            .show(ctx, |ui| {
                if !self.editor_mutation_allowed(){ui.disable();}
                if self.doc.is_none() {
                    self.empty(ui, ctx);
                    return;
                }
                if self.selecting_display.is_some() {
                    ui.horizontal(|ui| {
                        ui.label("Drag a crop around the content to capture repeatedly.");
                        let label = if self.doc.as_ref().unwrap().crop.is_some() {
                            "Use this area"
                        } else {
                            "Use full display"
                        };
                        if ui.button(label).clicked() {
                            self.use_session_area();
                        }
                        if ui.button("Cancel").clicked() {
                            self.selecting_display = None;
                            self.page = Page::Session;
                        }
                    });
                    ui.separator();
                }
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&self.name).size(15.0).strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.selectable_value(&mut self.preview, true, "Export preview");
                        ui.selectable_value(&mut self.preview, false, "Edit source");
                    });
                });
                ui.add_space(16.0);
                let texture = if self.preview {
                    self.preview_texture.as_ref()
                } else {
                    self.flat_texture.as_ref().or_else(|| self.doc.as_ref().filter(|d|d.annotations.is_empty()).and(self.source_texture.as_ref()))
                };
                let available = ui.available_size() - Vec2::new(0.0, 35.0);
                let size = texture.map(|texture|texture.size_vec2()).unwrap_or_else(|| {
                    let doc=self.doc.as_ref().unwrap();Vec2::new(doc.source.width() as f32,doc.source.height() as f32)
                });
                let scale = (available.x / size.x).min(available.y / size.y).max(0.01);
                let displayed = size * scale;
                let (space, _) = ui.allocate_exact_size(available, Sense::hover());
                let rect = egui::Rect::from_center_size(space.center(), displayed);
                let response = ui.interact(rect, ui.id().with("canvas"), Sense::click_and_drag());
                if let Some(texture)=texture {
                    ui.painter().image(texture.id(),rect,egui::Rect::from_min_max(Pos2::ZERO,Pos2::new(1.0,1.0)),Color32::WHITE);
                } else {
                    ui.painter().rect_filled(rect,0.0,INK);
                    ui.painter().text(rect.center(),egui::Align2::CENTER_CENTER,"Updating image preview…",egui::FontId::proportional(16.0),MUTED);
                }
                if !self.preview {
                    self.paint_selection(ui, rect);
                    let doc = self.doc.as_ref().unwrap();
                    let w = doc.source.width();
                    let h = doc.source.height();
                    let point = |pos: Pos2| source_point(pos, rect, w, h);
                    if response.clicked() {
                        if let Some(pos)=response.interact_pointer_pos() {
                            let p=point(pos);
                            match self.tool {
                                Tool::Select => { self.selected=editor::hit_test(&self.doc.as_ref().unwrap().annotations,p,6.0/scale); self.sync_selected(); if response.double_clicked() { self.begin_text_edit(); } }
                                Tool::Text | Tool::Step => self.place_annotation(p),
                                _ => {}
                            }
                        }
                    }
                    if response.drag_started() {
                        self.drag = ctx.input(|i| i.pointer.press_origin()).map(point);
                        self.freehand.clear();
                        if let Some(p)=self.drag {
                            if self.tool==Tool::Select {
                                self.selected=editor::hit_test(&self.doc.as_ref().unwrap().annotations,p,6.0/scale);
                                self.sync_selected();
                                self.moving=self.selected.and_then(|i|self.doc.as_ref()?.annotations.get(i).cloned());
                                if self.moving.is_some() {self.begin_move();}
                            } else {self.freehand.push(p);}
                        }
                    }
                    if let (Some(start), Some(pos)) = (self.drag, response.interact_pointer_pos()) {
                        let end = point(pos);
                        if self.tool==Tool::Select {
                            if let (Some(mut mark),Some(index))=(self.moving.clone(),self.selected) {
                                if editor::translate_annotation(&mut mark,Point{x:end.x-start.x,y:end.y-start.y},w,h).is_ok() {
                                    self.doc.as_mut().unwrap().annotations[index]=mark; self.changed();
                                }
                            }
                        } else if self.tool!=Tool::Text && self.tool!=Tool::Step {
                            if self.tool == Tool::Pen && self.freehand.len() < 10_000 && self.freehand.last().is_none_or(|p| (p.x-end.x).hypot(p.y-end.y)>1.0) {self.freehand.push(end);}
                            self.paint_draft(ui,rect,start,end);
                            if response.drag_stopped() {
                                if self.tool==Tool::Pen {self.commit_pen();}
                                else if (start.x-end.x).abs()+(start.y-end.y).abs()>3.0 {self.commit(start,end);}
                            }
                        }
                        if response.drag_stopped() {self.drag=None;if self.moving.is_some(){self.finish_move();}}
                    }
                    if !ctx.input(|i| i.pointer.any_down()) {
                        self.drag = None;
                        self.freehand.clear();
                    }
                }
                ui.add_space(10.0);
                ui.label(
                    RichText::new(if self.editing_capture.is_some() {
                        "Save edited copy creates a new PNG. The unedited original stays in your session folder."
                    } else if self.preview {
                        "Preview of the image you will copy or save."
                    } else {
                        "Edit the original. Crop and redactions are applied to every export."
                    })
                    .small()
                    .color(MUTED),
                );
            });
    }
    fn empty(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.add_space((ui.available_height() * 0.10).max(16.0));
        ui.vertical_centered(|ui| {
            let (rect, _) = ui.allocate_exact_size(Vec2::new(72.0, 60.0), Sense::hover());
            let outline = rect.shrink(8.0);
            ui.painter().rect_stroke(
                outline,
                10.0,
                Stroke::new(2.5_f32, ACCENT),
                egui::StrokeKind::Inside,
            );
            ui.painter().line_segment(
                [
                    outline.center() - Vec2::new(9.0, 0.0),
                    outline.center() + Vec2::new(9.0, 0.0),
                ],
                Stroke::new(2.0_f32, ACCENT),
            );
            ui.painter().line_segment(
                [
                    outline.center() - Vec2::new(0.0, 9.0),
                    outline.center() + Vec2::new(0.0, 9.0),
                ],
                Stroke::new(2.0_f32, ACCENT),
            );
            ui.add_space(18.0);
            ui.label(RichText::new("Take a screenshot").size(30.0).strong());
            ui.add_space(8.0);
            ui.label(
                RichText::new("Choose an area. Then copy, save or annotate it.")
                    .size(16.0)
                    .color(MUTED),
            );
            ui.add_space(26.0);
            if ui
                .add_enabled(
                    self.capture_available(),
                    egui::Button::new(
                        RichText::new("Capture Area…")
                            .size(16.0)
                            .color(Color32::WHITE),
                    )
                    .fill(ACCENT)
                    .min_size(Vec2::new(210.0, 44.0)),
                )
                .clicked()
            {
                self.capture(ctx, CaptureMode::Area);
            }
            ui.add_space(8.0);
            ui.label(
                RichText::new("Drag a rectangle on screen. Press Escape to cancel.")
                    .small()
                    .color(MUTED),
            );
            ui.add_space(20.0);
            ui.horizontal(|ui| {
                let gap = ((ui.available_width() - 518.0) / 2.0).max(0.0);
                ui.add_space(gap);
                ui.add_enabled_ui(self.capture_available(), |ui| {
                    if ui_icons::sized_button(
                        ui,
                        "Capture Window…",
                        Icon::Window,
                        false,
                        Some(166.0),
                    )
                    .clicked()
                    {
                        self.capture(ctx, CaptureMode::Window);
                    }
                    if ui_icons::sized_button(
                        ui,
                        "Capture Screen",
                        Icon::Screen,
                        false,
                        Some(166.0),
                    )
                    .clicked()
                    {
                        self.capture(ctx, CaptureMode::Screen);
                    }
                });
                if ui
                    .add_enabled_ui(self.editor_mutation_allowed(), |ui| {
                        ui_icons::sized_button(ui, "Open Image…", Icon::Folder, false, Some(166.0))
                    })
                    .inner
                    .clicked()
                {
                    self.open(ctx);
                }
            });
            ui.add_space(22.0);
            if self.quick_settings.setup_complete {
                ui.label(if self.quick_settings.shortcut_enabled {
                    format!("Your area shortcut: {}", self.quick_settings.shortcut)
                } else {
                    "Capture buttons are ready. Everyday shortcut is off.".into()
                });
            } else if ui_icons::button(ui, "Set up shortcuts & saving", Icon::Settings, false)
                .clicked()
            {
                self.open_setup();
            }
            ui.add_space(12.0);
            ui.label(
                RichText::new("Also available from the Framecraft icon in your Mac’s menu bar.")
                    .color(MUTED),
            );
            ui.add_space(8.0);
            if ui
                .link("Collect several screenshots in a session")
                .clicked()
            {
                self.page = Page::Session;
            }
            ui.add_space(8.0);
            if ui.link("Try the editor with a sample image").clicked() {
                self.load(ctx.clone(), editor::demo_image(), "Sample image".into());
            }
            ui.add_space(24.0);
            ui.label(
                RichText::new("Or drop a PNG or JPEG into this window.")
                    .small()
                    .color(MUTED),
            );
        });
    }
    fn commit(&mut self, from: Point, to: Point) {
        if (self.tool == Tool::Pixelate
            && ((from.x - to.x).abs() < 2.0 || (from.y - to.y).abs() < 2.0))
            || self.tool == Tool::Select
            || (self.tool != Tool::Arrow
                && ((from.x - to.x).abs() < 1.0 || (from.y - to.y).abs() < 1.0))
        {
            return;
        }
        self.checkpoint();
        let doc = self.doc.as_mut().unwrap();
        let colour = self.colour.to_srgba_unmultiplied();
        let rect = pixel_rect(from, to);
        match self.tool {
            Tool::Arrow => doc.annotations.push(Annotation::Arrow {
                from,
                to,
                colour,
                width: self.stroke,
            }),
            Tool::Box => doc.annotations.push(Annotation::Rectangle {
                rect,
                colour,
                width: self.stroke.round() as u32,
            }),
            Tool::Redact => doc.annotations.push(Annotation::Redact { rect }),
            Tool::Crop => doc.crop = Some(rect),
            Tool::Pixelate => doc.annotations.push(Annotation::Pixelate {
                rect,
                block_size: self.pixelate_block,
            }),
            Tool::Highlight => doc.annotations.push(Annotation::Highlight {
                rect,
                colour: [
                    self.highlight_colour[0],
                    self.highlight_colour[1],
                    self.highlight_colour[2],
                    80,
                ],
            }),
            Tool::Select | Tool::Pen | Tool::Text | Tool::Step => {}
        }
        self.changed();
    }
    fn commit_pen(&mut self) {
        if self.freehand.is_empty() {
            return;
        }
        self.checkpoint();
        if let Some(d) = self.doc.as_mut() {
            d.annotations.push(Annotation::Freehand {
                points: std::mem::take(&mut self.freehand),
                colour: self.colour.to_srgba_unmultiplied(),
                width: self.stroke,
            });
        }
        self.changed();
    }
    fn paint_selection(&self, ui: &egui::Ui, canvas: egui::Rect) {
        let doc = self.doc.as_ref().unwrap();
        if let Some(a) = self.selected.and_then(|i| doc.annotations.get(i)) {
            if let Some((min, max)) = editor::annotation_bounds(a) {
                ui.painter().rect_stroke(
                    egui::Rect::from_min_max(self.map(canvas, min), self.map(canvas, max)),
                    0.0,
                    Stroke::new(2.0_f32, ACCENT),
                    egui::StrokeKind::Outside,
                );
            }
        }
        for mark in &doc.annotations {
            if matches!(mark, Annotation::Redact { .. }) {
                self.paint_annotation(ui, canvas, mark);
            }
        }
        if let Some(crop) = doc.crop {
            ui.painter().rect_stroke(
                self.map_rect(canvas, crop),
                0.0,
                Stroke::new(2.0_f32, Color32::WHITE),
                egui::StrokeKind::Inside,
            );
        }
    }
    fn map(&self, canvas: egui::Rect, p: Point) -> Pos2 {
        let d = self.doc.as_ref().unwrap();
        Pos2::new(
            canvas.left() + p.x / d.source.width() as f32 * canvas.width(),
            canvas.top() + p.y / d.source.height() as f32 * canvas.height(),
        )
    }
    fn map_rect(&self, canvas: egui::Rect, r: Rect) -> egui::Rect {
        egui::Rect::from_min_max(
            self.map(
                canvas,
                Point {
                    x: r.x as f32,
                    y: r.y as f32,
                },
            ),
            self.map(
                canvas,
                Point {
                    x: (r.x + r.width) as f32,
                    y: (r.y + r.height) as f32,
                },
            ),
        )
    }
    fn paint_annotation(&self, ui: &egui::Ui, canvas: egui::Rect, a: &Annotation) {
        let p = ui.painter_at(canvas);
        let scale = canvas.width() / self.doc.as_ref().unwrap().source.width() as f32;
        match a {
            Annotation::Text { .. } | Annotation::Step { .. } | Annotation::Pixelate { .. } => {} // Composited by the shared raster renderer.
            Annotation::Highlight { rect, colour } => {
                p.rect_filled(
                    self.map_rect(canvas, *rect),
                    0.0,
                    Color32::from_rgba_unmultiplied(colour[0], colour[1], colour[2], colour[3]),
                );
            }
            Annotation::Freehand {
                points,
                colour,
                width,
            } => {
                let stroke = Stroke::new(
                    (*width * scale).max(1.0),
                    Color32::from_rgba_unmultiplied(colour[0], colour[1], colour[2], colour[3]),
                );
                if points.len() == 1 {
                    p.circle_filled(
                        self.map(canvas, points[0]),
                        stroke.width / 2.0,
                        stroke.color,
                    );
                } else {
                    p.add(egui::Shape::line(
                        points.iter().map(|pt| self.map(canvas, *pt)).collect(),
                        stroke,
                    ));
                }
            }
            Annotation::Arrow {
                from,
                to,
                colour,
                width,
            } => {
                let from = self.map(canvas, *from);
                let to = self.map(canvas, *to);
                p.arrow(
                    from,
                    to - from,
                    Stroke::new(
                        (width * scale).max(1.0),
                        Color32::from_rgba_unmultiplied(colour[0], colour[1], colour[2], colour[3]),
                    ),
                );
            }
            Annotation::Rectangle {
                rect,
                colour,
                width,
            } => {
                p.rect_stroke(
                    self.map_rect(canvas, *rect),
                    0.0,
                    Stroke::new(
                        (*width as f32 * scale).max(1.0),
                        Color32::from_rgba_unmultiplied(colour[0], colour[1], colour[2], colour[3]),
                    ),
                    egui::StrokeKind::Inside,
                );
            }
            Annotation::Redact { rect } => {
                p.rect_filled(
                    self.map_rect(canvas, *rect),
                    0.0,
                    Color32::from_rgb(20, 24, 30),
                );
            }
        }
    }
    fn paint_draft(&self, ui: &egui::Ui, canvas: egui::Rect, from: Point, to: Point) {
        let annotation = match self.tool {
            Tool::Pen => Annotation::Freehand {
                points: self.freehand.clone(),
                colour: self.colour.to_srgba_unmultiplied(),
                width: self.stroke,
            },
            Tool::Highlight => {
                let c = self.highlight_colour;
                Annotation::Highlight {
                    rect: pixel_rect(from, to),
                    colour: [c[0], c[1], c[2], 80],
                }
            }
            Tool::Arrow => Annotation::Arrow {
                from,
                to,
                colour: self.colour.to_srgba_unmultiplied(),
                width: self.stroke,
            },
            Tool::Redact => Annotation::Redact {
                rect: pixel_rect(from, to),
            },
            _ => Annotation::Rectangle {
                rect: pixel_rect(from, to),
                colour: self.colour.to_srgba_unmultiplied(),
                width: self.stroke as u32,
            },
        };
        self.paint_annotation(ui, canvas, &annotation);
        // Even a live annotation preview cannot reveal an existing redaction.
        for a in &self.doc.as_ref().unwrap().annotations {
            if matches!(a, Annotation::Redact { .. }) {
                self.paint_annotation(ui, canvas, a);
            }
        }
    }
}

impl Framecraft {
    fn request(&mut self, ctx: &egui::Context, action: Pending) {
        if self.setup.is_some() {
            self.notify("Finish or cancel Capture setup first.", false);
            return;
        }
        if self.closing || self.colour_dialog.is_some() || self.text_draft.is_some() {
            return;
        }
        if self.job.is_some() {
            self.notify("Wait for the current operation to finish.", true);
            return;
        }
        if self.doc.is_some()
            && (self.fresh_capture || self.revision != self.saved_revision)
            && self.selecting_display.is_none()
        {
            self.pending = Some(action);
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        } else {
            self.execute(ctx, action);
        }
    }
    fn execute(&mut self, ctx: &egui::Context, action: Pending) {
        match action {
            Pending::Open(path) => self.open_path_now(ctx, path),
            Pending::Capture(mode) => self.capture_now(ctx, mode),
            Pending::SelectDisplay(display) => self.select_session_source_now(ctx, display),
            Pending::Session(handle, variant) => {
                self.session.pause();
                self.start_job(ctx, "Opening selected image variant…", move || {
                    let (image, path) = handle.load_variant(&variant)?;
                    Ok(Outcome::SessionLoaded(image, path, handle))
                });
            }
            Pending::CloseScreenshot => self.close_screenshot(),
            Pending::Quit => {
                self.session.desktop_action(DesktopAction::Stop, ctx);
                self.closing = true;
            }
        }
    }
    fn sync_selected(&mut self) {
        if let Some(mark) = self
            .selected
            .and_then(|i| self.doc.as_ref()?.annotations.get(i))
        {
            match mark {
                Annotation::Text {
                    font_size, colour, ..
                } => {
                    self.text_size = *font_size;
                    self.colour =
                        Color32::from_rgba_unmultiplied(colour[0], colour[1], colour[2], colour[3]);
                }
                Annotation::Step {
                    number,
                    colour,
                    radius,
                    ..
                } => {
                    self.step_number = *number;
                    self.text_size = *radius;
                    self.colour =
                        Color32::from_rgba_unmultiplied(colour[0], colour[1], colour[2], colour[3]);
                }
                _ => {}
            }
        }
    }
    fn begin_text_edit(&mut self) {
        if !self.editor_mutation_allowed() {
            return;
        }
        if let Some((
            index,
            Annotation::Text {
                position,
                text,
                font_size,
                colour,
                background,
            },
        )) = self
            .selected
            .and_then(|i| Some((i, self.doc.as_ref()?.annotations.get(i)?)))
        {
            self.text_draft = Some(TextDraft {
                index: Some(index),
                position: *position,
                text: text.clone(),
                size: *font_size,
                colour: Color32::from_rgba_unmultiplied(colour[0], colour[1], colour[2], colour[3]),
                background: *background,
                focus: true,
            });
        }
    }
    fn cancel_text_draft(&mut self) {
        self.text_draft = None;
        self.notify(
            "Text changes cancelled. Your screenshot is unchanged.",
            false,
        );
    }
    fn commit_text_draft(&mut self) -> bool {
        let Some(draft) = &self.text_draft else {
            return false;
        };
        if let Some(error) = draft.validation_error() {
            self.notify(error, true);
            return false;
        }
        let mark = Annotation::Text {
            position: draft.position,
            text: draft.text.clone(),
            font_size: draft.size,
            colour: draft.colour.to_srgba_unmultiplied(),
            background: draft.background,
        };
        let index = draft.index;
        let Some(doc) = &self.doc else {
            return false;
        };
        if let Some(index) = index {
            if !matches!(doc.annotations.get(index), Some(Annotation::Text { .. })) {
                return false;
            }
            if doc.annotations.get(index) == Some(&mark) {
                self.text_draft = None;
                self.notify("Text is unchanged.", false);
                return true;
            }
        }
        self.checkpoint();
        let doc = self.doc.as_mut().unwrap();
        let selected = match index {
            Some(index) => {
                doc.annotations[index] = mark;
                index
            }
            None => {
                doc.annotations.push(mark);
                doc.annotations.len() - 1
            }
        };
        self.selected = Some(selected);
        self.text_draft = None;
        self.tool = Tool::Select;
        self.changed();
        self.sync_selected();
        self.notify(
            "Text applied. Double-click the label to edit it again; save the screenshot when ready.",
            false,
        );
        true
    }
    fn place_annotation(&mut self, at: Point) {
        if self.doc.is_none() || !self.editor_mutation_allowed() {
            return;
        }
        if self.tool == Tool::Text {
            self.selected = None;
            self.text_draft = Some(TextDraft {
                index: None,
                position: at,
                text: String::new(),
                size: self.text_size,
                colour: Color32::WHITE,
                background: Some([17, 22, 29, 235]),
                focus: true,
            });
            return;
        }
        if self.tool != Tool::Step {
            return;
        }
        let mark = Annotation::Step {
            center: at,
            number: self.step_number,
            radius: self.text_size.max(12.0),
            colour: self.colour.to_srgba_unmultiplied(),
            text_colour: [17, 22, 29, 255],
        };
        self.checkpoint();
        let doc = self.doc.as_mut().unwrap();
        doc.annotations.push(mark);
        self.selected = Some(doc.annotations.len() - 1);
        self.step_number = (self.step_number + 1).min(9999);
        self.changed();
    }
    fn delete_selected(&mut self) {
        if let Some(index) = self.selected {
            if self
                .doc
                .as_ref()
                .is_some_and(|d| index < d.annotations.len())
            {
                self.checkpoint();
                self.doc.as_mut().unwrap().annotations.remove(index);
                self.selected = None;
                self.changed();
            }
        }
    }
    fn move_selected(&mut self, delta: Point) {
        if let Some(index) = self.selected {
            let Some(doc) = self.doc.as_ref() else { return };
            let Some(mut mark) = doc.annotations.get(index).cloned() else {
                self.selected = None;
                return;
            };
            match editor::translate_annotation(
                &mut mark,
                delta,
                doc.source.width(),
                doc.source.height(),
            ) {
                Ok(true) => {
                    self.checkpoint();
                    self.doc.as_mut().unwrap().annotations[index] = mark;
                    self.changed();
                }
                Ok(false) => {}
                Err(error) => self.notify(error, true),
            }
        }
    }
    fn property_controls(&mut self, ui: &mut egui::Ui) {
        let selected = self
            .selected
            .and_then(|i| self.doc.as_ref()?.annotations.get(i));
        if self.tool == Tool::Text || matches!(selected, Some(Annotation::Text { .. })) {
            return; // Text has its own labelled editor rather than unrelated mark controls.
        }
        let highlight = matches!(selected, Some(Annotation::Highlight { .. }))
            || (selected.is_none() && self.tool == Tool::Highlight);
        let pixelate = matches!(selected, Some(Annotation::Pixelate { .. }))
            || (selected.is_none() && self.tool == Tool::Pixelate);
        section(
            ui,
            if self.selected.is_some() {
                "Selected mark"
            } else {
                "Next tool"
            },
        );
        if highlight {
            ui.heading(if self.selected.is_some() {
                "Selected highlight"
            } else {
                "Next highlight"
            });
            ui.label("Highlight colour");
            ui.horizontal_wrapped(|ui| {
                for (name, rgb) in [
                    ("Yellow", [255, 212, 71]),
                    ("Green", [135, 217, 137]),
                    ("Cyan", [112, 213, 228]),
                    ("Blue", [131, 169, 250]),
                    ("Pink", [239, 152, 190]),
                    ("Orange", [244, 174, 107]),
                ] {
                    let button = egui::Button::new(name)
                        .fill(Color32::from_rgb(rgb[0], rgb[1], rgb[2]))
                        .selected(self.highlight_value() == rgb);
                    let response = ui.add(button);
                    response.widget_info(|| {
                        egui::WidgetInfo::selected(
                            egui::WidgetType::Button,
                            ui.is_enabled(),
                            self.highlight_value() == rgb,
                            name,
                        )
                    });
                    if response.clicked() {
                        self.set_highlight_colour(rgb);
                    }
                }
            });
            let custom = ui.button("Custom colour…");
            if custom.clicked() {
                self.colour_return = Some(custom.id);
                self.begin_colour();
            }
            ui.label(
                RichText::new("Highlights stay translucent.")
                    .small()
                    .color(MUTED),
            );
        } else if pixelate {
            ui.heading(if self.selected.is_some() {
                "Selected pixelation"
            } else {
                "Next pixelation"
            });
            let mut value = self.block_value();
            let slider = ui.add(
                egui::Slider::new(&mut value, 2..=128)
                    .text("Block size")
                    .show_value(false),
            );
            if slider.changed() {
                self.preview_block(value);
            }
            if slider.drag_stopped() || (slider.changed() && !slider.dragged()) {
                self.finish_block(true);
            }
            let id = ui.make_persistent_id("pixelation-number");
            if !ui.memory(|m| m.has_focus(id)) && self.property_block_before.is_none() {
                self.block_input = self.block_value().to_string();
            }
            let block_label = ui.label("Block size in original-image pixels");
            let input = ui
                .add(
                    egui::TextEdit::singleline(&mut self.block_input)
                        .id(id)
                        .desired_width(90.)
                        .char_limit(4),
                )
                .labelled_by(block_label.id);
            if input.gained_focus() {
                self.begin_block();
            }
            if input.changed() {
                if let Some(value) = self.parse_block_input() {
                    self.preview_block(value);
                }
            }
            let escape = (input.has_focus() || input.lost_focus())
                && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
            let enter = (input.has_focus() || input.lost_focus())
                && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
            if escape {
                self.finish_block(false);
                self.block_error = None;
                ui.memory_mut(|m| m.surrender_focus(id));
            } else if enter {
                if self.parse_block_input().is_some() {
                    self.finish_block(true);
                    ui.memory_mut(|m| m.surrender_focus(id));
                } else {
                    input.request_focus();
                }
            } else if input.lost_focus() {
                let valid = self.parse_block_input().is_some();
                self.finish_block(valid);
            }
            if let Some(error) = &self.block_error {
                ui.colored_label(Color32::from_rgb(173, 52, 52), error);
            }
            ui.label("Original-image pixels. Larger blocks hide more detail.");
            ui.label(
                RichText::new("Use Redact for a solid cover. The original is retained.")
                    .small()
                    .color(MUTED),
            );
        } else if matches!(self.tool, Tool::Arrow | Tool::Box | Tool::Pen) {
            ui.horizontal(|ui| {
                ui.label("Colour");
                ui.color_edit_button_srgba(&mut self.colour);
            });
            if matches!(self.tool, Tool::Arrow | Tool::Box | Tool::Pen) {
                ui.add(egui::Slider::new(&mut self.stroke, 2.0..=16.0).text("Weight"));
            }
        } else {
            ui.heading(if self.tool == Tool::Redact {
                "Redact"
            } else {
                "Screenshot"
            });
            ui.label("Select a mark to adjust it, or choose a tool.");
        }
        if self.selected.is_some() {
            ui.label(
                RichText::new("Edits change this mark.")
                    .small()
                    .color(MUTED),
            );
        } else {
            ui.label(
                RichText::new("Properties apply to the next mark you draw.")
                    .small()
                    .color(MUTED),
            );
        }
        if matches!(self.tool, Tool::Highlight | Tool::Pixelate | Tool::Redact)
            && ui
                .button("Add centred region")
                .on_hover_text(
                    "Keyboard alternative to drawing; use Select and arrow keys to move it.",
                )
                .clicked()
        {
            if let Some(d) = &self.doc {
                let w = d.source.width() as f32;
                let h = d.source.height() as f32;
                let count = self.doc.as_ref().unwrap().annotations.len();
                self.commit(
                    Point {
                        x: w * 0.3,
                        y: h * 0.4,
                    },
                    Point {
                        x: w * 0.7,
                        y: h * 0.6,
                    },
                );
                if self.doc.as_ref().unwrap().annotations.len() > count {
                    self.selected = Some(count);
                    self.tool = Tool::Select;
                } else {
                    self.notify("Choose a region at least 2 by 2 pixels.", false);
                }
            }
        }
    }
    fn annotation_controls(&mut self, ui: &mut egui::Ui) {
        let selected_text = self.selected.is_some_and(|i| {
            matches!(
                self.doc.as_ref().and_then(|d| d.annotations.get(i)),
                Some(Annotation::Text { .. })
            )
        });
        let selected_step = self.selected.is_some_and(|i| {
            matches!(
                self.doc.as_ref().and_then(|d| d.annotations.get(i)),
                Some(Annotation::Step { .. })
            )
        });
        if selected_text {
            ui.heading("Text label");
            ui.label("Edit the wording, size and colour together.");
            if ui.button("Edit text…").clicked() {
                self.begin_text_edit();
            }
            ui.label("You can also double-click a label on the image.");
        } else if self.tool == Tool::Text {
            ui.heading("Add text");
            ui.label("Click where the label should go, then type your text.");
            if ui.button("Add text at centre…").clicked() {
                if let Some(doc) = &self.doc {
                    self.place_annotation(Point {
                        x: doc.source.width() as f32 * 0.5,
                        y: doc.source.height() as f32 * 0.5,
                    });
                }
            }
        }
        if self.tool == Tool::Step || selected_step {
            ui.label("Step properties");
            ui.add(egui::Slider::new(&mut self.text_size, 8.0..=128.0).text("Size"));
            ui.add(
                egui::DragValue::new(&mut self.step_number)
                    .range(1..=9999)
                    .prefix("Step "),
            );
            ui.horizontal(|ui| {
                ui.label("Step colour");
                ui.color_edit_button_srgba(&mut self.colour);
            });
            if selected_step && ui.button("Apply step changes").clicked() {
                self.checkpoint();
                if let Some(Annotation::Step {
                    number,
                    radius,
                    colour,
                    ..
                }) = self
                    .selected
                    .and_then(|i| self.doc.as_mut()?.annotations.get_mut(i))
                {
                    *number = self.step_number;
                    *radius = self.text_size.max(8.0);
                    *colour = self.colour.to_srgba_unmultiplied();
                }
                self.changed();
            }
        }
        let names = self
            .doc
            .as_ref()
            .map(|d| {
                d.annotations
                    .iter()
                    .enumerate()
                    .map(|(i, a)| {
                        format!(
                            "{}: {}",
                            i + 1,
                            match a {
                                Annotation::Text { .. } => "Text",
                                Annotation::Step { .. } => "Step",
                                Annotation::Arrow { .. } => "Arrow",
                                Annotation::Rectangle { .. } => "Rectangle",
                                Annotation::Highlight { .. } => "Highlight",
                                Annotation::Freehand { .. } => "Drawing",
                                Annotation::Redact { .. } => "Redaction",
                                Annotation::Pixelate { .. } => "Pixelation",
                            }
                        )
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        if !names.is_empty() {
            let mark_label = ui.label("Select mark");
            let mark_combo = egui::ComboBox::from_id_salt("selected-annotation")
                .width(ui.available_width() - 8.0)
                .selected_text(
                    self.selected
                        .and_then(|i| names.get(i))
                        .map_or("None", String::as_str),
                )
                .show_ui(ui, |ui| {
                    for (i, name) in names.iter().enumerate() {
                        if ui
                            .selectable_value(&mut self.selected, Some(i), name)
                            .changed()
                        {
                            self.tool = Tool::Select;
                            self.sync_selected();
                        }
                    }
                })
                .response
                .labelled_by(mark_label.id);
            mark_combo.widget_info(|| {
                egui::WidgetInfo::labeled(
                    egui::WidgetType::ComboBox,
                    ui.is_enabled(),
                    "Select mark",
                )
            });
            if ui
                .add_enabled(
                    self.selected.is_some(),
                    egui::Button::new("Delete selected mark"),
                )
                .clicked()
            {
                self.delete_selected();
            }
            egui::Grid::new("nudge-mark").num_columns(2).show(ui, |ui| {
                for (index, (name, delta)) in [
                    ("Left", Point { x: -1.0, y: 0.0 }),
                    ("Right", Point { x: 1.0, y: 0.0 }),
                    ("Up", Point { x: 0.0, y: -1.0 }),
                    ("Down", Point { x: 0.0, y: 1.0 }),
                ]
                .into_iter()
                .enumerate()
                {
                    if ui
                        .add_sized([86.0, 30.0], egui::Button::new(name))
                        .clicked()
                    {
                        self.move_selected(delta);
                    }
                    if index % 2 == 1 {
                        ui.end_row();
                    }
                }
            });
        }
    }
    fn recognise(&mut self, ctx: &egui::Context) {
        if self.ocr_job.is_some() {
            return;
        }
        let Some(doc) = self.doc.clone() else { return };
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancelled.clone();
        let (tx, rx) = mpsc::channel();
        let wake = ctx.clone();
        self.ocr_job = Some(OcrJob {
            revision: self.revision,
            cancelled,
            rx,
        });
        self.ocr_text = None;
        std::thread::spawn(move || {
            let result = Snapshot::current(&doc).and_then(|snapshot| {
                if worker_cancel.load(Ordering::Acquire) {
                    return Err("Text extraction cancelled.".into());
                }
                framecraft::naming::recognise_text(&snapshot.pixels)
            });
            if !worker_cancel.load(Ordering::Acquire) {
                let _ = tx.send(result);
            }
            wake.request_repaint();
        });
    }
    fn poll_ocr(&mut self, ctx: &egui::Context) {
        let result = self
            .ocr_job
            .as_ref()
            .and_then(|job| match job.rx.try_recv() {
                Ok(result) => Some((job.revision, job.cancelled.load(Ordering::Acquire), result)),
                Err(mpsc::TryRecvError::Disconnected) => {
                    Some((job.revision, true, Err("Text extraction stopped.".into())))
                }
                _ => None,
            });
        if let Some((revision, cancelled, result)) = result {
            self.ocr_job = None;
            if !cancelled && revision == self.revision {
                match result {
                    Ok(text) => self.ocr_text = Some((revision, text)),
                    Err(error) => self.notify(error, true),
                }
            }
        }
        if self.ocr_job.is_some() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }
    fn prepare_pack(&mut self, ctx: &egui::Context, frames: Vec<ReviewFrame>) {
        self.start_job(
            ctx,
            "Loading the exact selected variants for review…",
            move || {
                if frames.is_empty() || frames.len() > output::MAX_PACK_FRAMES {
                    return Err("Select 1 to 100 frames.".into());
                }
                let mut roots = vec![];
                let mut result = vec![];
                let mut pixels = 0u64;
                for (i, frame) in frames.into_iter().enumerate() {
                    let (image, path) = frame
                        .handle
                        .load_variant(&frame.variant)
                        .map_err(|e| format!("Frame {}: {e}", i + 1))?;
                    pixels += u64::from(image.width()) * u64::from(image.height());
                    if pixels > output::MAX_PACK_PIXELS {
                        return Err(
                            "The selection exceeds 64 million pixels. Select fewer frames.".into(),
                        );
                    }
                    if let Some(root) = path.parent() {
                        let root = root.to_path_buf();
                        if !roots.contains(&root) {
                            roots.push(root);
                        }
                    }
                    result.push(PackFrame {
                        snapshot: Snapshot::pixels(image)?,
                        caption: frame.caption,
                        variant_label: variant_label(&frame.variant),
                    });
                }
                output::validate_pack(&result)?;
                Ok(Outcome::PackPrepared(Arc::new(result), roots))
            },
        );
    }
    fn session_action(&mut self, ctx: &egui::Context, action: SessionAction) {
        match action {
            SessionAction::SelectDisplay(display) => self.select_session_source(ctx, display),
            SessionAction::Edit(handle) => {
                self.request(ctx, Pending::Session(handle, CaptureVariant::Original))
            }
            SessionAction::EditVariant(handle, variant) => {
                self.request(ctx, Pending::Session(handle, variant))
            }
            SessionAction::Copy(handle, variant) => {
                self.start_job(ctx, "Copying selected variant…", move || {
                    let (image, _) = handle.load_variant(&variant)?;
                    copy_snapshot(Snapshot::pixels(image)?).map(Outcome::Copied)
                })
            }
            SessionAction::Drag(handle, variant) => {
                self.start_job(ctx, "Preparing selected variant…", move || {
                    let (image, _) = handle.load_variant(&variant)?;
                    prepare_drag(
                        Snapshot::pixels(image)?,
                        None,
                        format!(
                            "Frame {} / {}",
                            handle.snapshot()?.sequence,
                            variant_label(&variant)
                        ),
                    )
                })
            }
            SessionAction::Reveal(handle, variant) => {
                self.start_job(ctx, "Finding selected variant…", move || {
                    let (_, path) = handle.load_variant(&variant)?;
                    reveal_file(&path)?;
                    Ok(Outcome::Revealed)
                })
            }
            SessionAction::ExportPack(frames) => self.prepare_pack(ctx, frames),
        }
    }
    fn reveal(&mut self, ctx: &egui::Context, path: &std::path::Path) {
        let path = path.to_path_buf();
        self.start_job(ctx, "Revealing selected image…", move || {
            reveal_file(&path)?;
            Ok(Outcome::Revealed)
        });
    }
    fn native_controls(&mut self, ctx: &egui::Context) {
        if !self.desktop_attempted {
            self.desktop_attempted = true;
            let wake = ctx.clone();
            match DesktopControls::new(move || wake.request_repaint()) {
                Ok(mut controls) => {
                    if self.quick_settings.setup_complete && self.quick_settings.shortcut_enabled {
                        if let Err(error) =
                            controls.configure_quick_capture(Some(&self.quick_settings.shortcut))
                        {
                            self.notify(format!("Everyday shortcut unavailable: {error} Open Capture setup to choose another."), true);
                        }
                    }
                    self.desktop = Some(controls);
                }
                Err(error) => self.notify(error, true),
            }
        }
        let capture_available = self.capture_available();
        let close_available = self.doc.is_some() && self.editor_mutation_allowed();
        let actions = if let Some(controls) = &mut self.desktop {
            let actions = controls.poll();
            let (state, count, source) = self.session.desktop_status();
            controls.update(state, count, &source);
            controls.set_capture_available(capture_available);
            controls.set_close_available(close_available);
            controls.set_capture_cancellable(self.capture_cancel.is_some());
            actions
        } else {
            vec![]
        };
        for action in actions {
            if self.closing {
                continue;
            }
            if action == DesktopAction::Quit {
                self.request(ctx, Pending::Quit);
                continue;
            }
            match action {
                DesktopAction::CloseScreenshot => {
                    if self.doc.is_some() && self.editor_mutation_allowed() {
                        self.request(ctx, Pending::CloseScreenshot);
                    }
                    continue;
                }
                DesktopAction::CancelCapture => {
                    if let Some(flag) = &self.capture_cancel {
                        flag.store(true, Ordering::Release);
                        self.notify("Cancelling capture…", false);
                    }
                    continue;
                }
                DesktopAction::CaptureArea => {
                    self.capture(ctx, CaptureMode::Area);
                    continue;
                }
                DesktopAction::CaptureWindow => {
                    self.capture(ctx, CaptureMode::Window);
                    continue;
                }
                DesktopAction::CaptureScreen => {
                    self.capture(ctx, CaptureMode::Screen);
                    continue;
                }
                DesktopAction::Settings => {
                    if self.pending.is_none() && !self.closing {
                        self.open_setup();
                    }
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                    continue;
                }
                DesktopAction::Review => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                }
                _ => {}
            }
            self.session.desktop_action(action, ctx);
        }
        if self.session.take_started() {
            self.source_epoch = self.source_epoch.wrapping_add(1);
            self.selecting_display = None;
            let result = self
                .desktop
                .as_mut()
                .ok_or("Menu controls are unavailable.".to_string())
                .and_then(|controls| controls.configure_shortcuts(&self.shortcuts_settings, true));
            match result {
                Ok(()) => {
                    self.shortcuts_registered = true;
                    self.session.set_controls_ready(true);
                }
                Err(error) => {
                    self.shortcuts_registered = false;
                    self.session.set_controls_ready(false);
                    self.notify(error, true);
                }
            }
        }
        if !self.session.active() && self.shortcuts_registered {
            if let Some(controls) = &mut self.desktop {
                controls.clear_shortcuts();
            }
            self.shortcuts_registered = false;
        }
    }
    fn text_dialog(&mut self, ctx: &egui::Context) {
        let Some(draft) = &mut self.text_draft else {
            return;
        };
        let editing = draft.index.is_some();
        let mut open = true;
        let mut apply = false;
        let mut cancel = false;
        egui::Window::new(if editing { "Edit text" } else { "Add text" })
            .id(egui::Id::new("text-label-dialog"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(360.0)
            .show(ctx, |ui| {
                let label = ui.label("Text label");
                let response = ui
                    .add(
                        egui::TextEdit::multiline(&mut draft.text)
                            .id(egui::Id::new("text-label-input"))
                            .desired_rows(4)
                            .desired_width(f32::INFINITY)
                            .char_limit(editor::MAX_TEXT_CHARS)
                            .hint_text("Type your label here…"),
                    )
                    .labelled_by(label.id);
                if draft.focus {
                    response.request_focus();
                    draft.focus = false;
                }
                ui.add(egui::Slider::new(&mut draft.size, 8.0..=128.0).text("Text size"));
                ui.horizontal(|ui| {
                    ui.label("Text colour");
                    ui.color_edit_button_srgba(&mut draft.colour);
                });
                if let Some(error) = draft.validation_error() {
                    ui.label(error);
                }
                ui.label("Return adds a new line. Escape cancels your changes.");
                ui.horizontal(|ui| {
                    apply = ui
                        .add_enabled(
                            draft.validation_error().is_none(),
                            egui::Button::new(if editing { "Apply changes" } else { "Add text" }),
                        )
                        .clicked();
                    cancel = ui.button("Cancel").clicked();
                });
            });
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            cancel = true;
        }
        if !open || cancel {
            self.cancel_text_draft();
        } else if apply {
            self.commit_text_draft();
        }
    }
    fn dialogs(&mut self, ctx: &egui::Context, frame: &eframe::Frame) {
        self.text_dialog(ctx);
        if let Some((original, mut draft)) = self.colour_dialog.take() {
            let mut finish = false;
            let mut commit = false;
            egui::Window::new("Highlight colour")
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label("Preview the colour, then apply or cancel.");
                    ui.color_edit_button_srgb(&mut draft);
                    ui.horizontal(|ui| {
                        if ui.button("Apply colour").clicked() {
                            finish = true;
                            commit = true;
                        }
                        if ui.button("Cancel").clicked() {
                            finish = true;
                        }
                    });
                });
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
                finish = true;
            }
            self.update_colour(original, draft, finish, commit);
            if finish {
                if let Some(id) = self.colour_return.take() {
                    ctx.memory_mut(|m| m.request_focus(id));
                }
            }
        }
        if self.pending.is_some() && !self.closing {
            egui::Window::new("Save this screenshot before closing?").collapsible(false).resizable(false).show(ctx,|ui| {
                ui.label("Copying does not save a file. Save a PNG, discard this unsaved result, or cancel to keep editing.");
                ui.add_enabled_ui(self.job.is_none(),|ui| {ui.horizontal(|ui| {
                    if ui.button("Save PNG…").clicked() {self.continue_after_save=true;if self.editing_capture.is_some(){self.save_edited_copy(ctx);}else{self.export(ctx,false);}}
                    if ui.button("Discard changes").clicked() {self.continue_after_save=false;self.saved_revision=self.revision;if let Some(action)=self.pending.take(){self.execute(ctx,action);}}
                    if ui.button("Cancel").clicked() {self.pending=None;self.continue_after_save=false;}
                });});
            });
        }
        if let Some(job) = &self.ocr_job {
            let cancelled = job.cancelled.clone();
            egui::Window::new("Reading current image")
                .collapsible(false)
                .show(ctx, |ui| {
                    ui.spinner();
                    ui.label("Only the rendered crop and visible pixels are read locally.");
                    if ui.button("Cancel extraction").clicked() {
                        cancelled.store(true, Ordering::Release);
                        self.ocr_text = None;
                    }
                });
        }
        if let Some((revision, mut text)) = self.ocr_text.take() {
            let mut keep = true;
            egui::Window::new("Review text before copying").default_width(540.0).show(ctx,|ui| {
                ui.label("Reading order and recognition can be wrong. Edit the result before copying.");
                if text.is_empty() {ui.label("No text was recognised. The clipboard has not changed.");}
                egui::ScrollArea::vertical().max_height(320.0).show(ui,|ui| {ui.add(egui::TextEdit::multiline(&mut text).desired_width(f32::INFINITY).desired_rows(10).char_limit(32768));});
                ui.horizontal(|ui| {
                    if ui.add_enabled(!text.is_empty() && revision==self.revision,egui::Button::new("Copy reviewed text")).clicked() {
                        match arboard::Clipboard::new().and_then(|mut clipboard|{clipboard.set_text(text.clone())?;Ok(clipboard)}) {Ok(clipboard)=>{self.clipboard=Some(clipboard);self.notify("Reviewed text copied.",false);keep=false;},Err(error)=>self.notify(error.to_string(),true)}
                    }
                    if ui.button("Close and forget").clicked(){keep=false;}
                });
            });
            if keep && revision == self.revision {
                self.ocr_text = Some((revision, text));
            }
        }
        if let Some(index) = self.drag_ready {
            let path = self.drag_files[index].path.clone();
            egui::Window::new("Drag reviewed PNG").collapsible(false).show(ctx,|ui| {
            ui.label(&self.drag_label);
            if let Some(preview)=&self.drag_preview {ui.image(preview);}
            ui.label("Only the prepared image is included. The temporary file lasts until Framecraft quits.");
            let response=ui.add(egui::Button::new("Drag frame-0001.png into another app").sense(Sense::drag()));
            if response.drag_started() {if let Err(error)=framecraft::desktop::start_file_drag(frame,&path){self.notify(error,true);}}
            if ui.button("Reveal prepared PNG").clicked(){self.reveal(ctx,&path);}
            if ui.button("Done").clicked(){self.drag_ready=None;}
        });
        }
        if let Some(mut draft) = self.pack.take() {
            let mut keep = true;
            let mut export_to = None;
            egui::Window::new("Review evidence pack").default_width(660.0).default_height(530.0).show(ctx,|ui| {
                ui.label(format!("{} selected PNGs + index.html. Images below are frozen snapshots of the exact output.",draft.frames.len()));
                ui.add_enabled_ui(self.job.is_none(),|ui| {
                    ui.horizontal(|ui| {ui.label("Title");ui.add(egui::TextEdit::singleline(&mut draft.title).char_limit(200));});
                    ui.horizontal(|ui| {ui.label("New folder");ui.text_edit_singleline(&mut draft.name);});
                });
                egui::ScrollArea::vertical().max_height(340.0).show(ui,|ui| {
                    for (i,image) in draft.frames.iter().enumerate() {ui.separator();ui.label(format!("{}. {}",i+1,image.variant_label));ui.image(&draft.previews[i]);ui.label(&image.caption);}
                });
                if self.job.is_some() {ui.spinner();if let Some(cancel)=&self.pack_cancel{if ui.button("Cancel export").clicked(){cancel.store(true,Ordering::Release);}}}
                else {ui.horizontal(|ui| {
                    if ui.button("Choose parent folder and export…").clicked(){export_to=rfd::FileDialog::new().pick_folder();}
                    if ui.button("Back to selection").clicked(){keep=false;}
                });}
            });
            if let Some(parent) = export_to {
                let frames = draft.frames.clone();
                let roots = draft.roots.clone();
                let title = draft.title.clone();
                let name = draft.name.clone();
                let cancel = Arc::new(AtomicBool::new(false));
                let worker_cancel = cancel.clone();
                let (tx, rx) = mpsc::channel();
                self.pack_cancel = Some(cancel);
                self.pack_progress = Some(rx);
                self.start_job(ctx, "Exporting reviewed evidence pack…", move || {
                    output::export_pack(
                        &parent,
                        &name,
                        &title,
                        &frames,
                        &roots,
                        &worker_cancel,
                        |n| {
                            let _ = tx.send(n);
                        },
                    )
                    .map(Outcome::PackExported)
                });
            }
            if keep {
                self.pack = Some(draft);
            }
        }
        if let Some(progress) = &self.pack_progress {
            if let Some(count) = progress.try_iter().last() {
                self.status = format!(
                    "Staged {count} PNGs. The pack is published only after all files succeed."
                );
            }
        }
        if self.settings_open {
            egui::Window::new("Session & app shortcuts")
                .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
                .collapsible(false).resizable(false).default_width(540.0)
                .vscroll(true).max_height((ctx.screen_rect().height() - 90.0).max(240.0))
                .show(ctx, |ui| {
                    ui.label("Command (⌘) and Option (⌥) are the keys beside the space bar.");
                    ui.label(RichText::new("You can always use the Capture Area button or menu bar icon.").color(MUTED));
                    ui.add_space(12.0);
                    ui.strong("Screenshot commands");
                    ui.label(RichText::new("While Framecraft is the active application").small().color(MUTED));
                    egui::Grid::new("capture-shortcut-reference").num_columns(2).spacing([32.0, 6.0]).show(ui, |ui| {
                        for (action, key) in [("Capture Area", "Command + Option + 4"), ("Capture Window", "Command + Option + 5"), ("Capture Screen", "Command + Option + 3"), ("Open Image", "Command + O"), ("Save PNG", "Command + S"), ("Copy Image", "Command + Shift + C")] {
                            ui.label(action); ui.label(key); ui.end_row();
                        }
                    });
                    ui.add_space(14.0); ui.separator(); ui.add_space(6.0);
                    ui.strong("During a capture session");
                    ui.label(RichText::new("These work from other apps after you start a collection in Sessions.").small().color(MUTED));
                    if self.session.active() { ui.label("Stop the session before changing these shortcuts."); }
                    let draft = self.shortcut_draft.get_or_insert_with(|| self.shortcuts_settings.clone());
                    ui.add_enabled_ui(!self.session.active(), |ui| {
                        shortcut_editor(ui, "Capture next frame", &mut draft.capture);
                        shortcut_editor(ui, "Pause or resume", &mut draft.pause_resume);
                        shortcut_editor(ui, "Stop collection", &mut draft.stop);
                    });
                    if let Some(error) = &self.settings_error { ui.colored_label(Color32::from_rgb(177, 39, 35), error); }
                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        if ui.add_enabled(!self.session.active(), egui::Button::new("Save Shortcuts")).clicked() {
                            let candidate = self.shortcut_draft.as_ref().unwrap().clone();
                            let result = candidate.validate()
                                .and_then(|_| if self.quick_settings.setup_complete && self.quick_settings.shortcut_enabled { self.quick_settings.validate(&candidate) } else { Ok(()) })
                                .and_then(|_| self.desktop.as_mut().ok_or("Menu controls are unavailable. Close and reopen Framecraft.")?.configure_shortcuts(&candidate, true))
                                .and_then(|_| candidate.save());
                            if let Some(controls) = &mut self.desktop { controls.clear_shortcuts(); }
                            match result {
                                Ok(()) => { self.shortcuts_settings = candidate; self.shortcut_draft = None; self.settings_open = false; self.settings_error = None; self.notify("Session shortcuts saved. They become active when a collection starts.", false); }
                                Err(error) => self.settings_error = Some(error),
                            }
                        }
                        if ui.add_enabled(!self.session.active(), egui::Button::new("Restore Defaults")).clicked() { self.shortcut_draft = Some(ShortcutSettings::default()); self.settings_error = None; }
                        if ui.button("Close").clicked() { self.shortcut_draft = None; self.settings_error = None; self.settings_open = false; }
                    });
                });
        }
    }
}
fn install_mac_font(_ctx: &egui::Context) {
    #[cfg(all(target_os = "macos", not(test)))]
    {
        // Use the user's installed system font; never package Apple's font files.
        let path = std::path::Path::new("/System/Library/Fonts/SFNS.ttf");
        if std::fs::metadata(path).is_ok_and(|m| m.len() <= 16 * 1024 * 1024) {
            if let Ok(bytes) = std::fs::read(path) {
                if ab_glyph::FontArc::try_from_vec(bytes.clone()).is_ok() {
                    let mut fonts = egui::FontDefinitions::default();
                    fonts.font_data.insert(
                        "mac-system".into(),
                        egui::FontData::from_owned(bytes).into(),
                    );
                    fonts
                        .families
                        .entry(egui::FontFamily::Proportional)
                        .or_default()
                        .insert(0, "mac-system".into());
                    _ctx.set_fonts(fonts);
                }
            }
        }
    }
}

pub(super) fn shortcut_editor(ui: &mut egui::Ui, label: &str, value: &mut String) {
    ui.push_id(label, |ui| {
        let Ok(mut parts) = framecraft::settings::ShortcutParts::parse(value) else {
            ui.label(format!(
                "{label}: choose Restore Defaults to repair this shortcut."
            ));
            return;
        };
        ui.add_space(8.0);
        ui.label(RichText::new(label).strong());
        let before = parts.display();
        ui.horizontal(|ui| {
            ui.checkbox(&mut parts.command, "Command");
            ui.checkbox(&mut parts.option, "Option");
            ui.checkbox(&mut parts.control, "Control");
            ui.checkbox(&mut parts.shift, "Shift");
            egui::ComboBox::from_id_salt("key")
                .selected_text(&parts.key)
                .width(56.0)
                .show_ui(ui, |ui| {
                    for key in ('A'..='Z')
                        .map(|c| c.to_string())
                        .chain((0..=9).map(|n| n.to_string()))
                        .chain((1..=24).map(|n| format!("F{n}")))
                    {
                        ui.selectable_value(&mut parts.key, key.clone(), key);
                    }
                });
        });
        // A shortcut always needs a modifier. Keep the last one selected rather
        // than creating a string the controls cannot parse on their next frame.
        if !(parts.command || parts.option || parts.control || parts.shift) {
            parts = framecraft::settings::ShortcutParts::parse(&before).unwrap();
        }
        let display = parts.display();
        if display != before {
            *value = display.clone();
        }
        ui.label(RichText::new(display).small().color(MUTED));
    });
}

fn reveal_file(path: &std::path::Path) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let status = std::process::Command::new("/usr/bin/open")
            .arg("-R")
            .arg(path)
            .status()
            .map_err(|e| e.to_string())?;
        if status.success() {
            Ok(())
        } else {
            Err("Finder could not reveal the selected file. Check that it still exists.".into())
        }
    }
    #[cfg(not(target_os = "macos"))]
    Err(format!(
        "Reveal in Finder requires macOS. File: {}",
        path.display()
    ))
}
fn prepare_drag(
    snapshot: Snapshot,
    revision: Option<u64>,
    label: String,
) -> Result<Outcome, String> {
    let preview = image::imageops::thumbnail(&snapshot.pixels, 480, 320);
    snapshot
        .drag_file()
        .map(|file| Outcome::DragReady(file, revision, preview, label))
}
fn copy_snapshot(snapshot: Snapshot) -> Result<arboard::Clipboard, String> {
    let mut clipboard = arboard::Clipboard::new().map_err(|e| e.to_string())?;
    clipboard
        .set_image(arboard::ImageData {
            width: snapshot.pixels.width() as usize,
            height: snapshot.pixels.height() as usize,
            bytes: Cow::Owned(snapshot.pixels.into_raw()),
        })
        .map_err(|e| e.to_string())?;
    Ok(clipboard)
}
fn variant_label(variant: &CaptureVariant) -> String {
    match variant {
        CaptureVariant::Original => "Original".into(),
        CaptureVariant::Edited(index) => format!("Saved edited copy {}", index + 1),
    }
}

impl eframe::App for Framecraft {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        self.session.poll(ctx);
        self.native_controls(ctx);
        self.poll(ctx);
        self.poll_ocr(ctx);
        if ctx.input(|i| i.viewport().close_requested()) && !self.allow_close {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.request(ctx, Pending::Quit);
        }
        if self.closing {
            if self.session.is_drained() && self.job.is_none() {
                self.allow_close = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.spinner();
                ui.label("Finishing saved images and metadata before quitting…");
            });
            ctx.request_repaint_after(Duration::from_millis(100));
            return;
        }
        if self.pending.is_some()
            && self.job.is_none()
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
        {
            self.pending = None;
            self.continue_after_save = false;
        }
        self.shortcuts(ctx);
        if self.editor_mutation_allowed() {
            let path = ctx.input(|i| i.raw.dropped_files.first().and_then(|f| f.path.clone()));
            if let Some(path) = path {
                self.open_path(ctx, path);
            }
        }
        self.header(ctx);
        egui::TopBottomPanel::bottom("status")
            .frame(egui::Frame::new().fill(SURFACE).inner_margin(10.0))
            .show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    if self.job.is_some() {
                        ui.spinner();
                    }
                    ui.label(RichText::new(&self.status).small().color(if self.error {
                        Color32::from_rgb(177, 39, 35)
                    } else {
                        MUTED
                    }));
                    if let Some(doc) = &self.doc {
                        if let Ok((w, h)) = editor::dimensions(doc) {
                            ui.label(
                                RichText::new(format!("  /  {w} x {h} px"))
                                    .small()
                                    .color(ACCENT),
                            );
                        }
                    }
                });
            });
        if self.page == Page::Session {
            let interactions = self.editor_mutation_allowed();
            if let Some(action) = self.session.show(ctx, interactions) {
                self.session_action(ctx, action);
            }
        } else {
            if self.doc.is_some() {
                self.toolrail(ctx);
                self.sidebar(ctx);
            }
            self.canvas(ctx);
        }
        self.dialogs(ctx, frame);
        if let Some(mut draft) = self.setup.take() {
            match draft.show(ctx) {
                SetupAction::Cancel => {}
                SetupAction::Finish => {
                    if let Err(error) = self.finish_setup(draft.settings.clone()) {
                        draft.error = Some(error);
                        self.setup = Some(draft);
                    }
                }
                SetupAction::None => self.setup = Some(draft),
            }
        }
    }
}

fn icon_enabled(ui: &mut egui::Ui, enabled: bool, label: &str, icon: Icon) -> egui::Response {
    ui.add_enabled_ui(enabled, |ui| ui_icons::button(ui, label, icon, false))
        .inner
}

fn texture(ctx: &egui::Context, name: &str, pixels: &image::RgbaImage) -> egui::TextureHandle {
    let resized;
    let pixels = if pixels.width() > 4096 || pixels.height() > 4096 {
        resized = image::DynamicImage::ImageRgba8(pixels.clone())
            .thumbnail(4096, 4096)
            .to_rgba8();
        &resized
    } else {
        pixels
    };
    let image = egui::ColorImage::from_rgba_unmultiplied(
        [pixels.width() as usize, pixels.height() as usize],
        pixels.as_raw(),
    );
    ctx.load_texture(name, image, egui::TextureOptions::LINEAR)
}
fn section(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).size(10.0).strong().color(MUTED));
    ui.add_space(6.0);
}
fn pixel_rect(a: Point, b: Point) -> Rect {
    let x = a.x.min(b.x).floor() as u32;
    let y = a.y.min(b.y).floor() as u32;
    Rect {
        x,
        y,
        width: (a.x.max(b.x).ceil() as u32).saturating_sub(x).max(1),
        height: (a.y.max(b.y).ceil() as u32).saturating_sub(y).max(1),
    }
}

fn source_point(pos: Pos2, rect: egui::Rect, width: u32, height: u32) -> Point {
    // Rectangles use exclusive right/bottom edges, so allow width and height.
    Point {
        x: ((pos.x - rect.left()) / rect.width() * width as f32).clamp(0.0, width as f32),
        y: ((pos.y - rect.top()) / rect.height() * height as f32).clamp(0.0, height as f32),
    }
}

fn save_edit(original: &std::path::Path, image: &image::RgbaImage) -> Result<PathBuf, String> {
    let parent = original.parent().ok_or("Capture has no parent folder.")?;
    let stem = original.file_stem().unwrap_or_default().to_string_lossy();
    let mut temporary = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    image
        .write_to(temporary.as_file_mut(), image::ImageFormat::Png)
        .map_err(|e| e.to_string())?;
    temporary.as_file().sync_all().map_err(|e| e.to_string())?;
    for n in 1..=10_000 {
        let path = parent.join(format!("{stem}-edited-{n:02}.png"));
        match temporary.persist_noclobber(&path) {
            Ok(_) => {
                std::fs::File::open(parent)
                    .and_then(|f| f.sync_all())
                    .map_err(|e| {
                        format!(
                            "PNG saved at {} but directory durability is uncertain: {e}",
                            path.display()
                        )
                    })?;
                return Ok(path);
            }
            Err(e) if e.error.kind() == std::io::ErrorKind::AlreadyExists => temporary = e.file,
            Err(e) => return Err(e.error.to_string()),
        }
    }
    Err("Too many edited copies with the same name.".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edited_fixture(tool: Tool) -> (egui::Context, Framecraft) {
        let ctx = egui::Context::default();
        let mut app = Framecraft::with_context(&ctx, true);
        app.tool = tool;
        app.commit(Point { x: 10., y: 10. }, Point { x: 100., y: 80. });
        app.selected = Some(0);
        (ctx, app)
    }
    #[test]
    fn dirty_session_header_actions_fit_minimum_window() {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let mut app = Framecraft::with_context(&ctx, true);
        app.editing_capture = Some(PathBuf::from("/tmp/synthetic-session-frame.png"));
        app.changed();
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    Pos2::ZERO,
                    Vec2::new(900.0, 620.0),
                )),
                ..Default::default()
            },
            |ctx| app.header(ctx),
        );
        let update = output.platform_output.accesskit_update.unwrap();
        for name in [
            "Open Image…",
            "Close screenshot",
            "Copy image",
            "Save PNG…",
            "Save Edited Copy",
            "More Actions",
        ] {
            let node = update
                .nodes
                .iter()
                .find(|(_, node)| node.label() == Some(name))
                .unwrap_or_else(|| panic!("Missing action: {name}"));
            let bounds = node.1.bounds().unwrap();
            assert!(
                bounds.x0 >= 0.0 && bounds.x1 <= 900.0 && bounds.y0 >= 0.0 && bounds.y1 <= 620.0,
                "{name} outside minimum window: {bounds:?}"
            );
        }
    }

    #[test]
    fn setup_blocks_capture_import_and_document_replacement() {
        let ctx = egui::Context::default();
        let mut app = Framecraft::with_context(&ctx, true);
        let revision = app.revision;
        app.open_setup();
        assert!(app.setup.is_some());
        assert!(!app.capture_available());
        assert!(!app.editor_mutation_allowed());
        app.request(&ctx, Pending::CloseScreenshot);
        assert!(app.doc.is_some());
        assert!(app.pending.is_none());
        app.open_settings();
        assert!(!app.settings_open);
        app.setup = None;
        assert_eq!(app.revision, revision);
        assert!(app.editor_mutation_allowed());
    }

    #[test]
    fn auto_save_failure_retains_pixels_as_unsaved_for_manual_recovery() {
        let ctx = egui::Context::default();
        let mut app = Framecraft::with_context(&ctx, false);
        let pixels = image::RgbaImage::from_pixel(8, 8, image::Rgba([1, 2, 3, 255]));
        let (tx, rx) = mpsc::channel();
        app.job = Some(Job { rx });
        tx.send(Ok(Outcome::AutoSaved(
            pixels.clone(),
            Err("Disk full".into()),
        )))
        .unwrap();
        app.poll(&ctx);
        assert_eq!(app.doc.as_ref().unwrap().source, pixels);
        assert!(app.fresh_capture);
        assert!(app.error);
        assert!(app.status.contains("Save PNG"));
        app.request(&ctx, Pending::CloseScreenshot);
        assert!(app.pending.is_some());
        assert!(app.doc.is_some());
    }

    #[test]
    fn auto_saved_capture_is_clean_but_subsequent_edits_are_unsaved() {
        let ctx = egui::Context::default();
        let mut app = Framecraft::with_context(&ctx, false);
        let (tx, rx) = mpsc::channel();
        app.job = Some(Job { rx });
        tx.send(Ok(Outcome::AutoSaved(
            image::RgbaImage::new(8, 8),
            Ok(PathBuf::from("/tmp/Screenshot-20261108-142233.png")),
        )))
        .unwrap();
        app.poll(&ctx);
        assert!(!app.fresh_capture);
        assert_eq!(app.saved_revision, app.revision);
        assert!(app.status.starts_with("Saved /tmp/"));
        app.changed();
        app.request(&ctx, Pending::CloseScreenshot);
        assert!(app.pending.is_some());
    }

    #[test]
    fn text_click_starts_a_draft_without_creating_an_invisible_mark() {
        let ctx = egui::Context::default();
        let mut app = Framecraft::with_context(&ctx, true);
        app.tool = Tool::Text;
        let revision = app.revision;
        app.place_annotation(Point { x: 50.0, y: 50.0 });
        assert!(app.text_draft.is_some());
        assert!(app.doc.as_ref().unwrap().annotations.is_empty());
        assert_eq!(app.revision, revision);
        assert!(!app.editor_mutation_allowed());
        assert!(!app.commit_text_draft());
        app.text_draft.as_mut().unwrap().text = "Review this\nNext step".into();
        assert!(app.commit_text_draft());
        assert_eq!(app.undo.len(), 1);
        assert_eq!(app.doc.as_ref().unwrap().annotations.len(), 1);
        assert!(app.text_draft.is_none());
        app.undo(false);
        assert!(app.doc.as_ref().unwrap().annotations.is_empty());
        app.undo(true);
        assert!(
            matches!(&app.doc.as_ref().unwrap().annotations[0], Annotation::Text { text, position, .. } if text == "Review this\nNext step" && position.x == 50.0)
        );
    }

    #[test]
    fn text_edit_commits_atomically_and_cancel_preserves_pixels() {
        let ctx = egui::Context::default();
        let mut app = Framecraft::with_context(&ctx, true);
        app.tool = Tool::Text;
        app.place_annotation(Point { x: 50.0, y: 50.0 });
        app.text_draft.as_mut().unwrap().text = "Original".into();
        assert!(app.commit_text_draft());
        let original = app.doc.as_ref().unwrap().annotations.clone();
        let revision = app.revision;
        app.begin_text_edit();
        let draft = app.text_draft.as_mut().unwrap();
        draft.text = "Discard this".into();
        draft.size = 40.0;
        draft.colour = Color32::RED;
        app.cancel_text_draft();
        assert_eq!(app.doc.as_ref().unwrap().annotations, original);
        assert_eq!(app.revision, revision);
        assert_eq!(app.undo.len(), 1);
        app.begin_text_edit();
        let draft = app.text_draft.as_mut().unwrap();
        draft.text = "Changed".into();
        draft.size = 40.0;
        draft.colour = Color32::RED;
        assert!(app.commit_text_draft());
        assert_eq!(app.undo.len(), 2);
        assert!(
            matches!(&app.doc.as_ref().unwrap().annotations[0], Annotation::Text { text, font_size, colour, .. } if text == "Changed" && *font_size == 40.0 && *colour == [255, 0, 0, 255])
        );
        app.undo(false);
        assert_eq!(app.doc.as_ref().unwrap().annotations, original);
        app.undo(true);
        assert!(
            matches!(&app.doc.as_ref().unwrap().annotations[0], Annotation::Text { text, .. } if text == "Changed")
        );
    }

    #[test]
    fn text_draft_rejects_invalid_content_and_protects_against_other_commands() {
        let ctx = egui::Context::default();
        let mut app = Framecraft::with_context(&ctx, true);
        app.tool = Tool::Text;
        app.place_annotation(Point { x: 50.0, y: 50.0 });
        for value in [
            " ".to_owned(),
            "a\n".repeat(editor::MAX_TEXT_LINES),
            "bad\tcontrol".into(),
            "a".repeat(editor::MAX_TEXT_CHARS + 1),
        ] {
            app.text_draft.as_mut().unwrap().text = value;
            assert!(!app.commit_text_draft());
            assert!(app.doc.as_ref().unwrap().annotations.is_empty());
            assert!(app.undo.is_empty());
        }
        app.request(&ctx, Pending::Quit);
        app.open_settings();
        assert!(!app.closing);
        assert!(app.pending.is_none());
        assert!(!app.settings_open);
        assert!(!app.capture_available());
        app.load(ctx, image::RgbaImage::new(20, 20), "next".into());
        assert!(app.text_draft.is_none());
    }

    #[test]
    fn text_dialog_focus_accepts_typing_and_escape_discards_only_draft() {
        let ctx = egui::Context::default();
        let mut app = Framecraft::with_context(&ctx, true);
        app.tool = Tool::Text;
        app.place_annotation(Point { x: 50.0, y: 50.0 });
        let revision = app.revision;
        let _ = ctx.run(egui::RawInput::default(), |ctx| app.text_dialog(ctx));
        assert!(ctx.memory(|m| m.has_focus(egui::Id::new("text-label-input"))));
        let _ = ctx.run(
            egui::RawInput {
                events: vec![egui::Event::Text("A typed label".into())],
                ..Default::default()
            },
            |ctx| {
                app.shortcuts(ctx);
                app.text_dialog(ctx);
            },
        );
        assert_eq!(app.text_draft.as_ref().unwrap().text, "A typed label");
        let _ = ctx.run(
            egui::RawInput {
                events: vec![egui::Event::Key {
                    key: egui::Key::Escape,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                }],
                ..Default::default()
            },
            |ctx| {
                app.shortcuts(ctx);
                app.text_dialog(ctx);
            },
        );
        assert!(app.text_draft.is_none());
        assert!(app.doc.as_ref().unwrap().annotations.is_empty());
        assert!(app.undo.is_empty());
        assert_eq!(app.revision, revision);
    }

    #[test]
    fn unchanged_text_apply_does_not_dirty_a_saved_document() {
        let ctx = egui::Context::default();
        let mut app = Framecraft::with_context(&ctx, true);
        app.tool = Tool::Text;
        app.place_annotation(Point { x: 50.0, y: 50.0 });
        app.text_draft.as_mut().unwrap().text = "Original".into();
        app.commit_text_draft();
        app.saved_revision = app.revision;
        let revision = app.revision;
        app.begin_text_edit();
        assert!(app.commit_text_draft());
        assert_eq!(app.revision, revision);
        assert_eq!(app.saved_revision, revision);
        assert_eq!(app.undo.len(), 1);
    }

    #[test]
    fn missing_permission_preserves_edits_and_opens_recovery_without_a_capture_job() {
        let (_, mut app) = edited_fixture(Tool::Highlight);
        let revision = app.revision;
        let pixels = editor::render(app.doc.as_ref().unwrap()).unwrap();
        for status in [
            capture::PermissionStatus::NotGranted,
            capture::PermissionStatus::Unknown,
        ] {
            assert!(!app.check_screen_access(status));
            assert!(app.screen_access_open);
            assert!(app.job.is_none());
            assert!(app.capture_cancel.is_none());
            assert!(app.pending.is_none());
            assert_eq!(app.revision, revision);
            assert_eq!(editor::render(app.doc.as_ref().unwrap()).unwrap(), pixels);
        }
        assert!(app.check_screen_access(capture::PermissionStatus::Granted));
        assert!(
            app.job.is_none(),
            "permission checks never auto-start capture"
        );
    }
    fn complete(app: &mut Framecraft, ctx: &egui::Context, result: Result<Outcome, String>) {
        let (tx, rx) = mpsc::channel();
        app.job = Some(Job { rx });
        tx.send(result).unwrap();
        app.poll(ctx);
    }
    #[test]
    fn replacement_clears_abandoned_transaction_state() {
        let (ctx, mut app) = edited_fixture(Tool::Pixelate);
        app.begin_block();
        app.block_error = Some("invalid".into());
        app.load(ctx, image::RgbaImage::new(20, 20), "next".into());
        assert!(app.property_block_before.is_none());
        assert!(app.property_before.is_none());
        assert!(app.block_error.is_none());
        assert!(app.gesture_before.is_none());
    }

    #[test]
    fn failed_and_stale_save_cannot_close_or_acknowledge_newer_work() {
        let (ctx, mut app) = edited_fixture(Tool::Highlight);
        app.pending = Some(Pending::CloseScreenshot);
        app.continue_after_save = true;
        complete(&mut app, &ctx, Err("disk full".into()));
        assert!(app.doc.is_some());
        assert!(app.pending.is_some());
        assert!(!app.continue_after_save);
        assert!(app.error);
        let stale = app.revision;
        app.changed();
        app.continue_after_save = true;
        complete(
            &mut app,
            &ctx,
            Ok(Outcome::Exported(PathBuf::from("saved.png"), stale)),
        );
        assert!(app.doc.is_some());
        assert_ne!(app.saved_revision, app.revision);
        let current = app.revision;
        complete(
            &mut app,
            &ctx,
            Ok(Outcome::Exported(PathBuf::from("saved.png"), current)),
        );
        assert!(app.doc.is_none());
        assert!(!app.closing);
    }
    #[test]
    fn colour_preview_cancel_restores_clean_state_and_commit_is_one_undo() {
        let (_, mut app) = edited_fixture(Tool::Highlight);
        app.saved_revision = app.revision;
        let original = app.highlight_value();
        let count = app.undo.len();
        app.begin_colour();
        app.update_colour(original, [0, 120, 240], false, false);
        assert_eq!(app.highlight_value(), [0, 120, 240]);
        assert_eq!(app.undo.len(), count);
        app.update_colour(original, [0, 120, 240], true, false);
        assert_eq!(app.highlight_value(), original);
        assert_eq!(app.saved_revision, app.revision);
        assert_eq!(app.undo.len(), count);
        app.begin_colour();
        app.update_colour(original, [240, 20, 30], false, false);
        app.update_colour(original, [240, 20, 30], true, true);
        assert_eq!(app.undo.len(), count + 1);
        app.undo(false);
        assert_eq!(app.highlight_value(), original);
    }
    #[test]
    fn next_colour_cancel_does_not_dirty_document() {
        let (_, mut app) = edited_fixture(Tool::Highlight);
        app.selected = None;
        let rev = app.revision;
        let original = app.highlight_colour;
        app.begin_colour();
        app.update_colour(original, [1, 2, 3], false, false);
        app.update_colour(original, [1, 2, 3], true, false);
        assert_eq!(app.highlight_colour, original);
        assert_eq!(app.revision, rev);
    }
    #[test]
    fn block_transaction_validates_rolls_back_and_commits_once() {
        let (_, mut app) = edited_fixture(Tool::Pixelate);
        app.saved_revision = app.revision;
        let count = app.undo.len();
        app.begin_block();
        app.preview_block(48);
        app.block_input = "999".into();
        assert!(app.parse_block_input().is_none());
        assert_eq!(app.block_value(), 48);
        app.finish_block(false);
        assert_eq!(app.block_value(), 12);
        assert_eq!(app.saved_revision, app.revision);
        assert_eq!(app.undo.len(), count);
        app.begin_block();
        app.preview_block(32);
        app.preview_block(64);
        app.finish_block(true);
        assert_eq!(app.undo.len(), count + 1);
        app.undo(false);
        app.selected = Some(0);
        assert_eq!(app.block_value(), 12);
        assert_eq!(app.pixelate_block, 12);
    }
    #[test]
    fn next_block_cancel_preserves_document_and_defaults() {
        let (_, mut app) = edited_fixture(Tool::Pixelate);
        app.selected = None;
        let rev = app.revision;
        app.preview_block(64);
        app.finish_block(false);
        assert_eq!(app.pixelate_block, 12);
        assert_eq!(app.revision, rev);
    }
    #[test]
    fn cancelled_move_preserves_redo_and_clean_state() {
        let (_, mut app) = edited_fixture(Tool::Redact);
        app.saved_revision = app.revision;
        app.redo.push(Edits::read(app.doc.as_ref().unwrap()));
        app.moving = Some(app.doc.as_ref().unwrap().annotations[0].clone());
        app.begin_move();
        editor::translate_annotation(
            &mut app.doc.as_mut().unwrap().annotations[0],
            Point { x: 4., y: 5. },
            1200,
            750,
        )
        .unwrap();
        app.changed();
        app.cancel_tool();
        assert_eq!(app.redo.len(), 1);
        assert_eq!(app.saved_revision, app.revision);
    }
    #[test]
    fn colour_modal_guards_replacement_and_capture() {
        let (ctx, mut app) = edited_fixture(Tool::Highlight);
        app.begin_colour();
        assert!(!app.capture_available());
        app.request(&ctx, Pending::CloseScreenshot);
        assert!(app.pending.is_none());
        assert!(app.doc.is_some());
    }
    #[test]
    fn tiny_pixelation_does_not_enter_history() {
        let (_, mut app) = edited_fixture(Tool::Pixelate);
        let count = app.undo.len();
        app.commit(Point { x: 1., y: 1. }, Point { x: 2., y: 2. });
        assert_eq!(app.undo.len(), count);
        assert_eq!(app.doc.as_ref().unwrap().annotations.len(), 1);
    }

    #[test]
    fn fresh_capture_close_is_guarded_but_saved_image_can_close() {
        let ctx = egui::Context::default();
        let mut app = Framecraft::with_context(&ctx, false);
        app.load(ctx.clone(), image::RgbaImage::new(80, 60), "capture".into());
        app.fresh_capture = true;
        app.request(&ctx, Pending::CloseScreenshot);
        assert!(matches!(app.pending, Some(Pending::CloseScreenshot)));
        assert!(app.doc.is_some());
        app.pending = None;
        app.fresh_capture = false;
        app.request(&ctx, Pending::CloseScreenshot);
        assert!(app.doc.is_none());
        assert!(!app.closing);
    }
    #[test]
    fn cancel_move_restores_original_without_creating_redo_edit() {
        let ctx = egui::Context::default();
        let mut app = Framecraft::with_context(&ctx, true);
        app.tool = Tool::Redact;
        app.commit(Point { x: 10., y: 10. }, Point { x: 40., y: 40. });
        let original = app.doc.as_ref().unwrap().annotations[0].clone();
        app.selected = Some(0);
        app.moving = Some(original.clone());
        app.begin_move();
        editor::translate_annotation(
            &mut app.doc.as_mut().unwrap().annotations[0],
            Point { x: 20., y: 20. },
            1200,
            750,
        )
        .unwrap();
        app.cancel_tool();
        assert_eq!(app.doc.as_ref().unwrap().annotations[0], original);
        assert!(app.redo.is_empty());
        assert_eq!(app.undo.len(), 1);
    }
    #[test]
    fn selected_highlight_changes_neither_next_default_nor_other_marks() {
        let ctx = egui::Context::default();
        let mut app = Framecraft::with_context(&ctx, true);
        app.tool = Tool::Highlight;
        app.commit(Point { x: 10., y: 10. }, Point { x: 40., y: 40. });
        app.selected = Some(0);
        let next = app.highlight_colour;
        app.set_highlight_colour([255, 0, 20]);
        assert_eq!(app.highlight_colour, next);
        assert!(matches!(
            app.doc.as_ref().unwrap().annotations[0],
            Annotation::Highlight {
                colour: [255, 0, 20, 80],
                ..
            }
        ));
        app.undo(false);
        assert!(
            matches!(app.doc.as_ref().unwrap().annotations[0],Annotation::Highlight{colour,..} if colour[..3]==next)
        );
    }

    #[test]
    fn first_launch_starts_with_single_capture_and_dialogs_gate_it() {
        let ctx = egui::Context::default();
        let mut app = Framecraft::with_context(&ctx, false);
        assert!(app.page == Page::Editor);
        assert!(app.doc.is_none());
        assert!(app.capture_available());
        app.settings_open = true;
        assert!(!app.capture_available());
        app.settings_open = false;
        app.pending = Some(Pending::Quit);
        assert!(!app.capture_available());
    }

    #[test]
    fn menu_capture_prompts_before_replacing_edits_and_repeated_capture_keeps_prompt() {
        let ctx = egui::Context::default();
        let mut app = Framecraft::with_context(&ctx, true);
        app.changed();
        app.page = Page::Session;
        app.capture_with_permission(&ctx, CaptureMode::Area, || {
            capture::PermissionStatus::Granted
        });
        assert!(matches!(
            app.pending,
            Some(Pending::Capture(CaptureMode::Area))
        ));
        assert!(app.job.is_none());
        app.capture_with_permission(&ctx, CaptureMode::Screen, || {
            capture::PermissionStatus::Granted
        });
        assert!(matches!(
            app.pending,
            Some(Pending::Capture(CaptureMode::Area))
        ));
        assert!(app.doc.is_some());
    }

    #[test]
    fn shortcut_draft_does_not_change_the_bindings_used_by_a_session() {
        let ctx = egui::Context::default();
        let mut app = Framecraft::with_context(&ctx, false);
        let committed = app.shortcuts_settings.clone();
        app.open_settings();
        app.shortcut_draft.as_mut().unwrap().capture = "Control + Shift + F9".into();
        assert_eq!(app.shortcuts_settings, committed);
        assert!(!app.editor_mutation_allowed());
        app.open_settings();
        assert_eq!(
            app.shortcut_draft.as_ref().unwrap().capture,
            "Control + Shift + F9"
        );
    }

    #[test]
    fn newly_loaded_image_keeps_original_dimensions_until_styling_is_chosen() {
        let ctx = egui::Context::default();
        let mut app = Framecraft::with_context(&ctx, false);
        app.load(ctx, image::RgbaImage::new(80, 60), "fixture".into());
        assert_eq!(
            editor::dimensions(app.doc.as_ref().unwrap()).unwrap(),
            (80, 60)
        );
    }

    #[test]
    fn started_then_stopped_session_still_rejects_old_source_selection() {
        let ctx = egui::Context::default();
        let mut app = Framecraft::with_context(&ctx, false);
        app.source_epoch = 2;
        let (tx, rx) = mpsc::channel();
        app.job = Some(Job { rx });
        tx.send(Ok(Outcome::RegionSource(
            image::RgbaImage::new(10, 10),
            1,
            1,
        )))
        .unwrap();
        app.poll(&ctx);
        assert!(app.selecting_display.is_none());
        assert!(app.doc.is_none());
        assert!(app.session.target.is_none());
    }
    #[test]
    fn stale_drag_completion_cannot_resurface_a_previous_revision() {
        let ctx = egui::Context::default();
        let mut app = Framecraft::with_context(&ctx, true);
        let revision = app.revision;
        let file = Snapshot::pixels(image::RgbaImage::from_pixel(
            2,
            2,
            image::Rgba([255, 0, 0, 255]),
        ))
        .unwrap()
        .drag_file()
        .unwrap();
        let path = file.path.clone();
        let (tx, rx) = mpsc::channel();
        app.job = Some(Job { rx });
        app.changed();
        tx.send(Ok(Outcome::DragReady(
            file,
            Some(revision),
            image::RgbaImage::new(2, 2),
            "Current edits".into(),
        )))
        .unwrap();
        app.poll(&ctx);
        assert!(app.drag_ready.is_none());
        assert!(app.drag_files.is_empty());
        assert!(!path.exists());
    }
    #[test]
    fn pending_operations_disable_document_mutation() {
        let ctx = egui::Context::default();
        let mut app = Framecraft::with_context(&ctx, true);
        let (_tx, rx) = mpsc::channel();
        app.job = Some(Job { rx });
        assert!(!app.editor_mutation_allowed());
        app.job = None;
        assert!(app.editor_mutation_allowed());
        app.pending = Some(Pending::Quit);
        assert!(!app.editor_mutation_allowed());
    }
    #[test]
    fn changing_a_redaction_invalidates_all_reviewed_pixels_and_handoff() {
        let ctx = egui::Context::default();
        let mut app = Framecraft::with_context(&ctx, true);
        app.tool = Tool::Redact;
        app.commit(Point { x: 4.0, y: 4.0 }, Point { x: 20.0, y: 20.0 });
        app.flat_texture = app.source_texture.clone();
        app.rendered_revision = Some(app.revision);
        assert!(app.handoff_ready());
        app.selected = Some(0);
        app.move_selected(Point { x: 40.0, y: 0.0 });
        assert!(app.flat_texture.is_none());
        assert!(app.preview_texture.is_none());
        assert!(!app.handoff_ready());
        let _ = ctx.run(egui::RawInput::default(), |ctx| app.canvas(ctx));
    }
    #[test]
    fn selecting_a_step_preserves_its_radius_in_properties() {
        let ctx = egui::Context::default();
        let mut app = Framecraft::with_context(&ctx, true);
        app.tool = Tool::Step;
        app.text_size = 44.0;
        app.place_annotation(Point { x: 100.0, y: 100.0 });
        app.text_size = 12.0;
        app.sync_selected();
        assert_eq!(app.text_size, 44.0);
    }
    #[test]
    fn session_page_does_not_consume_editor_undo_delete_or_escape() {
        let ctx = egui::Context::default();
        let mut app = Framecraft::with_context(&ctx, true);
        app.tool = Tool::Redact;
        app.commit(Point { x: 4.0, y: 4.0 }, Point { x: 20.0, y: 20.0 });
        app.selected = Some(0);
        app.page = Page::Session;
        let revision = app.revision;
        for (key, modifiers) in [
            (egui::Key::Z, egui::Modifiers::COMMAND),
            (egui::Key::Delete, egui::Modifiers::NONE),
            (egui::Key::Escape, egui::Modifiers::NONE),
        ] {
            let _ = ctx.run(
                egui::RawInput {
                    events: vec![egui::Event::Key {
                        key,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers,
                    }],
                    modifiers,
                    ..Default::default()
                },
                |ctx| app.shortcuts(ctx),
            );
        }
        assert_eq!(app.revision, revision);
        assert_eq!(app.doc.as_ref().unwrap().annotations.len(), 1);
        assert_eq!(app.selected, Some(0));
    }
    #[test]
    fn text_step_move_delete_undo_retains_composited_redaction() {
        let ctx = egui::Context::default();
        let mut app = Framecraft::with_context(&ctx, true);
        app.tool = Tool::Text;
        app.place_annotation(Point { x: 50.0, y: 50.0 });
        app.text_draft.as_mut().unwrap().text = "Review this".into();
        assert!(app.commit_text_draft());
        app.move_selected(Point { x: 10.0, y: 5.0 });
        assert!(
            matches!(&app.doc.as_ref().unwrap().annotations[0],Annotation::Text{position,..} if position.x==60.0)
        );
        app.delete_selected();
        assert!(app.doc.as_ref().unwrap().annotations.is_empty());
        app.undo(false);
        assert_eq!(app.doc.as_ref().unwrap().annotations.len(), 1);
        app.tool = Tool::Step;
        app.place_annotation(Point { x: 100.0, y: 100.0 });
        assert_eq!(app.step_number, 2);
        app.undo(false);
        assert_eq!(app.doc.as_ref().unwrap().annotations.len(), 1);
        app.undo(true);
        assert_eq!(app.doc.as_ref().unwrap().annotations.len(), 2);
        app.tool = Tool::Redact;
        app.commit(Point { x: 0.0, y: 0.0 }, Point { x: 200.0, y: 200.0 });
        let output = Snapshot::current(app.doc.as_ref().unwrap()).unwrap();
        assert!(output.pixels.width() > 0);
    }
    #[test]
    fn replacing_dirty_document_requires_a_decision() {
        let ctx = egui::Context::default();
        let mut app = Framecraft::with_context(&ctx, true);
        app.tool = Tool::Redact;
        app.commit(Point { x: 4.0, y: 4.0 }, Point { x: 20.0, y: 20.0 });
        app.open_path(&ctx, PathBuf::from("not-opened.png"));
        assert!(matches!(app.pending, Some(Pending::Open(_))));
        assert!(app.job.is_none());
        assert_eq!(app.doc.as_ref().unwrap().annotations.len(), 1);
    }
    #[test]
    fn stale_and_cancelled_ocr_never_create_copyable_text() {
        let ctx = egui::Context::default();
        let mut app = Framecraft::with_context(&ctx, true);
        for (revision, cancelled) in [
            (app.revision.saturating_sub(1), false),
            (app.revision, true),
        ] {
            let (tx, rx) = mpsc::channel();
            tx.send(Ok("SECRET stale text".into())).unwrap();
            app.ocr_job = Some(OcrJob {
                revision,
                cancelled: Arc::new(AtomicBool::new(cancelled)),
                rx,
            });
            app.poll_ocr(&ctx);
            assert!(app.ocr_text.is_none());
            assert!(app.clipboard.is_none());
        }
    }
    #[test]
    fn changed_document_forgets_recognised_text_and_drag_selection() {
        let ctx = egui::Context::default();
        let mut app = Framecraft::with_context(&ctx, true);
        app.ocr_text = Some((app.revision, "private".into()));
        app.drag_ready = Some(0);
        app.changed();
        assert!(app.ocr_text.is_none());
        assert!(app.drag_ready.is_none());
    }
    #[test]
    fn use_selected_area_retains_display_and_pixel_bounds() {
        let ctx = egui::Context::default();
        let mut app = Framecraft::with_context(&ctx, true);
        let rect = Rect {
            x: 40,
            y: 30,
            width: 300,
            height: 150,
        };
        app.doc.as_mut().unwrap().crop = Some(rect);
        app.selecting_display = Some(2);
        app.use_session_area();
        let target = app.session.target.unwrap();
        assert_eq!(target.display, 2);
        assert_eq!(target.region, rect);
        assert!(app.page == Page::Session);
        assert!(app.selecting_display.is_none());
    }

    #[test]
    fn quick_edit_save_preserves_original_and_never_clobbers_prior_edits() {
        let temp = tempfile::tempdir().unwrap();
        let original = temp.path().join("original.png");
        let pixels = image::RgbaImage::from_pixel(2, 2, image::Rgba([255, 0, 0, 255]));
        pixels.save(&original).unwrap();
        let bytes = std::fs::read(&original).unwrap();
        let edited = image::RgbaImage::from_pixel(2, 2, image::Rgba([0, 0, 0, 255]));
        let a = save_edit(&original, &edited).unwrap();
        let b = save_edit(&original, &edited).unwrap();
        assert_ne!(a, b);
        assert_eq!(std::fs::read(&original).unwrap(), bytes);
        assert_eq!(editor::load_image(&a).unwrap(), edited);
    }

    #[test]
    fn session_layout_runs_at_minimum_size() {
        let ctx = egui::Context::default();
        let mut app = Framecraft::with_context(&ctx, false);
        for _ in 0..2 {
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        Pos2::ZERO,
                        Vec2::new(900.0, 620.0),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    app.header(ctx);
                    app.session.show(ctx, true);
                },
            );
            assert!(!output.shapes.is_empty());
        }
    }

    #[test]
    fn drag_to_canvas_edge_redacts_last_source_pixel() {
        let canvas = egui::Rect::from_min_size(Pos2::new(40.0, 20.0), Vec2::new(200.0, 100.0));
        let end = source_point(canvas.right_bottom(), canvas, 800, 400);
        let rect = pixel_rect(Point { x: 790.0, y: 390.0 }, end);
        assert_eq!(
            rect,
            Rect {
                x: 790,
                y: 390,
                width: 10,
                height: 10
            }
        );
        let doc = Document {
            source: image::RgbaImage::from_pixel(800, 400, image::Rgba([255, 0, 0, 255])),
            crop: None,
            annotations: vec![Annotation::Redact { rect }],
            style: Style {
                padding: 0,
                radius: 0,
                shadow: false,
                theme: Theme::Paper,
            },
        };
        let output = editor::render(&doc).unwrap();
        assert_ne!(output.get_pixel(799, 399).0, [255, 0, 0, 255]);
        assert_eq!(output.get_pixel(799, 399).0[3], 255);
    }

    #[test]
    fn zero_area_boundary_gesture_does_not_add_invalid_crop() {
        let ctx = egui::Context::default();
        let mut app = Framecraft::with_context(&ctx, true);
        app.tool = Tool::Crop;
        let w = app.doc.as_ref().unwrap().source.width() as f32;
        app.commit(Point { x: w, y: 0.0 }, Point { x: w, y: 50.0 });
        assert!(app.doc.as_ref().unwrap().crop.is_none());
        assert!(app.undo.is_empty());
    }

    #[test]
    fn redaction_invalidates_preview_and_survives_undo_redo() {
        let ctx = egui::Context::default();
        let mut app = Framecraft::with_context(&ctx, true);
        app.preview_texture = app.source_texture.clone();
        app.tool = Tool::Redact;
        app.commit(Point { x: 10.0, y: 10.0 }, Point { x: 50.0, y: 50.0 });
        assert!(app.preview_texture.is_none());
        assert_eq!(app.doc.as_ref().unwrap().annotations.len(), 1);
        app.undo(false);
        assert!(app.doc.as_ref().unwrap().annotations.is_empty());
        app.undo(true);
        assert!(matches!(
            app.doc.as_ref().unwrap().annotations[0],
            Annotation::Redact { .. }
        ));
    }

    #[test]
    fn ui_layout_runs_at_minimum_window_size() {
        let ctx = egui::Context::default();
        let mut app = Framecraft::with_context(&ctx, true);
        for _ in 0..2 {
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        Pos2::ZERO,
                        Vec2::new(900.0, 620.0),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    app.header(ctx);
                    app.sidebar(ctx);
                    app.canvas(ctx);
                },
            );
            assert!(!output.shapes.is_empty());
        }
    }
}
