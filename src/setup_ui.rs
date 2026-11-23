//! Native, transactional first-run and everyday capture settings.
use crate::{
    app::shortcut_editor,
    ui_icons::{self, Icon},
};
use eframe::egui::{self, RichText};
use framecraft::{
    capture,
    quick_capture::{AfterCapture, QuickCaptureSettings},
    settings::{ShortcutParts, ShortcutSettings},
};

pub struct SetupDraft {
    pub settings: QuickCaptureSettings,
    pub error: Option<String>,
    step: usize,
    access: capture::PermissionStatus,
    recording: bool,
    registered_stroke: Option<RecordedShortcut>,
}

pub enum SetupAction {
    None,
    Cancel,
    Finish,
}

impl SetupDraft {
    pub fn new(settings: QuickCaptureSettings) -> Self {
        Self {
            settings,
            error: None,
            step: 0,
            access: capture::permission_status(),
            recording: false,
            registered_stroke: None,
        }
    }

    pub fn review_shortcut(settings: QuickCaptureSettings, warning: String) -> Self {
        let mut draft = Self::new(settings);
        draft.step = 1;
        draft.error = Some(warning);
        draft
    }

    pub fn offer_registered_stroke(&mut self, active: &QuickCaptureSettings) {
        if self.recording {
            self.registered_stroke = Some(RecordedShortcut {
                binding: active.shortcut.clone(),
                layout_key: active.recorded_key_label.clone(),
            });
        }
    }

    fn shortcut_step(&mut self, ui: &mut egui::Ui, session: &ShortcutSettings) {
        let registered_stroke = self.registered_stroke.take();
        ui.heading("Capture from any app");
        ui.label(
            "Framecraft must be running. The shortcut starts an area selection. Escape cancels it.",
        );
        ui.radio_value(
            &mut self.settings.shortcut_enabled,
            true,
            "Use a global shortcut",
        );
        ui.radio_value(
            &mut self.settings.shortcut_enabled,
            false,
            "Use menu bar only",
        );
        if !self.settings.shortcut_enabled {
            self.recording = false;
            return;
        }
        ui.label(self.settings.shortcut_label());
        let response = ui.button(if self.recording {
            "Press your shortcut…"
        } else {
            "Record shortcut"
        });
        if self.recording
            && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
        {
            self.recording = false;
            self.error = None;
        } else if response.clicked() && !self.recording {
            response.request_focus();
            self.recording = true;
            self.error = None;
        } else if self.recording {
            if response.has_focus() && ui.input(|i| i.focused) {
                let event = ui.input_mut(|i| {
                    let (index, key) =
                        i.events.iter().enumerate().find_map(|(index, event)| {
                            recorded_key(event).map(|key| (index, key))
                        })?;
                    i.events.remove(index);
                    Some(key)
                });
                if let Some(key) = event.or_else(|| registered_stroke.map(Ok)) {
                    let result = key.and_then(|stroke| {
                        let mut candidate = self.settings.clone();
                        candidate.shortcut = stroke.binding;
                        candidate.recorded_key_label = stroke.layout_key;
                        candidate.validate_shortcut(session)?;
                        self.settings = candidate;
                        Ok(())
                    });
                    self.recording = result.is_err();
                    self.error = result.err();
                }
            } else {
                self.recording = false;
            }
        }
        if self.recording {
            ui.label("Press the keys together. Escape keeps the previous choice.");
            ui.memory_mut(|m| {
                m.set_focus_lock_filter(
                    response.id,
                    egui::EventFilter {
                        escape: true,
                        ..Default::default()
                    },
                )
            });
        }
        ui.collapsing("Choose keys individually", |ui| {
            let before = self.settings.shortcut.clone();
            shortcut_editor(ui, "Capture Area", &mut self.settings.shortcut, 20);
            if self.settings.shortcut != before {
                self.settings.recorded_key_label = None;
                self.recording = false;
                self.error = None;
            }
        });
        ui.label("Finish setup checks availability. Some Mac apps share shortcuts without reporting a conflict. Try the keys in the apps you use, or choose a different combination.");
    }

    pub fn show(&mut self, ctx: &egui::Context, session: &ShortcutSettings) -> SetupAction {
        let mut action = SetupAction::None;
        egui::Window::new("Make capture yours")
            .id(egui::Id::new("capture-setup"))
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .collapsible(false).resizable(false).default_width(560.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    for (i, label) in ["Access", "Shortcut", "Files", "Finish"].iter().enumerate() {
                        let text = format!("{}  {label}", i + 1);
                        ui.label(if i == self.step { RichText::new(text).strong().color(egui::Color32::from_rgb(39,90,223)) } else { RichText::new(text).weak() });
                        if i < 3 { ui.label("/"); }
                    }
                });
                ui.add_space(16.0);
                egui::ScrollArea::vertical().max_height(350.0).show(ui, |ui| {
                    match self.step {
                        0 => {
                            ui.heading("A shortcut. A selection. Your screenshot.");
                            ui.label("Set up your everyday capture once. You can change these choices from Capture setup at any time.");
                            ui.add_space(14.0);
                            ui.strong("Allow screen capture");
                            ui.label(match self.access {
                                capture::PermissionStatus::Granted => "Screen access is granted to this running copy.",
                                capture::PermissionStatus::NotGranted => "macOS needs your permission before Framecraft can capture the screen.",
                                capture::PermissionStatus::Unknown => "Screen capture is available on macOS. You can still open and edit images.",
                            });
                            ui.horizontal_wrapped(|ui| {
                                if ui.button("Request access").clicked() { self.access = capture::request_permission(); }
                                if ui.button("System Settings").clicked() { self.error = capture::open_permission_settings().err(); }
                                if ui.button("Check again").clicked() { self.access = capture::permission_status(); }
                            });
                            ui.label("You can continue setup without granting access. Capture will ask when needed. If macOS requests a restart, quit and reopen this copy.");
                        }
                        1 => self.shortcut_step(ui, session),
                        2 => {
                            ui.heading("Give screenshots a home");
                            ui.label("Save location");
                            ui.label(self.settings.destination.display().to_string());
                            if ui_icons::button(ui, "Choose folder…", Icon::Folder, false).clicked() {
                                if let Some(path) = rfd::FileDialog::new().set_directory(&self.settings.destination).pick_folder() { self.settings.destination = path; }
                            }
                            ui.add_space(10.0);
                            let prefix_label = ui.label("Filename prefix");
                            ui.add(egui::TextEdit::singleline(&mut self.settings.filename_prefix).char_limit(64).desired_width(240.0)).labelled_by(prefix_label.id);
                            ui.checkbox(&mut self.settings.organise_by_date, "Group captures into date folders");
                            ui.add_space(10.0);
                            ui.strong("Example path");
                            let mut path = self.settings.destination.clone();
                            if self.settings.organise_by_date { path.push("2026-11-08"); }
                            path.push(format!("{}-20261108-142233.png", self.settings.filename_prefix));
                            ui.label(path.display().to_string());
                            ui.label("Dates and timestamps use UTC. A collision suffix keeps existing files safe. Folder and prefix also prefill Save PNG.");
                        }
                        _ => {
                            ui.heading("What happens after capture?");
                            ui.radio_value(&mut self.settings.after_capture, AfterCapture::Edit, "Open the editor");
                            ui.label("Review, annotate, then choose Save PNG or Copy image. Nothing is saved automatically.");
                            ui.add_space(8.0);
                            ui.radio_value(&mut self.settings.after_capture, AfterCapture::Save, "Save automatically, then open for optional editing");
                            ui.radio_value(&mut self.settings.after_capture, AfterCapture::CopyAndReturn, "Copy and return");
                            if self.settings.after_capture == AfterCapture::CopyAndReturn {
                                ui.label("Paste straight into the app you were using. This capture uses the clipboard on this Mac only.");
                            }
                            ui.radio_value(&mut self.settings.after_capture, AfterCapture::SaveAndReturn, "Save and return");
                            if self.settings.after_capture == AfterCapture::SaveAndReturn {
                                ui.label(format!("Stay in your app. PNGs go to {}.", self.settings.destination.display()));
                            }
                            if self.settings.after_capture.stays_in_source() {
                                ui.checkbox(&mut self.settings.show_thumbnail, "Show a thumbnail after capture");
                                ui.label("Edit Last Capture is also available from the Framecraft menu bar icon.");
                            }
                            ui.add_space(14.0);
                            ui.separator();
                            ui.strong("Your setup");
                            if self.settings.shortcut_enabled {
                                ui.label(format!("Area shortcut: {}", self.settings.shortcut_label()));
                                if ui.button("Use menu bar only").clicked() {
                                    self.settings.shortcut_enabled = false;
                                    self.error = None;
                                }
                            } else {
                                ui.label("Everyday shortcut is off. Use Capture Area in the menu bar.");
                            }
                            ui.label(format!("Folder: {}", self.settings.destination.display()));
                            ui.label(format!("Names start with: {}", self.settings.filename_prefix));
                            ui.label("Finish checks your choices and saves them on this Mac.");
                        }
                    }
                    if let Some(error) = &self.error {
                        ui.add_space(12.0);
                        ui.colored_label(egui::Color32::from_rgb(177,39,35), error);
                    }
                });
                ui.add_space(16.0);
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button(if self.settings.setup_complete { "Cancel" } else { "Not now" }).clicked() { action = SetupAction::Cancel; }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if self.step == 3 {
                            if ui_icons::button(ui, "Finish setup", Icon::Save, false).clicked() { action = SetupAction::Finish; }
                        } else if ui.add_enabled(!self.recording, egui::Button::new("Continue")).clicked() {
                            self.error = if self.step == 1 && self.settings.shortcut_enabled {
                                self.settings.validate_shortcut(session).err()
                            } else { None };
                            if self.error.is_none() { self.step += 1; }
                        }
                        if self.step > 0 && ui.button("Back").clicked() { self.step -= 1; self.recording = false; self.error = None; }
                    });
                });
            });
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            action = SetupAction::Cancel;
        }
        action
    }
}

struct RecordedShortcut {
    binding: String,
    layout_key: Option<String>,
}

fn recorded_key(event: &egui::Event) -> Option<Result<RecordedShortcut, String>> {
    match event {
        egui::Event::Copy | egui::Event::Cut | egui::Event::Paste(_) => Some(Err(
            "That shortcut belongs to the clipboard. Choose another combination.".into(),
        )),
        egui::Event::Key {
            key,
            physical_key,
            pressed: true,
            repeat: false,
            modifiers,
            ..
        } if !matches!(key, egui::Key::Tab | egui::Key::Escape) => {
            let position = physical_key.unwrap_or(*key);
            let parts = ShortcutParts {
                command: modifiers.mac_cmd || (modifiers.command && !modifiers.ctrl),
                option: modifiers.alt,
                control: modifiers.ctrl,
                shift: modifiers.shift,
                key: position.name().into(),
            };
            // GlobalHotKey uses hardware positions. Keep the actual key legend
            // as well on layouts such as AZERTY. Shifted punctuation stays on
            // its number key, e.g. Shift+2 still displays as Shift+2.
            let legend = key.name();
            let label = (*key != position
                && legend.len() == 1
                && legend.as_bytes()[0].is_ascii_alphanumeric())
            .then(|| legend.to_owned());
            Some(Ok(RecordedShortcut {
                binding: parts.display(),
                layout_key: label,
            }))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn render_shortcut_step(
        draft: &mut SetupDraft,
        ctx: &egui::Context,
        events: Vec<egui::Event>,
        focused: bool,
    ) -> egui::FullOutput {
        let mut strokes = Vec::new();
        for event in events {
            let release = match &event {
                egui::Event::Key {
                    key,
                    physical_key,
                    modifiers,
                    pressed: true,
                    ..
                } => Some(egui::Event::Key {
                    key: *key,
                    physical_key: *physical_key,
                    pressed: false,
                    repeat: false,
                    modifiers: *modifiers,
                }),
                _ => None,
            };
            strokes.push(event);
            strokes.extend(release);
        }
        ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(900., 620.),
                )),
                events: strokes,
                focused,
                ..Default::default()
            },
            |ctx| {
                assert!(matches!(
                    draft.show(ctx, &ShortcutSettings::default()),
                    SetupAction::None
                ));
            },
        )
    }
    fn arm_recorder(draft: &mut SetupDraft, ctx: &egui::Context) {
        let tree = render_shortcut_step(draft, ctx, vec![], true)
            .platform_output
            .accesskit_update
            .unwrap();
        let target = tree
            .nodes
            .iter()
            .find(|(_, node)| node.label() == Some("Record shortcut"))
            .unwrap()
            .0;
        let activate = egui::Event::AccessKitActionRequest(egui::accesskit::ActionRequest {
            action: egui::accesskit::Action::Click,
            target,
            data: None,
        });
        render_shortcut_step(draft, ctx, vec![activate], true);
        assert!(draft.recording);
    }
    fn chord(
        key: egui::Key,
        position: egui::Key,
        modifiers: egui::Modifiers,
        repeat: bool,
    ) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: Some(position),
            modifiers,
            repeat,
            pressed: true,
        }
    }
    #[test]
    fn recorder_is_focused_scoped_and_keeps_escape_inside_the_shortcut_step() {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let mut draft = SetupDraft::new(QuickCaptureSettings::default());
        draft.step = 1;
        let original = draft.settings.clone();
        let mods = egui::Modifiers::CTRL | egui::Modifiers::SHIFT;
        let change = chord(egui::Key::A, egui::Key::Q, mods, false);
        render_shortcut_step(&mut draft, &ctx, vec![change.clone()], true);
        assert_eq!(draft.settings, original);
        arm_recorder(&mut draft, &ctx);
        render_shortcut_step(&mut draft, &ctx, vec![egui::Event::Copy], true);
        assert!(draft.error.as_ref().unwrap().contains("clipboard"));
        assert_eq!(draft.settings, original);
        render_shortcut_step(
            &mut draft,
            &ctx,
            vec![chord(
                egui::Key::Escape,
                egui::Key::Escape,
                egui::Modifiers::NONE,
                false,
            )],
            true,
        );
        assert!(!draft.recording);
        assert_eq!(draft.settings, original);
        arm_recorder(&mut draft, &ctx);
        render_shortcut_step(&mut draft, &ctx, vec![], false);
        assert!(!draft.recording);
        render_shortcut_step(&mut draft, &ctx, vec![change.clone()], true);
        assert_eq!(draft.settings, original);
        arm_recorder(&mut draft, &ctx);
        render_shortcut_step(&mut draft, &ctx, vec![change], true);
        assert!(!draft.recording);
        assert_eq!(draft.settings.shortcut, "Control + Shift + Q");
        assert_eq!(draft.settings.recorded_key_label.as_deref(), Some("A"));
        assert!(!draft.settings.setup_complete);
        arm_recorder(&mut draft, &ctx);
        draft.offer_registered_stroke(&original);
        render_shortcut_step(&mut draft, &ctx, vec![], false);
        assert_eq!(draft.settings.recorded_key_label.as_deref(), Some("A"));
        arm_recorder(&mut draft, &ctx);
        draft.offer_registered_stroke(&original);
        render_shortcut_step(&mut draft, &ctx, vec![], true);
        assert_eq!(draft.settings, original);
    }
    #[test]
    fn recorder_ignores_key_repeats_releases_and_navigation() {
        let mods = egui::Modifiers::CTRL;
        assert!(recorded_key(&chord(egui::Key::A, egui::Key::A, mods, true)).is_none());
        assert!(recorded_key(&chord(egui::Key::Tab, egui::Key::Tab, mods, false)).is_none());
        let release = egui::Event::Key {
            key: egui::Key::A,
            physical_key: None,
            pressed: false,
            repeat: false,
            modifiers: mods,
        };
        assert!(recorded_key(&release).is_none());
    }
    #[test]
    fn all_setup_steps_render_at_minimum_window_and_escape_keeps_preferences_uncommitted() {
        let ctx = egui::Context::default();
        let settings = QuickCaptureSettings::default();
        let mut draft = SetupDraft::new(settings.clone());
        for step in 0..4 {
            draft.step = step;
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(900., 620.),
                )),
                ..Default::default()
            };
            let _ = ctx.run(input, |ctx| {
                assert!(matches!(
                    draft.show(ctx, &ShortcutSettings::default()),
                    SetupAction::None
                ));
            });
        }
        assert_eq!(draft.settings, settings);
        let input = egui::RawInput {
            events: vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| {
            assert!(matches!(
                draft.show(ctx, &ShortcutSettings::default()),
                SetupAction::Cancel
            ));
        });
        assert!(!draft.settings.setup_complete);
    }
}
