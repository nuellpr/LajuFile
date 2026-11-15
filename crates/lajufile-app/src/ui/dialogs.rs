//! Add-download and Settings dialogs.

use std::path::PathBuf;

use egui::{Align, Layout, RichText, Ui};
use lajufile_core::settings::{Settings, Theme};
use lajufile_core::video::{DEFAULT_SELECTOR, QualityChoice};

use crate::engine::AddRequest;

use super::Action;

#[derive(Debug, Clone)]
pub struct AddDraft {
    pub url: String,
    pub file_name: String,
    pub folder: String,
    pub start_now: bool,
    pub connections: u32,
    /// yt-dlp selector picked in the quality combo; `None` means best available.
    pub quality: Option<String>,
    pub formats: Vec<QualityChoice>,
}

impl Default for AddDraft {
    fn default() -> Self {
        Self {
            url: String::new(),
            file_name: String::new(),
            folder: String::new(),
            start_now: true,
            connections: 8,
            quality: None,
            formats: Vec::new(),
        }
    }
}

impl AddDraft {
    pub fn for_settings(settings: &Settings) -> Self {
        Self {
            connections: settings.max_connections.clamp(1, 32),
            ..Self::default()
        }
    }

    pub fn into_request(self) -> AddRequest {
        let video_format = youtube_url(&self.url).then(|| {
            self.quality
                .clone()
                .unwrap_or_else(|| DEFAULT_SELECTOR.to_string())
        });
        AddRequest {
            url: self.url.trim().to_string(),
            file_name: non_empty(self.file_name),
            save_dir: non_empty(self.folder).map(PathBuf::from),
            start_now: self.start_now,
            connections: Some(self.connections.clamp(1, 32)),
            referrer: None,
            user_agent: None,
            cookie: None,
            video_format,
        }
    }
}

/// True for the YouTube hosts yt-dlp handles.
pub fn youtube_url(url: &str) -> bool {
    let Ok(url) = url::Url::parse(url.trim()) else {
        return false;
    };
    matches!(
        url.host_str(),
        Some("youtube.com" | "www.youtube.com" | "m.youtube.com" | "youtu.be")
    )
}

pub fn add_dialog(
    ctx: &egui::Context,
    open: &mut bool,
    draft: &mut AddDraft,
    focus: &mut bool,
) -> Option<Action> {
    let mut action = None;
    egui::Window::new("Add download")
        .collapsible(false)
        .resizable(false)
        .default_width(600.0)
        .min_height(360.0)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .show(ctx, |ui| {
            ui.set_width(560.0);
            ui.label(RichText::new("Download link").strong());
            let url = ui.add_sized(
                [ui.available_width(), 30.0],
                egui::TextEdit::singleline(&mut draft.url).hint_text("https://..."),
            );
            if *focus {
                url.request_focus();
                *focus = false;
            }

            ui.add_space(8.0);
            ui.label(RichText::new("File name").strong());
            ui.add_sized(
                [ui.available_width(), 30.0],
                egui::TextEdit::singleline(&mut draft.file_name).hint_text("From URL"),
            );

            ui.add_space(8.0);
            ui.label(RichText::new("Save to folder").strong());
            ui.horizontal(|ui| {
                ui.add_sized(
                    [ui.available_width() - 96.0, 30.0],
                    egui::TextEdit::singleline(&mut draft.folder)
                        .hint_text("Default: Downloads\\LajuFile"),
                );
                if ui.button("Browse...").clicked() {
                    action = Some(Action::OpenFolderPicker);
                }
            });

            ui.add_space(12.0);
            ui.checkbox(&mut draft.start_now, "Start downloading now");
            ui.add(egui::Slider::new(&mut draft.connections, 1..=32).text("Connections"));
            ui.label(
                RichText::new("Parallel ranges are used when the server supports them.")
                    .size(11.0)
                    .color(ui.visuals().weak_text_color()),
            );

            if youtube_url(&draft.url) {
                ui.add_space(8.0);
                ui.label(RichText::new("Video quality").strong());
                if draft.formats.is_empty() {
                    if ui.button("Load qualities").clicked() {
                        action = Some(Action::LoadFormats(draft.url.trim().to_string()));
                    }
                } else {
                    let chosen = draft
                        .formats
                        .iter()
                        .position(|choice| Some(&choice.selector) == draft.quality.as_ref())
                        .map_or(0, |index| index + 1);
                    let mut selected = chosen;
                    egui::ComboBox::from_id_salt("quality")
                        .selected_text(match selected {
                            0 => "Best available".to_string(),
                            index => draft.formats[index - 1].label.clone(),
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut selected, 0, "Best available");
                            for (index, choice) in draft.formats.iter().enumerate() {
                                ui.selectable_value(&mut selected, index + 1, &choice.label);
                            }
                        });
                    draft.quality =
                        (selected > 0).then(|| draft.formats[selected - 1].selector.clone());
                }
            }

            ui.add_space(12.0);
            ui.separator();
            let can_add = !draft.url.trim().is_empty();
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui
                    .add_enabled(can_add, egui::Button::new("Add download"))
                    .clicked()
                {
                    draft.start_now = true;
                    action = Some(Action::Add(draft.clone().into_request()));
                    *open = false;
                }
                if ui
                    .add_enabled(can_add, egui::Button::new("Add to queue"))
                    .clicked()
                {
                    draft.start_now = false;
                    action = Some(Action::Add(draft.clone().into_request()));
                    *open = false;
                }
                if ui.button("Cancel").clicked() {
                    *open = false;
                }
            });
        });
    action
}

pub fn settings_dialog(
    ctx: &egui::Context,
    open: &mut bool,
    settings: &mut Settings,
) -> Option<Action> {
    let mut action = None;
    egui::Window::new("Settings")
        .collapsible(false)
        .resizable(false)
        .default_width(720.0)
        .default_height(0.0)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .show(ctx, |ui| {
            ui.set_width(680.0);
            egui::ScrollArea::vertical()
                .max_height(440.0)
                .show(ui, |ui| {
                    section(ui, "Downloads");
                    ui.label("Save location");
                    let mut folder = settings.download_dir.display().to_string();
                    let folder_response = ui.add_sized(
                        [ui.available_width(), 28.0],
                        egui::TextEdit::singleline(&mut folder),
                    );
                    if folder_response.changed() {
                        settings.download_dir = PathBuf::from(folder.trim());
                    }
                    ui.add(
                        egui::Slider::new(&mut settings.max_concurrent, 1..=10)
                            .text("Simultaneous downloads"),
                    );
                    ui.add(
                        egui::Slider::new(&mut settings.max_connections, 1..=32)
                            .text("Connections per download"),
                    );
                    ui.label(
                        RichText::new("Parallel ranges are used when the server supports them.")
                            .size(11.0)
                            .color(ui.visuals().weak_text_color()),
                    );
                    ui.add(
                        egui::DragValue::new(&mut settings.global_speed_limit_kbps)
                            .range(0..=1_000_000)
                            .speed(64)
                            .suffix(" KB/s")
                            .prefix("Speed limit "),
                    );
                    ui.label(
                        RichText::new("0 KB/s means unlimited.")
                            .size(11.0)
                            .color(ui.visuals().weak_text_color()),
                    );

                    ui.separator();
                    section(ui, "Behavior");
                    ui.checkbox(&mut settings.close_to_tray, "Close to tray");
                    ui.checkbox(&mut settings.auto_resume, "Auto-resume on startup");
                    ui.checkbox(
                        &mut settings.ask_where_to_save,
                        "Ask where to save each download",
                    );
                    ui.horizontal(|ui| {
                        ui.label("Theme");
                        ui.selectable_value(&mut settings.theme, Theme::Light, "Light");
                        ui.selectable_value(&mut settings.theme, Theme::Dark, "Dark");
                    });

                    ui.separator();
                    section(ui, "Video");
                    let tools_dir = crate::tools::tools_dir();
                    let ready = crate::tools::tools_ready(&tools_dir);
                    ui.label(format!(
                        "yt-dlp and ffmpeg: {}",
                        if ready { "installed" } else { "not installed" }
                    ));
                    ui.label(
                        RichText::new(tools_dir.display().to_string())
                            .size(11.0)
                            .color(ui.visuals().weak_text_color()),
                    );
                    if ui.button("Install or update video tools").clicked() {
                        action = Some(Action::UpdateTools);
                    }

                    ui.separator();
                    section(ui, "Browser capture");
                    ui.horizontal(|ui| {
                        ui.label("Port");
                        ui.add(
                            egui::DragValue::new(&mut settings.bridge_port)
                                .range(1024..=65_535)
                                .speed(1),
                        );
                    });
                    ui.label(
                        RichText::new("Port changes apply after restart")
                            .size(11.0)
                            .color(ui.visuals().weak_text_color()),
                    );
                    ui.horizontal(|ui| {
                        ui.label(format!("Token {}", mask_token(&settings.bridge_token)));
                        if ui.button("Copy").clicked() {
                            ui.ctx().copy_text(settings.bridge_token.clone());
                        }
                        if ui.button("Regenerate").clicked() {
                            action = Some(Action::RegenerateToken);
                        }
                    });
                    ui.label(
                        RichText::new("Regenerating invalidates the old token immediately.")
                            .size(11.0)
                            .color(ui.visuals().weak_text_color()),
                    );
                    if ui.button("Test connection").clicked() {
                        action = Some(Action::TestBridge(settings.bridge_port));
                    }
                    ui.label(
                        RichText::new(
                            "Paste this port and token into the LajuFile Edge extension options.",
                        )
                        .size(11.0)
                        .color(ui.visuals().weak_text_color()),
                    );
                });

            ui.separator();
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.button("Save").clicked() {
                    action = Some(Action::SaveSettings(settings.clone()));
                    *open = false;
                }
                if ui.button("Cancel").clicked() {
                    *open = false;
                }
            });
        });
    action
}

fn section(ui: &mut Ui, title: &str) {
    ui.add_space(4.0);
    ui.label(RichText::new(title).size(13.0).strong());
}

fn non_empty(value: String) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_string())
}

fn mask_token(token: &str) -> String {
    if token.len() >= 8 {
        format!("{}…{}", &token[..4], &token[token.len() - 4..])
    } else if token.is_empty() {
        "not generated".to_string()
    } else {
        "…".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_draft_keeps_non_empty_values() {
        let request = AddDraft {
            url: " https://example.com/a.bin ".into(),
            file_name: " a.bin ".into(),
            folder: " D:\\Downloads ".into(),
            start_now: false,
            ..AddDraft::default()
        }
        .into_request();
        assert_eq!(request.url, "https://example.com/a.bin");
        assert_eq!(request.file_name.as_deref(), Some("a.bin"));
        assert_eq!(request.save_dir, Some(PathBuf::from("D:\\Downloads")));
        assert!(!request.start_now);
        assert_eq!(request.connections, Some(8));
        assert_eq!(request.video_format, None);
    }

    #[test]
    fn youtube_links_become_video_jobs() {
        assert!(youtube_url("https://www.youtube.com/watch?v=abc"));
        assert!(youtube_url("https://youtu.be/abc"));
        assert!(youtube_url("https://m.youtube.com/watch?v=abc"));
        assert!(!youtube_url("https://example.com/a.bin"));
        assert!(!youtube_url("nonsense"));

        let request = AddDraft {
            url: "https://youtu.be/abc".into(),
            ..AddDraft::default()
        }
        .into_request();
        assert_eq!(request.video_format.as_deref(), Some(DEFAULT_SELECTOR));

        let chosen = AddDraft {
            url: "https://youtu.be/abc".into(),
            quality: Some("bv*[height<=720]+ba".into()),
            ..AddDraft::default()
        }
        .into_request();
        assert_eq!(chosen.video_format.as_deref(), Some("bv*[height<=720]+ba"));
    }

    #[test]
    fn add_draft_uses_settings_connections() {
        let settings = Settings {
            max_connections: 3,
            ..Settings::default()
        };
        let request = AddDraft::for_settings(&settings).into_request();
        assert_eq!(request.connections, Some(3));
    }

    #[test]
    fn add_draft_clamps_connections() {
        let request = AddDraft {
            connections: 99,
            ..AddDraft::default()
        }
        .into_request();
        assert_eq!(request.connections, Some(32));
    }

    #[test]
    fn masks_tokens_without_panicking() {
        assert_eq!(mask_token("abcdefgh"), "abcd…efgh");
        assert_eq!(mask_token("abc"), "…");
        assert_eq!(mask_token(""), "not generated");
    }
}
