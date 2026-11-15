//! Add-download and Settings dialogs.

use std::path::PathBuf;

use egui::{Align, Layout, RichText, Ui};
use lajufile_core::settings::{Settings, Theme};
use lajufile_core::store::{Download, DownloadStatus, Segment};
use lajufile_core::video::{DEFAULT_SELECTOR, QualityChoice};

use crate::engine::AddRequest;

use super::Action;
use super::format::{eta_seconds, format_eta, format_size, format_speed};

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

pub fn progress_dialog(
    ctx: &egui::Context,
    download: &Download,
    segments: &[Segment],
    speed: Option<u64>,
    show_details: &mut bool,
) -> (bool, Option<Action>) {
    let mut keep_open = true;
    let mut action = None;
    let fraction = match (download.size, download.status) {
        (_, DownloadStatus::Done) => 1.0,
        (Some(size), _) if size > 0 => (download.downloaded as f32 / size as f32).clamp(0.0, 1.0),
        _ => 0.0,
    };
    let title = format!("{:.0}% — {}", fraction * 100.0, download.file_name);

    egui::Window::new(title)
        .id(egui::Id::unique("lajufile_progress_dialog"))
        .collapsible(false)
        .resizable(false)
        .default_width(580.0)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .show(ctx, |ui| {
            ui.set_width(540.0);
            ui.label(
                RichText::new(&download.url)
                    .size(11.0)
                    .color(ui.visuals().weak_text_color()),
            );
            ui.add_space(6.0);

            let status_text = match download.status {
                DownloadStatus::Downloading => "Receiving data...",
                DownloadStatus::Queued => "Queued",
                DownloadStatus::Paused => "Paused",
                DownloadStatus::Done => "Completed",
                DownloadStatus::Error => download.error.as_deref().unwrap_or("Error"),
            };
            let eta = eta_seconds(download.size, download.downloaded, speed.unwrap_or(0));

            egui::Grid::new("lajufile_progress_meta")
                .num_columns(2)
                .spacing([24.0, 4.0])
                .show(ui, |ui| {
                    ui.label(RichText::new("Status:").strong());
                    ui.label(status_text);
                    ui.end_row();

                    ui.label(RichText::new("File size:").strong());
                    ui.label(
                        download
                            .size
                            .map(format_size)
                            .unwrap_or_else(|| "(unknown)".to_string()),
                    );
                    ui.end_row();

                    ui.label(RichText::new("Downloaded:").strong());
                    ui.label(format!(
                        "{} ({:.0}%)",
                        format_size(download.downloaded),
                        fraction * 100.0
                    ));
                    ui.end_row();

                    ui.label(RichText::new("Transfer rate:").strong());
                    ui.label(format_speed(speed));
                    ui.end_row();

                    ui.label(RichText::new("Time left:").strong());
                    ui.label(format_eta(eta));
                    ui.end_row();

                    ui.label(RichText::new("Resume capability:").strong());
                    ui.label(if download.ranged { "Yes" } else { "No" });
                    ui.end_row();
                });

            ui.add_space(8.0);
            ui.add(
                egui::ProgressBar::new(fraction)
                    .desired_width(ui.available_width())
                    .show_percentage(),
            );

            if *show_details && !segments.is_empty() {
                ui.add_space(8.0);
                ui.separator();
                ui.label(
                    RichText::new("Start positions and download progress by connections")
                        .size(11.0)
                        .strong(),
                );
                ui.add_space(4.0);
                egui::ScrollArea::vertical()
                    .max_height(160.0)
                    .show(ui, |ui| {
                        egui::Grid::new("lajufile_segment_grid")
                            .num_columns(4)
                            .striped(true)
                            .spacing([12.0, 4.0])
                            .show(ui, |ui| {
                                ui.label(RichText::new("N.").size(11.0).strong());
                                ui.label(RichText::new("Downloaded").size(11.0).strong());
                                ui.label(RichText::new("Progress").size(11.0).strong());
                                ui.label(RichText::new("Info").size(11.0).strong());
                                ui.end_row();

                                for segment in segments {
                                    let span =
                                        segment.end.saturating_sub(segment.start).saturating_add(1);
                                    let seg_frac = if download.status == DownloadStatus::Done {
                                        1.0
                                    } else if span > 0 {
                                        (segment.done as f32 / span as f32).clamp(0.0, 1.0)
                                    } else {
                                        0.0
                                    };
                                    let info = if download.status == DownloadStatus::Done
                                        || (span > 0 && segment.done >= span)
                                    {
                                        "Completed"
                                    } else if download.status == DownloadStatus::Downloading {
                                        "Receiving data..."
                                    } else if download.status == DownloadStatus::Paused {
                                        "Paused"
                                    } else {
                                        "Waiting"
                                    };
                                    ui.label(format!("{}", segment.idx + 1));
                                    ui.label(format_size(segment.done));
                                    ui.add(
                                        egui::ProgressBar::new(seg_frac)
                                            .desired_width(180.0)
                                            .text(format!("{:.0}%", seg_frac * 100.0)),
                                    );
                                    ui.label(info);
                                    ui.end_row();
                                }
                            });
                    });
            }

            ui.add_space(10.0);
            ui.separator();
            ui.horizontal(|ui| {
                let toggle_label = if *show_details {
                    "<< Hide details"
                } else {
                    "Show details >>"
                };
                if ui.button(toggle_label).clicked() {
                    *show_details = !*show_details;
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button("Close").clicked() {
                        keep_open = false;
                    }
                    match download.status {
                        DownloadStatus::Downloading | DownloadStatus::Queued => {
                            if ui.button("Pause").clicked() {
                                action = Some(Action::Pause(download.id));
                            }
                        }
                        DownloadStatus::Paused | DownloadStatus::Error => {
                            if ui.button("Start").clicked() {
                                action = Some(Action::Resume(download.id));
                            }
                        }
                        DownloadStatus::Done => {
                            if ui.button("Open").clicked() {
                                action = Some(Action::Open(download.id));
                                keep_open = false;
                            }
                        }
                    }
                });
            });
        });

    (keep_open, action)
}

pub fn complete_dialog(ctx: &egui::Context, download: &Download) -> (bool, Option<Action>) {
    let mut keep_open = true;
    let mut action = None;
    egui::Window::new("Download complete")
        .id(egui::Id::unique("lajufile_complete_dialog"))
        .collapsible(false)
        .resizable(false)
        .default_width(520.0)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .show(ctx, |ui| {
            ui.set_width(480.0);
            ui.label(RichText::new(&download.file_name).size(14.0).strong());
            if let Some(size) = download.size {
                ui.label(
                    RichText::new(format!("Size: {}", format_size(size)))
                        .size(11.0)
                        .color(ui.visuals().weak_text_color()),
                );
            }
            ui.add_space(6.0);
            ui.label(RichText::new("Saved as").strong());
            let mut saved = download.save_path.display().to_string();
            ui.add_sized(
                [ui.available_width(), 26.0],
                egui::TextEdit::singleline(&mut saved).interactive(false),
            );

            ui.add_space(10.0);
            ui.separator();
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.button("Close").clicked() {
                    keep_open = false;
                }
                if ui.button("Open folder").clicked() {
                    action = Some(Action::OpenFolder(download.id));
                    keep_open = false;
                }
                if ui.button("Open").clicked() {
                    action = Some(Action::Open(download.id));
                    keep_open = false;
                }
            });
        });
    (keep_open, action)
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
