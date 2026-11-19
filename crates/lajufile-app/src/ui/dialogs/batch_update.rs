use std::path::PathBuf;

use egui::{Align, Align2, Layout, Margin, RichText, Stroke, Vec2};
use lajufile_core::settings::Settings;
use lajufile_core::video::DEFAULT_SELECTOR;

use crate::engine::AddRequest;

use super::super::format::format_size;
use super::super::{Action, extract_urls_from_text};
use super::{SettingsPalette, figma_button, non_empty, settings_card, youtube_url};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BatchMode {
    UrlList,
    Pattern,
}

#[derive(Debug, Clone)]
pub struct BatchDraft {
    pub mode: BatchMode,
    pub urls_text: String,
    pub page_url: String,
    pub import_path: String,
    pub pattern_url: String,
    pub from: u32,
    pub to: u32,
    pub pad_width: u32,
    pub folder: String,
    pub connections: u32,
    pub import_error: Option<String>,
}

impl Default for BatchDraft {
    fn default() -> Self {
        Self {
            mode: BatchMode::UrlList,
            urls_text: String::new(),
            page_url: String::new(),
            import_path: String::new(),
            pattern_url: "https://example.com/file_*.zip".to_string(),
            from: 1,
            to: 10,
            pad_width: 2,
            folder: String::new(),
            connections: 8,
            import_error: None,
        }
    }
}

impl BatchDraft {
    pub fn for_settings(settings: &Settings) -> Self {
        Self {
            connections: settings.max_connections.clamp(1, 32),
            ..Self::default()
        }
    }

    pub fn expand_urls(&self) -> Vec<String> {
        const MAX_BATCH: usize = 500;
        let mut out = Vec::new();
        match self.mode {
            BatchMode::UrlList => {
                out.extend(
                    extract_urls_from_text(&self.urls_text)
                        .into_iter()
                        .take(MAX_BATCH),
                );
            }
            BatchMode::Pattern => {
                let pattern = self.pattern_url.trim();
                if !pattern.contains('*') {
                    return out;
                }
                let (start, end) = if self.from <= self.to {
                    (self.from, self.to)
                } else {
                    (self.to, self.from)
                };
                let width = self.pad_width.clamp(1, 6) as usize;
                for index in start..=end {
                    let num = format!("{index:0width$}");
                    let replaced = pattern.replacen('*', &num, 1);
                    if let Ok(parsed) = url::Url::parse(&replaced)
                        && matches!(parsed.scheme(), "http" | "https")
                    {
                        out.push(parsed.to_string());
                        if out.len() >= MAX_BATCH {
                            break;
                        }
                    }
                }
            }
        }
        out
    }

    pub fn into_requests(&self, start_now: bool) -> Vec<AddRequest> {
        let save_dir = non_empty(self.folder.clone()).map(PathBuf::from);
        let connections = Some(self.connections.clamp(1, 32));
        self.expand_urls()
            .into_iter()
            .map(|url| {
                let video_format = youtube_url(&url).then(|| DEFAULT_SELECTOR.to_string());
                AddRequest {
                    url,
                    file_name: None,
                    save_dir: save_dir.clone(),
                    start_now,
                    connections,
                    referrer: None,
                    user_agent: None,
                    cookie: None,
                    video_format,
                    video_size: None,
                }
            })
            .collect()
    }
}

pub fn batch_dialog(
    ctx: &egui::Context,
    open: &mut bool,
    draft: &mut BatchDraft,
    grabbing_page: bool,
) -> Option<Action> {
    let mut action = None;
    egui::Window::new("Add batch download")
        .id(egui::Id::unique("lajufile_batch_dialog"))
        .collapsible(false)
        .resizable(false)
        .default_width(620.0)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .show(ctx, |ui| {
            ui.set_width(580.0);
            ui.horizontal(|ui| {
                ui.selectable_value(
                    &mut draft.mode,
                    BatchMode::UrlList,
                    "URL List / Smart Grabber",
                );
                ui.selectable_value(
                    &mut draft.mode,
                    BatchMode::Pattern,
                    "Sequential Pattern (*)",
                );
            });
            ui.add_space(6.0);

            match draft.mode {
                BatchMode::UrlList => {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new("URLs or mixed text / Markdown (auto-extracted)").strong(),
                        );
                        if ui.small_button("Grab clipboard").clicked()
                            && let Ok(mut cb) = arboard::Clipboard::new()
                            && let Ok(text) = cb.get_text()
                        {
                            let extracted = extract_urls_from_text(&text);
                            let addition = if extracted.is_empty() {
                                text
                            } else {
                                extracted.join("\n")
                            };
                            if draft.urls_text.trim().is_empty() {
                                draft.urls_text = addition;
                            } else {
                                draft.urls_text.push('\n');
                                draft.urls_text.push_str(&addition);
                            }
                        }
                        if ui.small_button("Clear").clicked() {
                            draft.urls_text.clear();
                        }
                    });
                    ui.add_sized(
                        [ui.available_width(), 120.0],
                        egui::TextEdit::multiline(&mut draft.urls_text).hint_text(
                            "Paste URLs, Markdown links, or notes:\n[installer](https://example.com/setup.exe)",
                        ),
                    );
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.label("From web page:");
                        ui.add_sized(
                            [ui.available_width() - 104.0, 26.0],
                            egui::TextEdit::singleline(&mut draft.page_url)
                                .hint_text("https://example.com/releases"),
                        );
                        let label = if grabbing_page {
                            "Fetching..."
                        } else {
                            "Fetch links"
                        };
                        let can_fetch = !grabbing_page && !draft.page_url.trim().is_empty();
                        if ui
                            .add_enabled(can_fetch, egui::Button::new(label))
                            .clicked()
                        {
                            action = Some(Action::GrabPageLinks(draft.page_url.trim().to_string()));
                        }
                    });
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.label("Import .txt:");
                        ui.add_sized(
                            [ui.available_width() - 90.0, 26.0],
                            egui::TextEdit::singleline(&mut draft.import_path)
                                .hint_text("C:\\path\\to\\urls.txt"),
                        );
                        if ui.button("Load file").clicked() {
                            let path_str = draft.import_path.trim();
                            if path_str.is_empty() {
                                draft.import_error =
                                    Some("Enter a .txt file path to import".to_string());
                            } else {
                                match load_batch_import_file(std::path::Path::new(path_str)) {
                                    Ok(contents) => {
                                        draft.import_error = None;
                                        if draft.urls_text.trim().is_empty() {
                                            draft.urls_text = contents;
                                        } else {
                                            draft.urls_text.push('\n');
                                            draft.urls_text.push_str(&contents);
                                        }
                                    }
                                    Err(err) => {
                                        draft.import_error = Some(err);
                                    }
                                }
                            }
                        }
                    });
                    if let Some(err) = &draft.import_error {
                        ui.colored_label(ui.visuals().error_fg_color, err);
                    }
                }
                BatchMode::Pattern => {
                    ui.label(
                        RichText::new("Address with wildcard (*) — replaced with numbers").strong(),
                    );
                    ui.add_sized(
                        [ui.available_width(), 28.0],
                        egui::TextEdit::singleline(&mut draft.pattern_url)
                            .hint_text("https://example.com/part*.zip"),
                    );
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.label("From:");
                        ui.add(egui::DragValue::new(&mut draft.from).range(0..=99_999));
                        ui.add_space(12.0);
                        ui.label("To:");
                        ui.add(egui::DragValue::new(&mut draft.to).range(0..=99_999));
                        ui.add_space(12.0);
                        ui.label("Wildcard width (zero-pad):");
                        ui.add(egui::DragValue::new(&mut draft.pad_width).range(1..=6));
                    });
                }
            }

            let urls = draft.expand_urls();
            ui.add_space(6.0);
            let preview = match (urls.first(), urls.last()) {
                (Some(first), Some(last)) if urls.len() > 1 => {
                    format!("{} links ready: {first} … {last}", urls.len())
                }
                (Some(only), _) => format!("1 link ready: {only}"),
                _ => "No valid HTTP/HTTPS links yet.".to_string(),
            };
            ui.label(
                RichText::new(preview)
                    .size(11.0)
                    .color(ui.visuals().weak_text_color()),
            );

            ui.add_space(8.0);
            ui.label(RichText::new("Save to folder").strong());
            ui.horizontal(|ui| {
                ui.add_sized(
                    [ui.available_width() - 96.0, 28.0],
                    egui::TextEdit::singleline(&mut draft.folder)
                        .hint_text("Default: Downloads\\LajuFile"),
                );
                if ui.button("Browse...").clicked() {
                    action = Some(Action::OpenFolderPicker);
                }
            });
            ui.add_space(6.0);
            ui.add(egui::Slider::new(&mut draft.connections, 1..=32).text("Connections"));

            ui.add_space(10.0);
            ui.separator();
            let can_add = !urls.is_empty();
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui
                    .add_enabled(can_add, egui::Button::new("Download all now"))
                    .clicked()
                {
                    action = Some(Action::AddBatch(draft.into_requests(true)));
                    *open = false;
                }
                if ui
                    .add_enabled(can_add, egui::Button::new("Add all to queue"))
                    .clicked()
                {
                    action = Some(Action::AddBatch(draft.into_requests(false)));
                    *open = false;
                }
                if ui.button("Cancel").clicked() {
                    *open = false;
                }
            });
        });
    action
}

#[derive(Debug, Clone, Default)]
pub enum UpdateUiState {
    #[default]
    Idle,
    Checking,
    UpToDate(crate::updater::UpdateCheck),
    Available(crate::updater::UpdateInfo),
    Downloading {
        info: crate::updater::UpdateInfo,
        downloaded: u64,
        total: Option<u64>,
    },
    Failed(String),
}

pub fn update_dialog(
    ctx: &egui::Context,
    open: &mut bool,
    state: &UpdateUiState,
    dark: bool,
) -> Option<Action> {
    let mut action = None;
    let sp = SettingsPalette::from_dark(dark);
    let style = ctx.style_of(if dark {
        egui::Theme::Dark
    } else {
        egui::Theme::Light
    });
    let frame = egui::Frame::window(&style)
        .fill(sp.dialog_bg)
        .stroke(Stroke::new(1.0, sp.border))
        .corner_radius(14)
        .inner_margin(Margin::same(18));

    egui::Window::new(crate::i18n::t_ctx(ctx, "Software Update").as_ref())
        .id(egui::Id::unique("lajufile_update_dialog"))
        .collapsible(false)
        .movable(true)
        .resizable(false)
        .default_width(480.0)
        .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
        .frame(frame)
        .show(ctx, |ui| {
            ui.set_width(450.0);
            ui.label(
                RichText::new(concat!("LajuFile v", env!("CARGO_PKG_VERSION")))
                    .size(16.0)
                    .strong()
                    .color(sp.text),
            );
            ui.add_space(8.0);

            match state {
                UpdateUiState::Idle | UpdateUiState::Checking => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.add_space(6.0);
                        ui.label(
                            RichText::new("Checking GitHub Releases for updates...")
                                .size(12.5)
                                .color(sp.muted),
                        );
                    });
                }
                UpdateUiState::UpToDate(check) => {
                    ui.label(
                        RichText::new(format!(
                            "You are using the latest version ({}).",
                            check.latest.tag
                        ))
                        .size(13.0)
                        .strong()
                        .color(sp.green),
                    );
                }
                UpdateUiState::Available(info) => {
                    ui.label(
                        RichText::new(format!("New version available: {}", info.tag))
                            .size(14.0)
                            .strong()
                            .color(sp.accent),
                    );
                    if let Some(size) = info.asset_size {
                        ui.label(
                            RichText::new(format!("Installer size: {}", format_size(size)))
                                .size(11.5)
                                .color(sp.muted),
                        );
                    }
                    if !info.notes.trim().is_empty() {
                        ui.add_space(8.0);
                        settings_card(ui, &sp, |ui| {
                            egui::ScrollArea::vertical()
                                .max_height(130.0)
                                .show(ui, |ui| {
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(&info.notes).size(11.0).color(sp.muted),
                                        )
                                        .wrap(),
                                    );
                                });
                        });
                    }
                }
                UpdateUiState::Downloading {
                    info,
                    downloaded,
                    total,
                } => {
                    ui.label(
                        RichText::new(format!("Downloading {}...", info.tag))
                            .size(13.0)
                            .strong()
                            .color(sp.text),
                    );
                    ui.add_space(6.0);
                    let denom = total.or(info.asset_size).unwrap_or(0);
                    let frac = if denom > 0 {
                        (*downloaded as f32 / denom as f32).clamp(0.0, 1.0)
                    } else {
                        0.0
                    };
                    ui.add(
                        egui::ProgressBar::new(frac)
                            .desired_width(ui.available_width())
                            .fill(sp.accent)
                            .show_percentage(),
                    );
                    ui.add_space(4.0);
                    let size_line = if denom > 0 {
                        format!("{} / {}", format_size(*downloaded), format_size(denom))
                    } else {
                        format_size(*downloaded)
                    };
                    ui.label(RichText::new(size_line).size(11.0).color(sp.muted));
                }
                UpdateUiState::Failed(err) => {
                    ui.label(
                        RichText::new(format!("Update check failed: {err}"))
                            .size(12.0)
                            .color(sp.red),
                    );
                }
            }

            ui.add_space(14.0);
            ui.horizontal(|ui| {
                if matches!(state, UpdateUiState::UpToDate(_) | UpdateUiState::Failed(_))
                    && figma_button(ui, "Check again", 104.0, 30.0, false, &sp)
                {
                    action = Some(Action::CheckUpdatesNow);
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if figma_button(ui, "Close", 76.0, 30.0, false, &sp) {
                        *open = false;
                    }
                    if let UpdateUiState::Available(info) = state {
                        ui.add_space(8.0);
                        if let (Some(url), Some(name)) = (&info.asset_url, &info.asset_name) {
                            if figma_button(ui, "Update now", 112.0, 30.0, true, &sp) {
                                action = Some(Action::StartSelfUpdate {
                                    url: url.clone(),
                                    file_name: name.clone(),
                                });
                            }
                        } else if figma_button(ui, "Open GitHub", 112.0, 30.0, true, &sp) {
                            ctx.open_url(egui::OpenUrl::new_tab(&info.release_url));
                        }
                    } else if let UpdateUiState::Failed(_) = state {
                        ui.add_space(8.0);
                        if figma_button(ui, "Open GitHub", 104.0, 30.0, false, &sp) {
                            ctx.open_url(egui::OpenUrl::new_tab(
                                crate::updater::GITHUB_RELEASES_PAGE,
                            ));
                        }
                    }
                });
            });
        });

    action
}

pub(super) const MAX_BATCH_IMPORT_BYTES: u64 = 512 * 1024;

pub(super) fn load_batch_import_file(path: &std::path::Path) -> Result<String, String> {
    let meta = std::fs::metadata(path).map_err(|e| format!("Could not read file: {e}"))?;
    if !meta.is_file() {
        return Err("Selected path is not a file".to_string());
    }
    if meta.len() > MAX_BATCH_IMPORT_BYTES {
        return Err("Import file exceeds 512 KiB size limit".to_string());
    }
    std::fs::read_to_string(path).map_err(|e| format!("Could not read text file: {e}"))
}
