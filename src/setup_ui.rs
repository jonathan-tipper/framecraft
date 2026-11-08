//! Native, transactional first-run and everyday capture settings.
use crate::{
    app::shortcut_editor,
    ui_icons::{self, Icon},
};
use eframe::egui::{self, RichText};
use framecraft::{
    capture,
    quick_capture::{AfterCapture, QuickCaptureSettings},
};

pub struct SetupDraft {
    pub settings: QuickCaptureSettings,
    pub error: Option<String>,
    step: usize,
    access: capture::PermissionStatus,
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
        }
    }

    pub fn show(&mut self, ctx: &egui::Context) -> SetupAction {
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
                        1 => {
                            ui.heading("Capture from any app");
                            ui.label("Framecraft must be running. The shortcut starts an area selection; Escape cancels it.");
                            ui.add_space(12.0);
                            ui.checkbox(&mut self.settings.shortcut_enabled, "Enable everyday capture shortcut");
                            ui.add_enabled_ui(self.settings.shortcut_enabled, |ui| {
                                shortcut_editor(ui, "Capture an area", &mut self.settings.shortcut);
                            });
                            ui.label("Command is the ⌘ key; Option is ⌥. Session shortcuts remain separate. Conflicts are checked when you finish.");
                        }
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
                            ui.label("Saves the captured pixels immediately. Annotations are saved separately, so the original capture stays intact.");
                            ui.add_space(14.0);
                            ui.separator();
                            ui.strong("Your setup");
                            ui.label(if self.settings.shortcut_enabled { format!("Area shortcut: {}", self.settings.shortcut) } else { "Everyday shortcut is off. Capture buttons still work.".into() });
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
                        } else if ui.button("Continue").clicked() { self.step += 1; self.error = None; }
                        if self.step > 0 && ui.button("Back").clicked() { self.step -= 1; self.error = None; }
                    });
                });
            });
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            action = SetupAction::Cancel;
        }
        action
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
                assert!(matches!(draft.show(ctx), SetupAction::None));
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
            assert!(matches!(draft.show(ctx), SetupAction::Cancel));
        });
        assert!(!draft.settings.setup_complete);
    }
}
