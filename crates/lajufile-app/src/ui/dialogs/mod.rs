//! Add-download and Settings dialogs.

mod batch_update;
mod browser_lang;
mod settings;
mod stats_backup;
mod transfers;

pub use batch_update::{BatchDraft, BatchMode, UpdateUiState, batch_dialog, update_dialog};
pub use browser_lang::{browser_setup_dialog, language_dialog};
pub use settings::settings_dialog;
pub use stats_backup::{DownloadStats, stats_backup_dialog};
pub use transfers::{
    PropertiesDraft, clipboard_download_url, clipboard_download_url_with_rules, complete_dialog,
    delete_confirm_dialog, progress_dialog, properties_dialog,
};

use std::path::PathBuf;

use egui::{
    Align, Align2, Color32, FontId, Layout, Margin, Rect, RichText, Sense, Stroke, Ui, Vec2,
};
use lajufile_core::settings::Settings;
use lajufile_core::video::{
    DEFAULT_SELECTOR, MP3_SELECTOR, QualityChoice, SUBTITLES_ONLY_SELECTOR, VideoMode,
    compose_video_spec, is_valid_clip_range, parse_video_spec,
};

use crate::engine::AddRequest;
use crate::engine::huggingface::HfModelFile;
use crate::engine::smart_sources::{SmartSource, detect_source};

use super::Action;
use super::format::format_size;

#[derive(Debug, Clone)]
pub struct AddDraft {
    pub url: String,
    pub file_name: String,
    pub folder: String,
    pub start_now: bool,
    pub connections: u32,
    /// yt-dlp selector picked in the quality combo; `None` means best available.
    pub quality: Option<String>,
    /// Pre-estimated byte size for the selected quality choice.
    pub quality_size: Option<u64>,
    pub video_clip: String,
    pub formats: Vec<QualityChoice>,
    pub hf_files: Vec<HfModelFile>,
    pub hf_selected: Option<usize>,
    pub expected_sha256: String,
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
            quality_size: None,
            video_clip: String::new(),
            formats: Vec::new(),
            hf_files: Vec::new(),
            hf_selected: None,
            expected_sha256: String::new(),
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
        let chosen_hf = self
            .hf_selected
            .and_then(|idx| self.hf_files.get(idx))
            .cloned();
        let target_url = chosen_hf
            .as_ref()
            .map(|f| f.download_url.clone())
            .unwrap_or_else(|| self.url.trim().to_string());
        let is_video = chosen_hf.is_none() && is_video_site_url(&target_url);
        let clip = non_empty(self.video_clip);
        let video_format = is_video.then(|| {
            let base = self
                .quality
                .clone()
                .unwrap_or_else(|| DEFAULT_SELECTOR.to_string());
            compose_video_spec(&base, clip.as_deref())
        });
        let mode = video_format.as_deref().map(|s| parse_video_spec(s).mode);
        let mut file_name =
            non_empty(self.file_name).or_else(|| chosen_hf.as_ref().map(|f| f.file_name.clone()));
        if let (Some(mode), Some(name)) = (mode, file_name.as_mut()) {
            let lower = name.to_ascii_lowercase();
            if lower.ends_with(".mp4") {
                let stem = &name[..name.len() - 4];
                match mode {
                    VideoMode::AudioM4a => *name = format!("{stem}.m4a"),
                    VideoMode::AudioMp3 => *name = format!("{stem}.mp3"),
                    VideoMode::SubtitlesOnly => *name = format!("{stem}.srt"),
                    VideoMode::Video => {}
                }
            }
        }
        let video_size = is_video
            .then(|| {
                self.quality_size
                    .or_else(|| self.formats.first().and_then(|c| c.size))
            })
            .flatten();
        let expected_sha256 = {
            let norm = lajufile_core::checksum::normalize_checksum_input(&self.expected_sha256);
            if !norm.is_empty() {
                Some(norm)
            } else {
                chosen_hf.and_then(|f| f.sha256)
            }
        };
        AddRequest {
            url: target_url,
            file_name,
            save_dir: non_empty(self.folder).map(PathBuf::from),
            start_now: self.start_now,
            connections: Some(self.connections.clamp(1, 32)),
            referrer: None,
            user_agent: None,
            cookie: None,
            video_format,
            video_size,
            expected_sha256,
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

/// True for video/audio platforms handled by yt-dlp (YouTube, Vimeo, Twitch, TikTok, X, etc.).
pub fn is_video_site_url(url: &str) -> bool {
    if youtube_url(url) {
        return true;
    }
    let Ok(parsed) = url::Url::parse(url.trim()) else {
        return false;
    };
    let Some(host) = parsed.host_str().map(str::to_ascii_lowercase) else {
        return false;
    };
    let path = parsed.path();
    if path.len() <= 1 {
        return false;
    }
    const VIDEO_DOMAINS: &[&str] = &[
        "vimeo.com",
        "twitch.tv",
        "tiktok.com",
        "x.com",
        "twitter.com",
        "dailymotion.com",
        "soundcloud.com",
        "bilibili.com",
        "reddit.com",
        "instagram.com",
        "facebook.com",
    ];
    VIDEO_DOMAINS
        .iter()
        .any(|d| host == *d || host.ends_with(&format!(".{d}")))
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
            if url.changed() {
                let had_hf = !draft.hf_files.is_empty();
                draft.formats.clear();
                draft.quality = None;
                draft.quality_size = None;
                draft.video_clip.clear();
                draft.hf_files.clear();
                draft.hf_selected = None;
                if had_hf {
                    draft.expected_sha256.clear();
                }
            }
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
                    egui::TextEdit::singleline(&mut draft.folder).hint_text(if cfg!(windows) {
                        "Default: Downloads\\LajuFile"
                    } else {
                        "Default: Downloads/LajuFile"
                    }),
                );
                if ui.button("Browse...").clicked() {
                    action = Some(Action::OpenFolderPicker);
                }
            });

            ui.add_space(8.0);
            ui.label(RichText::new("Expected SHA-256 (optional)").strong());
            ui.add_sized(
                [ui.available_width(), 28.0],
                egui::TextEdit::singleline(&mut draft.expected_sha256)
                    .hint_text("Auto-verify file integrity after download (64 hex chars)"),
            );
            let sha_norm =
                lajufile_core::checksum::normalize_checksum_input(&draft.expected_sha256);
            let sha_invalid = !sha_norm.is_empty()
                && (sha_norm.len() != 64 || !sha_norm.bytes().all(|b| b.is_ascii_hexdigit()));
            if sha_invalid {
                ui.colored_label(
                    ui.visuals().error_fg_color,
                    "Expected SHA-256 must be 64 hexadecimal characters.",
                );
            }

            ui.add_space(12.0);
            ui.add(egui::Slider::new(&mut draft.connections, 1..=32).text("Connections"));
            ui.label(
                RichText::new("Parallel ranges are used when the server supports them.")
                    .size(11.0)
                    .color(ui.visuals().weak_text_color()),
            );

            if let Some(source) = detect_source(&draft.url) {
                let source_label = match source {
                    SmartSource::HuggingFace { .. } => "HuggingFace model file",
                    SmartSource::GitHubRelease { .. } => "GitHub Release asset",
                    SmartSource::Civitai { .. } => "Civitai model file",
                };
                ui.add_space(8.0);
                ui.label(RichText::new(source_label).strong());
                if draft.hf_files.is_empty() {
                    if ui.button("Load files").clicked() {
                        action = Some(Action::LoadHfFiles(draft.url.trim().to_string()));
                    }
                } else {
                    let current_idx = draft.hf_selected.unwrap_or(0).min(draft.hf_files.len() - 1);
                    let mut selected = current_idx;
                    let fmt_item = |f: &HfModelFile| {
                        let sha_tag = if f.sha256.is_some() {
                            " · SHA-256"
                        } else {
                            ""
                        };
                        if f.size > 0 {
                            format!("{} ({}){sha_tag}", f.path, format_size(f.size))
                        } else {
                            format!("{}{sha_tag}", f.path)
                        }
                    };
                    egui::ComboBox::from_id_salt("hf_model_file")
                        .width(ui.available_width())
                        .selected_text(fmt_item(&draft.hf_files[selected]))
                        .show_ui(ui, |ui| {
                            for (index, file) in draft.hf_files.iter().enumerate() {
                                ui.selectable_value(&mut selected, index, fmt_item(file));
                            }
                        });
                    if draft.hf_selected != Some(selected) {
                        draft.hf_selected = Some(selected);
                        let chosen = &draft.hf_files[selected];
                        draft.file_name = chosen.file_name.clone();
                        draft.expected_sha256 = chosen.sha256.clone().unwrap_or_default();
                    }
                }
            }

            let is_video_url = is_video_site_url(&draft.url);
            let mut clip_invalid = false;
            if is_video_url {
                ui.add_space(8.0);
                ui.label(RichText::new("Video quality / format").strong());
                let best_hint = draft
                    .formats
                    .first()
                    .and_then(|c| c.size)
                    .map(|sz| format!("Best available (~{})", format_size(sz)))
                    .unwrap_or_else(|| "Best available".to_string());
                let choice_text = |c: &QualityChoice| match c.size {
                    Some(sz) if sz > 0 => format!("{} (~{})", c.label, format_size(sz)),
                    _ => c.label.clone(),
                };
                let mut options: Vec<(Option<String>, Option<u64>, String)> = Vec::new();
                options.push((
                    None,
                    draft.formats.first().and_then(|c| c.size),
                    best_hint.clone(),
                ));
                for c in &draft.formats {
                    options.push((Some(c.selector.clone()), c.size, choice_text(c)));
                }
                options.push((
                    Some(MP3_SELECTOR.to_string()),
                    None,
                    "Audio only (MP3)".to_string(),
                ));
                options.push((
                    Some(SUBTITLES_ONLY_SELECTOR.to_string()),
                    None,
                    "Subtitles only (.srt)".to_string(),
                ));

                let chosen = options
                    .iter()
                    .position(|(sel, _, _)| *sel == draft.quality)
                    .unwrap_or(0);
                let mut selected = chosen;
                ui.horizontal(|ui| {
                    egui::ComboBox::from_id_salt("quality")
                        .selected_text(&options[selected].2)
                        .show_ui(ui, |ui| {
                            for (index, (_, _, label)) in options.iter().enumerate() {
                                ui.selectable_value(&mut selected, index, label);
                            }
                        });
                    if draft.formats.is_empty() && ui.button("Load qualities").clicked() {
                        action = Some(Action::LoadFormats(draft.url.trim().to_string()));
                    }
                });
                draft.quality = options[selected].0.clone();
                draft.quality_size = (selected > 0).then(|| options[selected].1).flatten();
                if selected != chosen {
                    let mode = draft
                        .quality
                        .as_deref()
                        .map(|s| parse_video_spec(s).mode)
                        .unwrap_or(VideoMode::Video);
                    let trimmed = draft.file_name.trim().to_string();
                    let lower = trimmed.to_ascii_lowercase();
                    let stem = [".mp4", ".m4a", ".mp3", ".srt"].iter().find_map(|ext| {
                        lower
                            .ends_with(ext)
                            .then(|| &trimmed[..trimmed.len() - ext.len()])
                    });
                    if let Some(stem) = stem {
                        draft.file_name = match mode {
                            VideoMode::AudioM4a => format!("{stem}.m4a"),
                            VideoMode::AudioMp3 => format!("{stem}.mp3"),
                            VideoMode::SubtitlesOnly => format!("{stem}.srt"),
                            VideoMode::Video => format!("{stem}.mp4"),
                        };
                    }
                }

                if draft.quality.as_deref() != Some(SUBTITLES_ONLY_SELECTOR) {
                    ui.add_space(6.0);
                    ui.label(RichText::new("Clip / trim range (optional)").strong());
                    ui.add_sized(
                        [ui.available_width(), 28.0],
                        egui::TextEdit::singleline(&mut draft.video_clip)
                            .hint_text("e.g. 00:30-01:45 or 1:02:00-1:05:30"),
                    );
                    clip_invalid = !draft.video_clip.trim().is_empty()
                        && !is_valid_clip_range(&draft.video_clip);
                    if clip_invalid {
                        ui.colored_label(
                            ui.visuals().error_fg_color,
                            "Clip range must use start-end timestamps (e.g. 00:30-01:45).",
                        );
                    }
                }
            }

            ui.add_space(12.0);
            ui.separator();
            let can_add = !draft.url.trim().is_empty() && !sha_invalid && !clip_invalid;
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

pub(crate) struct SettingsPalette {
    pub(crate) dialog_bg: Color32,
    pub(crate) nav_bg: Color32,
    pub(crate) card_bg: Color32,
    pub(crate) field_bg: Color32,
    pub(crate) border: Color32,
    pub(crate) text: Color32,
    pub(crate) muted: Color32,
    pub(crate) faint: Color32,
    pub(crate) accent: Color32,
    pub(crate) nav_selected: Color32,
    pub(crate) green: Color32,
    pub(crate) amber: Color32,
    pub(crate) red: Color32,
    pub(crate) toggle_off: Color32,
}

impl SettingsPalette {
    pub(crate) fn from_dark(dark: bool) -> Self {
        if dark {
            Self {
                dialog_bg: Color32::from_rgb(0x14, 0x1A, 0x21),
                nav_bg: Color32::from_rgb(0x11, 0x17, 0x1E),
                card_bg: Color32::from_rgb(0x16, 0x1D, 0x26),
                field_bg: Color32::from_rgb(0x0E, 0x13, 0x19),
                border: Color32::from_rgb(0x26, 0x30, 0x3B),
                text: Color32::from_rgb(0xE7, 0xED, 0xF5),
                muted: Color32::from_rgb(0x94, 0xA3, 0xB8),
                faint: Color32::from_rgb(0x64, 0x74, 0x8B),
                accent: Color32::from_rgb(0x4C, 0x9A, 0xFF),
                nav_selected: Color32::from_rgb(0x13, 0x26, 0x3D),
                green: Color32::from_rgb(0x3D, 0xDC, 0x97),
                amber: Color32::from_rgb(0xF5, 0xB4, 0x4A),
                red: Color32::from_rgb(0xFF, 0x6B, 0x6B),
                toggle_off: Color32::from_rgb(0x33, 0x41, 0x55),
            }
        } else {
            Self {
                dialog_bg: Color32::WHITE,
                nav_bg: Color32::from_rgb(0xF8, 0xFA, 0xFC),
                card_bg: Color32::WHITE,
                field_bg: Color32::from_rgb(0xF8, 0xFA, 0xFC),
                border: Color32::from_rgb(0xE1, 0xE7, 0xEF),
                text: Color32::from_rgb(0x18, 0x21, 0x2B),
                muted: Color32::from_rgb(0x66, 0x70, 0x85),
                faint: Color32::from_rgb(0x98, 0xA2, 0xB3),
                accent: Color32::from_rgb(0x0B, 0x6B, 0xCB),
                nav_selected: Color32::from_rgb(0xE8, 0xF1, 0xFE),
                green: Color32::from_rgb(0x17, 0x93, 0x6A),
                amber: Color32::from_rgb(0xD9, 0x77, 0x06),
                red: Color32::from_rgb(0xC2, 0x41, 0x3A),
                toggle_off: Color32::from_rgb(0xCB, 0xD5, 0xE1),
            }
        }
    }
}

pub(crate) fn settings_card(ui: &mut Ui, sp: &SettingsPalette, add_contents: impl FnOnce(&mut Ui)) {
    egui::Frame::new()
        .fill(sp.card_bg)
        .stroke(Stroke::new(1.0, sp.border))
        .corner_radius(10)
        .inner_margin(Margin::symmetric(16, 14))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add_contents(ui);
        });
}

pub(crate) fn card_kicker(ui: &mut Ui, text: &str, sp: &SettingsPalette) {
    ui.label(RichText::new(text).size(9.5).strong().color(sp.faint));
}

pub(crate) fn badge_pill(ui: &mut Ui, text: &str, color: Color32, sp: &SettingsPalette) {
    egui::Frame::new()
        .fill(color.gamma_multiply(0.14))
        .stroke(Stroke::new(1.0, color.gamma_multiply(0.45)))
        .corner_radius(99)
        .inner_margin(Margin::symmetric(9, 3))
        .show(ui, |ui| {
            ui.label(RichText::new(text).size(10.5).strong().color(sp.text));
        });
}

pub(crate) fn modal_header(ui: &mut Ui, title: &str, subtitle: &str, sp: &SettingsPalette) -> bool {
    let mut closed = false;
    egui::Frame::new()
        .fill(sp.dialog_bg)
        .inner_margin(Margin::symmetric(24, 14))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(RichText::new(title).size(17.0).strong().color(sp.text));
                    ui.add_space(2.0);
                    ui.label(RichText::new(subtitle).size(11.0).color(sp.muted));
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let (close_rect, close_resp) =
                        ui.allocate_exact_size(Vec2::splat(28.0), Sense::click());
                    let close_resp = close_resp.on_hover_cursor(egui::CursorIcon::PointingHand);
                    let close_fill = if close_resp.hovered() {
                        sp.border.gamma_multiply(0.6)
                    } else {
                        sp.nav_bg
                    };
                    ui.painter().rect_filled(close_rect, 8.0, close_fill);
                    ui.painter().rect_stroke(
                        close_rect,
                        8.0,
                        Stroke::new(1.0, sp.border),
                        egui::StrokeKind::Inside,
                    );
                    let c = close_rect.center();
                    ui.painter().line_segment(
                        [
                            egui::pos2(c.x - 4.0, c.y - 4.0),
                            egui::pos2(c.x + 4.0, c.y + 4.0),
                        ],
                        Stroke::new(1.5, sp.muted),
                    );
                    ui.painter().line_segment(
                        [
                            egui::pos2(c.x + 4.0, c.y - 4.0),
                            egui::pos2(c.x - 4.0, c.y + 4.0),
                        ],
                        Stroke::new(1.5, sp.muted),
                    );
                    if close_resp.clicked() {
                        closed = true;
                    }
                });
            });
        });
    closed
}

pub(super) fn setting_row(
    ui: &mut Ui,
    title: &str,
    subtitle: &str,
    sp: &SettingsPalette,
    control: impl FnOnce(&mut Ui),
) {
    ui.horizontal(|ui| {
        ui.set_width(ui.available_width());
        ui.vertical(|ui| {
            ui.label(RichText::new(title).size(12.0).color(sp.text));
            ui.label(RichText::new(subtitle).size(10.0).color(sp.faint));
        });
        ui.with_layout(Layout::right_to_left(Align::Center), control);
    });
}

pub(crate) fn toggle_row_interactive(
    ui: &mut Ui,
    title: &str,
    id_salt: &str,
    value: &mut bool,
    sp: &SettingsPalette,
) -> bool {
    let before = *value;
    ui.horizontal(|ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new(title).size(12.0).color(sp.text));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            toggle_switch(ui, id_salt, value, sp);
        });
    });
    *value != before
}

pub(crate) fn toggle_row(
    ui: &mut Ui,
    title: &str,
    id_salt: &str,
    value: &mut bool,
    sp: &SettingsPalette,
) {
    let _ = toggle_row_interactive(ui, title, id_salt, value, sp);
}

fn toggle_switch(ui: &mut Ui, id_salt: &str, on: &mut bool, sp: &SettingsPalette) {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(36.0, 20.0), Sense::click());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    if response.clicked() {
        *on = !*on;
    }
    let t = ui.ctx().animate_bool(ui.make_persistent_id(id_salt), *on);
    let track = lerp_color(sp.toggle_off, sp.accent, t);
    ui.painter().rect_filled(rect, 10.0, track);
    let thumb_x = rect.left() + 10.0 + t * 16.0;
    ui.painter()
        .circle_filled(egui::pos2(thumb_x, rect.center().y), 7.5, Color32::WHITE);
}

pub(super) fn stepper_widget(
    ui: &mut Ui,
    value: &mut u32,
    min: u32,
    max: u32,
    sp: &SettingsPalette,
) {
    ui.horizontal(|ui| {
        let can_dec = *value > min;
        if stepper_btn(ui, false, can_dec, sp) {
            *value = value.saturating_sub(1).max(min);
        }
        ui.add_space(4.0);
        let (val_rect, _) = ui.allocate_exact_size(Vec2::new(36.0, 30.0), Sense::hover());
        ui.painter().rect_filled(val_rect, 7.0, sp.field_bg);
        ui.painter().rect_stroke(
            val_rect,
            7.0,
            Stroke::new(1.0, sp.border),
            egui::StrokeKind::Inside,
        );
        ui.painter().text(
            val_rect.center(),
            Align2::CENTER_CENTER,
            value.to_string(),
            FontId::proportional(12.0),
            sp.text,
        );
        ui.add_space(4.0);
        let can_inc = *value < max;
        if stepper_btn(ui, true, can_inc, sp) {
            *value = (*value + 1).min(max);
        }
    });
}

fn stepper_btn(ui: &mut Ui, plus: bool, enabled: bool, sp: &SettingsPalette) -> bool {
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, mut response) = ui.allocate_exact_size(Vec2::splat(30.0), sense);
    if enabled {
        response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    }
    let fill = if enabled && response.hovered() {
        sp.field_bg
    } else {
        sp.card_bg
    };
    let painter = ui.painter();
    painter.rect_filled(rect, 7.0, fill);
    painter.rect_stroke(
        rect,
        7.0,
        Stroke::new(1.0, sp.border),
        egui::StrokeKind::Inside,
    );
    let icon_color = if enabled {
        sp.muted
    } else {
        sp.faint.gamma_multiply(0.5)
    };
    let c = rect.center();
    painter.line_segment(
        [egui::pos2(c.x - 5.0, c.y), egui::pos2(c.x + 5.0, c.y)],
        Stroke::new(1.5, icon_color),
    );
    if plus {
        painter.line_segment(
            [egui::pos2(c.x, c.y - 5.0), egui::pos2(c.x, c.y + 5.0)],
            Stroke::new(1.5, icon_color),
        );
    }
    enabled && response.clicked()
}

pub(super) fn segmented_options<T: Clone + PartialEq>(
    ui: &mut Ui,
    current: &mut T,
    id_prefix: &str,
    options: &[(T, &str, f32)],
    sp: &SettingsPalette,
) {
    let total_w: f32 = options.iter().map(|(_, _, w)| *w).sum::<f32>()
        + 6.0
        + 2.0 * options.len().saturating_sub(1) as f32;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(total_w, 26.0), Sense::hover());
    ui.painter().rect_filled(rect, 8.0, sp.field_bg);
    ui.painter().rect_stroke(
        rect,
        8.0,
        Stroke::new(1.0, sp.border),
        egui::StrokeKind::Inside,
    );
    let mut x = rect.left() + 3.0;
    for (candidate, label, width) in options {
        let seg_rect =
            Rect::from_min_size(egui::pos2(x, rect.top() + 3.0), Vec2::new(*width, 20.0));
        x += *width + 2.0;
        let selected = current == candidate;
        let resp = ui
            .interact(
                seg_rect,
                ui.make_persistent_id((id_prefix, *label)),
                Sense::click(),
            )
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        if selected {
            ui.painter().rect_filled(seg_rect, 6.0, sp.card_bg);
            ui.painter().rect_stroke(
                seg_rect,
                6.0,
                Stroke::new(1.0, sp.border),
                egui::StrokeKind::Inside,
            );
        }
        ui.painter().text(
            seg_rect.center(),
            Align2::CENTER_CENTER,
            *label,
            FontId::proportional(10.5),
            if selected { sp.text } else { sp.muted },
        );
        if resp.clicked() {
            *current = candidate.clone();
        }
    }
}

pub(super) fn paint_folder_icon(painter: &egui::Painter, rect: Rect, color: Color32) {
    painter.rect_filled(
        Rect::from_min_size(rect.min, Vec2::new(rect.width() * 0.45, 2.5)),
        1.0,
        color,
    );
    painter.rect_filled(
        Rect::from_min_max(egui::pos2(rect.left(), rect.top() + 2.0), rect.max),
        1.5,
        color,
    );
}

pub(crate) fn figma_button(
    ui: &mut Ui,
    label: &str,
    width: f32,
    height: f32,
    primary: bool,
    sp: &SettingsPalette,
) -> bool {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, height), Sense::click());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    let fill = if primary {
        if response.hovered() {
            lerp_color(sp.accent, Color32::WHITE, 0.14)
        } else {
            sp.accent
        }
    } else if response.hovered() {
        sp.field_bg
    } else {
        sp.card_bg
    };
    ui.painter().rect_filled(rect, 8.0, fill);
    if !primary {
        ui.painter().rect_stroke(
            rect,
            8.0,
            Stroke::new(1.0, sp.border),
            egui::StrokeKind::Inside,
        );
    }
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        label,
        FontId::proportional(11.5),
        if primary { Color32::WHITE } else { sp.text },
    );
    response.clicked()
}

pub(crate) fn draw_h_divider(ui: &mut Ui, color: Color32) {
    let rect = ui
        .allocate_exact_size(Vec2::new(ui.available_width(), 1.0), Sense::hover())
        .0;
    ui.painter().rect_filled(rect, 0.0, color);
}

pub(crate) fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    Color32::from_rgba_unmultiplied(
        (a.r() as f32 + (b.r() as f32 - a.r() as f32) * t).round() as u8,
        (a.g() as f32 + (b.g() as f32 - a.g() as f32) * t).round() as u8,
        (a.b() as f32 + (b.b() as f32 - a.b() as f32) * t).round() as u8,
        (a.a() as f32 + (b.a() as f32 - a.a() as f32) * t).round() as u8,
    )
}

pub(super) fn non_empty(value: String) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_string())
}

pub(super) fn mask_token(token: &str) -> String {
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
    use batch_update::{MAX_BATCH_IMPORT_BYTES, load_batch_import_file};
    use transfers::format_utc_timestamp;

    #[test]
    fn clipboard_detects_downloadable_urls_and_respects_rules() {
        assert_eq!(
            clipboard_download_url(" https://example.com/files/archive.zip "),
            Some("https://example.com/files/archive.zip".to_string())
        );
        assert_eq!(
            clipboard_download_url("https://www.youtube.com/watch?v=dQw4w9WgXcQ"),
            Some("https://www.youtube.com/watch?v=dQw4w9WgXcQ".to_string())
        );
        assert_eq!(clipboard_download_url("https://example.com/about"), None);
        assert_eq!(
            clipboard_download_url("http://127.0.0.1:53841/file.zip"),
            None
        );
        assert_eq!(
            clipboard_download_url("https://example.com/a.zip\nhttps://example.com/b.zip"),
            None
        );

        let custom = Settings {
            excluded_domains: "ads.example.com, *.blocked.net".to_string(),
            captured_extensions: "zip, custompkg".to_string(),
            ..Settings::default()
        };
        assert_eq!(
            clipboard_download_url_with_rules("https://ads.example.com/file.zip", &custom),
            None
        );
        assert_eq!(
            clipboard_download_url_with_rules("https://cdn.blocked.net/tool.zip", &custom),
            None
        );
        assert_eq!(
            clipboard_download_url_with_rules("https://files.example.org/app.custompkg", &custom),
            Some("https://files.example.org/app.custompkg".to_string())
        );
        assert_eq!(
            clipboard_download_url_with_rules("https://files.example.org/video.mp4", &custom),
            None
        );
    }

    #[test]
    fn batch_draft_expands_list_markdown_and_wildcard_pattern() {
        let list = BatchDraft {
            mode: BatchMode::UrlList,
            urls_text: "# comment\n[a](https://example.com/a.bin) and https://example.com/b.bin\nnot-a-url\nURL=https://example.com/a.bin"
                .to_string(),
            ..BatchDraft::default()
        };
        assert_eq!(
            list.expand_urls(),
            vec![
                "https://example.com/a.bin".to_string(),
                "https://example.com/b.bin".to_string()
            ]
        );

        let pattern = BatchDraft {
            mode: BatchMode::Pattern,
            pattern_url: "https://example.com/part*.rar".to_string(),
            from: 1,
            to: 3,
            pad_width: 2,
            ..BatchDraft::default()
        };
        assert_eq!(
            pattern.expand_urls(),
            vec![
                "https://example.com/part01.rar".to_string(),
                "https://example.com/part02.rar".to_string(),
                "https://example.com/part03.rar".to_string()
            ]
        );
    }

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

    #[test]
    fn formats_utc_timestamp() {
        assert_eq!(format_utc_timestamp(0), "1970-01-01 00:00:00 UTC");
        assert_eq!(
            format_utc_timestamp(1_700_000_000_000),
            "2023-11-14 22:13:20 UTC"
        );
    }

    #[test]
    fn batch_import_rejects_oversized_or_missing_files() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load_batch_import_file(&dir.path().join("missing.txt")).is_err());

        let small = dir.path().join("urls.txt");
        std::fs::write(&small, "https://example.com/a.zip\n").unwrap();
        assert_eq!(
            load_batch_import_file(&small).unwrap(),
            "https://example.com/a.zip\n"
        );

        let large = dir.path().join("huge.txt");
        let file = std::fs::File::create(&large).unwrap();
        file.set_len(MAX_BATCH_IMPORT_BYTES + 1).unwrap();
        assert!(load_batch_import_file(&large).is_err());
    }
}
