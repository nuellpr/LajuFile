//! Add-download and Settings dialogs.

use std::path::PathBuf;

use egui::{
    Align, Align2, Color32, FontId, Layout, Margin, Rect, RichText, Sense, Stroke, Ui, Vec2,
};
use lajufile_core::checksum::checksums_match;
use lajufile_core::settings::{DEFAULT_CAPTURE_EXTENSIONS, DuplicatePolicy, Settings, Theme};
use lajufile_core::store::{Download, DownloadStatus, Segment, now_millis};
use lajufile_core::video::{DEFAULT_SELECTOR, QualityChoice};

use crate::bridge::{BridgeState, BridgeStatus};
use crate::engine::AddRequest;

use super::format::{eta_seconds, format_eta, format_relative_time, format_size, format_speed};
use super::{Action, extract_urls_from_text};

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

#[derive(Clone, Copy, PartialEq, Eq)]
enum SettingsTab {
    General,
    Downloads,
    BrowserCapture,
    Appearance,
    About,
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

pub fn settings_dialog(
    ctx: &egui::Context,
    open: &mut bool,
    settings: &mut Settings,
    bridge: Option<&BridgeStatus>,
) -> Option<Action> {
    let sp = SettingsPalette::from_dark(matches!(settings.theme, Theme::Dark));
    let tab_id = egui::Id::unique("lajufile_settings_tab");
    let mut active_tab = ctx.data_mut(|data| {
        data.get_temp::<SettingsTab>(tab_id)
            .unwrap_or(SettingsTab::Downloads)
    });
    let mut action = None;

    let style = ctx.style_of(if matches!(settings.theme, Theme::Dark) {
        egui::Theme::Dark
    } else {
        egui::Theme::Light
    });
    let frame = egui::Frame::window(&style)
        .fill(sp.dialog_bg)
        .stroke(Stroke::new(1.0, sp.border))
        .corner_radius(14)
        .inner_margin(Margin::ZERO);

    egui::Window::new("Settings")
        .id(egui::Id::unique("lajufile_settings_modal"))
        .title_bar(false)
        .collapsible(false)
        .resizable(false)
        .frame(frame)
        .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
        .show(ctx, |ui| {
            ui.set_width(820.0);
            ui.spacing_mut().item_spacing = Vec2::ZERO;

            if modal_header(
                ui,
                &crate::i18n::t_ctx(ctx, "Settings"),
                "Manage downloads, performance, and browser capture.",
                &sp,
            ) {
                *open = false;
            }

            draw_h_divider(ui, sp.border);

            // Body: Left nav (188px) + right scrollable content (631px)
            let body_h = 436.0;
            ui.horizontal(|ui| {
                ui.set_height(body_h);
                ui.allocate_ui_with_layout(
                    Vec2::new(188.0, body_h),
                    Layout::top_down(Align::Min),
                    |ui| {
                        egui::Frame::new()
                            .fill(sp.nav_bg)
                            .inner_margin(Margin::symmetric(10, 14))
                            .show(ui, |ui| {
                                ui.set_width(168.0);
                                ui.set_height(body_h - 28.0);
                                ui.vertical(|ui| {
                                    ui.set_width(168.0);
                                    ui.set_height(body_h - 28.0);
                                    let bridge_dot = bridge.map(|status| match status.state {
                                        BridgeState::Bound => sp.green,
                                        BridgeState::Error => sp.red,
                                        BridgeState::Starting => sp.amber,
                                    });
                                    for (tab, label, dot) in [
                                        (SettingsTab::General, "General", None),
                                        (SettingsTab::Downloads, "Downloads", None),
                                        (
                                            SettingsTab::BrowserCapture,
                                            "Browser capture",
                                            bridge_dot,
                                        ),
                                        (SettingsTab::Appearance, "Appearance", None),
                                        (SettingsTab::About, "About", None),
                                    ] {
                                        if settings_nav_item(
                                            ui,
                                            tab,
                                            &crate::i18n::t_ctx(ctx, label),
                                            active_tab == tab,
                                            dot,
                                            &sp,
                                        ) {
                                            active_tab = tab;
                                        }
                                        ui.add_space(4.0);
                                    }
                                    let footer_pad = (ui.available_height() - 16.0).max(8.0);
                                    ui.add_space(footer_pad);
                                    ui.label(
                                        RichText::new(concat!(
                                            "LajuFile ",
                                            env!("CARGO_PKG_VERSION")
                                        ))
                                        .size(10.0)
                                        .color(sp.faint),
                                    );
                                });
                            });
                    },
                );

                let v_div = ui
                    .allocate_exact_size(Vec2::new(1.0, body_h), Sense::hover())
                    .0;
                ui.painter().rect_filled(v_div, 0.0, sp.border);

                ui.allocate_ui_with_layout(
                    Vec2::new(ui.available_width(), body_h),
                    Layout::top_down(Align::Min),
                    |ui| {
                        egui::ScrollArea::vertical()
                            .id_salt("lajufile_settings_scroll")
                            .max_height(body_h)
                            .show(ui, |ui| {
                                egui::Frame::new()
                                    .fill(sp.dialog_bg)
                                    .inner_margin(Margin::symmetric(24, 20))
                                    .show(ui, |ui| {
                                        ui.set_width(ui.available_width());
                                        draw_settings_tab(
                                            ui,
                                            active_tab,
                                            settings,
                                            bridge,
                                            &sp,
                                            &mut action,
                                        );
                                    });
                            });
                    },
                );
            });

            draw_h_divider(ui, sp.border);

            // Footer bar (Figma 9:455 - 9:458)
            egui::Frame::new()
                .fill(sp.dialog_bg)
                .inner_margin(Margin::symmetric(24, 12))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new("Click Done to save and apply changes")
                                .size(10.5)
                                .color(sp.faint),
                        );
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if figma_button(
                                ui,
                                &crate::i18n::t_ctx(ctx, "Done"),
                                90.0,
                                34.0,
                                true,
                                &sp,
                            ) {
                                action = Some(Action::SaveSettings(settings.clone()));
                                *open = false;
                            }
                            ui.add_space(8.0);
                            if figma_button(
                                ui,
                                &crate::i18n::t_ctx(ctx, "Cancel"),
                                84.0,
                                34.0,
                                false,
                                &sp,
                            ) {
                                *open = false;
                            }
                        });
                    });
                });
        });

    ctx.data_mut(|data| data.insert_temp(tab_id, active_tab));
    action
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BrowserKind {
    Chrome,
    Edge,
    Brave,
    OperaVivaldi,
    Firefox,
}

fn quick_pair_payload(settings: &Settings) -> String {
    serde_json::json!({
        "port": settings.bridge_port,
        "token": settings.bridge_token,
    })
    .to_string()
}

fn paint_browser_emblem(ui: &mut Ui, id: &str, sp: &SettingsPalette) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(30.0), Sense::hover());
    let (bg, fg, glyph) = match id {
        "chrome" => (Color32::from_rgb(0x1D, 0x4E, 0xD8), Color32::WHITE, "C"),
        "edge" => (Color32::from_rgb(0x02, 0x84, 0xC7), Color32::WHITE, "E"),
        "brave" => (Color32::from_rgb(0xEA, 0x58, 0x0C), Color32::WHITE, "B"),
        "opera" | "vivaldi" => (Color32::from_rgb(0xDC, 0x26, 0x26), Color32::WHITE, "O"),
        "firefox" => (Color32::from_rgb(0xEA, 0x58, 0x0C), Color32::WHITE, "F"),
        _ => (sp.nav_bg, sp.text, "W"),
    };
    ui.painter().circle_filled(rect.center(), 14.0, bg);
    ui.painter().circle_stroke(
        rect.center(),
        14.0,
        Stroke::new(1.0, sp.border.gamma_multiply(0.8)),
    );
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        glyph,
        FontId::proportional(13.0),
        fg,
    );
}

fn paint_capture_hero_badge(ui: &mut Ui, sp: &SettingsPalette) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(48.0), Sense::hover());
    let c = rect.center();
    ui.painter()
        .circle_filled(c, 22.0, sp.accent.gamma_multiply(0.16));
    ui.painter()
        .circle_stroke(c, 22.0, Stroke::new(1.5, sp.accent.gamma_multiply(0.65)));
    ui.painter()
        .circle_stroke(c, 13.0, Stroke::new(1.4, sp.accent));
    ui.painter().line_segment(
        [egui::pos2(c.x - 13.0, c.y), egui::pos2(c.x + 13.0, c.y)],
        Stroke::new(1.1, sp.accent.gamma_multiply(0.75)),
    );
    ui.painter().line_segment(
        [egui::pos2(c.x, c.y - 13.0), egui::pos2(c.x, c.y + 13.0)],
        Stroke::new(1.1, sp.accent.gamma_multiply(0.75)),
    );
    let badge_center = egui::pos2(rect.right() - 9.0, rect.bottom() - 9.0);
    ui.painter().circle_filled(badge_center, 8.5, sp.green);
    ui.painter().line(
        vec![
            egui::pos2(badge_center.x, badge_center.y - 4.0),
            egui::pos2(badge_center.x, badge_center.y + 3.0),
        ],
        Stroke::new(1.8, Color32::WHITE),
    );
    ui.painter().line(
        vec![
            egui::pos2(badge_center.x - 3.0, badge_center.y + 0.5),
            egui::pos2(badge_center.x, badge_center.y + 3.5),
            egui::pos2(badge_center.x + 3.0, badge_center.y + 0.5),
        ],
        Stroke::new(1.8, Color32::WHITE),
    );
}

pub fn browser_setup_dialog(
    ctx: &egui::Context,
    open: &mut bool,
    settings: &mut Settings,
    bridge: Option<&BridgeStatus>,
) -> Option<Action> {
    let sp = SettingsPalette::from_dark(matches!(settings.theme, Theme::Dark));
    let browser_id = egui::Id::unique("lajufile_browser_wizard_kind");
    let manual_id = egui::Id::unique("lajufile_browser_wizard_manual");
    let detected_id = egui::Id::unique("lajufile_detected_browsers");

    let mut selected_browser = ctx.data_mut(|d| {
        d.get_temp::<BrowserKind>(browser_id)
            .unwrap_or(BrowserKind::Chrome)
    });
    let mut manual_mode = ctx.data_mut(|d| d.get_temp::<bool>(manual_id).unwrap_or(false));
    let detected = ctx.data_mut(|d| {
        if let Some(list) = d.get_temp::<Vec<crate::tools::DetectedBrowser>>(detected_id) {
            list
        } else {
            let list = crate::tools::detect_installed_browsers();
            d.insert_temp(detected_id, list.clone());
            list
        }
    });
    let mut action = None;

    let style = ctx.style_of(if matches!(settings.theme, Theme::Dark) {
        egui::Theme::Dark
    } else {
        egui::Theme::Light
    });
    let frame = egui::Frame::window(&style)
        .fill(sp.dialog_bg)
        .stroke(Stroke::new(1.0, sp.border))
        .corner_radius(14)
        .inner_margin(Margin::ZERO);

    egui::Window::new("Add LajuFile to your browser")
        .id(egui::Id::unique("lajufile_browser_setup_wizard"))
        .title_bar(false)
        .collapsible(false)
        .resizable(false)
        .frame(frame)
        .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
        .show(ctx, |ui| {
            ui.set_width(580.0);
            ui.spacing_mut().item_spacing = Vec2::ZERO;

            if modal_header(
                ui,
                &crate::i18n::t_ctx(ctx, "Add LajuFile to your browser"),
                "Browser capture, PDF/media links, and YouTube download panel.",
                &sp,
            ) {
                *open = false;
            }
            draw_h_divider(ui, sp.border);

            egui::Frame::new()
                .fill(sp.dialog_bg)
                .inner_margin(Margin::symmetric(24, 18))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());

                    // Hero row (matching screenshot hierarchy with sharper dark/light card polish)
                    settings_card(ui, &sp, |ui| {
                        ui.horizontal(|ui| {
                            paint_capture_hero_badge(ui, &sp);
                            ui.add_space(14.0);
                            ui.vertical(|ui| {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new(crate::i18n::t_ctx(
                                            ctx,
                                            "Catch downloads from your browser",
                                        ))
                                        .size(15.0)
                                        .strong()
                                        .color(sp.text),
                                    );
                                    ui.add_space(8.0);
                                    if settings.browser_paired {
                                        badge_pill(ui, "Connected", sp.green, &sp);
                                    } else if bridge.is_some_and(|s| s.state == BridgeState::Error) {
                                        badge_pill(ui, "Bridge offline", sp.red, &sp);
                                    } else {
                                        badge_pill(ui, "Not connected", sp.amber, &sp);
                                    }
                                });
                                ui.add_space(3.0);
                                ui.label(
                                    RichText::new(
                                        "Add the extension: files you download in your browser come here, PDFs/media are captured, and videos get a Download button.",
                                    )
                                    .size(11.0)
                                    .color(sp.muted),
                                );
                            });
                        });
                    });

                    ui.add_space(12.0);

                    if !manual_mode {
                        settings_card(ui, &sp, |ui| {
                            card_kicker(ui, "DETECTED BROWSERS ON THIS PC", &sp);
                            ui.add_space(8.0);
                            for (idx, browser) in detected.iter().enumerate() {
                                if idx > 0 {
                                    ui.add_space(8.0);
                                }
                                egui::Frame::new()
                                    .fill(sp.field_bg)
                                    .stroke(Stroke::new(1.0, sp.border))
                                    .corner_radius(9)
                                    .inner_margin(Margin::symmetric(12, 9))
                                    .show(ui, |ui| {
                                        ui.set_width(ui.available_width());
                                        ui.horizontal(|ui| {
                                            paint_browser_emblem(ui, browser.id, &sp);
                                            ui.add_space(10.0);
                                            ui.vertical(|ui| {
                                                ui.label(
                                                    RichText::new(browser.name)
                                                        .size(13.0)
                                                        .strong()
                                                        .color(sp.text),
                                                );
                                                let subtitle = if browser.is_firefox {
                                                    "Requires signed AMO add-on (Firefox Release)"
                                                } else if browser.executable.is_some() {
                                                    "Chromium MV3 · Ready for Quick-Pair"
                                                } else {
                                                    "Chromium MV3 compatible"
                                                };
                                                ui.label(
                                                    RichText::new(subtitle)
                                                        .size(10.0)
                                                        .color(sp.faint),
                                                );
                                            });
                                            ui.with_layout(
                                                Layout::right_to_left(Align::Center),
                                                |ui| {
                                                    let primary = idx == 0 && !browser.is_firefox;
                                                    if figma_button(
                                                        ui,
                                                        "Add extension",
                                                        118.0,
                                                        31.0,
                                                        primary,
                                                        &sp,
                                                    ) {
                                                        selected_browser = match browser.id {
                                                            "edge" => BrowserKind::Edge,
                                                            "brave" => BrowserKind::Brave,
                                                            "opera" | "vivaldi" => {
                                                                BrowserKind::OperaVivaldi
                                                            }
                                                            "firefox" => BrowserKind::Firefox,
                                                            _ => BrowserKind::Chrome,
                                                        };
                                                        if !browser.is_firefox {
                                                            ui.ctx().copy_text(
                                                                quick_pair_payload(settings),
                                                            );
                                                            action = Some(Action::LaunchBrowser {
                                                                browser_name: browser
                                                                    .name
                                                                    .to_string(),
                                                                extensions_url: browser
                                                                    .extensions_url
                                                                    .to_string(),
                                                            });
                                                        }
                                                        manual_mode = true;
                                                    }
                                                },
                                            );
                                        });
                                    });
                            }
                            ui.add_space(10.0);
                            let note = if settings.browser_paired {
                                "Once added and paired, downloads flow straight to LajuFile automatically."
                            } else {
                                "Once it is added, this window updates automatically. Note: installing without Developer Mode requires Store publication (Chrome Web Store / Edge Add-ons / AMO); until then, Add extension copies Quick-Pair and opens install steps."
                            };
                            ui.label(RichText::new(note).size(10.5).color(sp.faint));
                        });
                    } else {
                        ui.horizontal(|ui| {
                            for (kind, label, width) in [
                                (BrowserKind::Chrome, "Chrome", 74.0),
                                (BrowserKind::Edge, "Edge", 64.0),
                                (BrowserKind::Brave, "Brave", 68.0),
                                (BrowserKind::OperaVivaldi, "Opera / Vivaldi", 110.0),
                                (BrowserKind::Firefox, "Firefox", 74.0),
                            ] {
                                if figma_button(
                                    ui,
                                    label,
                                    width,
                                    28.0,
                                    selected_browser == kind,
                                    &sp,
                                ) {
                                    selected_browser = kind;
                                }
                                ui.add_space(6.0);
                            }
                        });
                        ui.add_space(10.0);

                        if selected_browser == BrowserKind::Firefox {
                            settings_card(ui, &sp, |ui| {
                                card_kicker(ui, "FIREFOX RELEASE LIMITATION", &sp);
                                ui.add_space(6.0);
                                ui.label(
                                    RichText::new(
                                        "Release Firefox refuses permanent unsigned local add-ons unless signed via Mozilla Add-ons (AMO). Run extension/package.ps1 to generate lajufile-extension-firefox.zip for AMO upload, or use Chrome/Edge/Brave/Opera/Vivaldi locally right now.",
                                    )
                                    .size(11.0)
                                    .color(sp.muted),
                                );
                            });
                        } else {
                            let ext_page_url = match selected_browser {
                                BrowserKind::Edge => "edge://extensions",
                                BrowserKind::Brave => "brave://extensions",
                                BrowserKind::OperaVivaldi => "opera://extensions",
                                BrowserKind::Chrome | BrowserKind::Firefox => "chrome://extensions",
                            };
                            let ext_folder = crate::tools::extension_dir()
                                .map(|p| p.display().to_string())
                                .unwrap_or_else(|| "extension".to_string());

                            settings_card(ui, &sp, |ui| {
                                card_kicker(
                                    ui,
                                    "1. OPEN EXTENSIONS & ENABLE DEVELOPER MODE (UNTIL STORE PUBLISH)",
                                    &sp,
                                );
                                ui.add_space(6.0);
                                ui.horizontal(|ui| {
                                    egui::Frame::new()
                                        .fill(sp.field_bg)
                                        .stroke(Stroke::new(1.0, sp.border))
                                        .corner_radius(8)
                                        .inner_margin(Margin::symmetric(10, 6))
                                        .show(ui, |ui| {
                                            ui.set_width(
                                                (ui.available_width() - 110.0).max(150.0),
                                            );
                                            ui.label(
                                                RichText::new(ext_page_url)
                                                    .monospace()
                                                    .size(11.0)
                                                    .color(sp.text),
                                            );
                                        });
                                    ui.add_space(8.0);
                                    if figma_button(ui, "Copy URL", 86.0, 30.0, false, &sp) {
                                        ui.ctx().copy_text(ext_page_url.to_string());
                                    }
                                });

                                ui.add_space(10.0);
                                card_kicker(ui, "2. CLICK 'LOAD UNPACKED' & PICK EXTENSION FOLDER", &sp);
                                ui.add_space(6.0);
                                ui.horizontal(|ui| {
                                    egui::Frame::new()
                                        .fill(sp.field_bg)
                                        .stroke(Stroke::new(1.0, sp.border))
                                        .corner_radius(8)
                                        .inner_margin(Margin::symmetric(10, 6))
                                        .show(ui, |ui| {
                                            ui.set_width(
                                                (ui.available_width() - 196.0).max(110.0),
                                            );
                                            ui.label(
                                                RichText::new(&ext_folder)
                                                    .monospace()
                                                    .size(10.0)
                                                    .color(sp.text),
                                            );
                                        });
                                    ui.add_space(8.0);
                                    if figma_button(ui, "Copy path", 80.0, 30.0, false, &sp) {
                                        ui.ctx().copy_text(ext_folder.clone());
                                    }
                                    ui.add_space(8.0);
                                    if figma_button(ui, "Open folder", 92.0, 30.0, false, &sp) {
                                        action = Some(Action::OpenExtensionFolder);
                                    }
                                });

                                ui.add_space(10.0);
                                card_kicker(ui, "3. PASTE QUICK-PAIR IN POPUP OR OPTIONS", &sp);
                                ui.add_space(6.0);
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new(format!(
                                            "Port {} · Token {}",
                                            settings.bridge_port,
                                            mask_token(&settings.bridge_token)
                                        ))
                                        .monospace()
                                        .size(10.5)
                                        .color(sp.muted),
                                    );
                                    ui.with_layout(
                                        Layout::right_to_left(Align::Center),
                                        |ui| {
                                            if figma_button(
                                                ui,
                                                "Copy Quick-Pair",
                                                122.0,
                                                30.0,
                                                true,
                                                &sp,
                                            ) {
                                                ui.ctx().copy_text(quick_pair_payload(settings));
                                            }
                                        },
                                    );
                                });
                            });
                        }

                        ui.add_space(10.0);
                        settings_card(ui, &sp, |ui| {
                            let mut three_step_flow = settings.ask_where_to_save
                                && settings.show_progress_on_start
                                && settings.show_complete_dialog;
                            if toggle_row_interactive(
                                ui,
                                "Classic 3-step popup flow (File Info -> Progress -> Complete)",
                                "wizard_3step_flow",
                                &mut three_step_flow,
                                &sp,
                            ) {
                                settings.ask_where_to_save = three_step_flow;
                                settings.show_progress_on_start = three_step_flow;
                                settings.show_complete_dialog = true;
                                action = Some(Action::SaveSettings(settings.clone()));
                            }
                        });
                    }
                });

            draw_h_divider(ui, sp.border);
            egui::Frame::new()
                .fill(sp.dialog_bg)
                .inner_margin(Margin::symmetric(24, 12))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        let toggle_label = if manual_mode {
                            "<- Back to browsers"
                        } else {
                            "Install it by hand"
                        };
                        let resp = ui
                            .add(
                                egui::Label::new(
                                    RichText::new(toggle_label).size(12.0).color(sp.accent),
                                )
                                .sense(Sense::click()),
                            )
                            .on_hover_cursor(egui::CursorIcon::PointingHand);
                        if resp.clicked() {
                            manual_mode = !manual_mode;
                        }

                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if figma_button(
                                ui,
                                &crate::i18n::t_ctx(ctx, "Close"),
                                84.0,
                                32.0,
                                false,
                                &sp,
                            ) {
                                *open = false;
                            }
                            ui.add_space(8.0);
                            if figma_button(ui, "Don't ask again", 122.0, 32.0, false, &sp) {
                                settings.suppress_browser_wizard = true;
                                action = Some(Action::SaveSettings(settings.clone()));
                                *open = false;
                            }
                        });
                    });
                });
        });

    ctx.data_mut(|d| {
        d.insert_temp(browser_id, selected_browser);
        d.insert_temp(manual_id, manual_mode);
    });
    action
}

pub fn language_dialog(
    ctx: &egui::Context,
    open: &mut bool,
    current_setting: &str,
    dark: bool,
) -> Option<Action> {
    let sp = SettingsPalette::from_dark(dark);
    let search_id = egui::Id::unique("lajufile_lang_search");
    let sel_id = egui::Id::unique("lajufile_lang_selected");
    let mut query = ctx.data_mut(|d| d.get_temp::<String>(search_id).unwrap_or_default());
    let mut selected = ctx.data_mut(|d| {
        d.get_temp::<String>(sel_id)
            .unwrap_or_else(|| current_setting.to_string())
    });
    let mut action = None;

    let style = ctx.style_of(if dark {
        egui::Theme::Dark
    } else {
        egui::Theme::Light
    });
    let frame = egui::Frame::window(&style)
        .fill(sp.dialog_bg)
        .stroke(Stroke::new(1.0, sp.border))
        .corner_radius(14)
        .inner_margin(Margin::ZERO);

    egui::Window::new("Choose language")
        .id(egui::Id::unique("lajufile_language_picker"))
        .title_bar(false)
        .collapsible(false)
        .resizable(false)
        .frame(frame)
        .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
        .show(ctx, |ui| {
            ui.set_width(540.0);
            ui.spacing_mut().item_spacing = Vec2::ZERO;

            if modal_header(
                ui,
                &crate::i18n::t_ctx(ctx, "Choose language"),
                "Switches the interface immediately without restarting. Supports RTL mirroring & custom i18n/*.tsv catalogs.",
                &sp,
            ) {
                *open = false;
            }
            draw_h_divider(ui, sp.border);

            egui::Frame::new()
                .fill(sp.dialog_bg)
                .inner_margin(Margin::symmetric(24, 16))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    egui::Frame::new()
                        .fill(sp.field_bg)
                        .stroke(Stroke::new(1.0, sp.border))
                        .corner_radius(8)
                        .inner_margin(Margin::symmetric(10, 6))
                        .show(ui, |ui| {
                            ui.set_width((ui.available_width() - 20.0).max(180.0));
                            ui.add_sized(
                                [ui.available_width(), 22.0],
                                egui::TextEdit::singleline(&mut query)
                                    .frame(egui::Frame::NONE)
                                    .background_color(Color32::TRANSPARENT)
                                    .hint_text("Search language (name, native name, or code)..."),
                            );
                        });

                    ui.add_space(10.0);
                    let q = query.trim().to_ascii_lowercase();
                    let os_code = crate::i18n::detect_windows_language();
                    let os_label = crate::i18n::find_language(os_code)
                        .map(|info| info.native_name)
                        .unwrap_or("English");

                    egui::Frame::new()
                        .fill(sp.card_bg)
                        .stroke(Stroke::new(1.0, sp.border))
                        .corner_radius(10)
                        .inner_margin(Margin::symmetric(6, 6))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            egui::ScrollArea::vertical()
                                .id_salt("lajufile_lang_scroll")
                                .max_height(310.0)
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    if q.is_empty()
                                        || "windows".contains(&q)
                                        || "system".contains(&q)
                                        || "auto".contains(&q)
                                    {
                                        let is_sel = selected == "system";
                                        let (rect, resp) = ui.allocate_exact_size(
                                            Vec2::new(ui.available_width(), 34.0),
                                            Sense::click(),
                                        );
                                        let resp =
                                            resp.on_hover_cursor(egui::CursorIcon::PointingHand);
                                        if is_sel || resp.hovered() {
                                            ui.painter().rect_filled(
                                                rect,
                                                7.0,
                                                if is_sel { sp.nav_selected } else { sp.field_bg },
                                            );
                                        }
                                        ui.painter().text(
                                            egui::pos2(rect.left() + 12.0, rect.center().y),
                                            Align2::LEFT_CENTER,
                                            format!(
                                                "{} ({os_label})",
                                                crate::i18n::t_ctx(ctx, "Windows' language")
                                            ),
                                            FontId::proportional(12.0),
                                            if is_sel { sp.accent } else { sp.text },
                                        );
                                        ui.painter().text(
                                            egui::pos2(rect.right() - 12.0, rect.center().y),
                                            Align2::RIGHT_CENTER,
                                            "Auto",
                                            FontId::proportional(10.5),
                                            sp.muted,
                                        );
                                        if resp.clicked() {
                                            selected = "system".to_string();
                                        }
                                        if resp.double_clicked() {
                                            action = Some(Action::SetLanguage("system".to_string()));
                                            *open = false;
                                        }
                                        ui.add_space(4.0);
                                        draw_h_divider(ui, sp.border);
                                        ui.add_space(4.0);
                                    }

                                    for info in crate::i18n::CATALOG {
                                        if !q.is_empty()
                                            && !info.english_name.to_ascii_lowercase().contains(&q)
                                            && !info.native_name.to_ascii_lowercase().contains(&q)
                                            && !info.code.to_ascii_lowercase().contains(&q)
                                        {
                                            continue;
                                        }
                                        let is_sel = selected.eq_ignore_ascii_case(info.code);
                                        let (rect, resp) = ui.allocate_exact_size(
                                            Vec2::new(ui.available_width(), 32.0),
                                            Sense::click(),
                                        );
                                        let resp =
                                            resp.on_hover_cursor(egui::CursorIcon::PointingHand);
                                        if is_sel || resp.hovered() {
                                            ui.painter().rect_filled(
                                                rect,
                                                7.0,
                                                if is_sel { sp.nav_selected } else { sp.field_bg },
                                            );
                                        }
                                        let badge_rect = Rect::from_center_size(
                                            egui::pos2(rect.left() + 20.0, rect.center().y),
                                            Vec2::new(26.0, 18.0),
                                        );
                                        ui.painter().rect_filled(badge_rect, 4.0, sp.field_bg);
                                        ui.painter().rect_stroke(
                                            badge_rect,
                                            4.0,
                                            Stroke::new(1.0, sp.border),
                                            egui::StrokeKind::Inside,
                                        );
                                        ui.painter().text(
                                            badge_rect.center(),
                                            Align2::CENTER_CENTER,
                                            info.badge,
                                            FontId::proportional(9.0),
                                            sp.muted,
                                        );
                                        ui.painter().text(
                                            egui::pos2(rect.left() + 42.0, rect.center().y),
                                            Align2::LEFT_CENTER,
                                            format!(
                                                "{}  ·  {}",
                                                info.native_name, info.english_name
                                            ),
                                            FontId::proportional(11.5),
                                            if is_sel { sp.accent } else { sp.text },
                                        );
                                        let tag = if info.rtl {
                                            "RTL · Built-in"
                                        } else if crate::i18n::has_builtin_catalog(info.code) {
                                            "Built-in"
                                        } else {
                                            "TSV / EN"
                                        };
                                        ui.painter().text(
                                            egui::pos2(rect.right() - 10.0, rect.center().y),
                                            Align2::RIGHT_CENTER,
                                            tag,
                                            FontId::proportional(9.5),
                                            sp.faint,
                                        );
                                        if resp.clicked() {
                                            selected = info.code.to_string();
                                        }
                                        if resp.double_clicked() {
                                            action =
                                                Some(Action::SetLanguage(info.code.to_string()));
                                            *open = false;
                                        }
                                    }
                                });
                        });
                });

            draw_h_divider(ui, sp.border);
            egui::Frame::new()
                .fill(sp.dialog_bg)
                .inner_margin(Margin::symmetric(24, 12))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if figma_button(
                            ui,
                            &crate::i18n::t_ctx(ctx, "Apply"),
                            92.0,
                            34.0,
                            true,
                            &sp,
                        ) {
                            action = Some(Action::SetLanguage(selected.clone()));
                            *open = false;
                        }
                        ui.add_space(8.0);
                        if figma_button(
                            ui,
                            &crate::i18n::t_ctx(ctx, "Cancel"),
                            84.0,
                            34.0,
                            false,
                            &sp,
                        ) {
                            *open = false;
                        }
                    });
                });
        });

    ctx.data_mut(|d| {
        d.insert_temp(search_id, query);
        if *open {
            d.insert_temp(sel_id, selected);
        } else {
            d.remove::<String>(sel_id);
        }
    });
    action
}

fn draw_settings_tab(
    ui: &mut Ui,
    tab: SettingsTab,
    settings: &mut Settings,
    bridge: Option<&BridgeStatus>,
    sp: &SettingsPalette,
    action: &mut Option<Action>,
) {
    match tab {
        SettingsTab::Downloads => {
            tab_header(
                ui,
                "Downloads",
                "Choose where files go and how much bandwidth LajuFile uses.",
                sp,
            );
            draw_save_location_card(ui, settings, sp, action);
            ui.add_space(12.0);
            draw_category_folders_card(ui, settings, sp, action);
            ui.add_space(12.0);
            draw_performance_card(ui, settings, sp);
            ui.add_space(12.0);
            draw_browser_capture_card(ui, settings, bridge, sp, action);
            ui.add_space(12.0);
            draw_behavior_card(ui, settings, sp);
        }
        SettingsTab::General => {
            tab_header(
                ui,
                "General",
                "Startup behavior, tray integration, and folder defaults.",
                sp,
            );
            draw_save_location_card(ui, settings, sp, action);
            ui.add_space(12.0);
            draw_behavior_card(ui, settings, sp);
        }
        SettingsTab::BrowserCapture => {
            tab_header(
                ui,
                "Browser capture",
                "Pair LajuFile with Chrome, Brave, Edge, Opera, or Vivaldi to intercept downloads.",
                sp,
            );
            draw_browser_capture_card(ui, settings, bridge, sp, action);
            ui.add_space(12.0);
            settings_card(ui, sp, |ui| {
                card_kicker(ui, "BROWSER WORKFLOW & SETUP", sp);
                ui.add_space(8.0);
                let mut three_step_flow = settings.ask_where_to_save
                    && settings.show_progress_on_start
                    && settings.show_complete_dialog;
                if toggle_row_interactive(
                    ui,
                    "Classic 3-step popup flow (Info -> Progress -> Complete)",
                    "settings_browser_3step",
                    &mut three_step_flow,
                    sp,
                ) {
                    settings.ask_where_to_save = three_step_flow;
                    settings.show_progress_on_start = three_step_flow;
                    settings.show_complete_dialog = true;
                }
                ui.add_space(10.0);
                for step in [
                    "1. Open chrome://extensions or edge://extensions and enable Developer mode.",
                    "2. Click Load unpacked and pick LajuFile's extension folder.",
                    "3. Hold Alt while clicking any link to keep that download inside the browser.",
                    "4. Right-click any link -> Download with LajuFile to capture any file type.",
                ] {
                    ui.label(RichText::new(step).size(11.0).color(sp.muted));
                    ui.add_space(4.0);
                }
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if figma_button(ui, "Setup Wizard...", 128.0, 32.0, true, sp) {
                        *action = Some(Action::OpenBrowserWizard);
                    }
                    ui.add_space(8.0);
                    if figma_button(ui, "Open extension folder", 156.0, 32.0, false, sp) {
                        *action = Some(Action::OpenExtensionFolder);
                    }
                });
            });
        }
        SettingsTab::Appearance => {
            tab_header(
                ui,
                "Appearance",
                "Customize light and dark interface themes.",
                sp,
            );
            settings_card(ui, sp, |ui| {
                card_kicker(ui, "THEME", sp);
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new("Interface theme")
                                .size(12.0)
                                .strong()
                                .color(sp.text),
                        );
                        ui.label(
                            RichText::new("Applies across main window and dialogs.")
                                .size(10.0)
                                .color(sp.faint),
                        );
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        segmented_theme(ui, &mut settings.theme, sp);
                    });
                });
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    for (name, color) in [
                        ("Window", sp.field_bg),
                        ("Surface", sp.card_bg),
                        ("Accent", sp.accent),
                        ("Success", sp.green),
                    ] {
                        let (swatch, _) =
                            ui.allocate_exact_size(Vec2::new(72.0, 26.0), Sense::hover());
                        ui.painter().rect_filled(swatch, 6.0, color);
                        ui.painter().rect_stroke(
                            swatch,
                            6.0,
                            Stroke::new(1.0, sp.border),
                            egui::StrokeKind::Inside,
                        );
                        ui.painter().text(
                            swatch.center(),
                            Align2::CENTER_CENTER,
                            name,
                            FontId::proportional(9.5),
                            if color == sp.accent {
                                Color32::WHITE
                            } else {
                                sp.muted
                            },
                        );
                        ui.add_space(8.0);
                    }
                });
            });
            ui.add_space(12.0);
            settings_card(ui, sp, |ui| {
                card_kicker(ui, "INTERFACE SCALE", sp);
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("Zoom scale")
                            .size(12.0)
                            .strong()
                            .color(sp.text),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        for pct in [150u32, 125, 115, 100, 90] {
                            let active = settings.ui_scale_pct == pct;
                            if figma_button(ui, &format!("{pct}%"), 50.0, 26.0, active, sp) {
                                settings.ui_scale_pct = pct;
                            }
                            ui.add_space(6.0);
                        }
                    });
                });
            });
            ui.add_space(12.0);
            settings_card(ui, sp, |ui| {
                card_kicker(ui, "LANGUAGE", sp);
                ui.add_space(10.0);
                let active_label = if settings.language == "system" {
                    let os_code = crate::i18n::detect_windows_language();
                    let native = crate::i18n::find_language(os_code)
                        .map(|i| i.native_name)
                        .unwrap_or("English");
                    format!("Windows' language ({native})")
                } else {
                    crate::i18n::find_language(&settings.language)
                        .map(|i| format!("{} · {}", i.native_name, i.english_name))
                        .unwrap_or_else(|| settings.language.clone())
                };
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new("Interface language")
                                .size(12.0)
                                .strong()
                                .color(sp.text),
                        );
                        ui.label(RichText::new(active_label).size(10.5).color(sp.faint));
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if figma_button(ui, "Choose language...", 136.0, 30.0, false, sp) {
                            *action = Some(Action::OpenLanguagePicker);
                        }
                    });
                });
            });
        }
        SettingsTab::About => {
            tab_header(
                ui,
                "About",
                "Video extraction tools and application information.",
                sp,
            );
            draw_video_tools_card(ui, sp, action);
            ui.add_space(12.0);
            settings_card(ui, sp, |ui| {
                card_kicker(ui, "APPLICATION", sp);
                ui.add_space(8.0);
                ui.label(
                    RichText::new(concat!("LajuFile v", env!("CARGO_PKG_VERSION")))
                        .size(13.0)
                        .strong()
                        .color(sp.text),
                );
                ui.add_space(2.0);
                ui.label(
                    RichText::new(
                        "Segmented multi-connection download manager with browser capture and yt-dlp integration.",
                    )
                    .size(11.0)
                    .color(sp.muted),
                );
            });
        }
    }
}

fn tab_header(ui: &mut Ui, title: &str, subtitle: &str, sp: &SettingsPalette) {
    ui.label(RichText::new(title).size(17.0).strong().color(sp.text));
    ui.add_space(2.0);
    ui.label(RichText::new(subtitle).size(11.0).color(sp.muted));
    ui.add_space(14.0);
}

fn draw_save_location_card(
    ui: &mut Ui,
    settings: &mut Settings,
    sp: &SettingsPalette,
    action: &mut Option<Action>,
) {
    settings_card(ui, sp, |ui| {
        card_kicker(ui, "SAVE LOCATION", sp);
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            let field_w = (ui.available_width() - 122.0).max(200.0);
            egui::Frame::new()
                .fill(sp.field_bg)
                .stroke(Stroke::new(1.0, sp.border))
                .corner_radius(8)
                .inner_margin(Margin::symmetric(10, 6))
                .show(ui, |ui| {
                    ui.set_width(field_w - 20.0);
                    ui.set_height(22.0);
                    ui.horizontal_centered(|ui| {
                        let (icon_rect, _) =
                            ui.allocate_exact_size(Vec2::new(12.0, 10.0), Sense::hover());
                        paint_folder_icon(ui.painter(), icon_rect, sp.muted);
                        ui.add_space(8.0);
                        let mut folder = settings.download_dir.display().to_string();
                        if ui
                            .add_sized(
                                [ui.available_width(), 20.0],
                                egui::TextEdit::singleline(&mut folder)
                                    .frame(egui::Frame::NONE)
                                    .background_color(Color32::TRANSPARENT),
                            )
                            .changed()
                        {
                            settings.download_dir = PathBuf::from(folder.trim());
                        }
                    });
                });
            ui.add_space(10.0);
            if figma_button(ui, "Change", 112.0, 34.0, false, sp) {
                *action = Some(Action::OpenFolderPicker);
            }
        });
    });
}

fn draw_category_folders_card(
    ui: &mut Ui,
    settings: &mut Settings,
    sp: &SettingsPalette,
    action: &mut Option<Action>,
) {
    settings_card(ui, sp, |ui| {
        card_kicker(ui, "CATEGORY FOLDERS", sp);
        ui.add_space(8.0);
        for cat in ["Video", "Music", "Documents", "Programs", "Compressed"] {
            let custom = settings.category_dirs.get(cat).cloned();
            ui.horizontal(|ui| {
                ui.set_width(ui.available_width());
                ui.allocate_ui_with_layout(
                    Vec2::new(96.0, 26.0),
                    Layout::left_to_right(Align::Center),
                    |ui| {
                        ui.label(RichText::new(cat).size(11.5).strong().color(sp.text));
                    },
                );
                let display = custom
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "Default save folder".to_string());
                let text_color = if custom.is_some() { sp.text } else { sp.faint };
                let btn_reserve = if custom.is_some() { 132.0 } else { 70.0 };
                let path_w = (ui.available_width() - btn_reserve).max(100.0);
                egui::Frame::new()
                    .fill(sp.field_bg)
                    .stroke(Stroke::new(1.0, sp.border))
                    .corner_radius(6)
                    .inner_margin(Margin::symmetric(8, 4))
                    .show(ui, |ui| {
                        ui.set_width(path_w);
                        ui.label(RichText::new(display).size(11.0).color(text_color));
                    });
                ui.add_space(6.0);
                if figma_button(ui, "Pick", 54.0, 26.0, false, sp) {
                    *action = Some(Action::BrowseCategoryDir(cat.to_string()));
                }
                if custom.is_some() {
                    ui.add_space(6.0);
                    if figma_button(ui, "Reset", 54.0, 26.0, false, sp) {
                        settings.category_dirs.remove(cat);
                    }
                }
            });
            ui.add_space(6.0);
        }
    });
}

fn draw_performance_card(ui: &mut Ui, settings: &mut Settings, sp: &SettingsPalette) {
    settings_card(ui, sp, |ui| {
        card_kicker(ui, "PERFORMANCE", sp);
        ui.add_space(10.0);

        setting_row(
            ui,
            "Connections per download",
            "Parallel segments requested from the server.",
            sp,
            |ui| {
                stepper_widget(ui, &mut settings.max_connections, 1, 32, sp);
            },
        );
        ui.add_space(10.0);
        setting_row(
            ui,
            "Simultaneous downloads",
            "Files downloading at the same time.",
            sp,
            |ui| {
                stepper_widget(ui, &mut settings.max_concurrent, 1, 10, sp);
            },
        );
        ui.add_space(10.0);
        setting_row(ui, "Global speed limit", "0 means unlimited.", sp, |ui| {
            egui::Frame::new()
                .fill(sp.field_bg)
                .stroke(Stroke::new(1.0, sp.border))
                .corner_radius(8)
                .inner_margin(Margin::symmetric(10, 5))
                .show(ui, |ui| {
                    ui.set_width(130.0);
                    ui.set_height(20.0);
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::DragValue::new(&mut settings.global_speed_limit_kbps)
                                .range(0..=1_000_000)
                                .speed(64),
                        );
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let hint = if settings.global_speed_limit_kbps == 0 {
                                "Unlimited"
                            } else {
                                "KB/s"
                            };
                            ui.label(RichText::new(hint).size(9.5).color(sp.faint));
                        });
                    });
                });
        });
    });
}

fn draw_browser_capture_card(
    ui: &mut Ui,
    settings: &mut Settings,
    bridge: Option<&BridgeStatus>,
    sp: &SettingsPalette,
    action: &mut Option<Action>,
) {
    settings_card(ui, sp, |ui| {
        let (status_label, dot_color) = match bridge {
            Some(status) if status.state == BridgeState::Bound => ("Connected", sp.green),
            Some(status) if status.state == BridgeState::Error => ("Bridge unavailable", sp.red),
            _ => ("Starting bridge", sp.amber),
        };
        let bound_port = bridge
            .filter(|status| status.port != 0)
            .map(|status| status.port)
            .unwrap_or(settings.bridge_port);

        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    let (dot_rect, _) = ui.allocate_exact_size(Vec2::splat(8.0), Sense::hover());
                    ui.painter()
                        .circle_filled(dot_rect.center(), 3.5, dot_color);
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new(status_label)
                            .size(12.0)
                            .strong()
                            .color(dot_color),
                    );
                });
                ui.add_space(2.0);
                ui.label(
                    RichText::new(format!("Browser extension · 127.0.0.1:{bound_port}"))
                        .size(10.5)
                        .color(sp.muted),
                );
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if figma_button(ui, "Test connection", 118.0, 30.0, false, sp) {
                    *action = Some(Action::TestBridge(settings.bridge_port));
                }
                ui.add_space(8.0);
                let pair_label = if settings.browser_paired {
                    "Extension paired"
                } else {
                    "Setup Wizard"
                };
                if figma_button(ui, pair_label, 118.0, 30.0, !settings.browser_paired, sp) {
                    *action = Some(Action::OpenBrowserWizard);
                }
            });
        });

        ui.add_space(10.0);
        setting_row(
            ui,
            "Bridge port",
            "Port changes apply after restart.",
            sp,
            |ui| {
                egui::Frame::new()
                    .fill(sp.field_bg)
                    .stroke(Stroke::new(1.0, sp.border))
                    .corner_radius(8)
                    .inner_margin(Margin::symmetric(10, 4))
                    .show(ui, |ui| {
                        ui.set_width(88.0);
                        ui.set_height(20.0);
                        ui.add(
                            egui::DragValue::new(&mut settings.bridge_port)
                                .range(1024..=65_535)
                                .speed(1),
                        );
                    });
            },
        );

        ui.add_space(12.0);
        card_kicker(ui, "ACCESS TOKEN", sp);
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let field_w = (ui.available_width() - 310.0).max(120.0);
            let display = if settings.bridge_token.len() >= 8 {
                format!(
                    "lf_••••••••••••••••••••{}",
                    &settings.bridge_token[settings.bridge_token.len() - 4..]
                )
            } else {
                mask_token(&settings.bridge_token)
            };
            egui::Frame::new()
                .fill(sp.field_bg)
                .stroke(Stroke::new(1.0, sp.border))
                .corner_radius(8)
                .inner_margin(Margin::symmetric(10, 6))
                .show(ui, |ui| {
                    ui.set_width(field_w);
                    ui.set_height(20.0);
                    ui.horizontal_centered(|ui| {
                        ui.label(RichText::new(display).monospace().size(10.5).color(sp.text));
                    });
                });
            ui.add_space(8.0);
            if figma_button(ui, "Copy", 58.0, 34.0, false, sp) {
                ui.ctx().copy_text(settings.bridge_token.clone());
            }
            ui.add_space(8.0);
            if figma_button(ui, "Quick-Pair", 92.0, 34.0, false, sp) {
                ui.ctx().copy_text(quick_pair_payload(settings));
            }
            ui.add_space(8.0);
            if figma_button(ui, "Regenerate", 104.0, 34.0, false, sp) {
                *action = Some(Action::RegenerateToken);
            }
        });
        ui.add_space(6.0);
        ui.label(
            RichText::new(
                "Paste this port and token into the LajuFile browser extension options (Chrome, Brave, Edge).",
            )
            .size(10.0)
            .color(sp.faint),
        );

        ui.add_space(12.0);
        card_kicker(ui, "CAPTURED EXTENSIONS (CLIPBOARD & BROWSER)", sp);
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let field_w = (ui.available_width() - 96.0).max(160.0);
            egui::Frame::new()
                .fill(sp.field_bg)
                .stroke(Stroke::new(1.0, sp.border))
                .corner_radius(8)
                .inner_margin(Margin::symmetric(10, 6))
                .show(ui, |ui| {
                    ui.set_width(field_w);
                    ui.add_sized(
                        [ui.available_width(), 20.0],
                        egui::TextEdit::singleline(&mut settings.captured_extensions)
                            .frame(egui::Frame::NONE)
                            .background_color(Color32::TRANSPARENT)
                            .hint_text(DEFAULT_CAPTURE_EXTENSIONS),
                    );
                });
            ui.add_space(8.0);
            if figma_button(ui, "Reset", 64.0, 32.0, false, sp) {
                settings.captured_extensions = DEFAULT_CAPTURE_EXTENSIONS.to_string();
            }
        });

        ui.add_space(12.0);
        card_kicker(ui, "EXCLUDED DOMAINS", sp);
        ui.add_space(6.0);
        egui::Frame::new()
            .fill(sp.field_bg)
            .stroke(Stroke::new(1.0, sp.border))
            .corner_radius(8)
            .inner_margin(Margin::symmetric(10, 6))
            .show(ui, |ui| {
                ui.set_width((ui.available_width() - 22.0).max(160.0));
                ui.add_sized(
                    [ui.available_width(), 20.0],
                    egui::TextEdit::singleline(&mut settings.excluded_domains)
                        .frame(egui::Frame::NONE)
                        .background_color(Color32::TRANSPARENT)
                        .hint_text("example.com, *.internal.net"),
                );
            });
        ui.add_space(4.0);
        ui.label(
            RichText::new(
                "Domains skipped by clipboard monitoring and automatic browser capture (comma or space separated).",
            )
            .size(10.0)
            .color(sp.faint),
        );
    });
}

fn draw_behavior_card(ui: &mut Ui, settings: &mut Settings, sp: &SettingsPalette) {
    settings_card(ui, sp, |ui| {
        card_kicker(ui, "BEHAVIOR", sp);
        ui.add_space(10.0);

        toggle_row(
            ui,
            "Close to tray",
            "settings_tray",
            &mut settings.close_to_tray,
            sp,
        );
        ui.add_space(8.0);
        toggle_row(
            ui,
            "Start LajuFile with Windows",
            "settings_autostart",
            &mut settings.autostart,
            sp,
        );
        ui.add_space(8.0);
        toggle_row(
            ui,
            "Auto-resume on startup",
            "settings_autoresume",
            &mut settings.auto_resume,
            sp,
        );
        ui.add_space(8.0);
        toggle_row(
            ui,
            "Monitor clipboard for download links (Ctrl+C)",
            "settings_clipboard",
            &mut settings.clipboard_monitor,
            sp,
        );
        ui.add_space(8.0);
        toggle_row(
            ui,
            "Ask where to save each download",
            "settings_ask_folder",
            &mut settings.ask_where_to_save,
            sp,
        );
        ui.add_space(8.0);
        toggle_row(
            ui,
            "Show progress window when download starts",
            "settings_progress_start",
            &mut settings.show_progress_on_start,
            sp,
        );
        ui.add_space(8.0);
        toggle_row(
            ui,
            "Show dialog when download completes",
            "settings_complete_dialog",
            &mut settings.show_complete_dialog,
            sp,
        );
        ui.add_space(8.0);
        toggle_row(
            ui,
            "Download video subtitles when available",
            "settings_video_subs",
            &mut settings.video_subtitles,
            sp,
        );
        ui.add_space(8.0);
        toggle_row(
            ui,
            "Embed video chapters (yt-dlp)",
            "settings_video_chapters",
            &mut settings.video_chapters,
            sp,
        );
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new("Duplicate URLs").size(12.0).color(sp.text));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                segmented_duplicate_policy(ui, &mut settings.duplicate_policy, sp);
            });
        });
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new("Theme").size(12.0).color(sp.text));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                segmented_theme(ui, &mut settings.theme, sp);
            });
        });
    });
}

fn draw_video_tools_card(ui: &mut Ui, sp: &SettingsPalette, action: &mut Option<Action>) {
    let tools_dir = crate::tools::tools_dir();
    let ready = crate::tools::tools_ready(&tools_dir);
    settings_card(ui, sp, |ui| {
        card_kicker(ui, "VIDEO TOOLS", sp);
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    let (dot_rect, _) = ui.allocate_exact_size(Vec2::splat(8.0), Sense::hover());
                    let color = if ready { sp.green } else { sp.amber };
                    ui.painter().circle_filled(dot_rect.center(), 3.5, color);
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new(format!(
                            "yt-dlp and ffmpeg: {}",
                            if ready { "Installed" } else { "Not installed" }
                        ))
                        .size(12.0)
                        .strong()
                        .color(sp.text),
                    );
                });
                ui.add_space(2.0);
                ui.label(
                    RichText::new(tools_dir.display().to_string())
                        .size(10.0)
                        .color(sp.faint),
                );
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if figma_button(ui, "Install / Update", 124.0, 32.0, false, sp) {
                    *action = Some(Action::UpdateTools);
                }
            });
        });
    });
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

fn setting_row(
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

fn stepper_widget(ui: &mut Ui, value: &mut u32, min: u32, max: u32, sp: &SettingsPalette) {
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

fn segmented_options<T: Clone + PartialEq>(
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

fn segmented_theme(ui: &mut Ui, theme: &mut Theme, sp: &SettingsPalette) {
    segmented_options(
        ui,
        theme,
        "lajufile_theme_seg",
        &[(Theme::Light, "Light", 64.0), (Theme::Dark, "Dark", 64.0)],
        sp,
    );
}

fn segmented_duplicate_policy(ui: &mut Ui, policy: &mut DuplicatePolicy, sp: &SettingsPalette) {
    segmented_options(
        ui,
        policy,
        "lajufile_dup_seg",
        &[
            (DuplicatePolicy::Reject, "Skip", 52.0),
            (DuplicatePolicy::ResumeExisting, "Resume", 62.0),
            (DuplicatePolicy::RenameNumbered, "Rename (#)", 76.0),
        ],
        sp,
    );
}

fn settings_nav_item(
    ui: &mut Ui,
    tab: SettingsTab,
    label: &str,
    selected: bool,
    status_dot: Option<Color32>,
    sp: &SettingsPalette,
) -> bool {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 36.0), Sense::click());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    let fill = if selected {
        sp.nav_selected
    } else if response.hovered() {
        sp.border.gamma_multiply(0.35)
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, 8.0, fill);
    if selected {
        ui.painter().rect_filled(
            Rect::from_min_size(
                egui::pos2(rect.left() - 10.0, rect.top() + 8.0),
                Vec2::new(3.0, 20.0),
            ),
            2.0,
            sp.accent,
        );
    }
    let icon_rect = Rect::from_center_size(
        egui::pos2(rect.left() + 14.0, rect.center().y),
        Vec2::splat(11.0),
    );
    let fg = if selected { sp.accent } else { sp.muted };
    paint_settings_nav_icon(ui.painter(), icon_rect, tab, fg);
    ui.painter().text(
        egui::pos2(rect.left() + 26.0, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        FontId::proportional(11.5),
        fg,
    );
    if let Some(dot) = status_dot {
        ui.painter()
            .circle_filled(egui::pos2(rect.right() - 10.0, rect.center().y), 3.0, dot);
    }
    response.clicked()
}

fn paint_settings_nav_icon(painter: &egui::Painter, rect: Rect, tab: SettingsTab, color: Color32) {
    let c = rect.center();
    let pt = |x: f32, y: f32| {
        egui::pos2(
            rect.left() + (x / 12.0) * rect.width(),
            rect.top() + (y / 12.0) * rect.height(),
        )
    };
    match tab {
        SettingsTab::General => {
            painter.circle_stroke(c, 2.3, Stroke::new(1.2, color));
            for step in 0..6 {
                let angle = step as f32 * (std::f32::consts::TAU / 6.0);
                let dir = Vec2::new(angle.cos(), angle.sin());
                painter.line_segment([c + dir * 2.7, c + dir * 4.5], Stroke::new(1.2, color));
            }
        }
        SettingsTab::Downloads => {
            painter.line_segment([pt(6.0, 1.5), pt(6.0, 8.0)], Stroke::new(1.3, color));
            painter.line(
                vec![pt(3.5, 5.5), pt(6.0, 8.0), pt(8.5, 5.5)],
                Stroke::new(1.3, color),
            );
            painter.line_segment([pt(2.0, 10.2), pt(10.0, 10.2)], Stroke::new(1.3, color));
        }
        SettingsTab::BrowserCapture => {
            painter.circle_stroke(c, 4.5, Stroke::new(1.2, color));
            painter.line_segment([pt(1.6, 6.0), pt(10.4, 6.0)], Stroke::new(1.0, color));
            painter.line_segment([pt(6.0, 1.6), pt(6.0, 10.4)], Stroke::new(1.0, color));
        }
        SettingsTab::Appearance => {
            painter.circle_stroke(c, 4.3, Stroke::new(1.2, color));
            painter.circle_filled(egui::pos2(c.x - 1.3, c.y), 1.6, color);
        }
        SettingsTab::About => {
            painter.circle_stroke(c, 4.5, Stroke::new(1.2, color));
            painter.circle_filled(pt(6.0, 3.8), 0.8, color);
            painter.line_segment([pt(6.0, 5.5), pt(6.0, 9.0)], Stroke::new(1.3, color));
        }
    }
}

fn paint_folder_icon(painter: &egui::Painter, rect: Rect, color: Color32) {
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

fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    Color32::from_rgba_unmultiplied(
        (a.r() as f32 + (b.r() as f32 - a.r() as f32) * t).round() as u8,
        (a.g() as f32 + (b.g() as f32 - a.g() as f32) * t).round() as u8,
        (a.b() as f32 + (b.b() as f32 - a.b() as f32) * t).round() as u8,
        (a.a() as f32 + (b.a() as f32 - a.a() as f32) * t).round() as u8,
    )
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
    speed_history: &[u64],
    show_details: &mut bool,
    dark: bool,
) -> (bool, Option<Action>) {
    let mut keep_open = true;
    let mut action = None;
    let sp = SettingsPalette::from_dark(dark);
    let fraction = match (download.size, download.status) {
        (_, DownloadStatus::Done) => 1.0,
        (Some(size), _) if size > 0 => (download.downloaded as f32 / size as f32).clamp(0.0, 1.0),
        _ => 0.0,
    };
    let title = format!("{:.0}% — {}", fraction * 100.0, download.file_name);
    let style = ctx.style_of(if dark {
        egui::Theme::Dark
    } else {
        egui::Theme::Light
    });
    let frame = egui::Frame::window(&style)
        .fill(sp.dialog_bg)
        .stroke(Stroke::new(1.0, sp.border))
        .corner_radius(14)
        .inner_margin(Margin::ZERO);

    let viewport_rect = ctx.input(|i| i.content_rect());
    let initial_pos = egui::pos2(
        (viewport_rect.center().x - 300.0).max(16.0),
        (viewport_rect.center().y - 210.0).max(24.0),
    );
    egui::Window::new(title)
        .id(egui::Id::unique("lajufile_progress_dialog"))
        .collapsible(true)
        .movable(true)
        .resizable(false)
        .default_width(600.0)
        .default_pos(initial_pos)
        .frame(frame)
        .show(ctx, |ui| {
            ui.set_width(580.0);
            ui.spacing_mut().item_spacing = Vec2::ZERO;
            egui::Frame::new()
                .inner_margin(Margin::symmetric(20, 16))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        super::icons::draw_file_icon(
                            ui,
                            &download.file_name,
                            download.category,
                            download.kind == "video",
                            38.0,
                        );
                        ui.add_space(10.0);
                        ui.vertical(|ui| {
                            ui.label(
                                RichText::new(&download.file_name)
                                    .size(13.5)
                                    .strong()
                                    .color(sp.text),
                            );
                            ui.add_space(2.0);
                            ui.label(RichText::new(&download.url).size(10.5).color(sp.muted));
                        });
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let pct_color = if download.status == DownloadStatus::Done {
                                sp.green
                            } else {
                                sp.accent
                            };
                            badge_pill(ui, &format!("{:.1}%", fraction * 100.0), pct_color, &sp);
                        });
                    });
                    ui.add_space(10.0);

                    let (status_text, status_color) = match download.status {
                        DownloadStatus::Downloading => ("Receiving data...", sp.accent),
                        DownloadStatus::Queued => ("Queued", sp.amber),
                        DownloadStatus::Paused => ("Paused", sp.muted),
                        DownloadStatus::Done => ("Completed", sp.green),
                        DownloadStatus::Error => {
                            (download.error.as_deref().unwrap_or("Error"), sp.red)
                        }
                    };
                    let eta = eta_seconds(download.size, download.downloaded, speed.unwrap_or(0));

                    settings_card(ui, &sp, |ui| {
                        egui::Grid::new("lajufile_progress_meta")
                            .num_columns(2)
                            .spacing([24.0, 5.0])
                            .show(ui, |ui| {
                                ui.label(RichText::new("Status").size(11.5).color(sp.muted));
                                ui.label(
                                    RichText::new(status_text)
                                        .size(11.5)
                                        .strong()
                                        .color(status_color),
                                );
                                ui.end_row();

                                ui.label(RichText::new("File size").size(11.5).color(sp.muted));
                                ui.label(
                                    RichText::new(
                                        download
                                            .size
                                            .map(format_size)
                                            .unwrap_or_else(|| "(unknown)".to_string()),
                                    )
                                    .size(11.5)
                                    .color(sp.text),
                                );
                                ui.end_row();

                                ui.label(RichText::new("Downloaded").size(11.5).color(sp.muted));
                                ui.label(
                                    RichText::new(format!(
                                        "{} ({:.1}%)",
                                        format_size(download.downloaded),
                                        fraction * 100.0
                                    ))
                                    .size(11.5)
                                    .color(sp.text),
                                );
                                ui.end_row();

                                ui.label(RichText::new("Transfer rate").size(11.5).color(sp.muted));
                                ui.label(
                                    RichText::new(format_speed(speed)).size(11.5).color(sp.text),
                                );
                                ui.end_row();

                                ui.label(RichText::new("Time left").size(11.5).color(sp.muted));
                                ui.label(RichText::new(format_eta(eta)).size(11.5).color(sp.text));
                                ui.end_row();

                                ui.label(
                                    RichText::new("Resume capability")
                                        .size(11.5)
                                        .color(sp.muted),
                                );
                                ui.label(
                                    RichText::new(if download.ranged { "Yes" } else { "No" })
                                        .size(11.5)
                                        .color(sp.text),
                                );
                                ui.end_row();
                            });

                        ui.add_space(10.0);
                        ui.add(
                            egui::ProgressBar::new(fraction)
                                .desired_width(ui.available_width())
                                .fill(if download.status == DownloadStatus::Done {
                                    sp.green
                                } else {
                                    sp.accent
                                })
                                .show_percentage(),
                        );

                        if speed_history.len() >= 2 {
                            ui.add_space(10.0);
                            draw_speed_sparkline(ui, speed_history, &sp);
                        }
                    });

                    if *show_details && !segments.is_empty() {
                        ui.add_space(10.0);
                        settings_card(ui, &sp, |ui| {
                            card_kicker(ui, "START POSITIONS & PROGRESS BY CONNECTIONS", &sp);
                            ui.add_space(6.0);
                            draw_segment_map(ui, download, segments, &sp);
                            ui.add_space(8.0);
                            egui::ScrollArea::vertical()
                                .max_height(132.0)
                                .show(ui, |ui| {
                                    egui::Grid::new("lajufile_segment_grid")
                                        .num_columns(4)
                                        .striped(true)
                                        .spacing([14.0, 4.0])
                                        .show(ui, |ui| {
                                            ui.label(
                                                RichText::new("N.")
                                                    .size(11.0)
                                                    .strong()
                                                    .color(sp.muted),
                                            );
                                            ui.label(
                                                RichText::new("Downloaded")
                                                    .size(11.0)
                                                    .strong()
                                                    .color(sp.muted),
                                            );
                                            ui.label(
                                                RichText::new("Progress")
                                                    .size(11.0)
                                                    .strong()
                                                    .color(sp.muted),
                                            );
                                            ui.label(
                                                RichText::new("Info")
                                                    .size(11.0)
                                                    .strong()
                                                    .color(sp.muted),
                                            );
                                            ui.end_row();

                                            let seg_count = segments.len();
                                            for segment in segments {
                                                let span =
                                                    segment_span(download, segment, seg_count);
                                                let seg_frac =
                                                    if download.status == DownloadStatus::Done {
                                                        1.0
                                                    } else if span > 0 {
                                                        (segment.done as f32 / span as f32)
                                                            .clamp(0.0, 1.0)
                                                    } else {
                                                        0.0
                                                    };
                                                let info = if download.status
                                                    == DownloadStatus::Done
                                                    || (span > 0 && segment.done >= span)
                                                {
                                                    "Completed"
                                                } else if download.status
                                                    == DownloadStatus::Downloading
                                                {
                                                    "Receiving data..."
                                                } else if download.status == DownloadStatus::Paused
                                                {
                                                    "Paused"
                                                } else {
                                                    "Waiting"
                                                };
                                                ui.label(
                                                    RichText::new(format!("{}", segment.idx + 1))
                                                        .size(11.0)
                                                        .color(sp.text),
                                                );
                                                ui.label(
                                                    RichText::new(format_size(segment.done))
                                                        .size(11.0)
                                                        .color(sp.text),
                                                );
                                                ui.add(
                                                    egui::ProgressBar::new(seg_frac)
                                                        .desired_width(176.0)
                                                        .fill(sp.accent)
                                                        .text(format!("{:.0}%", seg_frac * 100.0)),
                                                );
                                                ui.label(
                                                    RichText::new(info).size(11.0).color(sp.muted),
                                                );
                                                ui.end_row();
                                            }
                                        });
                                });
                        });
                    }

                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        let toggle_label = if *show_details {
                            "<< Hide details"
                        } else {
                            "Show details >>"
                        };
                        if figma_button(ui, toggle_label, 120.0, 30.0, false, &sp) {
                            *show_details = !*show_details;
                        }
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if figma_button(ui, "Close", 72.0, 30.0, false, &sp) {
                                keep_open = false;
                            }
                            ui.add_space(6.0);
                            if figma_button(
                                ui,
                                crate::i18n::t_ctx(ctx, "Hide").as_ref(),
                                92.0,
                                30.0,
                                false,
                                &sp,
                            ) {
                                action = Some(Action::HideProgress(download.id));
                                keep_open = false;
                            }
                            ui.add_space(6.0);
                            match download.status {
                                DownloadStatus::Downloading | DownloadStatus::Queued => {
                                    if figma_button(ui, "Pause", 76.0, 30.0, false, &sp) {
                                        action = Some(Action::Pause(download.id));
                                    }
                                }
                                DownloadStatus::Paused | DownloadStatus::Error => {
                                    if figma_button(ui, "Resume", 82.0, 30.0, true, &sp) {
                                        action = Some(Action::Resume(download.id));
                                    }
                                }
                                DownloadStatus::Done => {
                                    if figma_button(ui, "Open", 76.0, 30.0, true, &sp) {
                                        action = Some(Action::Open(download.id));
                                        keep_open = false;
                                    }
                                }
                            }
                            ui.add_space(6.0);
                            if figma_button(ui, "Properties", 86.0, 30.0, false, &sp) {
                                action = Some(Action::OpenProperties(download.id));
                            }
                        });
                    });
                });
        });

    (keep_open, action)
}

fn draw_speed_sparkline(ui: &mut Ui, history: &[u64], sp: &SettingsPalette) {
    let peak = history.iter().copied().max().unwrap_or(0);
    ui.horizontal(|ui| {
        ui.label(
            RichText::new("SPEED HISTORY")
                .size(9.5)
                .strong()
                .color(sp.faint),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(
                RichText::new(format!("Peak: {}", format_speed(Some(peak))))
                    .size(10.0)
                    .color(sp.muted),
            );
        });
    });
    ui.add_space(4.0);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 38.0), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 6.0, sp.field_bg);
    painter.rect_stroke(
        rect,
        6.0,
        Stroke::new(1.0, sp.border),
        egui::StrokeKind::Inside,
    );

    if history.len() >= 2 && peak > 0 {
        let inner = rect.shrink2(Vec2::new(6.0, 5.0));
        let count = history.len();
        let points: Vec<egui::Pos2> = history
            .iter()
            .enumerate()
            .map(|(i, &val)| {
                let x = inner.left() + (i as f32 / (count - 1) as f32) * inner.width();
                let y =
                    inner.bottom() - (val as f32 / peak as f32).clamp(0.0, 1.0) * inner.height();
                egui::pos2(x, y)
            })
            .collect();
        painter.line(points, Stroke::new(1.6, sp.accent));
    }
}

fn segment_span(download: &Download, segment: &Segment, count: usize) -> u64 {
    if count == 1 && segment.start == 0 && segment.end == 0 {
        download.size.unwrap_or(0)
    } else {
        segment.end.saturating_sub(segment.start).saturating_add(1)
    }
}

fn draw_segment_map(ui: &mut Ui, download: &Download, segments: &[Segment], sp: &SettingsPalette) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 16.0), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 4.0, sp.field_bg);
    let total = download
        .size
        .unwrap_or_else(|| {
            segments
                .iter()
                .map(|s| s.end.saturating_add(1))
                .max()
                .unwrap_or(1)
        })
        .max(1);

    let seg_count = segments.len();
    for (idx, segment) in segments.iter().enumerate() {
        let span = segment_span(download, segment, seg_count);
        let seg_x0 = rect.left() + (segment.start as f32 / total as f32) * rect.width();
        let seg_x1 = rect.left()
            + ((segment.start.saturating_add(span)) as f32 / total as f32).min(1.0) * rect.width();
        let fill_frac = if download.status == DownloadStatus::Done {
            1.0
        } else if span > 0 {
            (segment.done as f32 / span as f32).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let fill_x1 = seg_x0 + (seg_x1 - seg_x0) * fill_frac;
        if fill_x1 > seg_x0 + 0.5 {
            painter.rect_filled(
                Rect::from_min_max(
                    egui::pos2(seg_x0, rect.top() + 1.0),
                    egui::pos2(fill_x1, rect.bottom() - 1.0),
                ),
                1.0,
                if download.status == DownloadStatus::Done {
                    sp.green
                } else {
                    sp.accent
                },
            );
        }
        if idx > 0 {
            painter.line_segment(
                [
                    egui::pos2(seg_x0, rect.top()),
                    egui::pos2(seg_x0, rect.bottom()),
                ],
                Stroke::new(1.0, sp.red),
            );
        }
    }
    painter.rect_stroke(
        rect,
        4.0,
        Stroke::new(1.0, sp.border),
        egui::StrokeKind::Inside,
    );
}

pub fn delete_confirm_dialog(
    ctx: &egui::Context,
    downloads: &[&Download],
    delete_file: &mut bool,
    dark: bool,
) -> Option<bool> {
    let mut choice = None;
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
        .inner_margin(Margin::ZERO);
    let count = downloads.len();
    let heading = if count > 1 {
        format!("Remove {count} downloads?")
    } else {
        "Remove download?".to_string()
    };
    let prompt_text = if let [only] = downloads {
        format!(
            "Remove \"{}\" from your LajuFile download list?",
            only.file_name
        )
    } else {
        format!("Remove {count} selected downloads from your LajuFile list?")
    };
    let path_note = if let [only] = downloads {
        only.save_path.display().to_string()
    } else {
        "Removes finished files and temporary parts for all selected items.".to_string()
    };

    egui::Window::new("Remove download")
        .id(egui::Id::unique("lajufile_delete_confirm_modal"))
        .title_bar(false)
        .collapsible(false)
        .resizable(false)
        .frame(frame)
        .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
        .show(ctx, |ui| {
            ui.set_width(430.0);
            ui.spacing_mut().item_spacing = Vec2::ZERO;
            egui::Frame::new()
                .inner_margin(Margin::symmetric(20, 18))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new(heading).size(15.5).strong().color(sp.text));
                    ui.add_space(6.0);
                    ui.label(RichText::new(prompt_text).size(12.0).color(sp.muted));
                    ui.add_space(12.0);
                    settings_card(ui, &sp, |ui| {
                        ui.horizontal(|ui| {
                            ui.checkbox(delete_file, "");
                            ui.add_space(6.0);
                            ui.vertical(|ui| {
                                ui.label(
                                    RichText::new(if count > 1 {
                                        "Also delete files from disk"
                                    } else {
                                        "Also delete file from disk"
                                    })
                                    .size(12.0)
                                    .strong()
                                    .color(if *delete_file { sp.red } else { sp.text }),
                                );
                                ui.add_space(2.0);
                                ui.label(RichText::new(path_note).size(10.5).color(sp.faint));
                            });
                        });
                    });
                    ui.add_space(14.0);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let del_rect = ui
                            .allocate_exact_size(Vec2::new(92.0, 32.0), Sense::click())
                            .1
                            .on_hover_cursor(egui::CursorIcon::PointingHand);
                        let fill = if del_rect.hovered() {
                            lerp_color(sp.red, Color32::WHITE, 0.12)
                        } else {
                            sp.red
                        };
                        ui.painter().rect_filled(del_rect.rect, 8.0, fill);
                        ui.painter().text(
                            del_rect.rect.center(),
                            Align2::CENTER_CENTER,
                            "Remove",
                            FontId::proportional(11.5),
                            Color32::WHITE,
                        );
                        if del_rect.clicked() {
                            choice = Some(true);
                        }
                        ui.add_space(8.0);
                        if figma_button(ui, "Cancel", 80.0, 32.0, false, &sp) {
                            choice = Some(false);
                        }
                    });
                });
        });

    choice
}

#[derive(Debug, Clone)]
pub struct PropertiesDraft {
    pub download_id: i64,
    pub url: String,
    pub file_name: String,
    pub folder: String,
    pub checksum_input: String,
}

impl PropertiesDraft {
    pub fn for_download(download: &Download) -> Self {
        Self {
            download_id: download.id,
            url: download.url.clone(),
            file_name: download.file_name.clone(),
            folder: download
                .save_path
                .parent()
                .map(|p| p.display().to_string())
                .unwrap_or_default(),
            checksum_input: String::new(),
        }
    }
}

pub fn properties_dialog(
    ctx: &egui::Context,
    download: &Download,
    draft: &mut PropertiesDraft,
    checksum_busy: bool,
    checksum_result: Option<&Result<String, String>>,
    dark: bool,
) -> (bool, Option<Action>) {
    let mut keep_open = true;
    let mut action = None;
    let sp = SettingsPalette::from_dark(dark);
    let now = now_millis();
    let style = ctx.style_of(if dark {
        egui::Theme::Dark
    } else {
        egui::Theme::Light
    });
    let frame = egui::Frame::window(&style)
        .fill(sp.dialog_bg)
        .stroke(Stroke::new(1.0, sp.border))
        .corner_radius(14)
        .inner_margin(Margin::ZERO);

    egui::Window::new("Properties")
        .id(egui::Id::unique("lajufile_properties_modal"))
        .title_bar(false)
        .collapsible(false)
        .resizable(false)
        .frame(frame)
        .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
        .show(ctx, |ui| {
            ui.set_width(560.0);
            ui.spacing_mut().item_spacing = Vec2::ZERO;
            egui::Frame::new()
                .inner_margin(Margin::symmetric(22, 18))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        super::icons::draw_file_icon(
                            ui,
                            &download.file_name,
                            download.category,
                            download.kind == "video",
                            40.0,
                        );
                        ui.add_space(12.0);
                        ui.vertical(|ui| {
                            ui.label(
                                RichText::new(&download.file_name)
                                    .size(14.5)
                                    .strong()
                                    .color(sp.text),
                            );
                            ui.add_space(2.0);
                            ui.label(
                                RichText::new(format!(
                                    "{} · {} · {} connections",
                                    download.category.as_str(),
                                    download.kind,
                                    download.connections
                                ))
                                .size(11.0)
                                .color(sp.muted),
                            );
                        });
                        ui.with_layout(Layout::right_to_left(Align::TOP), |ui| {
                            if figma_button(ui, "×", 28.0, 28.0, false, &sp) {
                                keep_open = false;
                            }
                        });
                    });

                    ui.add_space(12.0);
                    egui::ScrollArea::vertical()
                        .max_height(460.0)
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            settings_card(ui, &sp, |ui| {
                                let size_text = download
                                    .size
                                    .map(|s| {
                                        format!(
                                            "{} ({} bytes)",
                                            format_size(s),
                                            format_bytes_commas(s)
                                        )
                                    })
                                    .unwrap_or_else(|| "(unknown)".to_string());
                                let pct = match (download.size, download.status) {
                                    (_, DownloadStatus::Done) => 100.0,
                                    (Some(s), _) if s > 0 => (download.downloaded as f32 / s as f32
                                        * 100.0)
                                        .clamp(0.0, 100.0),
                                    _ => 0.0,
                                };
                                egui::Grid::new("lajufile_props_grid")
                                    .num_columns(2)
                                    .spacing([20.0, 5.0])
                                    .show(ui, |ui| {
                                        ui.label(
                                            RichText::new("Status").size(11.5).color(sp.muted),
                                        );
                                        ui.label(
                                            RichText::new(download.status.as_str())
                                                .size(11.5)
                                                .strong()
                                                .color(match download.status {
                                                    DownloadStatus::Done => sp.green,
                                                    DownloadStatus::Error => sp.red,
                                                    DownloadStatus::Downloading => sp.accent,
                                                    _ => sp.text,
                                                }),
                                        );
                                        ui.end_row();

                                        ui.label(
                                            RichText::new("File size").size(11.5).color(sp.muted),
                                        );
                                        ui.label(
                                            RichText::new(size_text).size(11.5).color(sp.text),
                                        );
                                        ui.end_row();

                                        ui.label(
                                            RichText::new("Downloaded").size(11.5).color(sp.muted),
                                        );
                                        ui.label(
                                            RichText::new(format!(
                                                "{} ({:.1}%)",
                                                format_size(download.downloaded),
                                                pct
                                            ))
                                            .size(11.5)
                                            .color(sp.text),
                                        );
                                        ui.end_row();

                                        ui.label(
                                            RichText::new("Resume support")
                                                .size(11.5)
                                                .color(sp.muted),
                                        );
                                        ui.label(
                                            RichText::new(if download.ranged {
                                                "Yes"
                                            } else {
                                                "No"
                                            })
                                            .size(11.5)
                                            .color(sp.text),
                                        );
                                        ui.end_row();

                                        ui.label(
                                            RichText::new("Added").size(11.5).color(sp.muted),
                                        );
                                        ui.label(
                                            RichText::new(format!(
                                                "{} ({})",
                                                format_utc_timestamp(download.created_at),
                                                format_relative_time(now, download.created_at)
                                            ))
                                            .size(11.5)
                                            .color(sp.text),
                                        );
                                        ui.end_row();

                                        if let Some(done_at) = download.completed_at {
                                            ui.label(
                                                RichText::new("Completed")
                                                    .size(11.5)
                                                    .color(sp.muted),
                                            );
                                            ui.label(
                                                RichText::new(format!(
                                                    "{} ({})",
                                                    format_utc_timestamp(done_at),
                                                    format_relative_time(now, done_at)
                                                ))
                                                .size(11.5)
                                                .color(sp.text),
                                            );
                                            ui.end_row();
                                        }
                                    });

                                if let Some(error) = download.error.as_deref() {
                                    ui.add_space(8.0);
                                    draw_h_divider(ui, sp.border);
                                    ui.add_space(8.0);
                                    ui.label(
                                        RichText::new("Last error")
                                            .size(11.0)
                                            .strong()
                                            .color(sp.red),
                                    );
                                    ui.add_space(4.0);
                                    ui.label(RichText::new(error).size(11.0).color(sp.red));
                                }
                            });

                            ui.add_space(10.0);
                            settings_card(ui, &sp, |ui| {
                                card_kicker(ui, "SAVED LOCATION & MOVE / RENAME", &sp);
                                ui.add_space(6.0);
                                if download.status == DownloadStatus::Done {
                                    ui.horizontal(|ui| {
                                        let w = (ui.available_width() - 168.0).max(120.0);
                                        egui::Frame::new()
                                            .fill(sp.field_bg)
                                            .stroke(Stroke::new(1.0, sp.border))
                                            .corner_radius(8)
                                            .inner_margin(Margin::symmetric(10, 6))
                                            .show(ui, |ui| {
                                                ui.set_width(w);
                                                ui.add(
                                                    egui::TextEdit::singleline(
                                                        &mut draft.file_name,
                                                    )
                                                    .frame(egui::Frame::NONE)
                                                    .hint_text("File name"),
                                                );
                                            });
                                        ui.add_space(6.0);
                                        if figma_button(ui, "Copy path", 76.0, 30.0, false, &sp) {
                                            action =
                                                Some(Action::CopyPath(download.save_path.clone()));
                                        }
                                        ui.add_space(6.0);
                                        if figma_button(ui, "Open folder", 80.0, 30.0, false, &sp)
                                        {
                                            action = Some(Action::OpenFolder(download.id));
                                        }
                                    });
                                    ui.add_space(6.0);
                                    ui.horizontal(|ui| {
                                        let w = (ui.available_width() - 168.0).max(120.0);
                                        egui::Frame::new()
                                            .fill(sp.field_bg)
                                            .stroke(Stroke::new(1.0, sp.border))
                                            .corner_radius(8)
                                            .inner_margin(Margin::symmetric(10, 6))
                                            .show(ui, |ui| {
                                                ui.set_width(w);
                                                ui.add(
                                                    egui::TextEdit::singleline(&mut draft.folder)
                                                        .frame(egui::Frame::NONE)
                                                        .hint_text("Destination folder"),
                                                );
                                            });
                                        ui.add_space(6.0);
                                        if figma_button(ui, "Browse", 64.0, 30.0, false, &sp) {
                                            action = Some(Action::BrowseMoveDir(download.id));
                                        }
                                        ui.add_space(6.0);
                                        if figma_button(ui, "Apply move", 92.0, 30.0, true, &sp) {
                                            action = Some(Action::MoveRename {
                                                id: download.id,
                                                file_name: draft.file_name.clone(),
                                                dir: non_empty(draft.folder.clone())
                                                    .map(PathBuf::from),
                                            });
                                        }
                                    });
                                } else {
                                    let mut saved = download.save_path.display().to_string();
                                    ui.horizontal(|ui| {
                                        let w = (ui.available_width() - 168.0).max(120.0);
                                        egui::Frame::new()
                                            .fill(sp.field_bg)
                                            .stroke(Stroke::new(1.0, sp.border))
                                            .corner_radius(8)
                                            .inner_margin(Margin::symmetric(10, 6))
                                            .show(ui, |ui| {
                                                ui.set_width(w);
                                                ui.add(
                                                    egui::TextEdit::singleline(&mut saved)
                                                        .frame(egui::Frame::NONE)
                                                        .interactive(false),
                                                );
                                            });
                                        ui.add_space(6.0);
                                        if figma_button(ui, "Copy", 64.0, 30.0, false, &sp) {
                                            action =
                                                Some(Action::CopyPath(download.save_path.clone()));
                                        }
                                        ui.add_space(6.0);
                                        if figma_button(ui, "Open folder", 92.0, 30.0, false, &sp)
                                        {
                                            action = Some(Action::OpenFolder(download.id));
                                        }
                                    });
                                }

                                ui.add_space(10.0);
                                let can_edit_url = matches!(
                                    download.status,
                                    DownloadStatus::Paused | DownloadStatus::Error
                                );
                                card_kicker(
                                    ui,
                                    if can_edit_url {
                                        "SOURCE URL (EDIT TO REFRESH EXPIRED LINK)"
                                    } else {
                                        "SOURCE URL"
                                    },
                                    &sp,
                                );
                                ui.add_space(6.0);
                                ui.horizontal(|ui| {
                                    let btn_w = if can_edit_url { 198.0 } else { 94.0 };
                                    let w = (ui.available_width() - btn_w).max(120.0);
                                    egui::Frame::new()
                                        .fill(sp.field_bg)
                                        .stroke(Stroke::new(1.0, sp.border))
                                        .corner_radius(8)
                                        .inner_margin(Margin::symmetric(10, 6))
                                        .show(ui, |ui| {
                                            ui.set_width(w);
                                            ui.add(
                                                egui::TextEdit::singleline(&mut draft.url)
                                                    .frame(egui::Frame::NONE)
                                                    .interactive(can_edit_url),
                                            );
                                        });
                                    ui.add_space(6.0);
                                    if can_edit_url {
                                        if figma_button(ui, "Copy URL", 74.0, 30.0, false, &sp) {
                                            action = Some(Action::CopyUrl(draft.url.clone()));
                                        }
                                        ui.add_space(6.0);
                                        if figma_button(
                                            ui,
                                            "Update & Resume",
                                            112.0,
                                            30.0,
                                            true,
                                            &sp,
                                        ) {
                                            action = Some(Action::UpdateUrlAndResume {
                                                id: download.id,
                                                url: draft.url.clone(),
                                            });
                                            keep_open = false;
                                        }
                                    } else if figma_button(
                                        ui, "Copy URL", 86.0, 30.0, false, &sp,
                                    ) {
                                        action = Some(Action::CopyUrl(download.url.clone()));
                                    }
                                });

                                if let Some(referrer) = download.referrer.as_deref() {
                                    ui.add_space(8.0);
                                    ui.label(
                                        RichText::new(format!("Referrer: {referrer}"))
                                            .size(10.5)
                                            .color(sp.faint),
                                    );
                                }
                            });

                            if download.status == DownloadStatus::Done {
                                ui.add_space(10.0);
                                settings_card(ui, &sp, |ui| {
                                    card_kicker(ui, "VERIFY CHECKSUM (SHA-256)", &sp);
                                    ui.add_space(6.0);
                                    ui.horizontal(|ui| {
                                        let w = (ui.available_width() - 104.0).max(140.0);
                                        egui::Frame::new()
                                            .fill(sp.field_bg)
                                            .stroke(Stroke::new(1.0, sp.border))
                                            .corner_radius(8)
                                            .inner_margin(Margin::symmetric(10, 6))
                                            .show(ui, |ui| {
                                                ui.set_width(w);
                                                ui.add(
                                                    egui::TextEdit::singleline(
                                                        &mut draft.checksum_input,
                                                    )
                                                    .frame(egui::Frame::NONE)
                                                    .hint_text(
                                                        "Paste expected SHA-256 (optional)",
                                                    ),
                                                );
                                            });
                                        ui.add_space(6.0);
                                        let btn_label = if checksum_busy {
                                            "Checking..."
                                        } else if draft.checksum_input.trim().is_empty() {
                                            "Compute"
                                        } else {
                                            "Verify"
                                        };
                                        if figma_button(ui, btn_label, 96.0, 30.0, false, &sp)
                                            && !checksum_busy
                                        {
                                            action = Some(Action::VerifyChecksum(download.id));
                                        }
                                    });
                                    if checksum_busy {
                                        ui.add_space(6.0);
                                        ui.label(
                                            RichText::new("Computing SHA-256 checksum on disk...")
                                                .size(11.0)
                                                .color(sp.muted),
                                        );
                                    } else if let Some(result) = checksum_result {
                                        ui.add_space(6.0);
                                        match result {
                                            Ok(actual) => {
                                                let has_expected =
                                                    !draft.checksum_input.trim().is_empty();
                                                let matched =
                                                    checksums_match(&draft.checksum_input, actual);
                                                if has_expected {
                                                    ui.label(
                                                        RichText::new(if matched {
                                                            "Matched: file SHA-256 equals expected checksum"
                                                        } else {
                                                            "Mismatch: computed SHA-256 does NOT match expected"
                                                        })
                                                        .size(11.0)
                                                        .strong()
                                                        .color(if matched {
                                                            sp.green
                                                        } else {
                                                            sp.red
                                                        }),
                                                    );
                                                    ui.add_space(4.0);
                                                }
                                                ui.horizontal(|ui| {
                                                    ui.label(
                                                        RichText::new(actual)
                                                            .size(10.0)
                                                            .color(sp.muted),
                                                    );
                                                    ui.with_layout(
                                                        Layout::right_to_left(Align::Center),
                                                        |ui| {
                                                            if figma_button(
                                                                ui, "Copy", 52.0, 22.0, false, &sp,
                                                            ) {
                                                                action = Some(
                                                                    Action::CopyChecksum(
                                                                        actual.clone(),
                                                                    ),
                                                                );
                                                            }
                                                        },
                                                    );
                                                });
                                            }
                                            Err(err) => {
                                                ui.label(
                                                    RichText::new(format!(
                                                        "Checksum failed: {err}"
                                                    ))
                                                    .size(11.0)
                                                    .color(sp.red),
                                                );
                                            }
                                        }
                                    }
                                });
                            }
                        });

                    ui.add_space(14.0);
                    ui.horizontal(|ui| {
                        if !matches!(
                            download.status,
                            DownloadStatus::Downloading | DownloadStatus::Queued
                        ) && figma_button(ui, "Redownload", 98.0, 32.0, false, &sp)
                        {
                            action = Some(Action::Redownload(download.id));
                            keep_open = false;
                        }
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if figma_button(ui, "Close", 78.0, 32.0, false, &sp) {
                                keep_open = false;
                            }
                            if download.status == DownloadStatus::Done {
                                ui.add_space(8.0);
                                if figma_button(ui, "Open file", 88.0, 32.0, true, &sp) {
                                    action = Some(Action::Open(download.id));
                                    keep_open = false;
                                }
                            }
                        });
                    });
                });
        });

    (keep_open, action)
}

fn format_bytes_commas(bytes: u64) -> String {
    let s = bytes.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (idx, ch) in s.chars().enumerate() {
        if idx > 0 && (s.len() - idx) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

fn format_utc_timestamp(millis: i64) -> String {
    let secs = millis.div_euclid(1000);
    let days = secs.div_euclid(86_400);
    let day_secs = secs.rem_euclid(86_400);
    let hour = day_secs / 3_600;
    let minute = (day_secs % 3_600) / 60;
    let second = day_secs % 60;

    // Civil-from-days algorithm (epoch 1970-01-01).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = y + i64::from(month <= 2);

    format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02} UTC")
}

pub fn complete_dialog(
    ctx: &egui::Context,
    download: &Download,
    dark: bool,
) -> (bool, Option<Action>) {
    let mut keep_open = true;
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
        .inner_margin(Margin::ZERO);

    egui::Window::new("Download complete")
        .id(egui::Id::unique("lajufile_complete_dialog"))
        .title_bar(false)
        .collapsible(false)
        .resizable(false)
        .frame(frame)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .show(ctx, |ui| {
            ui.set_width(540.0);
            ui.spacing_mut().item_spacing = Vec2::ZERO;

            if modal_header(
                ui,
                &crate::i18n::t_ctx(ctx, "Download complete"),
                "Your file has finished downloading and passed assembly checks.",
                &sp,
            ) {
                keep_open = false;
            }
            draw_h_divider(ui, sp.border);

            egui::Frame::new()
                .fill(sp.dialog_bg)
                .inner_margin(Margin::symmetric(24, 18))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    settings_card(ui, &sp, |ui| {
                        ui.horizontal(|ui| {
                            super::icons::draw_file_icon(
                                ui,
                                &download.file_name,
                                download.category,
                                download.kind == "video",
                                46.0,
                            );
                            ui.add_space(12.0);
                            ui.vertical(|ui| {
                                ui.label(
                                    RichText::new(&download.file_name)
                                        .size(14.5)
                                        .strong()
                                        .color(sp.text),
                                );
                                ui.add_space(4.0);
                                ui.horizontal(|ui| {
                                    badge_pill(
                                        ui,
                                        &crate::i18n::t_ctx(ctx, "Completed"),
                                        sp.green,
                                        &sp,
                                    );
                                    if let Some(size) = download.size {
                                        let lang = crate::i18n::ctx_lang(ctx);
                                        ui.add_space(6.0);
                                        badge_pill(
                                            ui,
                                            &crate::i18n::format_size_locale(size, &lang),
                                            sp.accent,
                                            &sp,
                                        );
                                    }
                                });
                            });
                        });
                    });

                    ui.add_space(12.0);
                    settings_card(ui, &sp, |ui| {
                        card_kicker(ui, "SAVED LOCATION", &sp);
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            let mut saved = download.save_path.display().to_string();
                            egui::Frame::new()
                                .fill(sp.field_bg)
                                .stroke(Stroke::new(1.0, sp.border))
                                .corner_radius(8)
                                .inner_margin(Margin::symmetric(10, 6))
                                .show(ui, |ui| {
                                    ui.set_width((ui.available_width() - 96.0).max(160.0));
                                    ui.add_sized(
                                        [ui.available_width(), 20.0],
                                        egui::TextEdit::singleline(&mut saved)
                                            .frame(egui::Frame::NONE)
                                            .background_color(Color32::TRANSPARENT)
                                            .interactive(false),
                                    );
                                });
                            ui.add_space(8.0);
                            if figma_button(ui, "Copy path", 76.0, 32.0, false, &sp) {
                                ui.ctx().copy_text(download.save_path.display().to_string());
                            }
                        });
                    });
                });

            draw_h_divider(ui, sp.border);
            egui::Frame::new()
                .fill(sp.dialog_bg)
                .inner_margin(Margin::symmetric(24, 12))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if figma_button(
                            ui,
                            &crate::i18n::t_ctx(ctx, "Close"),
                            86.0,
                            34.0,
                            false,
                            &sp,
                        ) {
                            keep_open = false;
                        }
                        ui.add_space(8.0);
                        if figma_button(
                            ui,
                            &crate::i18n::t_ctx(ctx, "Open folder"),
                            116.0,
                            34.0,
                            false,
                            &sp,
                        ) {
                            action = Some(Action::OpenFolder(download.id));
                            keep_open = false;
                        }
                        ui.add_space(8.0);
                        if figma_button(
                            ui,
                            &crate::i18n::t_ctx(ctx, "Open with..."),
                            120.0,
                            34.0,
                            false,
                            &sp,
                        ) {
                            action = Some(Action::OpenWith(download.id));
                            keep_open = false;
                        }
                        ui.add_space(8.0);
                        if figma_button(ui, &crate::i18n::t_ctx(ctx, "Open"), 92.0, 34.0, true, &sp)
                        {
                            action = Some(Action::Open(download.id));
                            keep_open = false;
                        }
                    });
                });
        });
    (keep_open, action)
}

/// Returns a normalized URL if `text` is a single-line downloadable link matching `settings` rules.
pub fn clipboard_download_url_with_rules(text: &str, settings: &Settings) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() || trimmed.contains(['\n', '\r', ' ', '\t']) {
        return None;
    }
    let parsed = url::Url::parse(trimmed).ok()?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return None;
    }
    let host = parsed.host_str()?.to_ascii_lowercase();
    if matches!(host.as_str(), "localhost" | "127.0.0.1") || settings.is_domain_excluded(&host) {
        return None;
    }
    if matches!(
        host.as_str(),
        "youtube.com" | "www.youtube.com" | "m.youtube.com"
    ) && parsed.path() == "/watch"
        && parsed.query_pairs().any(|(k, _)| k == "v")
    {
        return Some(parsed.to_string());
    }
    if host == "youtu.be" && parsed.path().len() > 1 {
        return Some(parsed.to_string());
    }
    let last_segment = parsed.path_segments()?.next_back()?;
    let ext = std::path::Path::new(last_segment)
        .extension()
        .and_then(|ext| ext.to_str())?;
    settings
        .is_extension_captured(ext)
        .then(|| parsed.to_string())
}

/// Returns a normalized URL if `text` is a single-line downloadable link or YouTube watch link.
pub fn clipboard_download_url(text: &str) -> Option<String> {
    clipboard_download_url_with_rules(text, &Settings::default())
}

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
                        if ui.button("Load file").clicked()
                            && let Ok(contents) = std::fs::read_to_string(draft.import_path.trim())
                        {
                            if draft.urls_text.trim().is_empty() {
                                draft.urls_text = contents;
                            } else {
                                draft.urls_text.push('\n');
                                draft.urls_text.push_str(&contents);
                            }
                        }
                    });
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

#[cfg(test)]
mod tests {
    use super::*;

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
    fn formats_bytes_with_commas() {
        assert_eq!(format_bytes_commas(0), "0");
        assert_eq!(format_bytes_commas(999), "999");
        assert_eq!(format_bytes_commas(1_234_567), "1,234,567");
    }

    #[test]
    fn formats_utc_timestamp() {
        assert_eq!(format_utc_timestamp(0), "1970-01-01 00:00:00 UTC");
        assert_eq!(
            format_utc_timestamp(1_700_000_000_000),
            "2023-11-14 22:13:20 UTC"
        );
    }
}
