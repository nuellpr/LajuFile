//! Main window: menu, toolbar, sidebar, download table, and status bar.

mod chrome;
mod table;

use std::time::Duration;

use egui::{Align, Align2, Color32, FontId, Layout, Margin, Rect, RichText, Sense, Stroke, Vec2};
use lajufile_core::settings::Theme;
use lajufile_core::store::{Download, DownloadStatus};

use super::{Action, Filter, SortOrder, matches_search};
use crate::app::LajuFileApp;
use crate::bridge::BridgeState;
use chrome::{draw_menu_bar, draw_sidebar, draw_toolbar};
use table::{draw_table, progress_fraction};

pub(super) struct Palette {
    pub(super) window: Color32,
    pub(super) bar: Color32,
    pub(super) sidebar: Color32,
    pub(super) surface: Color32,
    pub(super) surface_alt: Color32,
    pub(super) border: Color32,
    pub(super) text: Color32,
    pub(super) muted: Color32,
    pub(super) faint: Color32,
    pub(super) accent: Color32,
    pub(super) selected: Color32,
    pub(super) sky: Color32,
    pub(super) blue: Color32,
    pub(super) indigo: Color32,
    pub(super) emerald: Color32,
    pub(super) emerald_bright: Color32,
    pub(super) amber_bright: Color32,
    pub(super) red_bright: Color32,
    pub(super) track: Color32,
    pub(super) track_ring: Color32,
    pub(super) speed_bg: Color32,
}

impl Palette {
    fn for_theme(theme: &Theme) -> Self {
        match theme {
            Theme::Light => Self {
                window: Color32::from_rgb(0xF7, 0xF8, 0xFA),
                bar: Color32::WHITE,
                sidebar: Color32::from_rgb(0xF2, 0xF5, 0xF9),
                surface: Color32::WHITE,
                surface_alt: Color32::from_rgb(0xF8, 0xFA, 0xFC),
                border: Color32::from_rgb(0xE1, 0xE7, 0xEF),
                text: Color32::from_rgb(0x18, 0x21, 0x2B),
                muted: Color32::from_rgb(0x66, 0x70, 0x85),
                faint: Color32::from_rgb(0x98, 0xA2, 0xB3),
                accent: Color32::from_rgb(0x0B, 0x6B, 0xCB),
                selected: Color32::from_rgb(0xED, 0xF4, 0xFF),
                sky: Color32::from_rgb(0x02, 0x84, 0xC7),
                blue: Color32::from_rgb(0x25, 0x63, 0xEB),
                indigo: Color32::from_rgb(0x4F, 0x46, 0xE5),
                emerald: Color32::from_rgb(0x05, 0x96, 0x69),
                emerald_bright: Color32::from_rgb(0x05, 0x96, 0x69),
                amber_bright: Color32::from_rgb(0xD9, 0x77, 0x06),
                red_bright: Color32::from_rgb(0xDC, 0x26, 0x26),
                track: Color32::from_rgb(0xE2, 0xE8, 0xF0),
                track_ring: Color32::from_rgb(0xCB, 0xD5, 0xE1),
                speed_bg: Color32::from_rgb(0xF8, 0xFA, 0xFC),
            },
            Theme::Dark => Self {
                window: Color32::from_rgb(0x0E, 0x13, 0x19),
                bar: Color32::from_rgb(0x14, 0x1A, 0x21),
                sidebar: Color32::from_rgb(0x11, 0x17, 0x1E),
                surface: Color32::from_rgb(0x16, 0x1D, 0x26),
                surface_alt: Color32::from_rgb(0x1B, 0x23, 0x2D),
                border: Color32::from_rgb(0x26, 0x30, 0x3B),
                text: Color32::from_rgb(0xE7, 0xED, 0xF5),
                muted: Color32::from_rgb(0x94, 0xA3, 0xB8),
                faint: Color32::from_rgb(0x64, 0x74, 0x8B),
                accent: Color32::from_rgb(0x4C, 0x9A, 0xFF),
                selected: Color32::from_rgb(0x13, 0x26, 0x3D),
                sky: Color32::from_rgb(0x38, 0xBD, 0xF8),
                blue: Color32::from_rgb(0x25, 0x63, 0xEB),
                indigo: Color32::from_rgb(0x63, 0x66, 0xF1),
                emerald: Color32::from_rgb(0x10, 0xB9, 0x81),
                emerald_bright: Color32::from_rgb(0x34, 0xD3, 0x99),
                amber_bright: Color32::from_rgb(0xFB, 0xBF, 0x24),
                red_bright: Color32::from_rgb(0xF8, 0x71, 0x71),
                track: Color32::from_rgb(0x1E, 0x29, 0x3B),
                track_ring: Color32::from_rgb(0x33, 0x41, 0x55),
                speed_bg: Color32::from_rgb(0x0F, 0x17, 0x2A),
            },
        }
    }
}

pub fn draw(app: &mut LajuFileApp, ui: &mut egui::Ui) -> Vec<Action> {
    let lang = crate::i18n::ctx_lang(ui.ctx());
    let theme = app
        .snapshot
        .as_ref()
        .map(|snapshot| snapshot.settings.theme.clone())
        .unwrap_or_else(|| app.theme.clone());
    let palette = Palette::for_theme(&theme);
    let snapshot = app.snapshot.clone();
    let downloads: &[Download] = snapshot
        .as_ref()
        .map(|snapshot| snapshot.downloads.as_slice())
        .unwrap_or(&[]);
    let empty_speeds = std::collections::HashMap::new();
    let speeds = snapshot
        .as_ref()
        .map(|snapshot| &snapshot.speeds)
        .unwrap_or(&empty_speeds);
    let mut actions = Vec::new();
    ui.painter().rect_filled(ui.max_rect(), 0.0, palette.window);
    ui.spacing_mut().item_spacing = Vec2::ZERO;

    let available_update = match &app.update_state {
        crate::ui::dialogs::UpdateUiState::Available(info) => Some(info.tag.clone()),
        _ => None,
    };
    draw_menu_bar(
        ui,
        &lang,
        &palette,
        available_update.as_deref(),
        &mut actions,
    );
    draw_toolbar(
        ui,
        app.selected,
        &app.selected_ids,
        downloads,
        &lang,
        &palette,
        &mut actions,
    );

    let speed_pill_clicked = egui::Panel::bottom("lajufile_status")
        .exact_size(32.0)
        .frame(
            egui::Frame::new()
                .fill(palette.bar)
                .stroke(Stroke::new(1.0, palette.border))
                .inner_margin(Margin::symmetric(12, 0)),
        )
        .show(ui, |ui| {
            draw_status_bar(ui, app, snapshot.as_deref(), &lang, &palette)
        })
        .inner;

    if app.show_speed_popover {
        draw_speed_popover(ui.ctx(), app, speed_pill_clicked, &palette, &mut actions);
    }

    let body_height = ui.available_height().max(100.0);
    let rtl = crate::i18n::is_rtl(&lang);
    ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
        if rtl {
            let content_w = (ui.available_width() - 232.0).max(200.0);
            ui.allocate_ui_with_layout(
                Vec2::new(content_w, body_height),
                Layout::top_down(Align::Min),
                |ui| {
                    ui.set_min_height(body_height);
                    draw_content(ui, app, downloads, speeds, &lang, &palette, &mut actions);
                },
            );
            ui.allocate_ui_with_layout(
                Vec2::new(232.0, body_height),
                Layout::top_down(Align::Min),
                |ui| {
                    ui.set_min_height(body_height);
                    draw_sidebar(ui, app, downloads, &palette);
                },
            );
        } else {
            ui.allocate_ui_with_layout(
                Vec2::new(232.0, body_height),
                Layout::top_down(Align::Min),
                |ui| {
                    ui.set_min_height(body_height);
                    draw_sidebar(ui, app, downloads, &palette);
                },
            );
            ui.allocate_ui_with_layout(
                Vec2::new(ui.available_width(), body_height),
                Layout::top_down(Align::Min),
                |ui| {
                    ui.set_min_height(body_height);
                    draw_content(ui, app, downloads, speeds, &lang, &palette, &mut actions);
                },
            );
        }
    });

    if let Some((text, at)) = &app.toast {
        if at.elapsed() < Duration::from_secs(4) {
            egui::Area::new(egui::Id::unique("lajufile_toast"))
                .anchor(Align2::RIGHT_BOTTOM, Vec2::new(-24.0, -48.0))
                .show(ui.ctx(), |ui| {
                    egui::Frame::new()
                        .fill(palette.text)
                        .corner_radius(6)
                        .inner_margin(Margin::symmetric(12, 8))
                        .show(ui, |ui| {
                            ui.label(RichText::new(text).size(12.0).color(palette.bar));
                        });
                });
        }
    }

    actions
}

pub(crate) fn visible_downloads<'a>(
    app: &LajuFileApp,
    downloads: &'a [Download],
) -> Vec<&'a Download> {
    let mut visible: Vec<&Download> = downloads
        .iter()
        .filter(|download| {
            app.filter.matches(download) && matches_search(download, &app.search_query)
        })
        .collect();
    app.sort_order.sort_downloads(&mut visible);
    visible
}

fn draw_content(
    ui: &mut egui::Ui,
    app: &mut LajuFileApp,
    downloads: &[Download],
    speeds: &std::collections::HashMap<i64, u64>,
    lang: &str,
    palette: &Palette,
    actions: &mut Vec<Action>,
) {
    let visible = visible_downloads(app, downloads);
    let mut selected = app.selected_ids.clone();
    if let Some(id) = app.selected {
        selected.insert(id);
    }
    let selected_count = selected.len();
    let is_dragging_file = ui.ctx().input(|input| !input.raw.hovered_files.is_empty());

    egui::Frame::new()
        .fill(palette.window)
        .inner_margin(Margin::symmetric(20, 16))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.set_width(ui.available_width());
                ui.label(
                    RichText::new(crate::i18n::t_ctx(ui.ctx(), app.filter.label()).as_ref())
                        .size(22.0)
                        .strong()
                        .color(palette.text),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    draw_sort_dropdown(ui, &mut app.sort_order, palette);
                    ui.add_space(8.0);
                    draw_search_box(ui, app, palette);
                });
            });
            ui.label(
                RichText::new(format!(
                    "{} items · {} unfinished",
                    visible.len(),
                    downloads
                        .iter()
                        .filter(|download| Filter::Unfinished.matches(download))
                        .count()
                ))
                .size(11.0)
                .color(palette.muted),
            );
            ui.add_space(12.0);

            let card_stroke = if is_dragging_file {
                Stroke::new(2.0, palette.accent)
            } else {
                Stroke::new(1.0, palette.border)
            };
            egui::Frame::new()
                .fill(palette.surface)
                .stroke(card_stroke)
                .corner_radius(10)
                .inner_margin(Margin::symmetric(1, 2))
                .show(ui, |ui| {
                    if is_dragging_file {
                        egui::Frame::new()
                            .fill(palette.selected)
                            .inner_margin(Margin::symmetric(12, 6))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.label(
                                    RichText::new(
                                        "Drop .url shortcut or .txt link list to add downloads",
                                    )
                                    .size(11.5)
                                    .strong()
                                    .color(palette.accent),
                                );
                            });
                    }
                    if visible.is_empty() {
                        draw_empty_table_state(ui, app, downloads.len(), palette, actions);
                    } else {
                        draw_table(
                            ui,
                            &visible,
                            speeds,
                            &selected,
                            &mut app.sort_order,
                            (lang, palette),
                            actions,
                        );
                    }
                });

            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.set_width(ui.available_width());
                let showing_text = if selected_count > 1 {
                    format!(
                        "Showing {} of {} downloads · {} selected",
                        visible.len(),
                        downloads.len(),
                        selected_count
                    )
                } else {
                    format!("Showing {} of {} downloads", visible.len(), downloads.len())
                };
                ui.label(RichText::new(showing_text).size(10.0).color(palette.faint));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(
                        RichText::new("Last updated just now")
                            .size(10.0)
                            .color(palette.faint),
                    );
                });
            });
        });
}

fn draw_search_box(ui: &mut egui::Ui, app: &mut LajuFileApp, palette: &Palette) {
    let has_query = !app.search_query.is_empty();
    let frame_stroke = if app.search_focus {
        Stroke::new(1.0, palette.accent)
    } else {
        Stroke::new(1.0, palette.border)
    };
    egui::Frame::new()
        .fill(palette.surface)
        .stroke(frame_stroke)
        .corner_radius(8)
        .inner_margin(Margin::symmetric(10, 5))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let (icon_rect, _) = ui.allocate_exact_size(Vec2::splat(13.0), Sense::hover());
                paint_search_icon(ui.painter(), icon_rect, palette.muted);
                ui.add_space(6.0);
                let input_w = if has_query { 142.0 } else { 160.0 };
                let response = ui.add_sized(
                    [input_w, 16.0],
                    egui::TextEdit::singleline(&mut app.search_query)
                        .frame(egui::Frame::NONE)
                        .hint_text(
                            RichText::new("Search... (Ctrl+F)")
                                .size(11.5)
                                .color(palette.faint),
                        ),
                );
                if app.search_focus {
                    response.request_focus();
                    app.search_focus = false;
                }
                if has_query {
                    ui.add_space(4.0);
                    let (clear_rect, clear_res) =
                        ui.allocate_exact_size(Vec2::splat(14.0), Sense::click());
                    let clear_res = clear_res.on_hover_cursor(egui::CursorIcon::PointingHand);
                    let clear_color = if clear_res.hovered() {
                        palette.text
                    } else {
                        palette.muted
                    };
                    let c = clear_rect.center();
                    ui.painter().line_segment(
                        [
                            egui::pos2(c.x - 3.0, c.y - 3.0),
                            egui::pos2(c.x + 3.0, c.y + 3.0),
                        ],
                        Stroke::new(1.3, clear_color),
                    );
                    ui.painter().line_segment(
                        [
                            egui::pos2(c.x - 3.0, c.y + 3.0),
                            egui::pos2(c.x + 3.0, c.y - 3.0),
                        ],
                        Stroke::new(1.3, clear_color),
                    );
                    if clear_res.clicked() {
                        app.search_query.clear();
                    }
                }
            });
        });
}

fn draw_sort_dropdown(ui: &mut egui::Ui, sort_order: &mut SortOrder, palette: &Palette) {
    let menu_id = egui::Id::unique("lajufile_sort_menu_open");
    let mut is_open = ui
        .ctx()
        .data_mut(|data| data.get_temp::<bool>(menu_id).unwrap_or(false));

    let label = format!("Sort: {}", sort_order.label());
    let text_w = ui.fonts_mut(|fonts| {
        fonts
            .layout_no_wrap(label.clone(), FontId::proportional(11.5), palette.text)
            .size()
            .x
    });
    let pill_size = Vec2::new(text_w + 38.0, 28.0);
    let (pill_rect, pill_res) = ui.allocate_exact_size(pill_size, Sense::click());
    let pill_res = pill_res.on_hover_cursor(egui::CursorIcon::PointingHand);
    let fill = if pill_res.hovered() || is_open {
        palette.surface_alt
    } else {
        palette.surface
    };
    let stroke_color = if is_open {
        palette.accent
    } else {
        palette.border
    };
    ui.painter().rect_filled(pill_rect, 8.0, fill);
    ui.painter().rect_stroke(
        pill_rect,
        8.0,
        Stroke::new(1.0, stroke_color),
        egui::StrokeKind::Inside,
    );
    ui.painter().text(
        egui::pos2(pill_rect.left() + 12.0, pill_rect.center().y),
        Align2::LEFT_CENTER,
        label,
        FontId::proportional(11.5),
        palette.text,
    );
    let c = egui::pos2(pill_rect.right() - 14.0, pill_rect.center().y);
    ui.painter().line(
        vec![
            egui::pos2(c.x - 3.2, c.y - 1.5),
            egui::pos2(c.x, c.y + 1.8),
            egui::pos2(c.x + 3.2, c.y - 1.5),
        ],
        Stroke::new(1.3, palette.muted),
    );

    if pill_res.clicked() {
        is_open = !is_open;
    }

    if is_open {
        let popup_pos = egui::pos2(
            (pill_rect.right() - 156.0).max(8.0),
            pill_rect.bottom() + 4.0,
        );
        let area_res = egui::Area::new(egui::Id::unique("lajufile_sort_menu_popup"))
            .order(egui::Order::Foreground)
            .fixed_pos(popup_pos)
            .show(ui.ctx(), |ui| {
                egui::Frame::new()
                    .fill(palette.surface)
                    .stroke(Stroke::new(1.0, palette.border))
                    .corner_radius(10)
                    .inner_margin(Margin::symmetric(6, 6))
                    .show(ui, |ui| {
                        ui.set_width(144.0);
                        for option in SortOrder::ALL {
                            let active = *sort_order == option;
                            let (item_rect, item_res) =
                                ui.allocate_exact_size(Vec2::new(144.0, 26.0), Sense::click());
                            let item_res = item_res.on_hover_cursor(egui::CursorIcon::PointingHand);
                            if active {
                                ui.painter().rect_filled(item_rect, 6.0, palette.selected);
                            } else if item_res.hovered() {
                                ui.painter()
                                    .rect_filled(item_rect, 6.0, palette.surface_alt);
                            }
                            if active {
                                ui.painter().circle_filled(
                                    egui::pos2(item_rect.left() + 11.0, item_rect.center().y),
                                    2.5,
                                    palette.accent,
                                );
                            }
                            ui.painter().text(
                                egui::pos2(item_rect.left() + 22.0, item_rect.center().y),
                                Align2::LEFT_CENTER,
                                option.label(),
                                FontId::proportional(11.5),
                                if active { palette.accent } else { palette.text },
                            );
                            if item_res.clicked() {
                                *sort_order = option;
                                is_open = false;
                            }
                        }
                    });
            });
        if !pill_res.clicked() && area_res.response.clicked_elsewhere() {
            is_open = false;
        }
    }

    ui.ctx().data_mut(|data| data.insert_temp(menu_id, is_open));
}

fn draw_empty_table_state(
    ui: &mut egui::Ui,
    app: &mut LajuFileApp,
    total_downloads: usize,
    palette: &Palette,
    actions: &mut Vec<Action>,
) {
    let h = (ui.available_height() - 34.0).max(160.0);
    ui.allocate_ui_with_layout(
        Vec2::new(ui.available_width(), h),
        Layout::top_down(Align::Center),
        |ui| {
            ui.add_space((h * 0.34).max(24.0));
            let has_search = !app.search_query.trim().is_empty();
            let title = if has_search {
                format!("No downloads match \"{}\"", app.search_query.trim())
            } else if total_downloads > 0 {
                format!(
                    "No downloads in {}",
                    crate::i18n::t_ctx(ui.ctx(), app.filter.label())
                )
            } else {
                crate::i18n::t_ctx(ui.ctx(), "No downloads yet").into_owned()
            };
            ui.label(RichText::new(title).size(14.0).strong().color(palette.text));
            ui.add_space(6.0);
            let sub = if has_search {
                "Try a different filename, host, category, or clear the search filter."
            } else {
                "Click Add URL (Ctrl+N) or drop a .url / .txt link file onto this window."
            };
            ui.label(RichText::new(sub).size(11.5).color(palette.muted));
            ui.add_space(12.0);
            if has_search {
                if ui.button("Clear search").clicked() {
                    app.search_query.clear();
                }
            } else if total_downloads > 0 && app.filter != Filter::All {
                if ui.button("Show All downloads").clicked() {
                    app.filter = Filter::All;
                }
            } else if ui.button("Add URL (Ctrl+N)").clicked() {
                actions.push(Action::OpenAdd);
            }
        },
    );
}

fn paint_search_icon(painter: &egui::Painter, rect: Rect, color: Color32) {
    let center = egui::pos2(rect.left() + 5.2, rect.top() + 5.2);
    painter.circle_stroke(center, 3.6, Stroke::new(1.3, color));
    painter.line_segment(
        [
            egui::pos2(center.x + 2.6, center.y + 2.6),
            egui::pos2(rect.right() - 1.5, rect.bottom() - 1.5),
        ],
        Stroke::new(1.4, color),
    );
}

fn draw_status_bar(
    ui: &mut egui::Ui,
    app: &mut LajuFileApp,
    snapshot: Option<&crate::engine::Snapshot>,
    lang: &str,
    palette: &Palette,
) -> bool {
    let mut toggled = false;
    let downloads = snapshot.map(|s| s.downloads.as_slice()).unwrap_or(&[]);
    let bridge = snapshot.map(|s| &s.bridge);
    let active = downloads
        .iter()
        .filter(|download| download.status == DownloadStatus::Downloading)
        .count();
    let queued = downloads
        .iter()
        .filter(|download| download.status == DownloadStatus::Queued)
        .count();
    let speed: u64 = snapshot.map(|s| s.speeds.values().sum()).unwrap_or(0);
    let speed_text = if speed > 0 {
        format!("{}/s", crate::i18n::format_size_locale(speed, lang))
    } else {
        "0 B/s".to_string()
    };
    let limit_kbps = snapshot
        .map(|s| s.settings.global_speed_limit_kbps)
        .unwrap_or(0);
    let limit_text = if limit_kbps == 0 {
        "Limit: Unlimited".to_string()
    } else {
        format!(
            "Limit: {}",
            crate::i18n::format_speed_locale(Some(limit_kbps * 1024), lang)
        )
    };

    let (capture_label, dot_color, port_text) = match bridge {
        Some(status) if status.state == BridgeState::Bound => (
            "Browser capture on",
            palette.emerald_bright,
            format!("127.0.0.1:{}", status.port),
        ),
        Some(status) if status.state == BridgeState::Error => (
            "Browser capture off",
            palette.red_bright,
            format!("127.0.0.1:{}", status.port),
        ),
        Some(status) => (
            "Browser capture starting",
            palette.amber_bright,
            format!("127.0.0.1:{}", status.port),
        ),
        None => (
            "Browser capture off",
            palette.faint,
            "127.0.0.1:53841".to_string(),
        ),
    };

    ui.set_width(ui.available_width());
    ui.set_height(ui.available_height());
    ui.horizontal_centered(|ui| {
        ui.label(
            RichText::new(format!("{active} active · {queued} queued · {speed_text}"))
                .size(10.5)
                .color(palette.muted),
        );
        ui.add_space(10.0);
        let div = ui
            .allocate_exact_size(Vec2::new(1.0, 12.0), Sense::hover())
            .0;
        ui.painter().rect_filled(div, 0.0, palette.border);
        ui.add_space(10.0);

        let limit_color = if limit_kbps > 0 {
            palette.amber_bright
        } else {
            palette.muted
        };
        let text_w = ui.fonts_mut(|fonts| {
            fonts
                .layout_no_wrap(limit_text.clone(), FontId::proportional(10.5), limit_color)
                .size()
                .x
        });
        let (pill_rect, pill_res) =
            ui.allocate_exact_size(Vec2::new(text_w + 16.0, 20.0), Sense::click());
        let pill_res = pill_res.on_hover_cursor(egui::CursorIcon::PointingHand);
        let pill_fill = if pill_res.hovered() || app.show_speed_popover {
            palette.surface_alt
        } else {
            palette.surface
        };
        let pill_stroke = if limit_kbps > 0 || app.show_speed_popover {
            limit_color.gamma_multiply(0.7)
        } else {
            palette.border
        };
        ui.painter().rect_filled(pill_rect, 6.0, pill_fill);
        ui.painter().rect_stroke(
            pill_rect,
            6.0,
            Stroke::new(1.0, pill_stroke),
            egui::StrokeKind::Inside,
        );
        ui.painter().text(
            pill_rect.center(),
            Align2::CENTER_CENTER,
            limit_text,
            FontId::proportional(10.5),
            limit_color,
        );
        if pill_res.clicked() {
            app.show_speed_popover = !app.show_speed_popover;
            toggled = true;
            if app.show_speed_popover {
                app.custom_speed_kbps = limit_kbps.to_string();
            }
        }

        if app.progress_id.is_none()
            && let Some(target) = app
                .hidden_progress_id
                .and_then(|id| downloads.iter().find(|d| d.id == id))
                .or_else(|| {
                    downloads
                        .iter()
                        .find(|d| d.status == DownloadStatus::Downloading)
                })
        {
            ui.add_space(8.0);
            let show_label = crate::i18n::translate(lang, "Show progress").into_owned();
            let pct = progress_fraction(target.size, target.downloaded, target.status) * 100.0;
            let btn = egui::Button::new(
                RichText::new(format!("{show_label} ({pct:.0}%)"))
                    .size(10.5)
                    .color(palette.sky),
            )
            .fill(palette.surface)
            .stroke(Stroke::new(1.0, palette.sky.gamma_multiply(0.4)))
            .corner_radius(10);
            if ui
                .add_sized([124.0, 20.0], btn)
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .clicked()
            {
                app.progress_id = Some(target.id);
                app.hidden_progress_id = None;
            }
        }

        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(RichText::new(port_text).size(10.5).color(palette.faint));
            ui.add_space(10.0);
            let div = ui
                .allocate_exact_size(Vec2::new(1.0, 12.0), Sense::hover())
                .0;
            ui.painter().rect_filled(div, 0.0, palette.border);
            ui.add_space(10.0);
            ui.label(RichText::new(capture_label).size(10.5).color(palette.muted));
            ui.add_space(6.0);
            let (dot_rect, _) = ui.allocate_exact_size(Vec2::splat(6.0), Sense::hover());
            ui.painter()
                .circle_filled(dot_rect.center(), 3.0, dot_color);
        });
    });
    toggled
}

fn draw_speed_popover(
    ctx: &egui::Context,
    app: &mut LajuFileApp,
    toggled_this_frame: bool,
    palette: &Palette,
    actions: &mut Vec<Action>,
) {
    let current_kbps = app
        .snapshot
        .as_ref()
        .map(|s| s.settings.global_speed_limit_kbps)
        .unwrap_or(0);
    let area_res = egui::Area::new(egui::Id::unique("lajufile_speed_limiter_popover"))
        .order(egui::Order::Foreground)
        .anchor(Align2::LEFT_BOTTOM, Vec2::new(172.0, -40.0))
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(palette.surface)
                .stroke(Stroke::new(1.0, palette.border))
                .corner_radius(10)
                .inner_margin(Margin::symmetric(14, 12))
                .show(ui, |ui| {
                    ui.set_width(276.0);
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new("Speed Limiter")
                                .size(13.0)
                                .strong()
                                .color(palette.text),
                        );
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.label(
                                RichText::new(if current_kbps == 0 {
                                    "Unlimited".to_string()
                                } else {
                                    format!("{current_kbps} KB/s")
                                })
                                .size(11.0)
                                .color(if current_kbps > 0 {
                                    palette.amber_bright
                                } else {
                                    palette.emerald_bright
                                }),
                            );
                        });
                    });
                    ui.add_space(8.0);
                    const PRESETS: &[(u64, &str)] = &[
                        (0, "Unlimited"),
                        (256, "256 KB/s"),
                        (512, "512 KB/s"),
                        (1024, "1 MB/s"),
                        (2048, "2 MB/s"),
                        (5120, "5 MB/s"),
                        (10240, "10 MB/s"),
                    ];
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = Vec2::new(6.0, 6.0);
                        for &(kbps, label) in PRESETS {
                            let active = current_kbps == kbps;
                            let btn = egui::Button::new(
                                RichText::new(label).size(11.0).color(if active {
                                    Color32::WHITE
                                } else {
                                    palette.text
                                }),
                            )
                            .fill(if active {
                                palette.accent
                            } else {
                                palette.surface_alt
                            })
                            .stroke(Stroke::new(
                                1.0,
                                if active {
                                    palette.accent
                                } else {
                                    palette.border
                                },
                            ))
                            .corner_radius(6);
                            if ui.add(btn).clicked() {
                                actions.push(Action::SetSpeedLimit(kbps));
                                app.show_speed_popover = false;
                            }
                        }
                    });
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new("Custom (KB/s):")
                                .size(11.0)
                                .color(palette.muted),
                        );
                        ui.add_space(6.0);
                        let resp = ui.add_sized(
                            [86.0, 24.0],
                            egui::TextEdit::singleline(&mut app.custom_speed_kbps),
                        );
                        ui.add_space(6.0);
                        let apply = ui.button("Apply").clicked()
                            || (resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)));
                        if apply {
                            if let Ok(val) = app.custom_speed_kbps.trim().parse::<u64>() {
                                actions.push(Action::SetSpeedLimit(val));
                                app.show_speed_popover = false;
                            }
                        }
                    });
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new(
                            "0 = unlimited. Applies immediately to all active downloads.",
                        )
                        .size(10.0)
                        .color(palette.faint),
                    );
                });
        });
    if !toggled_this_frame && area_res.response.clicked_elsewhere() {
        app.show_speed_popover = false;
    }
}

#[cfg(test)]
mod tests {
    use super::progress_fraction;
    use super::table::{ease_in_out, extension_label};
    use lajufile_core::store::DownloadStatus;

    #[test]
    fn extension_label_handles_unicode() {
        assert_eq!(extension_label("report.文档"), "文档");
        assert_eq!(extension_label("archive.backup"), "BACK");
    }

    #[test]
    fn progress_fraction_handles_done_partial_unknown_and_clamping() {
        assert!((progress_fraction(None, 0, DownloadStatus::Done) - 1.0).abs() < f32::EPSILON);
        assert!(
            (progress_fraction(Some(100), 25, DownloadStatus::Downloading) - 0.25).abs()
                < f32::EPSILON
        );
        assert!(progress_fraction(None, 50, DownloadStatus::Downloading).abs() < f32::EPSILON);
        assert!(progress_fraction(Some(0), 50, DownloadStatus::Downloading).abs() < f32::EPSILON);
        assert!(
            (progress_fraction(Some(100), 250, DownloadStatus::Downloading) - 1.0).abs()
                < f32::EPSILON
        );
    }

    #[test]
    fn ease_in_out_stays_within_unit_interval() {
        assert!(ease_in_out(-0.5).abs() < f32::EPSILON);
        assert!(ease_in_out(0.0).abs() < f32::EPSILON);
        assert!((ease_in_out(0.5) - 0.5).abs() < f32::EPSILON);
        assert!((ease_in_out(1.0) - 1.0).abs() < f32::EPSILON);
        assert!((ease_in_out(1.5) - 1.0).abs() < f32::EPSILON);
        assert!(ease_in_out(0.25) < 0.25);
        assert!(ease_in_out(0.75) > 0.75);
    }
}
