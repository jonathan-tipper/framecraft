use crate::session_ui::{Action as SessionAction, SessionView};
use eframe::egui::{self, Color32, Pos2, RichText, Sense, Stroke, Vec2};
use framecraft::{
    capture::{self, CaptureMode},
    editor::{self, Annotation, Document, Point, Rect, Style, Theme},
};
use std::{
    borrow::Cow,
    path::PathBuf,
    sync::mpsc,
    time::{Duration, Instant},
};

const ACCENT: Color32 = Color32::from_rgb(91, 221, 203);
const MUTED: Color32 = Color32::from_rgb(150, 164, 178);
const INK: Color32 = Color32::from_rgb(17, 22, 29);

#[derive(Clone, Copy, PartialEq)]
enum Tool {
    Select,
    Arrow,
    Box,
    Redact,
    Crop,
    Highlight,
    Pen,
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
    Exported(PathBuf),
    Copied(arboard::Clipboard),
    RegionSource(image::RgbaImage, u32),
    SessionLoaded(image::RgbaImage, PathBuf),
}
struct Job {
    rx: mpsc::Receiver<Result<Outcome, String>>,
}

pub struct Framecraft {
    page: Page,
    session: SessionView,
    selecting_display: Option<u32>,
    editing_capture: Option<PathBuf>,
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
    render_job: Option<(u64, mpsc::Receiver<Result<image::RgbaImage, String>>)>,
    clipboard: Option<arboard::Clipboard>,
}

impl Framecraft {
    pub fn new(cc: &eframe::CreationContext<'_>, demo: bool) -> Self {
        Self::with_context(&cc.egui_ctx, demo)
    }

    fn with_context(ctx: &egui::Context, demo: bool) -> Self {
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = INK;
        visuals.window_fill = INK;
        visuals.selection.bg_fill = Color32::from_rgb(36, 92, 88);
        visuals.selection.stroke = Stroke::new(1.0_f32, ACCENT);
        ctx.set_visuals(visuals);
        ctx.style_mut(|s| {
            s.spacing.item_spacing = Vec2::new(10.0, 10.0);
            s.spacing.button_padding = Vec2::new(13.0, 9.0);
        });
        let mut app = Self {
            page: if demo { Page::Editor } else { Page::Session },
            session: SessionView::new(),
            selecting_display: None,
            editing_capture: None,
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
            status: "Local by default. Yours by design.".into(),
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
            self.changed();
        }
    }
    fn load(&mut self, ctx: egui::Context, pixels: image::RgbaImage, name: String) {
        self.page = Page::Editor;
        self.editing_capture = None;
        self.selecting_display = None;
        self.freehand.clear();
        self.source_texture = Some(texture(&ctx, "source", &pixels));
        self.doc = Some(Document {
            source: pixels,
            crop: None,
            annotations: vec![],
            style: Style::default(),
        });
        self.name = name;
        self.undo.clear();
        self.redo.clear();
        self.style_before = None;
        self.preview_texture = None;
        self.preview = false;
        self.drag = None;
        self.changed();
        self.notify(
            "Drag a tool across the image, then preview or export.",
            false,
        );
    }
    fn start_job<F>(&mut self, ctx: &egui::Context, label: &str, f: F)
    where
        F: FnOnce() -> Result<Outcome, String> + Send + 'static,
    {
        if self.job.is_some() {
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
    fn capture(&mut self, ctx: &egui::Context, mode: CaptureMode) {
        self.session.pause();
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
        if self.job.is_some() {
            return;
        }
        self.session.pause();
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        let restore = ctx.clone();
        self.start_job(
            ctx,
            "Preparing the display. Drag around the shared content.",
            move || {
                std::thread::sleep(Duration::from_millis(350));
                let result =
                    capture::capture_display(display).map(|im| Outcome::RegionSource(im, display));
                restore.send_viewport_cmd(egui::ViewportCommand::Visible(true));
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
            self.session.set_target(capture::CaptureTarget {
                display,
                region,
                source_width: doc.source.width(),
                source_height: doc.source.height(),
            });
            self.selecting_display = None;
            self.page = Page::Session;
        }
    }
    fn save_edited_copy(&mut self, ctx: &egui::Context) {
        if self.job.is_some() {
            return;
        }
        if let (Some(path), Some(doc)) = (self.editing_capture.clone(), self.doc.clone()) {
            self.start_job(
                ctx,
                "Saving edited copy in the session folder...",
                move || {
                    let output = editor::render(&doc)?;
                    save_edit(&path, &output).map(Outcome::Exported)
                },
            );
        }
    }
    fn export(&mut self, ctx: &egui::Context, copy: bool) {
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
        self.start_job(
            ctx,
            if copy {
                "Rendering to clipboard..."
            } else {
                "Rendering PNG..."
            },
            move || {
                let output = editor::render(&doc)?;
                if let Some(path) = path {
                    output
                        .save_with_format(&path, image::ImageFormat::Png)
                        .map_err(|e| e.to_string())?;
                    Ok(Outcome::Exported(path))
                } else {
                    let mut clipboard = arboard::Clipboard::new().map_err(|e| e.to_string())?;
                    clipboard
                        .set_image(arboard::ImageData {
                            width: output.width() as usize,
                            height: output.height() as usize,
                            bytes: Cow::Owned(output.into_raw()),
                        })
                        .map_err(|e| e.to_string())?;
                    Ok(Outcome::Copied(clipboard))
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
                Ok(Outcome::RegionSource(pixels, display)) => {
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
                Ok(Outcome::SessionLoaded(pixels, path)) => {
                    let name = path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned();
                    self.load(ctx.clone(), pixels, name);
                    self.editing_capture = Some(path);
                    if let Some(d) = self.doc.as_mut() {
                        d.style.padding = 0;
                        d.style.radius = 0;
                        d.style.shadow = false;
                    }
                    self.changed();
                }
                Ok(Outcome::Loaded(pixels, name)) => self.load(ctx.clone(), pixels, name),
                Ok(Outcome::Captured(Some(pixels))) => {
                    self.load(ctx.clone(), pixels, "New capture".into())
                }
                Ok(Outcome::Captured(None)) => self.notify("Capture cancelled.", false),
                Ok(Outcome::Exported(path)) => {
                    self.notify(format!("Saved {}", path.display()), false)
                }
                Ok(Outcome::Copied(clipboard)) => {
                    self.clipboard = Some(clipboard);
                    self.notify("Copied. Redactions are baked into the pixels.", false);
                }
                Err(error) => self.notify(error, true),
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
                    Ok(pixels) => self.preview_texture = Some(texture(ctx, "preview", &pixels)),
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
                    let result = editor::render(&doc);
                    let _ = tx.send(result);
                    repaint.request_repaint();
                });
            }
        }
        if self.dirty.is_some() || self.job.is_some() || self.render_job.is_some() {
            ctx.request_repaint_after(Duration::from_millis(50));
        }
    }
    fn shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.wants_keyboard_input() {
            return;
        }
        let cmd = egui::Modifiers::COMMAND;
        let shift = cmd | egui::Modifiers::SHIFT;
        let capture_keys = cmd | egui::Modifiers::ALT;
        let take = |modifiers, key| ctx.input_mut(|i| i.consume_key(modifiers, key));
        if take(capture_keys, egui::Key::Num4) {
            self.capture(ctx, CaptureMode::Area);
        } else if take(capture_keys, egui::Key::Num5) {
            self.capture(ctx, CaptureMode::Window);
        } else if take(capture_keys, egui::Key::Num3) {
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
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.drag = None;
            self.freehand.clear();
            self.tool = Tool::Select;
        }
    }
    fn header(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("header")
            .frame(egui::Frame::new().fill(INK).inner_margin(16.0))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("[f]").size(25.0).color(ACCENT).strong());
                    ui.label(RichText::new("Framecraft").size(20.0).strong());
                    ui.add_space(20.0);
                    ui.selectable_value(&mut self.page, Page::Session, "Session");
                    ui.selectable_value(&mut self.page, Page::Editor, "Editor");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(RichText::new("LOCAL / NO CLOUD").small().color(MUTED));
                    });
                });
                if self.page == Page::Editor {
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        ui.add_enabled_ui(self.job.is_none(), |ui| {
                            ui.menu_button("Capture once", |ui| {
                                for (label, mode) in [
                                    ("Area", CaptureMode::Area),
                                    ("Window", CaptureMode::Window),
                                    ("Primary display", CaptureMode::Screen),
                                ] {
                                    if ui.button(label).clicked() {
                                        ui.close_menu();
                                        self.capture(ctx, mode);
                                    }
                                }
                            });
                            if ui.button("Open image").clicked() {
                                self.open(ctx);
                            }
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let ready = self.doc.is_some() && self.job.is_none();
                            if ui
                                .add_enabled(
                                    ready,
                                    egui::Button::new(
                                        RichText::new("Copy image").color(INK).strong(),
                                    )
                                    .fill(ACCENT),
                                )
                                .clicked()
                            {
                                self.export(ctx, true);
                            }
                            if ui
                                .add_enabled(ready, egui::Button::new("Save PNG..."))
                                .clicked()
                            {
                                self.export(ctx, false);
                            }
                            if self.editing_capture.is_some()
                                && ui
                                    .add_enabled(ready, egui::Button::new("Save edited copy"))
                                    .clicked()
                            {
                                self.save_edited_copy(ctx);
                            }
                        });
                    });
                }
            });
    }
    fn sidebar(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("controls")
            .exact_width(258.0)
            .resizable(false)
            .frame(egui::Frame::new().fill(INK).inner_margin(20.0))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    section(ui, "01  /  MARK IT UP");
                    ui.add_enabled_ui(self.doc.is_some(), |ui| {
                        for (tool, label, detail) in [
                            (Tool::Select, "Inspect", "View your source"),
                            (Tool::Arrow, "Arrow", "Point to what matters"),
                            (Tool::Pen, "Draw", "Draw freehand"),
                            (
                                Tool::Highlight,
                                "Highlight",
                                "Highlight without obscuring text",
                            ),
                            (Tool::Box, "Rectangle", "Frame a detail"),
                            (Tool::Redact, "Redact", "Opaque, pixel-level removal"),
                            (Tool::Crop, "Crop", "Keep the useful part"),
                        ] {
                            let chosen = self.tool == tool;
                            if ui
                                .add_sized([218.0, 35.0], egui::Button::new(label).selected(chosen))
                                .on_hover_text(detail)
                                .clicked()
                            {
                                self.tool = tool;
                                self.preview = false;
                                self.drag = None;
                                self.freehand.clear();
                            }
                        }
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            ui.label("Ink");
                            ui.color_edit_button_srgba(&mut self.colour);
                        });
                        ui.add(egui::Slider::new(&mut self.stroke, 2.0..=16.0).text("Weight"));
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
                    ui.add_space(24.0);
                    section(ui, "02  /  SET THE SCENE");
                    let mut changed = false;
                    let before = self.doc.as_ref().map(Edits::read);
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
                            .add(egui::Slider::new(&mut doc.style.padding, 0..=180).text("Padding"))
                            .changed();
                        changed |= ui
                            .add(egui::Slider::new(&mut doc.style.radius, 0..=64).text("Corners"))
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
                    ui.label(RichText::new("ON YOUR MAC").small().color(ACCENT));
                    ui.label(
                    RichText::new(
                        "No account. No upload. No tracking.\nOnly the PNG leaves when you choose.",
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
                    .fill(Color32::from_rgb(24, 30, 39))
                    .inner_margin(24.0),
            )
            .show(ctx, |ui| {
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
                    self.source_texture.as_ref()
                };
                let available = ui.available_size() - Vec2::new(0.0, 35.0);
                let Some(texture) = texture else {
                    ui.spinner();
                    ui.label("Rendering your frame...");
                    return;
                };
                let size = texture.size_vec2();
                let scale = (available.x / size.x).min(available.y / size.y).max(0.01);
                let displayed = size * scale;
                let (space, _) = ui.allocate_exact_size(available, Sense::hover());
                let rect = egui::Rect::from_center_size(space.center(), displayed);
                let response = ui.interact(rect, ui.id().with("canvas"), Sense::click_and_drag());
                ui.painter().image(
                    texture.id(),
                    rect,
                    egui::Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                    Color32::WHITE,
                );
                if !self.preview {
                    self.paint_annotations(ui, rect);
                    let doc = self.doc.as_ref().unwrap();
                    let w = doc.source.width();
                    let h = doc.source.height();
                    let point = |pos: Pos2| source_point(pos, rect, w, h);
                    if response.drag_started() && self.tool != Tool::Select {
                        self.drag = ctx.input(|i| i.pointer.press_origin()).map(point);
                        self.freehand.clear();
                        if let Some(p) = self.drag {
                            self.freehand.push(p);
                        }
                    }
                    if let (Some(start), Some(pos)) = (self.drag, response.interact_pointer_pos()) {
                        let end = point(pos);
                        if self.tool == Tool::Pen
                            && self.freehand.len() < 10_000
                            && self
                                .freehand
                                .last()
                                .is_none_or(|p| (p.x - end.x).hypot(p.y - end.y) > 1.0)
                        {
                            self.freehand.push(end);
                        }
                        self.paint_draft(ui, rect, start, end);
                        if response.drag_stopped() {
                            self.drag = None;
                            if self.tool == Tool::Pen {
                                self.commit_pen();
                            } else if (start.x - end.x).abs() + (start.y - end.y).abs() > 3.0 {
                                self.commit(start, end);
                            }
                        }
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
        ui.add_space((ui.available_height() * 0.18).max(20.0));
        ui.vertical_centered(|ui| {
            ui.label(RichText::new("[  +  ]").size(50.0).color(ACCENT));
            ui.add_space(24.0);
            ui.label(RichText::new("Make your point.").size(36.0).strong());
            ui.add_space(8.0);
            ui.label(
                RichText::new("Capture a detail. Remove the noise. Give it a little space.")
                    .size(16.0)
                    .color(MUTED),
            );
            ui.add_space(28.0);
            if ui
                .add_enabled(
                    self.job.is_none(),
                    egui::Button::new(RichText::new("Capture an area").color(INK).strong())
                        .fill(ACCENT),
                )
                .clicked()
            {
                self.capture(ctx, CaptureMode::Area);
            }
            if ui.button("Open a PNG or JPEG").clicked() {
                self.open(ctx);
            }
            ui.add_space(24.0);
            if ui.link("Explore a sample canvas").clicked() {
                self.load(ctx.clone(), editor::demo_image(), "Sample canvas".into());
            }
            ui.add_space(8.0);
            ui.label(RichText::new("Or drop an image here.").small().color(MUTED));
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
            Tool::Select | Tool::Pen => {}
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
    fn paint_annotations(&self, ui: &egui::Ui, canvas: egui::Rect) {
        let doc = self.doc.as_ref().unwrap();
        // Redactions are painted last, mirroring the export's privacy invariant.
        for redact_pass in [false, true] {
            for annotation in &doc.annotations {
                if matches!(annotation, Annotation::Redact { .. }) == redact_pass {
                    self.paint_annotation(ui, canvas, annotation);
                }
            }
        }
        if let Some(crop) = doc.crop {
            let rect = self.map_rect(canvas, crop);
            ui.painter().rect_stroke(
                rect,
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

impl eframe::App for Framecraft {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll(ctx);
        self.session.poll(ctx);
        self.shortcuts(ctx);
        if self.job.is_none() {
            let path = ctx.input(|i| i.raw.dropped_files.first().and_then(|f| f.path.clone()));
            if let Some(path) = path {
                self.open_path(ctx, path);
            }
        }
        self.header(ctx);
        egui::TopBottomPanel::bottom("status")
            .frame(egui::Frame::new().fill(INK).inner_margin(12.0))
            .show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    if self.job.is_some() {
                        ui.spinner();
                    }
                    ui.label(RichText::new(&self.status).small().color(if self.error {
                        Color32::from_rgb(255, 161, 146)
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
            if let Some(action) = self.session.show(ctx) {
                match action {
                    SessionAction::SelectDisplay(display) => {
                        self.select_session_source(ctx, display)
                    }
                    SessionAction::Edit(capture) => {
                        self.start_job(ctx, "Opening captured frame...", move || {
                            capture
                                .load()
                                .map(|(image, path)| Outcome::SessionLoaded(image, path))
                        })
                    }
                }
            }
        } else {
            self.sidebar(ctx);
            self.canvas(ctx);
        }
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
            Ok(_) => return Ok(path),
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
                    app.session.show(ctx);
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
