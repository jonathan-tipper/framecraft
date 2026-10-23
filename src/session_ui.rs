use eframe::egui::{self, Color32, RichText};
use framecraft::{
    capture::CaptureTarget,
    session::{CaptureHandle, ChangeConfig, SavedCapture, SessionStore},
    session_runner::{CaptureSchedule, RunConfig, SessionEvent, SessionRunner},
};
use std::{path::PathBuf, time::Duration};

pub enum Action {
    SelectDisplay(u32),
    Edit(CaptureHandle),
}
struct CaptureRow {
    handle: CaptureHandle,
    capture: SavedCapture,
    texture: egui::TextureHandle,
    draft: String,
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
    captures: Vec<CaptureRow>,
    status: String,
    saw_unfocused: bool,
    #[cfg(target_os = "macos")]
    hotkeys: Option<(
        global_hotkey::GlobalHotKeyManager,
        [global_hotkey::hotkey::HotKey; 3],
    )>,
}

impl SessionView {
    pub fn new() -> Self {
        Self {
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
            captures: vec![],
            status: "Select the shared content once. Capture again without selecting it again."
                .into(),
            saw_unfocused: false,
            #[cfg(target_os = "macos")]
            hotkeys: None,
        }
    }
    pub fn active(&self) -> bool {
        self.runner.as_ref().is_some_and(|r| !r.is_stopped())
    }
    pub fn pause(&mut self) {
        if let Some(r) = &self.runner {
            r.pause();
        }
    }
    pub fn set_target(&mut self, target: CaptureTarget) {
        self.pause();
        self.target = Some(target);
        self.status = "Area set. Choose a destination and start the session.".into();
    }
    pub fn poll(&mut self, ctx: &egui::Context) {
        let events: Vec<_> = self
            .runner
            .as_ref()
            .map(|r| r.events.try_iter().collect())
            .unwrap_or_default();
        for event in events {
            match event {
                SessionEvent::Saved(handle, pixels) => {
                    let capture = match handle.snapshot() {
                        Ok(capture) => capture,
                        Err(error) => {
                            self.status = error;
                            continue;
                        }
                    };
                    let image = egui::ColorImage::from_rgba_unmultiplied(
                        [pixels.width() as usize, pixels.height() as usize],
                        pixels.as_raw(),
                    );
                    let texture = ctx.load_texture(
                        format!("capture-{}", capture.sequence),
                        image,
                        egui::TextureOptions::LINEAR,
                    );
                    self.status = format!(
                        "Saved {}. {} captures in this session.",
                        capture.title,
                        self.captures.len() + 1
                    );
                    self.captures.push(CaptureRow {
                        handle,
                        draft: capture.title.clone(),
                        capture,
                        texture,
                    });
                }
                SessionEvent::Named { .. } => {
                    // The handle is authoritative: a manual rename may already
                    // have superseded this queued OCR notification.
                    for row in &mut self.captures {
                        if let Ok(capture) = row.handle.snapshot() {
                            if row.draft == row.capture.title {
                                row.draft = capture.title.clone();
                            }
                            row.capture = capture;
                        }
                    }
                }
                SessionEvent::Paused(message)
                | SessionEvent::Finished(message)
                | SessionEvent::Note(message) => self.status = message,
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
        #[cfg(target_os = "macos")]
        self.poll_hotkeys(ctx);
        if self.active() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
        #[cfg(target_os = "macos")]
        if !self.active() {
            self.unregister_hotkeys();
        }
    }
    fn start(&mut self, ctx: &egui::Context) {
        if !cfg!(target_os = "macos") {
            self.status =
                "Live capture requires macOS. The editor and CLI run on this platform.".into();
            return;
        }
        let Some(target) = self.target else {
            self.status = "Select a shared area first.".into();
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
        self.captures.clear();
        self.saw_unfocused = false;
        #[cfg(target_os = "macos")]
        if let Err(e) = self.register_hotkeys() {
            self.pause();
            self.status = format!("Session paused because global shortcuts could not register: {e}. Resolve the shortcut conflict before resuming.");
            return;
        } else {
            self.status = "Running. Command + Option + S captures the selected area.".into();
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
    }
    fn resume(&mut self, ctx: &egui::Context) {
        #[cfg(target_os = "macos")]
        if self.hotkeys.is_none() {
            if let Err(e) = self.register_hotkeys() {
                self.status = format!("Could not register global shortcuts: {e}");
                return;
            }
        }
        if let Some(r) = &self.runner {
            r.resume();
            self.saw_unfocused = false;
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
            self.status = "Session resumed.".into();
        }
    }
    fn stop(&mut self) {
        if let Some(r) = &self.runner {
            r.stop();
        }
        self.status = "Stopped. Your captures are saved.".into();
    }
    pub fn show(&mut self, ctx: &egui::Context) -> Option<Action> {
        let mut action = None;
        let mut rename = None;
        egui::CentralPanel::default().show(ctx,|ui| {
            egui::ScrollArea::vertical().show(ui,|ui| {
                ui.add_space(16.0);ui.heading("Stay in the meeting.");
                ui.label("A capture session remembers the area, the destination and how you want to collect frames.");
                ui.add_space(12.0);
                let active=self.active();
                ui.add_enabled_ui(!active,|ui| {
                    ui.horizontal(|ui| {ui.label("Session");ui.text_edit_singleline(&mut self.title);});
                    ui.horizontal(|ui| {
                        if ui.button("Choose folder").clicked() {if let Some(p)=rfd::FileDialog::new().pick_folder(){self.folder=Some(p);}}
                        ui.label(self.folder.as_ref().map(|p|p.display().to_string()).unwrap_or_else(||"Choose once; a dated session folder is created inside it.".into()));
                    });
                    ui.horizontal(|ui| {
                        ui.label("Display");ui.add(egui::DragValue::new(&mut self.display).range(1..=16));
                        if ui.button("Select shared area").clicked() {action=Some(Action::SelectDisplay(self.display));}
                        if let Some(t)=self.target {ui.label(format!("Display {} / {} x {} px",t.display,t.region.width,t.region.height));}
                    });
                    ui.label(RichText::new("Select the slide or browser content, leaving participant tiles and chat outside the area.").small());
                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        ui.selectable_value(&mut self.schedule,CaptureSchedule::Manual,"On shortcut");
                        ui.selectable_value(&mut self.schedule,CaptureSchedule::Interval,"On a timer");
                        ui.selectable_value(&mut self.schedule,CaptureSchedule::Change,"When content changes");
                    });
                    match self.schedule {
                        CaptureSchedule::Manual=>{ui.label("Command + Option + S. No selector, editor or save dialog between captures.");},
                        CaptureSchedule::Interval=>{ui.add(egui::Slider::new(&mut self.interval,5..=120).text("Seconds between captures"));},
                        CaptureSchedule::Change=>{
                            ui.add(egui::Slider::new(&mut self.threshold,0.01..=0.25).text("Changed area fraction"));
                            ui.add(egui::Slider::new(&mut self.settle,0.5..=5.0).text("Stable seconds before saving"));
                            ui.label(RichText::new("Samples once a second. Small cursor changes are ignored. Video or animation can prevent a stable capture.").small());
                        }
                    }
                    ui.checkbox(&mut self.auto_name,"Name from the visible heading with local Apple Vision OCR");
                    ui.label(RichText::new("Save first, name afterwards. No cloud calls or model downloads. Diagrams may keep a timestamp name.").small());
                    ui.horizontal(|ui| {
                        ui.label("Stop after");ui.add(egui::DragValue::new(&mut self.max_minutes).range(1..=480));ui.label("minutes or");
                        ui.add(egui::DragValue::new(&mut self.max_frames).range(1..=2000));ui.label("captures");
                    });
                });
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if !active {
                        if ui.add_enabled(self.target.is_some()&&self.folder.is_some(),egui::Button::new("Start session").fill(Color32::from_rgb(36,92,88))).clicked(){self.start(ctx);}
                    } else {
                        let paused=self.runner.as_ref().is_some_and(SessionRunner::is_paused);
                        if paused {if ui.button("Resume and minimise").clicked(){self.resume(ctx);}}
                        else if ui.button("Pause").clicked(){self.pause();}
                        if ui.button("Stop session").clicked(){self.stop();}
                    }
                    if let Some(r)=&self.runner {ui.label(format!("{} captures",self.captures.len()));ui.label(r.folder.display().to_string());}
                });
                ui.label(RichText::new(&self.status).color(Color32::from_rgb(91,221,203)));
                ui.label(RichText::new("While running: Cmd + Option + S capture · P pause/resume · X stop. Opening Framecraft pauses capture.").small());
                ui.separator();ui.heading("Collected frames");
                if self.captures.is_empty(){ui.label("Saved frames will appear here. Open one to redact, highlight or annotate it.");}
                for (index,row) in self.captures.iter_mut().enumerate().rev() {
                    ui.horizontal(|ui| {
                        ui.add(egui::Image::new(&row.texture).max_size(egui::vec2(150.0,90.0)));
                        ui.vertical(|ui| {
                            ui.horizontal(|ui| {
                                ui.label(format!("{:04}",row.capture.sequence));
                                ui.text_edit_singleline(&mut row.draft);
                                if ui.button("Rename").clicked(){rename=Some((index,row.draft.clone()));}
                                if ui.button("Edit").clicked(){action=Some(Action::Edit(row.handle.clone()));}
                            });
                            ui.label(RichText::new(row.capture.path.file_name().unwrap_or_default().to_string_lossy()).small());
                        });
                    });ui.separator();
                }
            });
        });
        if let Some((i, title)) = rename {
            match self.captures[i].handle.rename(&title) {
                Ok(c) => {
                    self.captures[i].draft = c.title.clone();
                    self.captures[i].capture = c;
                }
                Err(e) => self.status = e,
            }
        }
        if matches!(action, Some(Action::Edit(_))) {
            self.pause();
        }
        action
    }
}

#[cfg(target_os = "macos")]
impl SessionView {
    fn register_hotkeys(&mut self) -> Result<(), String> {
        use global_hotkey::{
            hotkey::{Code, HotKey, Modifiers},
            GlobalHotKeyManager,
        };
        self.unregister_hotkeys();
        let manager = GlobalHotKeyManager::new().map_err(|e| e.to_string())?;
        let modifiers = Some(Modifiers::SUPER | Modifiers::ALT);
        let keys = [
            HotKey::new(modifiers, Code::KeyS),
            HotKey::new(modifiers, Code::KeyP),
            HotKey::new(modifiers, Code::KeyX),
        ];
        for (i, key) in keys.iter().enumerate() {
            if let Err(e) = manager.register(*key) {
                for prior in &keys[..i] {
                    let _ = manager.unregister(*prior);
                }
                return Err(e.to_string());
            }
        }
        self.hotkeys = Some((manager, keys));
        Ok(())
    }
    fn unregister_hotkeys(&mut self) {
        if let Some((manager, keys)) = self.hotkeys.take() {
            for key in keys {
                let _ = manager.unregister(key);
            }
        }
    }
    fn poll_hotkeys(&mut self, ctx: &egui::Context) {
        use global_hotkey::{GlobalHotKeyEvent, HotKeyState};
        while let Ok(event) = GlobalHotKeyEvent::receiver().try_recv() {
            if event.state != HotKeyState::Pressed {
                continue;
            }
            let Some((_, keys)) = &self.hotkeys else {
                continue;
            };
            if event.id == keys[0].id() {
                if let Some(r) = &self.runner {
                    if let Err(e) = r.capture_now() {
                        self.status = e;
                    }
                }
            } else if event.id == keys[1].id() {
                if self.runner.as_ref().is_some_and(SessionRunner::is_running) {
                    self.pause();
                    self.status = "Paused from shortcut.".into();
                } else {
                    self.resume(ctx);
                }
            } else if event.id == keys[2].id() {
                self.stop();
            }
        }
    }
}
