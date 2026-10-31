use eframe::egui::{self, RichText};
pub use framecraft::library::CaptureVariant;
use framecraft::{
    capture::CaptureTarget,
    library::{
        ImageState, OpenPreview, RecentSessions, SessionLibrary, SessionPreferences, SourceHint,
        VariantInfo,
    },
    session::{CaptureHandle, ChangeConfig, SavedCapture, SessionStore},
    session_runner::{CaptureSchedule, RunConfig, SessionEvent, SessionRunner},
};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    time::Duration,
};

#[derive(Clone)]
pub struct ReviewFrame {
    pub handle: CaptureHandle,
    pub variant: CaptureVariant,
    pub caption: String,
}
pub enum Action {
    SelectDisplay(u32),
    Edit(CaptureHandle),
    EditVariant(CaptureHandle, CaptureVariant),
    Copy(CaptureHandle, CaptureVariant),
    Reveal(CaptureHandle, CaptureVariant),
    Drag(CaptureHandle, CaptureVariant),
    ExportPack(Vec<ReviewFrame>),
}
struct CaptureRow {
    handle: CaptureHandle,
    capture: SavedCapture,
    texture: Option<egui::TextureHandle>,
    draft: String,
    selected: bool,
    variant: CaptureVariant,
    variant_confirmed: bool,
    variants: Vec<VariantInfo>,
    caption: String,
    error: Option<String>,
}
type LoadedRows = Vec<(
    CaptureHandle,
    Option<image::RgbaImage>,
    Vec<VariantInfo>,
    Option<String>,
)>;
enum LoadResult {
    Preview(Result<OpenPreview, String>),
    Opened(Result<(Arc<SessionLibrary>, LoadedRows), String>),
    Variant(usize, CaptureVariant, Result<image::RgbaImage, String>),
}
struct LoadJob {
    receiver: mpsc::Receiver<LoadResult>,
    cancel: Arc<AtomicBool>,
    progress: Arc<Mutex<String>>,
    committing: Arc<Mutex<bool>>,
}
pub struct SessionView {
    pub target: Option<CaptureTarget>,
    title: String,
    folder: Option<PathBuf>,
    display: u32,
    schedule: CaptureSchedule,
    interval: u64,
    threshold: f32,
    settle: f32,
    auto_name: bool,
    max_minutes: u64,
    max_frames: u64,
    runner: Option<SessionRunner>,
    library: Option<Arc<SessionLibrary>>,
    captures: Vec<CaptureRow>,
    status: String,
    saw_unfocused: bool,
    started: bool,
    failed: bool,
    controls_ready: bool,
    waiting_for_controls: bool,
    loading: Option<LoadJob>,
    preview: Option<OpenPreview>,
    recents: RecentSessions,
    recent_path: Option<PathBuf>,
    preference_path: Option<PathBuf>,
    source_hint: Option<SourceHint>,
}
fn thumbnail(ctx: &egui::Context, pixels: &image::RgbaImage, name: &str) -> egui::TextureHandle {
    let pixels = image::imageops::thumbnail(pixels, 320, 200);
    ctx.load_texture(
        name,
        egui::ColorImage::from_rgba_unmultiplied(
            [pixels.width() as usize, pixels.height() as usize],
            pixels.as_raw(),
        ),
        egui::TextureOptions::LINEAR,
    )
}
impl SessionView {
    pub fn new() -> Self {
        #[cfg(test)]
        let recent_path: Option<PathBuf> = None;
        #[cfg(not(test))]
        let recent_path = std::env::var_os("HOME").map(|home| {
            PathBuf::from(home).join("Library/Application Support/Framecraft/recent-sessions.json")
        });
        let recent_result = recent_path
            .as_ref()
            .map(|p| RecentSessions::load(p))
            .transpose();
        let (recents, status) = match recent_result {
            Ok(value) => (
                value.unwrap_or_default(),
                "Choose an area and a save folder to start a collection, or open a saved session."
                    .into(),
            ),
            Err(error) => (RecentSessions::default(), error),
        };
        let preference_path = recent_path
            .as_ref()
            .and_then(|p| p.parent())
            .map(|p| p.join("capture-preferences.json"));
        let preferences = preference_path
            .as_ref()
            .map(|p| SessionPreferences::load(p))
            .transpose();
        let (preferences, status) = match preferences {
            Ok(p) => (p.unwrap_or_default(), status),
            Err(error) => (SessionPreferences::default(), format!("{status} {error}")),
        };
        let mut view = Self {
            target: None,
            title: "Meeting notes".into(),
            folder: None,
            display: 1,
            schedule: CaptureSchedule::Manual,
            interval: 15,
            threshold: 0.04,
            settle: 1.0,
            auto_name: true,
            max_minutes: 120,
            max_frames: 500,
            runner: None,
            library: None,
            captures: vec![],
            status,
            saw_unfocused: false,
            started: false,
            failed: false,
            controls_ready: false,
            waiting_for_controls: false,
            loading: None,
            preview: None,
            recents,
            recent_path,
            preference_path,
            source_hint: None,
        };
        view.apply_preferences(preferences);
        view
    }
    fn preferences(&self) -> SessionPreferences {
        SessionPreferences {
            version: 1,
            title: self.title.chars().take(120).collect(),
            destination: self.folder.clone(),
            display: self.display,
            schedule: match self.schedule {
                CaptureSchedule::Manual => "manual",
                CaptureSchedule::Interval => "interval",
                CaptureSchedule::Change => "change",
            }
            .into(),
            interval: self.interval,
            threshold: self.threshold,
            settle: self.settle,
            auto_name: self.auto_name,
            max_minutes: self.max_minutes,
            max_frames: self.max_frames,
            source_hint: self.source_hint.clone(),
        }
    }
    fn apply_preferences(&mut self, preferences: SessionPreferences) {
        self.title = preferences.title;
        self.folder = preferences.destination;
        self.display = preferences.display;
        self.schedule = match preferences.schedule.as_str() {
            "interval" => CaptureSchedule::Interval,
            "change" => CaptureSchedule::Change,
            _ => CaptureSchedule::Manual,
        };
        self.interval = preferences.interval;
        self.threshold = preferences.threshold;
        self.settle = preferences.settle;
        self.auto_name = preferences.auto_name;
        self.max_minutes = preferences.max_minutes;
        self.max_frames = preferences.max_frames;
        self.source_hint = preferences.source_hint;
        self.target = None;
    }
    fn save_preferences(&mut self) {
        if let Some(path) = &self.preference_path {
            if let Err(error) = self.preferences().save(path) {
                self.status = format!("Capture defaults were not saved: {error}");
            }
        }
    }
    pub fn active(&self) -> bool {
        self.runner.as_ref().is_some_and(|r| !r.is_stopped())
    }
    pub fn is_drained(&self) -> bool {
        self.runner.as_ref().is_none_or(SessionRunner::is_drained) && self.loading.is_none()
    }
    pub fn pause(&mut self) {
        self.failed = false;
        if let Some(r) = &self.runner {
            r.pause();
        }
    }
    pub fn set_controls_ready(&mut self, ready: bool) {
        self.controls_ready = ready;
        if !ready {
            if self.active() {
                self.pause();
                self.status="Capture controls are unavailable. Stop the session, resolve the shortcut conflict or change Shortcuts, then start again.".into();
            }
        } else if self.waiting_for_controls {
            self.waiting_for_controls = false;
            if let Some(runner) = &self.runner {
                if !runner.is_stopped() {
                    runner.resume();
                    self.status = "Running. Capture controls are registered.".into();
                }
            }
        }
    }
    pub fn take_started(&mut self) -> bool {
        std::mem::take(&mut self.started)
    }
    pub fn set_target(&mut self, target: CaptureTarget) -> Result<(), String> {
        if self.active() {
            return Err("Stop the active session before choosing another capture source. Pausing keeps its source fixed.".into());
        }
        self.source_hint = Some(SourceHint {
            display: target.display,
            x: target.region.x,
            y: target.region.y,
            width: target.region.width,
            height: target.region.height,
        });
        self.target = Some(target);
        self.status = "Area set. Choose a destination and start the session.".into();
        Ok(())
    }
    fn remember(&mut self, folder: PathBuf) {
        self.recents.remember(folder);
        if let Some(path) = &self.recent_path {
            if let Err(error) = self.recents.save(path) {
                self.status = format!(
                    "{} Recent-session reference was not saved: {error}",
                    self.status
                );
            }
        }
    }
    fn begin_open(&mut self, folder: PathBuf, ctx: &egui::Context) {
        if self.active() || !self.is_drained() {
            self.status = "Stop the current capture session and wait for saved-image operations to finish before opening another folder.".into();
            return;
        }
        if self
            .library
            .as_ref()
            .is_some_and(|l| folder.canonicalize().ok() == l.folder().canonicalize().ok())
        {
            self.status = "This session is already open. Your current review is preserved.".into();
            return;
        }
        self.preview = None;
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        let (sender, receiver) = mpsc::channel();
        let wake = ctx.clone();
        let progress = Arc::new(Mutex::new(
            "Inspecting session files and checking images…".into(),
        ));
        std::thread::spawn(move || {
            let result = SessionLibrary::preview(&folder, &worker_cancel);
            let _ = sender.send(LoadResult::Preview(result));
            wake.request_repaint();
        });
        self.loading = Some(LoadJob {
            receiver,
            cancel,
            progress,
            committing: Arc::new(Mutex::new(false)),
        });
    }
    fn confirm_open(&mut self, preview: OpenPreview, ctx: &egui::Context) {
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        let (sender, receiver) = mpsc::channel();
        let wake = ctx.clone();
        let progress = Arc::new(Mutex::new("Loading session thumbnails…".into()));
        let worker_progress = progress.clone();
        let committing = Arc::new(Mutex::new(false));
        let worker_committing = committing.clone();
        std::thread::spawn(move || {
            let result = (|| {
                // Decode before committing import metadata, keeping cancelled loads read-only.
                let mut images = Vec::new();
                for (index, capture) in preview.manifest.captures.iter().enumerate() {
                    if worker_cancel.load(Ordering::Relaxed) {
                        return Err("Opening cancelled. The previous session is unchanged.".into());
                    }
                    if let Ok(mut value) = worker_progress.lock() {
                        *value = format!(
                            "Loading frame {} of {}",
                            index + 1,
                            preview.manifest.captures.len()
                        );
                    }
                    // Limit the initial thumbnail cache to 200 frames (~50 MiB).
                    let variants = preview.variants(capture);
                    let image = if index < 200
                        && variants
                            .first()
                            .is_some_and(|v| v.state == ImageState::Available)
                    {
                        Some(
                            framecraft::editor::load_image(
                                &preview.folder.join(&capture.original.file),
                            )
                            .map(|p| image::imageops::thumbnail(&p, 320, 200)),
                        )
                    } else {
                        None
                    };
                    let error = image
                        .as_ref()
                        .and_then(|p| p.as_ref().err().cloned())
                        .or_else(|| {
                            variants
                                .first()
                                .filter(|v| v.state != ImageState::Available)
                                .map(|v| v.state.label())
                        });
                    images.push((image.and_then(Result::ok), variants, error));
                }
                {
                    // Cancellation and the final metadata commit have one boundary.
                    let mut stage = worker_committing
                        .lock()
                        .map_err(|_| "Opening was interrupted.".to_string())?;
                    if worker_cancel.load(Ordering::Relaxed) {
                        return Err("Opening cancelled. The previous session is unchanged.".into());
                    }
                    *stage = true;
                }
                if let Ok(mut value) = worker_progress.lock() {
                    *value = "Finishing session metadata…".into();
                }
                let library = SessionLibrary::confirm(preview, &worker_cancel)?;
                let handles = library.capture_handles()?;
                let rows = handles
                    .into_iter()
                    .zip(images)
                    .map(|(handle, (pixels, variants, error))| (handle, pixels, variants, error))
                    .collect();
                Ok((library, rows))
            })();
            let _ = sender.send(LoadResult::Opened(result));
            wake.request_repaint();
        });
        self.loading = Some(LoadJob {
            receiver,
            cancel,
            progress,
            committing,
        });
    }
    fn choose_variant(&mut self, index: usize, variant: CaptureVariant, ctx: &egui::Context) {
        self.pause();
        self.captures[index].variant = variant.clone();
        self.captures[index].texture = None;
        self.captures[index].error = None;
        let handle = self.captures[index].handle.clone();
        let (sender, receiver) = mpsc::channel();
        let wake = ctx.clone();
        std::thread::spawn(move || {
            let result = handle
                .load_variant(&variant)
                .map(|(pixels, _)| image::imageops::thumbnail(&pixels, 320, 200));
            let _ = sender.send(LoadResult::Variant(index, variant, result));
            wake.request_repaint();
        });
        self.loading = Some(LoadJob {
            receiver,
            cancel: Arc::new(AtomicBool::new(false)),
            progress: Arc::new(Mutex::new("Loading selected image variant…".into())),
            committing: Arc::new(Mutex::new(false)),
        });
    }
    pub fn refresh_variants(&mut self) {
        for row in &mut self.captures {
            if let Ok(variants) = row.handle.variant_choices() {
                row.variants = variants;
            }
        }
    }
    pub fn poll(&mut self, ctx: &egui::Context) {
        let result = self
            .loading
            .as_ref()
            .and_then(|job| match job.receiver.try_recv() {
                Ok(result) => Some(result),
                Err(mpsc::TryRecvError::Disconnected) => Some(LoadResult::Preview(Err(
                    "Session opening stopped unexpectedly. Your prior review is unchanged.".into(),
                ))),
                Err(mpsc::TryRecvError::Empty) => None,
            });
        if let Some(result) = result {
            let was_cancelled = self
                .loading
                .take()
                .is_some_and(|job| job.cancel.load(Ordering::Relaxed));
            if was_cancelled {
                self.status = "Opening cancelled. Your previous review is unchanged.".into();
            } else {
                match result {
                    LoadResult::Preview(Ok(preview)) if preview.needs_confirmation => {
                        self.status = format!(
                            "Review {} recognised frames before opening.",
                            preview.manifest.captures.len()
                        );
                        self.preview = Some(preview);
                    }
                    LoadResult::Preview(Ok(preview)) => {
                        self.status = preview.messages.join("\n");
                        self.confirm_open(preview, ctx);
                    }
                    LoadResult::Opened(Ok((library, rows))) => {
                        self.runner = None;
                        self.target = None;
                        self.saw_unfocused = false;
                        self.captures = rows
                            .into_iter()
                            .filter_map(|(handle, pixels, variants, error)| {
                                let capture = handle.snapshot().ok()?;
                                Some(CaptureRow {
                                    texture: pixels.map(|p| {
                                        thumbnail(ctx, &p, &format!("reopen-{}", capture.sequence))
                                    }),
                                    draft: capture.title.clone(),
                                    capture,
                                    handle,
                                    selected: false,
                                    variant: CaptureVariant::Original,
                                    variant_confirmed: false,
                                    variants,
                                    caption: String::new(),
                                    error,
                                })
                            })
                            .collect();
                        self.title = library.manifest().map(|m| m.title).unwrap_or_default();
                        self.status=format!("Opened {} frames. Capture is stopped; choose a source again before starting a new session.",self.captures.len());
                        self.remember(library.folder().to_path_buf());
                        self.library = Some(library);
                    }
                    LoadResult::Variant(index, variant, result) => {
                        if let Some(row) = self.captures.get_mut(index) {
                            if row.variant == variant {
                                match result {
                                    Ok(pixels) => {
                                        row.texture = Some(thumbnail(
                                            ctx,
                                            &pixels,
                                            &format!("variant-{index}"),
                                        ))
                                    }
                                    Err(e) => row.error = Some(e),
                                }
                            }
                        }
                    }
                    LoadResult::Preview(Err(error)) | LoadResult::Opened(Err(error)) => {
                        self.status = error
                    }
                }
            }
        }
        let events: Vec<_> = self
            .runner
            .as_ref()
            .map(|r| r.events.try_iter().collect())
            .unwrap_or_default();
        for event in events {
            match event {
                SessionEvent::Saved(handle, pixels) => {
                    let capture = match handle.snapshot() {
                        Ok(c) => c,
                        Err(e) => {
                            self.status = e;
                            continue;
                        }
                    };
                    let variants = handle.variant_choices().unwrap_or_default();
                    self.status = handle.metadata_warning().unwrap_or_else(|| {
                        format!(
                            "Saved {}. {} captures in this session.",
                            capture.title,
                            self.captures.len() + 1
                        )
                    });
                    self.captures.push(CaptureRow {
                        texture: Some(thumbnail(
                            ctx,
                            &pixels,
                            &format!("capture-{}", capture.sequence),
                        )),
                        draft: capture.title.clone(),
                        capture,
                        handle,
                        selected: false,
                        variant: CaptureVariant::Original,
                        variant_confirmed: false,
                        variants,
                        caption: String::new(),
                        error: None,
                    });
                }
                SessionEvent::Named { .. } => {
                    for row in &mut self.captures {
                        if let Ok(capture) = row.handle.snapshot() {
                            if row.draft == row.capture.title {
                                row.draft = capture.title.clone();
                            }
                            row.capture = capture;
                            if let Some(w) = row.handle.metadata_warning() {
                                self.status = w;
                            }
                        }
                    }
                }
                SessionEvent::Paused(message) => {
                    self.status = message;
                    self.failed = true;
                }
                SessionEvent::Finished(message) | SessionEvent::Note(message) => {
                    self.status = message
                }
            }
        }
        if self.runner.as_ref().is_some_and(SessionRunner::is_running) {
            let focused = ctx.input(|i| i.focused);
            if !focused {
                self.saw_unfocused = true;
            }
            if focused && self.saw_unfocused {
                self.pause();
                self.status = "Paused while you review. Resume to return to capturing.".into();
                self.saw_unfocused = false;
            }
        }
        let cached = self.captures.iter().filter(|r| r.texture.is_some()).count();
        if cached > 200 {
            let mut remove = cached - 200;
            for row in &mut self.captures {
                if remove > 0 && row.texture.is_some() {
                    row.texture = None;
                    remove -= 1;
                }
            }
        }
        if self.active() || !self.is_drained() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }
    fn start(&mut self, ctx: &egui::Context) {
        if !self.is_drained() {
            self.status =
                "Finishing saved-image operations. Start a new session when they complete.".into();
            return;
        }
        if self.active() {
            return;
        }
        if !cfg!(target_os = "macos") {
            self.status =
                "Live capture requires macOS. The editor and CLI run on this platform.".into();
            return;
        }
        let Some(target) = self.target else {
            self.status = "Choose a capture area first.".into();
            return;
        };
        let Some(folder) = self.folder.as_ref() else {
            self.status = "Choose the folder where your captures should go.".into();
            return;
        };
        let store = match SessionStore::create(folder, &self.title) {
            Ok(s) => s,
            Err(e) => {
                self.status = e;
                return;
            }
        };
        self.save_preferences();
        let library = store.library();
        let wake = ctx.clone();
        self.runner = Some(SessionRunner::start(
            store,
            RunConfig {
                target,
                schedule: self.schedule,
                interval: Duration::from_secs(self.interval),
                change: ChangeConfig {
                    threshold: self.threshold,
                    settle: Duration::from_secs_f32(self.settle),
                    cooldown: Duration::from_secs(2),
                },
                auto_name: self.auto_name,
                max_captures: self.max_frames,
                max_duration: Duration::from_secs(self.max_minutes * 60),
            },
            move || wake.request_repaint(),
        ));
        // Native controls register on the main thread after take_started().
        // No capture may begin before that acknowledgement.
        self.pause();
        self.controls_ready = false;
        self.waiting_for_controls = true;
        self.captures.clear();
        self.saw_unfocused = false;
        self.started = true;
        self.failed = false;
        self.status = "Session prepared. Registering capture controls…".into();
        self.remember(library.folder().to_path_buf());
        self.library = Some(library);
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
    }
    fn resume(&mut self, ctx: &egui::Context) {
        if !self.controls_ready {
            self.status="Capture controls are unavailable. Stop the session, resolve the shortcut conflict or change Shortcuts, then start again.".into();
            return;
        }
        self.failed = false;
        if let Some(r) = &self.runner {
            if r.is_stopped() {
                return;
            }
            r.resume();
            self.saw_unfocused = false;
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
            self.status = "Session resumed.".into();
        }
    }
    fn stop(&mut self) {
        self.controls_ready = false;
        self.waiting_for_controls = false;
        if let Some(r) = &self.runner {
            r.stop();
        }
        if let Some(job) = &self.loading {
            if let Ok(stage) = job.committing.lock() {
                if !*stage {
                    job.cancel.store(true, Ordering::Relaxed);
                }
            }
        }
        self.status = if self.is_drained(){"Stopped. Your captures are saved."}else{"Capture stopped. Finishing saved-image operations; keep Framecraft open until they complete."}.into();
    }
    pub fn desktop_action(
        &mut self,
        action: framecraft::desktop::DesktopAction,
        ctx: &egui::Context,
    ) {
        use framecraft::desktop::DesktopAction;
        match action {
            DesktopAction::CancelCapture | DesktopAction::CloseScreenshot => {}
            DesktopAction::Capture => {
                if let Some(r) = &self.runner {
                    if let Err(error) = r.capture_now() {
                        self.status = error;
                    }
                }
            }
            DesktopAction::PauseResume => {
                if self.runner.as_ref().is_some_and(SessionRunner::is_running) {
                    self.pause();
                    self.status = "Session paused.".into();
                } else {
                    self.resume(ctx);
                }
            }
            DesktopAction::Stop | DesktopAction::Quit => self.stop(),
            DesktopAction::CaptureArea
            | DesktopAction::CaptureWindow
            | DesktopAction::CaptureScreen
            | DesktopAction::Settings => {}
            DesktopAction::Review => {
                self.pause();
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            }
        }
    }
    pub fn desktop_status(&self) -> (framecraft::desktop::DesktopState, usize, String) {
        use framecraft::desktop::DesktopState;
        let state = match &self.runner {
            Some(r) if r.is_running() => DesktopState::Running,
            Some(r) if r.is_paused() && !r.is_stopped() => {
                if self.failed {
                    DesktopState::Error
                } else {
                    DesktopState::Paused
                }
            }
            _ => DesktopState::Stopped,
        };
        let source = self
            .target
            .map(|t| {
                format!(
                    "Display {} · {} × {} px",
                    t.display, t.region.width, t.region.height
                )
            })
            .unwrap_or_else(|| "No capture source selected".into());
        (state, self.captures.len(), source)
    }
    pub fn show(&mut self, ctx: &egui::Context, interactions: bool) -> Option<Action> {
        let mut action = None;
        let mut rename = None;
        let mut open = None;
        let mut forget = None;
        let mut variant = None;
        let mut reorder = None;
        let mut relink = None;
        let mut confirm = false;
        let mut cancel_preview = false;
        let busy = self.loading.is_some();
        egui::CentralPanel::default()
            .frame(egui::Frame::central_panel(&ctx.style()).inner_margin(24.0))
            .show(ctx, |ui| {
                if !interactions { ui.disable(); }
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.heading("Capture sessions");
                    ui.label("Collect multiple screenshots into a saved folder, then review and share them together.");
                    ui.add_space(16.0);
                    let active = self.active();
                    let state = self.desktop_status().0;
                    let phase = match state {
                        framecraft::desktop::DesktopState::Running => "Collecting",
                        framecraft::desktop::DesktopState::Paused => "Paused",
                        framecraft::desktop::DesktopState::Stopped => "Stopped",
                        framecraft::desktop::DesktopState::Error => "Needs attention",
                    };
                    ui.group(|ui| {
                        ui.horizontal_wrapped(|ui| {
                            ui.strong(phase);
                            ui.label(format!("{} screenshots saved", self.captures.len()));
                            if active {
                                if ui.add_enabled(
                                    state == framecraft::desktop::DesktopState::Running,
                                    egui::Button::new("Capture now"),
                                ).clicked() {
                                    if let Some(runner) = &self.runner {
                                        if let Err(error) = runner.capture_now() {
                                            self.status = error;
                                        }
                                    }
                                }
                                if self.runner.as_ref().is_some_and(SessionRunner::is_paused) {
                                    if ui.button("Resume collection").on_hover_text("Minimises Framecraft and resumes capture.").clicked() {
                                        self.resume(ctx);
                                    }
                                } else if ui.button("Pause collection").clicked() {
                                    self.pause();
                                }
                                if ui.button("Stop collection").clicked() {
                                    self.stop();
                                }
                            }
                        });
                        if self.failed {
                            ui.colored_label(ui.visuals().error_fg_color, &self.status);
                        } else {
                            ui.label(&self.status);
                        }
                        if let Some(target) = self.target {
                            ui.label(RichText::new(format!(
                                "Capture area: display {} · {} × {} pixels",
                                target.display, target.region.width, target.region.height
                            )).small());
                        }
                        if let Some(library) = &self.library {
                            ui.label(RichText::new(library.folder().display().to_string()).small().weak());
                            if let Some(warning) = library.warning() {
                                ui.colored_label(ui.visuals().warn_fg_color, warning);
                            }
                        }
                    });
                    ui.add_space(12.0);

                    if let Some(job) = &self.loading {
                        ui.horizontal_wrapped(|ui| {
                            ui.spinner();
                            ui.label(job.progress.lock().map(|s| s.clone()).unwrap_or_default());
                            if let Ok(stage) = job.committing.lock() {
                                if ui.add_enabled(!*stage, egui::Button::new("Cancel opening")).clicked() {
                                    job.cancel.store(true, Ordering::Relaxed);
                                }
                            }
                        });
                    }
                    if let Some(preview) = &self.preview {
                        ui.group(|ui| {
                            ui.strong("Review import or recovery");
                            ui.label(format!("{} screenshots in capture order; {} recovered or imported.", preview.manifest.captures.len(), preview.import_count));
                            egui::ScrollArea::vertical().max_height(220.0).id_salt("import-preview").show(ui, |ui| {
                                for message in &preview.messages { ui.label(message); }
                                for capture in &preview.manifest.captures {
                                    ui.label(format!("{:04} · {}", capture.sequence, capture.original.file));
                                }
                            });
                            ui.horizontal(|ui| {
                                if ui.button("Confirm and open").clicked() { confirm = true; }
                                if ui.button("Cancel import").clicked() { cancel_preview = true; }
                            });
                        });
                    }

                    ui.add_enabled_ui(!active && !busy && self.preview.is_none(), |ui| {
                        egui::CollapsingHeader::new("Start a new collection")
                            .default_open(self.captures.is_empty())
                            .show(ui, |ui| {
                                ui.add_space(4.0);
                                ui.strong("1. Choose what to capture");
                                ui.horizontal_wrapped(|ui| {
                                    ui.label("Display");
                                    ui.add(egui::DragValue::new(&mut self.display).range(1..=16));
                                    if ui.button("Choose capture area…").clicked() {
                                        action = Some(Action::SelectDisplay(self.display));
                                    }
                                });
                                if let Some(target) = self.target {
                                    ui.label(format!("Selected: display {} · {} × {} pixels", target.display, target.region.width, target.region.height));
                                } else {
                                    ui.label("Draw a rectangle on the display preview, then confirm the capture area.");
                                    if let Some(hint) = &self.source_hint {
                                        ui.label(RichText::new(format!("Previously display {}, {} × {} pixels. Choose the area again before starting.", hint.display, hint.width, hint.height)).small().weak());
                                    }
                                }
                                ui.add_space(12.0);
                                ui.strong("2. Choose where to save");
                                ui.horizontal_wrapped(|ui| {
                                    if ui.button("Choose save folder…").clicked() {
                                        if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                                            self.folder = Some(folder);
                                        }
                                    }
                                    ui.label(self.folder.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| "No folder selected".into()));
                                });
                                ui.horizontal_wrapped(|ui| {
                                    ui.label("Collection name");
                                    ui.add(egui::TextEdit::singleline(&mut self.title).desired_width(280.0));
                                });
                                ui.label(RichText::new("A dated folder is created here. Existing screenshots are kept.").small().weak());
                                ui.add_space(12.0);
                                ui.strong("3. Choose when to capture");
                                ui.horizontal_wrapped(|ui| {
                                    ui.selectable_value(&mut self.schedule, CaptureSchedule::Manual, "When I click");
                                    ui.selectable_value(&mut self.schedule, CaptureSchedule::Interval, "On a timer");
                                    ui.selectable_value(&mut self.schedule, CaptureSchedule::Change, "When content changes");
                                });
                                match self.schedule {
                                    CaptureSchedule::Manual => { ui.label("Click Capture now in the menu bar, or use your session shortcut."); }
                                    CaptureSchedule::Interval => { ui.label(format!("Captures every {} seconds. Change the interval in Collection options.", self.interval)); }
                                    CaptureSchedule::Change => { ui.label("Experimental: small changes may be missed, and video or animation can prevent a stable capture."); }
                                }
                                ui.add_space(8.0);
                                egui::CollapsingHeader::new("Collection options").show(ui, |ui| {
                                    match self.schedule {
                                        CaptureSchedule::Manual => {}
                                        CaptureSchedule::Interval => {
                                            ui.add(egui::Slider::new(&mut self.interval, 5..=120).text("Seconds between captures"));
                                        }
                                        CaptureSchedule::Change => {
                                            ui.add(egui::Slider::new(&mut self.threshold, 0.01..=0.25).text("Changed area fraction"));
                                            ui.add(egui::Slider::new(&mut self.settle, 0.5..=5.0).text("Stable seconds before saving"));
                                        }
                                    }
                                    ui.checkbox(&mut self.auto_name, "Name images from the visible heading");
                                    ui.label(RichText::new("Text recognition runs on this Mac.").small().weak());
                                    ui.horizontal_wrapped(|ui| {
                                        ui.label("Stop after");
                                        ui.add(egui::DragValue::new(&mut self.max_minutes).range(1..=480));
                                        ui.label("minutes or");
                                        ui.add(egui::DragValue::new(&mut self.max_frames).range(1..=2000));
                                        ui.label("screenshots");
                                    });
                                    ui.horizontal_wrapped(|ui| {
                                        if ui.button("Save as defaults").clicked() { self.save_preferences(); }
                                        if ui.button("Restore defaults").clicked() {
                                            self.apply_preferences(SessionPreferences::default());
                                            self.save_preferences();
                                        }
                                    });
                                });
                                ui.add_space(8.0);
                                ui.horizontal_wrapped(|ui| {
                                    if ui.add_enabled(self.target.is_some() && self.folder.is_some(), egui::Button::new("Start collection")).clicked() {
                                        self.start(ctx);
                                    }
                                    ui.label(RichText::new(format!("Stops after {} minutes or {} screenshots.", self.max_minutes, self.max_frames)).small().weak());
                                });
                                if self.target.is_none() || self.folder.is_none() {
                                    ui.label(RichText::new("Choose a capture area and save folder to start.").small());
                                }
                            });
                    });
                    ui.add_space(12.0);
                    ui.separator();
                    ui.add_enabled_ui(!active && !busy, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            ui.strong("Saved sessions");
                            if ui.button("Open session folder…").clicked() {
                                open = rfd::FileDialog::new().pick_folder();
                            }
                        });
                        egui::CollapsingHeader::new("Recent sessions").show(ui, |ui| {
                            if self.recents.folders.is_empty() { ui.label("No recent sessions yet."); }
                            for folder in &self.recents.folders {
                                ui.horizontal_wrapped(|ui| {
                                    let available = folder.is_dir();
                                    let name = folder.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| folder.display().to_string());
                                    if ui.add_enabled(available, egui::Button::new(name)).on_hover_text(folder.display().to_string()).clicked() {
                                        open = Some(folder.clone());
                                    }
                                    if !available { ui.label("Unavailable. Use Open session folder to locate it."); }
                                    if ui.small_button("Forget").on_hover_text("Remove this recent entry. The session files stay on disk.").clicked() {
                                        forget = Some(folder.clone());
                                    }
                                });
                            }
                        });
                    });
                    ui.add_space(12.0);
                    ui.separator();
                    ui.heading("Collected screenshots");
                    let selected = self.captures.iter().filter(|row| row.selected).count();
                    if self.captures.is_empty() {
                        ui.label("Your saved screenshots will appear here. You can also open a previous session above.");
                    } else {
                        ui.horizontal_wrapped(|ui| {
                            ui.label(format!("{selected} selected"));
                            if ui.add_enabled(selected > 0 && !busy, egui::Button::new("Review share pack…")).clicked() {
                                action = Some(Action::ExportPack(self.captures.iter().filter(|row| row.selected).map(|row| ReviewFrame {
                                    handle: row.handle.clone(), variant: row.variant.clone(), caption: row.caption.clone(),
                                }).collect()));
                            }
                            if ui.add_enabled(selected > 0, egui::Button::new("Clear selection")).clicked() {
                                for row in &mut self.captures { row.selected = false; }
                            }
                            if ui.button("Refresh saved edits").clicked() { self.refresh_variants(); }
                        });
                        if selected > 0 {
                            ui.label(RichText::new(format!("The pack will contain {selected} images and an offline HTML index.")).small().weak());
                        }
                    }
                    let row_count = self.captures.len();
                    ui.add_enabled_ui(!busy, |ui| {
                        for (index, row) in self.captures.iter_mut().enumerate() {
                            ui.push_id(index, |ui| {
                                ui.group(|ui| {
                                    ui.horizontal(|ui| {
                                        if let Some(texture) = &row.texture {
                                            ui.add(egui::Image::new(texture).max_size(egui::vec2(150.0, 100.0)));
                                        } else if ui.button("Load preview").clicked() {
                                            variant = Some((index, row.variant.clone()));
                                        }
                                        ui.vertical(|ui| {
                                            ui.horizontal_wrapped(|ui| {
                                                ui.add_enabled(row.variant_confirmed, egui::Checkbox::new(&mut row.selected, format!("Image {}", row.capture.sequence)));
                                                ui.strong(&row.capture.title);
                                            });
                                            ui.horizontal_wrapped(|ui| {
                                                let previous = row.variant.clone();
                                                egui::ComboBox::from_id_salt("variant").selected_text(if !row.variant_confirmed {
                                                    "Choose image to share".to_string()
                                                } else {
                                                    match row.variant {
                                                        CaptureVariant::Original => "Original screenshot".into(),
                                                        CaptureVariant::Edited(i) => format!("Edited copy {}", i + 1),
                                                    }
                                                }).show_ui(ui, |ui| {
                                                    if ui.selectable_value(&mut row.variant, CaptureVariant::Original, "Original screenshot").clicked() {
                                                        row.variant_confirmed = true;
                                                        variant = Some((index, row.variant.clone()));
                                                    }
                                                    for info in &row.variants {
                                                        if let CaptureVariant::Edited(i) = info.variant {
                                                            if ui.selectable_value(&mut row.variant, CaptureVariant::Edited(i), format!("Edited copy {}", i + 1)).clicked() {
                                                                row.variant_confirmed = true;
                                                                variant = Some((index, row.variant.clone()));
                                                            }
                                                        }
                                                    }
                                                });
                                                if previous != row.variant { variant = Some((index, row.variant.clone())); }
                                                if ui.button("Edit image").clicked() {
                                                    action = Some(if row.variant == CaptureVariant::Original {
                                                        Action::Edit(row.handle.clone())
                                                    } else { Action::EditVariant(row.handle.clone(), row.variant.clone()) });
                                                }
                                                if ui.add_enabled(row.variant_confirmed, egui::Button::new("Copy image")).clicked() {
                                                    action = Some(Action::Copy(row.handle.clone(), row.variant.clone()));
                                                }
                                                ui.menu_button("More", |ui| {
                                                    if ui.add_enabled(row.variant_confirmed, egui::Button::new("Show in Finder")).clicked() {
                                                        action = Some(Action::Reveal(row.handle.clone(), row.variant.clone()));
                                                        ui.close_menu();
                                                    }
                                                    if ui.add_enabled(row.variant_confirmed, egui::Button::new("Drag a copy…")).clicked() {
                                                        action = Some(Action::Drag(row.handle.clone(), row.variant.clone()));
                                                        ui.close_menu();
                                                    }
                                                });
                                            });
                                            if let Some(error) = &row.error { ui.colored_label(ui.visuals().error_fg_color, error); }
                                            if row.error.is_some() && ui.button("Import replacement original…").clicked() { relink = Some(index); }
                                            if !row.variant_confirmed {
                                                ui.label(RichText::new("Choose the original or an edited copy before sharing.").small());
                                            }
                                            egui::CollapsingHeader::new("Rename image").show(ui, |ui| {
                                                ui.horizontal_wrapped(|ui| {
                                                    ui.add(egui::TextEdit::singleline(&mut row.draft).desired_width(260.0));
                                                    if ui.button("Save name").clicked() { rename = Some((index, row.draft.clone())); }
                                                });
                                            });
                                            if row.selected {
                                                ui.horizontal_wrapped(|ui| {
                                                    ui.label("Pack caption");
                                                    ui.text_edit_singleline(&mut row.caption);
                                                    if ui.add_enabled(index > 0, egui::Button::new("Move up")).clicked() { reorder = Some((index, index.saturating_sub(1))); }
                                                    if ui.add_enabled(index + 1 < row_count, egui::Button::new("Move down")).clicked() { reorder = Some((index, index + 1)); }
                                                });
                                            }
                                        });
                                    });
                                });
                                ui.add_space(6.0);
                            });
                        }
                    });
                });
            });

        if let Some(folder) = forget {
            self.recents.forget(&folder);
            if let Some(path) = &self.recent_path {
                if let Err(e) = self.recents.save(path) {
                    self.status = e;
                }
            }
        }
        if let Some(folder) = open {
            self.begin_open(folder, ctx);
        }
        if cancel_preview {
            self.preview = None;
            self.status = "Import cancelled. No session files were changed.".into();
        }
        if confirm {
            if let Some(preview) = self.preview.take() {
                self.confirm_open(preview, ctx);
            }
        }
        if let Some((index, title)) = rename {
            match self.captures[index].handle.rename(&title) {
                Ok(c) => {
                    self.captures[index].draft = c.title.clone();
                    self.captures[index].capture = c;
                    if let Some(warning) = self.captures[index].handle.metadata_warning() {
                        self.status = warning;
                    }
                }
                Err(e) => self.status = e,
            }
        }
        if let Some(index) = relink {
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("PNG image", &["png"])
                .pick_file()
            {
                match self.captures[index].handle.import_replacement(&path) {
                    Ok(()) => {
                        if let Ok(c) = self.captures[index].handle.snapshot() {
                            self.captures[index].capture = c;
                        }
                        self.refresh_variants();
                        variant = Some((index, CaptureVariant::Original));
                    }
                    Err(e) => self.status = e,
                }
            }
        }
        if let Some((index, choice)) = variant {
            self.choose_variant(index, choice, ctx);
        }
        if let Some((from, to)) = reorder {
            self.captures.swap(from, to);
        }
        if matches!(
            action,
            Some(
                Action::Edit(_)
                    | Action::EditVariant(..)
                    | Action::Copy(..)
                    | Action::Drag(..)
                    | Action::ExportPack(_)
            )
        ) {
            self.pause();
        }
        action
    }
}
impl Drop for SessionView {
    fn drop(&mut self) {
        self.save_preferences();
        if let Some(job) = &self.loading {
            if let Ok(stage) = job.committing.lock() {
                if !*stage {
                    job.cancel.store(true, Ordering::Relaxed);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use framecraft::{
        editor::Rect,
        session_runner::{CaptureAdapter, NamingAdapter, WorkerAdapters},
    };
    struct NoCapture;
    impl CaptureAdapter for NoCapture {
        fn capture(
            &self,
            _: &CaptureTarget,
            _: &dyn Fn() -> bool,
        ) -> Result<image::RgbaImage, String> {
            Err("This test must not capture a display.".into())
        }
    }
    impl NamingAdapter for NoCapture {
        fn suggest(&self, _: &image::RgbaImage) -> Result<Option<String>, String> {
            Ok(None)
        }
    }
    fn view() -> SessionView {
        let mut view = SessionView::new();
        view.preference_path = None;
        view.recent_path = None;
        view
    }
    fn target(display: u32) -> CaptureTarget {
        CaptureTarget {
            display,
            region: Rect {
                x: 0,
                y: 0,
                width: 4,
                height: 4,
            },
            source_width: 4,
            source_height: 4,
        }
    }
    #[test]
    fn active_and_paused_sessions_reject_target_changes_until_stopped() {
        let directory = tempfile::tempdir().unwrap();
        let store = SessionStore::create(directory.path(), "Target guard").unwrap();
        let mut view = view();
        view.target = Some(target(1));
        view.runner = Some(SessionRunner::start_with_adapters(
            store,
            RunConfig {
                target: target(1),
                schedule: CaptureSchedule::Manual,
                interval: Duration::from_secs(15),
                change: ChangeConfig::default(),
                auto_name: false,
                max_captures: 10,
                max_duration: Duration::from_secs(600),
            },
            WorkerAdapters {
                capture: Arc::new(NoCapture),
                naming: Arc::new(NoCapture),
            },
            || {},
        ));
        assert!(view.set_target(target(2)).is_err());
        assert_eq!(view.target.unwrap().display, 1);
        view.pause();
        assert!(view.active());
        assert!(view.set_target(target(2)).is_err());
        assert_eq!(view.target.unwrap().display, 1);
        view.stop();
        assert!(view.set_target(target(2)).is_ok());
        assert_eq!(view.target.unwrap().display, 2);
    }
    #[test]
    fn resume_cannot_bypass_failed_control_registration() {
        let directory = tempfile::tempdir().unwrap();
        let store = SessionStore::create(directory.path(), "Controls guard").unwrap();
        let mut view = view();
        let ctx = egui::Context::default();
        let runner = SessionRunner::start_with_adapters(
            store,
            RunConfig {
                target: target(1),
                schedule: CaptureSchedule::Manual,
                interval: Duration::from_secs(15),
                change: ChangeConfig::default(),
                auto_name: false,
                max_captures: 10,
                max_duration: Duration::from_secs(600),
            },
            WorkerAdapters {
                capture: Arc::new(NoCapture),
                naming: Arc::new(NoCapture),
            },
            || {},
        );
        runner.pause();
        view.runner = Some(runner);
        view.set_controls_ready(false);
        view.resume(&ctx);
        assert!(view.runner.as_ref().unwrap().is_paused());
        assert!(view.status.contains("Stop the session"));
        view.set_controls_ready(true);
        view.resume(&ctx);
        assert!(view.runner.as_ref().unwrap().is_running());
        view.stop();
    }
    #[cfg(target_os = "macos")]
    #[test]
    fn new_session_waits_for_control_registration_acknowledgement() {
        let directory = tempfile::tempdir().unwrap();
        let mut view = view();
        let ctx = egui::Context::default();
        view.folder = Some(directory.path().to_path_buf());
        view.set_target(target(1)).unwrap();
        view.start(&ctx);
        assert!(view.active());
        assert!(view.runner.as_ref().unwrap().is_paused());
        assert!(!view.is_drained());
        assert!(view.take_started());
        view.set_controls_ready(true);
        assert!(view.runner.as_ref().unwrap().is_running());
        view.stop();
    }
    #[test]
    fn restoring_preferences_never_restores_a_capture_target() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("preferences.json");
        let mut view = view();
        view.set_target(target(2)).unwrap();
        view.schedule = CaptureSchedule::Change;
        view.interval = 31;
        view.auto_name = false;
        view.folder = Some(directory.path().to_path_buf());
        view.preferences().save(&path).unwrap();
        let saved = SessionPreferences::load(&path).unwrap();
        let mut restored = SessionView::new();
        restored.preference_path = None;
        restored.recent_path = None;
        restored.apply_preferences(saved);
        assert!(restored.target.is_none());
        assert!(!restored.active());
        assert_eq!(restored.source_hint.as_ref().unwrap().display, 2);
        assert_eq!(restored.schedule, CaptureSchedule::Change);
        assert_eq!(restored.interval, 31);
        assert!(!restored.auto_name);
    }
    #[test]
    fn headless_review_surface_does_not_start_capture_or_register_controls() {
        let mut view = view();
        let ctx = egui::Context::default();
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            assert!(view.show(ctx, true).is_none());
        });
        assert!(!view.active());
        assert!(view.target.is_none());
        assert!(!view.take_started());
        view.started = true;
        assert!(view.take_started());
        assert!(!view.take_started());
    }
}
