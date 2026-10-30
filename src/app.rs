use crate::session_ui::{Action as SessionAction, ReviewFrame, SessionView};
use eframe::egui::{self, Color32, Pos2, RichText, Sense, Stroke, Vec2};
use framecraft::desktop::{DesktopAction, DesktopControls};
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

const ACCENT: Color32 = Color32::from_rgb(0, 102, 220);
const MUTED: Color32 = Color32::from_rgb(91, 99, 112);
const INK: Color32 = Color32::from_rgb(30, 35, 43);
const SURFACE: Color32 = Color32::from_rgb(248, 249, 251);
const BORDER: Color32 = Color32::from_rgb(218, 222, 229);

#[derive(Clone, Copy, PartialEq)]
enum Tool {
    Select,
    Arrow,
    Box,
    Redact,
    Crop,
    Highlight,
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

pub struct Framecraft {
    page: Page,
    session: SessionView,
    selecting_display: Option<u32>,
    source_epoch: u64,
    editing_capture: Option<PathBuf>,
    editing_handle: Option<CaptureHandle>,
    selected: Option<usize>,
    moving: Option<Annotation>,
    text_input: String,
    text_size: f32,
    step_number: u32,
    flat_texture: Option<egui::TextureHandle>,
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
        Self::with_context(&cc.egui_ctx, demo)
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
            widget.corner_radius = egui::CornerRadius::same(6);
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
            text_input: String::new(),
            text_size: 28.0,
            step_number: 1,
            flat_texture: None,
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
    fn open_settings(&mut self) {
        if !self.settings_open {
            self.shortcut_draft = Some(self.shortcuts_settings.clone());
            self.settings_error = None;
            self.settings_open = true;
        }
    }
    fn capture_available(&self) -> bool {
        self.editor_mutation_allowed() && !self.session.active()
    }
    fn capture(&mut self, ctx: &egui::Context, mode: CaptureMode) {
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
        self.request(ctx, Pending::Capture(mode));
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
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        let restore = ctx.clone();
        self.start_job(
            ctx,
            "Select an area or window. Escape cancels.",
            move || {
                // Give the compositor a moment to remove our editor from the desktop.
                std::thread::sleep(Duration::from_millis(350));
                let result = capture::capture(mode).map(Outcome::Captured);
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
                .set_file_name("Framecraft.png")
                .add_filter("PNG", &["png"])
                .save_file()
            {
                // Preserve the exact path approved by the save dialog.
                // The file is encoded as PNG regardless of its chosen suffix.
                Some(path) => Some(path),
                None => return,
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
                    self.load(ctx.clone(), pixels, "New capture".into())
                }
                Ok(Outcome::Captured(None)) => self.notify("Capture cancelled.", false),
                Ok(Outcome::Exported(path, revision)) => {
                    if revision == self.revision {
                        self.saved_revision = revision;
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
                        let mut flat = doc;
                        flat.crop = None;
                        flat.style.padding = 0;
                        flat.style.radius = 0;
                        flat.style.shadow = false;
                        editor::render(&flat).map(|flat| (output, flat))
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
    }
    fn shortcuts(&mut self, ctx: &egui::Context) {
        if !self.closing
            && self.pending.is_none()
            && self.job.is_none()
            && !ctx.wants_keyboard_input()
        {
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Comma)) {
                self.open_settings();
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
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            if self.moving.take().is_some() {
                self.undo(false);
            }
            self.drag = None;
            self.freehand.clear();
            self.selected = None;
            self.tool = Tool::Select;
        }
    }
    fn header(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("header")
            .frame(egui::Frame::new().fill(SURFACE).inner_margin(14.0))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Framecraft").size(18.0).strong());
                    ui.add_space(16.0);
                    ui.selectable_value(&mut self.page, Page::Editor, "Screenshot");
                    ui.selectable_value(&mut self.page, Page::Session, "Sessions");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Keyboard Shortcuts…").clicked() { self.open_settings(); }
                        ui.add_enabled_ui(self.capture_available(), |ui| {
                            ui.menu_button("More Capture", |ui| {
                                for (label, mode) in [("Capture Window…", CaptureMode::Window), ("Capture Screen", CaptureMode::Screen)] {
                                    if ui.button(label).clicked() { ui.close_menu(); self.capture(ctx, mode); }
                                }
                            });
                            if ui.add(egui::Button::new(RichText::new("Capture Area…").color(Color32::WHITE)).fill(ACCENT))
                                .on_hover_text("Draw a rectangle on screen. Escape cancels. Command + Option + 4.").clicked() {
                                self.capture(ctx, CaptureMode::Area);
                            }
                        });
                    });
                });
                if self.session.active() {
                    ui.add_space(6.0);
                    ui.label(RichText::new("A capture session is active. Stop it in Sessions before taking a separate screenshot.").small().color(MUTED));
                }
                if self.page == Page::Editor && self.doc.is_some() {
                    ui.add_space(12.0);
                    ui.separator();
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if ui.add_enabled(self.editor_mutation_allowed(), egui::Button::new("Open Image…")).clicked() { self.open(ctx); }
                        if self.saved_revision != self.revision { ui.label(RichText::new("Unsaved edits").small().color(MUTED)); }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let ready = self.handoff_ready() && self.editor_mutation_allowed();
                            if ui.add_enabled(ready, egui::Button::new(RichText::new("Copy Image").color(Color32::WHITE)).fill(ACCENT)).clicked() { self.export(ctx, true); }
                            if ui.add_enabled(ready, egui::Button::new("Save PNG…")).clicked() { self.export(ctx, false); }
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
                    });
                }
            });
    }
    fn sidebar(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("controls")
            .exact_width(236.0)
            .resizable(false)
            .frame(egui::Frame::new().fill(SURFACE).inner_margin(16.0))
            .show(ctx, |ui| {
                if !self.editor_mutation_allowed() {
                    ui.disable();
                }
                egui::ScrollArea::vertical().show(ui, |ui| {
                    section(ui, "Annotate");
                    ui.add_enabled_ui(self.doc.is_some(), |ui| {
                        egui::Grid::new("annotation-tools")
                            .num_columns(2)
                            .spacing([6.0, 6.0])
                            .show(ui, |ui| {
                                for (index, (tool, label, detail)) in [
                            (
                                Tool::Select,
                                "Select / move",
                                "Select a mark to edit, move or delete",
                            ),
                            (Tool::Text, "Text", "Click to place a text label"),
                            (
                                Tool::Step,
                                "Numbered step",
                                "Click to place a numbered marker",
                            ),
                            (Tool::Arrow, "Arrow", "Point to what matters"),
                            (Tool::Pen, "Draw", "Draw freehand"),
                            (
                                Tool::Highlight,
                                "Highlight",
                                "Highlight without obscuring text",
                            ),
                            (Tool::Box, "Rectangle", "Frame a detail"),
                            (
                                Tool::Redact,
                                "Redact",
                                "Hide a detail in the exported image; the original is retained",
                            ),
                            (Tool::Crop, "Crop", "Keep the useful part"),
                        ].into_iter().enumerate() {
                            let chosen = self.tool == tool;
                            if ui
                                .add_sized([99.0, 34.0], egui::Button::new(label).selected(chosen))
                                .on_hover_text(detail)
                                .clicked()
                            {
                                self.tool = tool;
                                self.preview = false;
                                self.drag = None;
                                self.freehand.clear();
                            }
                            if index % 2 == 1 { ui.end_row(); }
                        }
                            });
                        self.annotation_controls(ui);
                        ui.add_space(6.0);
                        if !matches!(self.tool, Tool::Select | Tool::Crop) {
                            ui.horizontal(|ui| {
                                ui.label("Ink");
                                ui.color_edit_button_srgba(&mut self.colour);
                            });
                            ui.add(egui::Slider::new(&mut self.stroke, 2.0..=16.0).text("Weight"));
                        }
                        ui.horizontal(|ui| {
                            if ui
                                .add_enabled(!self.undo.is_empty(), egui::Button::new("Undo"))
                                .clicked()
                            {
                                self.undo(false);
                            }
                            if ui
                                .add_enabled(!self.redo.is_empty(), egui::Button::new("Redo"))
                                .clicked()
                            {
                                self.undo(true);
                            }
                        });
                    });
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
                    .fill(Color32::from_rgb(237, 240, 245))
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
                                Tool::Select => { self.selected=editor::hit_test(&self.doc.as_ref().unwrap().annotations,p,6.0/scale); self.sync_selected(); }
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
                                if self.moving.is_some() {self.checkpoint();}
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
                        if response.drag_stopped() {self.drag=None;self.moving=None;}
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
                let gap = ((ui.available_width() - 470.0) / 2.0).max(0.0);
                ui.add_space(gap);
                ui.add_enabled_ui(self.capture_available(), |ui| {
                    if ui.button("Capture Window…").clicked() {
                        self.capture(ctx, CaptureMode::Window);
                    }
                    if ui.button("Capture Screen").clicked() {
                        self.capture(ctx, CaptureMode::Screen);
                    }
                });
                if ui
                    .add_enabled(
                        self.editor_mutation_allowed(),
                        egui::Button::new("Open Image…"),
                    )
                    .clicked()
                {
                    self.open(ctx);
                }
            });
            ui.add_space(26.0);
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
        if self.tool == Tool::Select
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
            Tool::Highlight => doc.annotations.push(Annotation::Highlight {
                rect,
                colour: [colour[0], colour[1], colour[2], 80],
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
            Annotation::Text { .. } | Annotation::Step { .. } => {} // Composited by the shared raster renderer.
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
                let c = self.colour.to_srgba_unmultiplied();
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
        if self.closing {
            return;
        }
        if self.job.is_some() {
            self.notify("Wait for the current operation to finish.", true);
            return;
        }
        if self.doc.is_some()
            && self.revision != self.saved_revision
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
                    text,
                    font_size,
                    colour,
                    ..
                } => {
                    self.text_input = text.clone();
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
    fn place_annotation(&mut self, at: Point) {
        if self.doc.is_none() {
            return;
        }
        let colour = self.colour.to_srgba_unmultiplied();
        let mark = match self.tool {
            Tool::Text if !self.text_input.trim().is_empty() => Annotation::Text {
                position: at,
                text: self.text_input.clone(),
                font_size: self.text_size,
                colour,
                background: Some([17, 22, 29, 235]),
            },
            Tool::Step => Annotation::Step {
                center: at,
                number: self.step_number,
                radius: self.text_size.max(12.0),
                colour,
                text_colour: [17, 22, 29, 255],
            },
            _ => {
                self.notify("Enter a text label, then click the image.", false);
                return;
            }
        };
        self.checkpoint();
        let doc = self.doc.as_mut().unwrap();
        doc.annotations.push(mark);
        self.selected = Some(doc.annotations.len() - 1);
        if self.tool == Tool::Step {
            self.step_number = (self.step_number + 1).min(9999);
        }
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
    fn annotation_controls(&mut self, ui: &mut egui::Ui) {
        if matches!(self.tool, Tool::Text | Tool::Step) || self.selected.is_some() {
            ui.label("Label / step properties");
            ui.add(
                egui::TextEdit::multiline(&mut self.text_input)
                    .desired_rows(2)
                    .char_limit(2048)
                    .hint_text("Text label"),
            );
            ui.add(egui::Slider::new(&mut self.text_size, 8.0..=128.0).text("Size"));
            ui.add(
                egui::DragValue::new(&mut self.step_number)
                    .range(1..=9999)
                    .prefix("Step "),
            );
            if self.selected.is_some() && ui.button("Apply label / step properties").clicked() {
                self.checkpoint();
                let colour = self.colour.to_srgba_unmultiplied();
                if let Some(mark) = self
                    .selected
                    .and_then(|i| self.doc.as_mut()?.annotations.get_mut(i))
                {
                    match mark {
                        Annotation::Text {
                            text,
                            font_size,
                            colour: c,
                            ..
                        } => {
                            *text = self.text_input.clone();
                            *font_size = self.text_size;
                            *c = colour;
                        }
                        Annotation::Step {
                            number,
                            radius,
                            colour: c,
                            ..
                        } => {
                            *number = self.step_number;
                            *radius = self.text_size.max(8.0);
                            *c = colour;
                        }
                        _ => {}
                    }
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
                            }
                        )
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        if !names.is_empty() {
            egui::ComboBox::from_label("Select mark")
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
            ui.horizontal(|ui| {
                for (name, delta) in [
                    ("Left", Point { x: -1.0, y: 0.0 }),
                    ("Right", Point { x: 1.0, y: 0.0 }),
                    ("Up", Point { x: 0.0, y: -1.0 }),
                    ("Down", Point { x: 0.0, y: 1.0 }),
                ] {
                    if ui.small_button(name).clicked() {
                        self.move_selected(delta);
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
                Ok(controls) => self.desktop = Some(controls),
                Err(error) => self.notify(error, true),
            }
        }
        let capture_available = self.capture_available();
        let actions = if let Some(controls) = &mut self.desktop {
            let actions = controls.poll();
            let (state, count, source) = self.session.desktop_status();
            controls.update(state, count, &source);
            controls.set_capture_available(capture_available);
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
                        self.open_settings();
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
    fn dialogs(&mut self, ctx: &egui::Context, frame: &eframe::Frame) {
        if self.pending.is_some() && !self.closing {
            egui::Window::new("Keep your current edits?").collapsible(false).resizable(false).show(ctx,|ui| {
                ui.label("Edits are held in memory. Save a flattened PNG before replacing this image or quitting.");
                ui.add_enabled_ui(self.job.is_none(),|ui| {ui.horizontal(|ui| {
                    if ui.button("Save PNG…").clicked() {self.continue_after_save=true;if self.editing_capture.is_some(){self.save_edited_copy(ctx);}else{self.export(ctx,false);}}
                    if ui.button("Discard edits").clicked() {self.continue_after_save=false;self.saved_revision=self.revision;if let Some(action)=self.pending.take(){self.execute(ctx,action);}}
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
            egui::Window::new("Keyboard Shortcuts")
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

fn shortcut_editor(ui: &mut egui::Ui, label: &str, value: &mut String) {
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
        self.shortcuts(ctx);
        if self.job.is_none() {
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
                self.sidebar(ctx);
            }
            self.canvas(ctx);
        }
        self.dialogs(ctx, frame);
    }
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
        app.capture(&ctx, CaptureMode::Area);
        assert!(matches!(
            app.pending,
            Some(Pending::Capture(CaptureMode::Area))
        ));
        assert!(app.job.is_none());
        app.capture(&ctx, CaptureMode::Screen);
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
        app.text_input = "Review this".into();
        app.place_annotation(Point { x: 50.0, y: 50.0 });
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
