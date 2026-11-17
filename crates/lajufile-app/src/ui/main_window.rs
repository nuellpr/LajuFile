//! Main window: menu, toolbar, sidebar, download table, and status bar.

#[cfg(test)]
use std::path::Path;
use std::time::Duration;

use egui::{Align, Align2, Color32, FontId, Layout, Margin, Rect, RichText, Sense, Stroke, Vec2};
use egui_extras::{Column, TableBuilder};
use lajufile_core::settings::Theme;
use lajufile_core::store::{Category, Download, DownloadStatus, now_millis};
use url::Url;

use super::{Action, Filter, SortOrder, format::*, matches_search};
use crate::app::LajuFileApp;
use crate::bridge::BridgeState;

struct Palette {
    window: Color32,
    bar: Color32,
    sidebar: Color32,
    surface: Color32,
    surface_alt: Color32,
    border: Color32,
    text: Color32,
    muted: Color32,
    faint: Color32,
    accent: Color32,
    selected: Color32,
    sky: Color32,
    blue: Color32,
    indigo: Color32,
    emerald: Color32,
    emerald_bright: Color32,
    amber_bright: Color32,
    red_bright: Color32,
    track: Color32,
    track_ring: Color32,
    speed_bg: Color32,
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

fn draw_menu_bar(
    ui: &mut egui::Ui,
    lang: &str,
    palette: &Palette,
    available_update: Option<&str>,
    actions: &mut Vec<Action>,
) {
    let t = |en: &str| crate::i18n::translate(lang, en).into_owned();
    let rtl = crate::i18n::is_rtl(lang);
    egui::Frame::new()
        .fill(palette.bar)
        .stroke(Stroke::new(1.0, palette.border))
        .inner_margin(Margin::symmetric(12, 0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.set_height(28.0);
            let layout = if rtl {
                Layout::right_to_left(Align::Center)
            } else {
                Layout::left_to_right(Align::Center)
            };
            ui.with_layout(layout, |ui| {
                ui.set_width(ui.available_width());
                ui.menu_button(t("File"), |ui| {
                    if ui.button(t("Add URL...")).clicked() {
                        actions.push(Action::OpenAdd);
                        ui.close();
                    }
                    if ui.button(t("Settings...")).clicked() {
                        actions.push(Action::OpenSettings);
                        ui.close();
                    }
                    ui.separator();
                    if ui.button(t("Exit")).clicked() {
                        actions.push(Action::Exit);
                        ui.close();
                    }
                });
                ui.menu_button(t("Tasks"), |ui| {
                    if ui
                        .button(format!("{}\tCtrl+N", t("Add new download...")))
                        .clicked()
                    {
                        actions.push(Action::OpenAdd);
                        ui.close();
                    }
                    if ui
                        .button(format!("{}\tCtrl+B", t("Add batch download...")))
                        .clicked()
                    {
                        actions.push(Action::OpenBatch);
                        ui.close();
                    }
                    if ui
                        .button(format!("{}\tCtrl+F", t("Find download...")))
                        .clicked()
                    {
                        actions.push(Action::FocusSearch);
                        ui.close();
                    }
                    if ui.button(format!("{}\tCtrl+A", t("Select all"))).clicked() {
                        actions.push(Action::SelectAll);
                        ui.close();
                    }
                    if ui.button(t("Export links (.txt)")).clicked() {
                        actions.push(Action::ExportLinks);
                        ui.close();
                    }
                    ui.separator();
                    if ui.button(t("Resume all")).clicked() {
                        actions.push(Action::ResumeAll);
                        ui.close();
                    }
                    if ui.button(t("Stop all")).clicked() {
                        actions.push(Action::PauseAll);
                        ui.close();
                    }
                    if ui.button(t("Delete completed")).clicked() {
                        actions.push(Action::RemoveCompleted);
                        ui.close();
                    }
                });
                ui.menu_button(t("View"), |ui| {
                    if ui.button(t("Light")).clicked() {
                        actions.push(Action::SetTheme(Theme::Light));
                        ui.close();
                    }
                    if ui.button(t("Dark")).clicked() {
                        actions.push(Action::SetTheme(Theme::Dark));
                        ui.close();
                    }
                    ui.separator();
                    ui.menu_button(t("Interface size"), |ui| {
                        for pct in [90u32, 100, 115, 125, 150] {
                            if ui.button(format!("{pct}%")).clicked() {
                                actions.push(Action::SetScale(pct));
                                ui.close();
                            }
                        }
                    });
                    if ui.button(t("Language...")).clicked() {
                        actions.push(Action::OpenLanguagePicker);
                        ui.close();
                    }
                });
                ui.menu_button(t("Help"), |ui| {
                    if ui.button(t("Check for updates...")).clicked() {
                        actions.push(Action::OpenUpdateChecker);
                        ui.close();
                    }
                    if ui.button(t("Browser extension wizard...")).clicked() {
                        actions.push(Action::OpenBrowserWizard);
                        ui.close();
                    }
                    ui.separator();
                    ui.label(concat!("LajuFile ", env!("CARGO_PKG_VERSION")));
                    ui.label("Fast multi-connection download manager");
                });
                let version_layout = if rtl {
                    Layout::left_to_right(Align::Center)
                } else {
                    Layout::right_to_left(Align::Center)
                };
                ui.with_layout(version_layout, |ui| {
                    ui.label(
                        RichText::new(concat!("v", env!("CARGO_PKG_VERSION")))
                            .size(10.0)
                            .color(palette.faint),
                    );
                    if let Some(tag) = available_update {
                        ui.add_space(8.0);
                        let badge = egui::Button::new(
                            RichText::new(format!("{} {tag}", t("Update available")))
                                .size(10.0)
                                .color(Color32::WHITE),
                        )
                        .fill(palette.accent)
                        .corner_radius(4);
                        if ui.add(badge).clicked() {
                            actions.push(Action::OpenUpdateChecker);
                        }
                    }
                });
            });
        });
}

#[derive(Clone, Copy)]
enum ToolbarIcon {
    Plus,
    Batch,
    Play,
    Pause,
    StopAll,
    Trash,
    Clean,
    Gear,
}

#[derive(Clone, Copy)]
enum NavIcon {
    All,
    Unfinished,
    Finished,
}

#[derive(Clone, Copy)]
enum SidebarIcon {
    Nav(NavIcon),
    Category(super::icons::IconKey),
}

fn draw_toolbar(
    ui: &mut egui::Ui,
    selected: Option<i64>,
    selected_ids: &std::collections::HashSet<i64>,
    downloads: &[Download],
    lang: &str,
    palette: &Palette,
    actions: &mut Vec<Action>,
) {
    let targets: Vec<i64> = downloads
        .iter()
        .filter(|d| selected_ids.contains(&d.id) || selected == Some(d.id))
        .map(|d| d.id)
        .collect();
    let pause_targets: Vec<i64> = downloads
        .iter()
        .filter(|d| {
            targets.contains(&d.id)
                && matches!(
                    d.status,
                    DownloadStatus::Downloading | DownloadStatus::Queued
                )
        })
        .map(|d| d.id)
        .collect();
    let resume_targets: Vec<i64> = downloads
        .iter()
        .filter(|d| {
            targets.contains(&d.id)
                && matches!(d.status, DownloadStatus::Paused | DownloadStatus::Error)
        })
        .map(|d| d.id)
        .collect();
    let can_pause = !pause_targets.is_empty();
    let can_resume = !resume_targets.is_empty();
    let has_selection = !targets.is_empty();
    let has_active = downloads.iter().any(|download| {
        matches!(
            download.status,
            DownloadStatus::Downloading | DownloadStatus::Queued
        )
    });
    let has_completed = downloads
        .iter()
        .any(|download| download.status == DownloadStatus::Done);

    let t = |en: &str| crate::i18n::translate(lang, en).into_owned();
    let rtl = crate::i18n::is_rtl(lang);
    egui::Frame::new()
        .fill(palette.bar)
        .stroke(Stroke::new(1.0, palette.border))
        .inner_margin(Margin::symmetric(12, 0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.set_height(60.0);
            let layout = if rtl {
                Layout::right_to_left(Align::Center)
            } else {
                Layout::left_to_right(Align::Center)
            };
            ui.with_layout(layout, |ui| {
                ui.set_width(ui.available_width());
                if toolbar_button(ui, ToolbarIcon::Plus, &t("Add URL"), true, true, palette) {
                    actions.push(Action::OpenAdd);
                }
                ui.add_space(6.0);
                if toolbar_button(ui, ToolbarIcon::Batch, &t("Batch"), true, false, palette) {
                    actions.push(Action::OpenBatch);
                }
                ui.add_space(8.0);
                let div_rect = ui
                    .allocate_exact_size(Vec2::new(1.0, 20.0), Sense::hover())
                    .0;
                ui.painter().rect_filled(div_rect, 0.0, palette.border);
                ui.add_space(8.0);
                if toolbar_button(
                    ui,
                    ToolbarIcon::Play,
                    &t("Resume"),
                    can_resume,
                    false,
                    palette,
                ) {
                    for id in resume_targets {
                        actions.push(Action::Resume(id));
                    }
                }
                ui.add_space(6.0);
                if toolbar_button(
                    ui,
                    ToolbarIcon::Pause,
                    &t("Stop"),
                    can_pause,
                    false,
                    palette,
                ) {
                    for id in pause_targets {
                        actions.push(Action::Pause(id));
                    }
                }
                ui.add_space(6.0);
                if toolbar_button(
                    ui,
                    ToolbarIcon::StopAll,
                    &t("Stop All"),
                    has_active,
                    false,
                    palette,
                ) {
                    actions.push(Action::PauseAll);
                }
                ui.add_space(6.0);
                if toolbar_button(
                    ui,
                    ToolbarIcon::Trash,
                    &t("Delete"),
                    has_selection,
                    false,
                    palette,
                ) {
                    let shift = ui.input(|i| i.modifiers.shift);
                    actions.push(Action::ConfirmRemove(targets, shift));
                }
                ui.add_space(6.0);
                if toolbar_button(
                    ui,
                    ToolbarIcon::Clean,
                    &t("Delete Completed"),
                    has_completed,
                    false,
                    palette,
                ) {
                    actions.push(Action::RemoveCompleted);
                }
                let opt_layout = if rtl {
                    Layout::left_to_right(Align::Center)
                } else {
                    Layout::right_to_left(Align::Center)
                };
                ui.with_layout(opt_layout, |ui| {
                    if toolbar_button(ui, ToolbarIcon::Gear, &t("Options"), true, false, palette) {
                        actions.push(Action::OpenSettings);
                    }
                });
            });
        });
}

fn toolbar_button(
    ui: &mut egui::Ui,
    icon: ToolbarIcon,
    label: &str,
    enabled: bool,
    primary: bool,
    palette: &Palette,
) -> bool {
    let text_color = if primary {
        Color32::WHITE
    } else if enabled {
        palette.text
    } else {
        palette.faint
    };
    let galley =
        ui.painter()
            .layout_no_wrap(label.to_string(), FontId::proportional(12.5), text_color);
    let width = 12.0 + 10.0 + 6.0 + galley.size().x + 12.0;
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, mut response) = ui.allocate_exact_size(Vec2::new(width, 34.0), sense);
    if enabled {
        response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    }

    let fill = if primary {
        if !enabled {
            palette.accent.gamma_multiply(0.45)
        } else if response.hovered() {
            lerp_color(palette.accent, Color32::WHITE, 0.14)
        } else {
            palette.accent
        }
    } else if enabled && response.hovered() {
        palette.surface_alt
    } else {
        palette.surface
    };

    let painter = ui.painter();
    painter.rect_filled(rect, 8.0, fill);
    if !primary {
        let stroke_color = if enabled && response.hovered() {
            palette.muted.gamma_multiply(0.55)
        } else {
            palette.border
        };
        painter.rect_stroke(
            rect,
            8.0,
            Stroke::new(1.0, stroke_color),
            egui::StrokeKind::Inside,
        );
    }

    let icon_color = if primary {
        Color32::WHITE
    } else if enabled {
        palette.muted
    } else {
        palette.faint.gamma_multiply(0.55)
    };
    let icon_rect = Rect::from_center_size(
        egui::pos2(rect.left() + 12.0 + 5.0, rect.center().y),
        Vec2::splat(10.0),
    );
    paint_toolbar_icon(painter, icon_rect, icon, icon_color);
    painter.galley(
        egui::pos2(
            icon_rect.right() + 6.0,
            rect.center().y - galley.size().y * 0.5,
        ),
        galley,
        text_color,
    );
    enabled && response.clicked()
}

fn paint_toolbar_icon(painter: &egui::Painter, rect: Rect, icon: ToolbarIcon, color: Color32) {
    let c = rect.center();
    let pt = |x: f32, y: f32| {
        egui::pos2(
            rect.left() + (x / 10.0) * rect.width(),
            rect.top() + (y / 10.0) * rect.height(),
        )
    };
    match icon {
        ToolbarIcon::Plus => {
            painter.line_segment(
                [
                    egui::pos2(rect.left() + 1.0, c.y),
                    egui::pos2(rect.right() - 1.0, c.y),
                ],
                Stroke::new(1.5, color),
            );
            painter.line_segment(
                [
                    egui::pos2(c.x, rect.top() + 1.0),
                    egui::pos2(c.x, rect.bottom() - 1.0),
                ],
                Stroke::new(1.5, color),
            );
        }
        ToolbarIcon::Batch => {
            for y in [2.0, 5.0, 8.0] {
                let end = if y > 6.0 { 5.2 } else { 9.0 };
                painter.line_segment([pt(1.0, y), pt(end, y)], Stroke::new(1.3, color));
            }
            painter.line_segment([pt(6.5, 7.8), pt(9.5, 7.8)], Stroke::new(1.3, color));
            painter.line_segment([pt(8.0, 6.3), pt(8.0, 9.3)], Stroke::new(1.3, color));
        }
        ToolbarIcon::Play => {
            painter.add(egui::Shape::convex_polygon(
                vec![pt(2.0, 1.2), pt(8.8, 5.0), pt(2.0, 8.8)],
                color,
                Stroke::NONE,
            ));
        }
        ToolbarIcon::Pause => {
            painter.rect_filled(Rect::from_min_max(pt(1.8, 1.2), pt(4.0, 8.8)), 0.8, color);
            painter.rect_filled(Rect::from_min_max(pt(6.0, 1.2), pt(8.2, 8.8)), 0.8, color);
        }
        ToolbarIcon::StopAll => {
            painter.rect_filled(Rect::from_min_max(pt(1.6, 1.6), pt(8.4, 8.4)), 1.5, color);
        }
        ToolbarIcon::Trash => {
            painter.line_segment([pt(0.8, 2.4), pt(9.2, 2.4)], Stroke::new(1.2, color));
            painter.line_segment([pt(3.4, 1.0), pt(6.6, 1.0)], Stroke::new(1.2, color));
            painter.rect_stroke(
                Rect::from_min_max(pt(1.8, 3.4), pt(8.2, 9.2)),
                1.0,
                Stroke::new(1.1, color),
                egui::StrokeKind::Inside,
            );
            painter.line_segment([pt(3.9, 4.8), pt(3.9, 7.8)], Stroke::new(1.0, color));
            painter.line_segment([pt(6.1, 4.8), pt(6.1, 7.8)], Stroke::new(1.0, color));
        }
        ToolbarIcon::Clean => {
            painter.line(
                vec![pt(1.0, 5.2), pt(3.2, 7.5), pt(7.8, 2.2)],
                Stroke::new(1.3, color),
            );
            painter.line(vec![pt(4.8, 7.8), pt(9.2, 2.8)], Stroke::new(1.3, color));
        }
        ToolbarIcon::Gear => {
            painter.circle_stroke(c, 2.5, Stroke::new(1.2, color));
            painter.circle_filled(c, 0.9, color);
            for step in 0..8 {
                let angle = step as f32 * std::f32::consts::FRAC_PI_4;
                let dir = Vec2::new(angle.cos(), angle.sin());
                painter.line_segment([c + dir * 2.8, c + dir * 4.5], Stroke::new(1.2, color));
            }
        }
    }
}

fn draw_sidebar(
    ui: &mut egui::Ui,
    app: &mut LajuFileApp,
    downloads: &[Download],
    palette: &Palette,
) {
    let ctx = ui.ctx().clone();
    let t = |en: &str| crate::i18n::t_ctx(&ctx, en).into_owned();
    egui::Frame::new()
        .fill(palette.sidebar)
        .stroke(Stroke::new(1.0, palette.border))
        .inner_margin(Margin::symmetric(12, 16))
        .show(ui, |ui| {
            ui.set_width(232.0);
            ui.set_min_height(ui.available_height());
            section_label(ui, &t("LIBRARY"), palette);

            let all = downloads.len();
            let unfinished = downloads
                .iter()
                .filter(|download| Filter::Unfinished.matches(download))
                .count();
            let finished = downloads
                .iter()
                .filter(|download| Filter::Finished.matches(download))
                .count();

            if sidebar_row(
                ui,
                &t("All downloads"),
                all,
                Filter::All,
                app.filter,
                Some(SidebarIcon::Nav(NavIcon::All)),
                palette,
            ) {
                app.filter = Filter::All;
            }
            if sidebar_row(
                ui,
                &t("Unfinished"),
                unfinished,
                Filter::Unfinished,
                app.filter,
                Some(SidebarIcon::Nav(NavIcon::Unfinished)),
                palette,
            ) {
                app.filter = Filter::Unfinished;
            }
            if sidebar_row(
                ui,
                &t("Finished"),
                finished,
                Filter::Finished,
                app.filter,
                Some(SidebarIcon::Nav(NavIcon::Finished)),
                palette,
            ) {
                app.filter = Filter::Finished;
            }

            ui.add_space(8.0);
            let div = ui
                .allocate_exact_size(Vec2::new(ui.available_width(), 1.0), Sense::hover())
                .0;
            ui.painter().rect_filled(div, 0.0, palette.border);
            ui.add_space(8.0);

            section_label(ui, &t("CATEGORIES"), palette);
            for category in [
                Category::Video,
                Category::Music,
                Category::Documents,
                Category::Programs,
                Category::Compressed,
                Category::Other,
            ] {
                let count = downloads
                    .iter()
                    .filter(|download| download.category == category)
                    .count();
                let filter = Filter::Category(category);
                if sidebar_row(
                    ui,
                    &t(filter.label()),
                    count,
                    filter,
                    app.filter,
                    Some(SidebarIcon::Category(super::icons::category_key(category))),
                    palette,
                ) {
                    app.filter = filter;
                }
            }
        });
}

fn section_label(ui: &mut egui::Ui, label: &str, palette: &Palette) {
    ui.add_space(4.0);
    ui.label(RichText::new(label).size(9.0).strong().color(palette.faint));
    ui.add_space(6.0);
}

fn sidebar_row(
    ui: &mut egui::Ui,
    label: &str,
    count: usize,
    filter: Filter,
    active: Filter,
    icon: Option<SidebarIcon>,
    palette: &Palette,
) -> bool {
    let selected = filter == active;
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 32.0), Sense::click());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    let fill = if selected {
        palette.selected
    } else if response.hovered() {
        palette.surface_alt
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, 8.0, fill);
    if selected {
        ui.painter().rect_filled(
            Rect::from_min_size(
                egui::pos2(rect.left(), rect.top() + 7.0),
                Vec2::new(3.0, rect.height() - 14.0),
            ),
            2,
            palette.accent,
        );
    }
    let mut x = rect.left() + 10.0;
    if let Some(icon) = icon {
        match icon {
            SidebarIcon::Category(key) => {
                let icon_rect = Rect::from_center_size(
                    egui::pos2(x + 10.0, rect.center().y),
                    Vec2::splat(20.0),
                );
                super::icons::paint_icon_at(ui, icon_rect, key);
                x += 26.0;
            }
            SidebarIcon::Nav(nav) => {
                let icon_rect =
                    Rect::from_center_size(egui::pos2(x + 6.0, rect.center().y), Vec2::splat(12.0));
                let nav_color = if selected {
                    palette.accent
                } else {
                    palette.muted
                };
                paint_nav_icon(ui.painter(), icon_rect, nav, nav_color);
                x += 20.0;
            }
        }
    }
    ui.painter().text(
        egui::pos2(x, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        FontId::proportional(12.5),
        if selected {
            palette.accent
        } else {
            palette.text
        },
    );
    ui.painter().text(
        egui::pos2(rect.right() - 10.0, rect.center().y),
        Align2::RIGHT_CENTER,
        count.to_string(),
        FontId::proportional(11.0),
        palette.muted,
    );
    response.clicked()
}

fn paint_nav_icon(painter: &egui::Painter, rect: Rect, icon: NavIcon, color: Color32) {
    let c = rect.center();
    let pt = |x: f32, y: f32| {
        egui::pos2(
            rect.left() + (x / 12.0) * rect.width(),
            rect.top() + (y / 12.0) * rect.height(),
        )
    };
    match icon {
        NavIcon::All => {
            painter.line(
                vec![pt(1.5, 7.0), pt(1.5, 10.2), pt(10.5, 10.2), pt(10.5, 7.0)],
                Stroke::new(1.2, color),
            );
            painter.line_segment([pt(6.0, 1.6), pt(6.0, 7.2)], Stroke::new(1.2, color));
            painter.line(
                vec![pt(3.8, 5.2), pt(6.0, 7.5), pt(8.2, 5.2)],
                Stroke::new(1.2, color),
            );
        }
        NavIcon::Unfinished => {
            painter.circle_stroke(c, 4.6, Stroke::new(1.2, color));
            painter.line(
                vec![pt(6.0, 3.2), pt(6.0, 6.2), pt(8.2, 7.4)],
                Stroke::new(1.2, color),
            );
        }
        NavIcon::Finished => {
            painter.circle_stroke(c, 4.6, Stroke::new(1.2, color));
            painter.line(
                vec![pt(3.5, 6.2), pt(5.3, 8.0), pt(8.6, 4.3)],
                Stroke::new(1.2, color),
            );
        }
    }
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

fn draw_table(
    ui: &mut egui::Ui,
    visible: &[&Download],
    speeds: &std::collections::HashMap<i64, u64>,
    selected: &std::collections::HashSet<i64>,
    sort_order: &mut SortOrder,
    (lang, palette): (&str, &Palette),
    actions: &mut Vec<Action>,
) {
    let now = now_millis();
    let mods = ui.input(|i| i.modifiers);
    let ctx = ui.ctx().clone();
    let table_height = (ui.available_height() - 34.0).max(120.0);
    TableBuilder::new(ui)
        .striped(false)
        .sense(Sense::click())
        .cell_layout(Layout::left_to_right(Align::Center))
        .column(Column::remainder())
        .column(Column::exact(80.0))
        .column(Column::exact(110.0))
        .column(Column::exact(172.0))
        .column(Column::exact(96.0))
        .column(Column::exact(64.0))
        .column(Column::exact(86.0))
        .auto_shrink([false, false])
        .max_scroll_height(table_height)
        .header(34.0, |mut header| {
            let col_lbl = |en: &str| crate::i18n::t_ctx(&ctx, en).into_owned();
            header.col(|ui| {
                sortable_header_cell(
                    ui,
                    &col_lbl("NAME"),
                    false,
                    sort_order,
                    Some((SortOrder::NameAsc, SortOrder::NameDesc)),
                    palette,
                )
            });
            header.col(|ui| {
                sortable_header_cell(
                    ui,
                    &col_lbl("SIZE"),
                    true,
                    sort_order,
                    Some((SortOrder::SizeDesc, SortOrder::SizeAsc)),
                    palette,
                )
            });
            header.col(|ui| {
                sortable_header_cell(ui, &col_lbl("STATUS"), false, sort_order, None, palette)
            });
            header.col(|ui| {
                sortable_header_cell(
                    ui,
                    &col_lbl("PROGRESS"),
                    false,
                    sort_order,
                    Some((SortOrder::Progress, SortOrder::Newest)),
                    palette,
                )
            });
            header.col(|ui| {
                sortable_header_cell(ui, &col_lbl("SPEED"), true, sort_order, None, palette)
            });
            header.col(|ui| {
                sortable_header_cell(ui, &col_lbl("ETA"), true, sort_order, None, palette)
            });
            header.col(|ui| {
                sortable_header_cell(
                    ui,
                    &col_lbl("ADDED"),
                    true,
                    sort_order,
                    Some((SortOrder::Newest, SortOrder::Oldest)),
                    palette,
                )
            });
        })
        .body(|body| {
            body.rows(62.0, visible.len(), |mut row| {
                let index = row.index();
                let download = visible[index];
                let is_selected = selected.contains(&download.id);
                row.set_selected(is_selected);
                row.col(|ui| {
                    paint_row_edge(ui, is_selected, true, palette);
                    name_cell(ui, download, is_selected, palette);
                });
                row.col(|ui| {
                    paint_row_edge(ui, is_selected, false, palette);
                    right_label(
                        ui,
                        download
                            .size
                            .map(|s| crate::i18n::format_size_locale(s, lang))
                            .unwrap_or_else(|| "—".to_string()),
                        palette.text,
                    );
                });
                row.col(|ui| {
                    paint_row_edge(ui, is_selected, false, palette);
                    status_cell(ui, download.status, lang, palette);
                });
                row.col(|ui| {
                    paint_row_edge(ui, is_selected, false, palette);
                    progress_cell(ui, download, lang, palette);
                });
                row.col(|ui| {
                    paint_row_edge(ui, is_selected, false, palette);
                    speed_cell(
                        ui,
                        download.status,
                        speeds.get(&download.id).copied(),
                        lang,
                        palette,
                    );
                });
                row.col(|ui| {
                    paint_row_edge(ui, is_selected, false, palette);
                    let eta = eta_seconds(
                        download.size,
                        download.downloaded,
                        speeds.get(&download.id).copied().unwrap_or(0),
                    );
                    right_label(ui, format_eta(eta), palette.muted);
                });
                row.col(|ui| {
                    paint_row_edge(ui, is_selected, false, palette);
                    right_label(
                        ui,
                        format_relative_time(now, download.created_at),
                        palette.faint,
                    );
                });

                let response = row.response();
                if response.clicked() {
                    actions.push(Action::SelectRow {
                        id: download.id,
                        ctrl: mods.ctrl || mods.command,
                        shift: mods.shift,
                    });
                }
                if response.double_clicked() {
                    if download.status == DownloadStatus::Done {
                        actions.push(Action::Open(download.id));
                    } else {
                        actions.push(Action::ShowProgress(download.id));
                    }
                }
                response.context_menu(|ui| {
                    if ui
                        .add_enabled(
                            download.status == DownloadStatus::Done,
                            egui::Button::new("Open"),
                        )
                        .clicked()
                    {
                        actions.push(Action::Open(download.id));
                        ui.close();
                    }
                    if ui
                        .add_enabled(
                            download.status == DownloadStatus::Done,
                            egui::Button::new("Open with..."),
                        )
                        .clicked()
                    {
                        actions.push(Action::OpenWith(download.id));
                        ui.close();
                    }
                    if ui.button("Open folder").clicked() {
                        actions.push(Action::OpenFolder(download.id));
                        ui.close();
                    }
                    if ui.button("Download progress").clicked() {
                        actions.push(Action::ShowProgress(download.id));
                        ui.close();
                    }
                    if ui.button("Properties\tAlt+Enter").clicked() {
                        actions.push(Action::OpenProperties(download.id));
                        ui.close();
                    }
                    if download.status == DownloadStatus::Done {
                        if ui.button("Move / Rename...\tCtrl+M").clicked() {
                            actions.push(Action::OpenProperties(download.id));
                            ui.close();
                        }
                        if ui.button("Verify checksum (SHA-256)...").clicked() {
                            actions.push(Action::OpenProperties(download.id));
                            actions.push(Action::VerifyChecksum(download.id));
                            ui.close();
                        }
                    }
                    if ui.button("Copy URL").clicked() {
                        actions.push(Action::CopyUrl(download.url.clone()));
                        ui.close();
                    }
                    if ui.button("Copy file path").clicked() {
                        actions.push(Action::CopyPath(download.save_path.clone()));
                        ui.close();
                    }
                    ui.separator();
                    if ui
                        .add_enabled(
                            matches!(
                                download.status,
                                DownloadStatus::Downloading | DownloadStatus::Queued
                            ),
                            egui::Button::new("Pause"),
                        )
                        .clicked()
                    {
                        actions.push(Action::Pause(download.id));
                        ui.close();
                    }
                    if ui
                        .add_enabled(
                            matches!(
                                download.status,
                                DownloadStatus::Paused | DownloadStatus::Error
                            ),
                            egui::Button::new("Resume"),
                        )
                        .clicked()
                    {
                        actions.push(Action::Resume(download.id));
                        ui.close();
                    }
                    if ui
                        .add_enabled(
                            !matches!(
                                download.status,
                                DownloadStatus::Downloading | DownloadStatus::Queued
                            ),
                            egui::Button::new("Redownload"),
                        )
                        .clicked()
                    {
                        actions.push(Action::Redownload(download.id));
                        ui.close();
                    }
                    if ui.button("Remove...\tDel").clicked() {
                        let ids = if selected.len() > 1 && selected.contains(&download.id) {
                            selected.iter().copied().collect()
                        } else {
                            vec![download.id]
                        };
                        actions.push(Action::ConfirmRemove(ids, ui.input(|i| i.modifiers.shift)));
                        ui.close();
                    }
                });
            });
        });
}

fn paint_row_edge(ui: &egui::Ui, selected: bool, first_col: bool, palette: &Palette) {
    let rect = ui.max_rect();
    if selected && first_col {
        ui.painter().rect_filled(
            Rect::from_min_size(rect.left_top(), Vec2::new(3.0, rect.height())),
            0.0,
            palette.accent,
        );
    }
    let y = rect.bottom() - 0.5;
    ui.painter().line_segment(
        [egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)],
        Stroke::new(1.0, palette.border.gamma_multiply(0.55)),
    );
}

fn sortable_header_cell(
    ui: &mut egui::Ui,
    label: &str,
    right: bool,
    sort_order: &mut SortOrder,
    toggle: Option<(SortOrder, SortOrder)>,
    palette: &Palette,
) {
    let rect = ui.max_rect();
    let response = if toggle.is_some() {
        ui.interact(
            rect,
            ui.make_persistent_id(format!("hdr_{label}")),
            Sense::click(),
        )
        .on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        ui.interact(
            rect,
            ui.make_persistent_id(format!("hdr_{label}")),
            Sense::hover(),
        )
    };
    let fill = if toggle.is_some() && response.hovered() {
        palette.selected
    } else {
        palette.surface_alt
    };
    ui.painter().rect_filled(rect, 0.0, fill);
    let y = rect.bottom() - 0.5;
    ui.painter().line_segment(
        [egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)],
        Stroke::new(1.0, palette.border),
    );

    let indicator = toggle.and_then(|(first, second)| {
        if *sort_order == first {
            Some(true)
        } else if *sort_order == second && second != SortOrder::Newest {
            Some(false)
        } else {
            None
        }
    });
    if let Some((first, second)) = toggle
        && response.clicked()
    {
        *sort_order = if *sort_order == first { second } else { first };
    }

    let text_color = if indicator.is_some() {
        palette.accent
    } else {
        palette.muted
    };
    ui.add_space(10.0);
    if right {
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.add_space(8.0);
            if let Some(asc_or_primary) = indicator {
                paint_sort_arrow(ui, asc_or_primary, text_color);
                ui.add_space(4.0);
            }
            ui.label(RichText::new(label).size(9.5).strong().color(text_color));
        });
    } else {
        ui.label(RichText::new(label).size(9.5).strong().color(text_color));
        if let Some(asc_or_primary) = indicator {
            ui.add_space(5.0);
            paint_sort_arrow(ui, asc_or_primary, text_color);
        }
    }
}

fn paint_sort_arrow(ui: &mut egui::Ui, primary: bool, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(7.0, 5.0), Sense::hover());
    let c = rect.center();
    let dy = if primary { 1.4 } else { -1.4 };
    ui.painter().line(
        vec![
            egui::pos2(c.x - 2.6, c.y - dy),
            egui::pos2(c.x, c.y + dy),
            egui::pos2(c.x + 2.6, c.y - dy),
        ],
        Stroke::new(1.2, color),
    );
}

fn name_cell(ui: &mut egui::Ui, download: &Download, selected: bool, palette: &Palette) {
    ui.set_max_width(430.0);
    ui.horizontal(|ui| {
        super::icons::draw_file_icon(
            ui,
            &download.file_name,
            download.category,
            download.kind == "video",
            40.0,
        );
        ui.add_space(8.0);
        ui.vertical(|ui| {
            let name = RichText::new(&download.file_name)
                .size(12.5)
                .color(if selected {
                    palette.accent
                } else {
                    palette.text
                });
            ui.add(egui::Label::new(name).truncate());
            ui.label(
                RichText::new(host_of(download))
                    .size(10.0)
                    .color(palette.faint),
            );
        });
    });
}

#[cfg(test)]
fn extension_label(file_name: &str) -> String {
    let extension = Path::new(file_name)
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or("FILE")
        .to_ascii_uppercase();
    extension.chars().take(4).collect()
}

fn progress_fraction(size: Option<u64>, downloaded: u64, status: DownloadStatus) -> f32 {
    match (size, status) {
        (_, DownloadStatus::Done) => 1.0,
        (Some(size), _) if size > 0 => (downloaded as f32 / size as f32).clamp(0.0, 1.0),
        _ => 0.0,
    }
}

fn ease_in_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    if t < 0.5 {
        4.0 * t * t * t
    } else {
        1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
    }
}

fn scale_pt(rect: Rect, x: f32, y: f32) -> egui::Pos2 {
    egui::pos2(
        rect.left() + (x / 24.0) * rect.width(),
        rect.top() + (y / 24.0) * rect.height(),
    )
}

fn paint_arc(
    painter: &egui::Painter,
    center: egui::Pos2,
    radius: f32,
    start_angle: f32,
    sweep_angle: f32,
    steps: usize,
    stroke: Stroke,
) {
    let points: Vec<egui::Pos2> = (0..=steps)
        .map(|step| {
            let angle = start_angle + sweep_angle * (step as f32 / steps as f32);
            center + Vec2::new(angle.cos(), angle.sin()) * radius
        })
        .collect();
    painter.line(points, stroke);
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

fn paint_gradient_fill(painter: &egui::Painter, rect: Rect, stops: &[Color32; 3]) {
    if rect.width() <= 6.0 {
        painter.rect_filled(rect, 2.5, stops[1]);
        return;
    }
    let radius = rect.height() * 0.5;
    let cap_w = (radius * 2.0).min(rect.width());
    painter.rect_filled(
        Rect::from_min_size(rect.min, Vec2::new(cap_w, rect.height())),
        radius,
        stops[0],
    );
    painter.rect_filled(
        Rect::from_min_max(egui::pos2(rect.right() - cap_w, rect.top()), rect.max),
        radius,
        stops[2],
    );
    let inner_left = rect.left() + radius;
    let inner_right = rect.right() - radius;
    let span = (inner_right - inner_left).max(0.0);
    if span <= 0.5 {
        return;
    }
    let steps = 12usize;
    for idx in 0..steps {
        let t0 = idx as f32 / steps as f32;
        let t1 = (idx + 1) as f32 / steps as f32;
        let t_mid = (t0 + t1) * 0.5;
        let color = if t_mid < 0.5 {
            lerp_color(stops[0], stops[1], t_mid * 2.0)
        } else {
            lerp_color(stops[1], stops[2], (t_mid - 0.5) * 2.0)
        };
        let x0 = inner_left + span * t0;
        let x1 = (inner_left + span * t1 + 0.5).min(inner_right);
        painter.rect_filled(
            Rect::from_min_max(egui::pos2(x0, rect.top()), egui::pos2(x1, rect.bottom())),
            0.0,
            color,
        );
    }
}

fn paint_skewed_shimmer(painter: &egui::Painter, rect: Rect, cycle: f32) {
    let shimmer_w = (rect.width() * 0.38).clamp(10.0, 26.0);
    let t = -0.4 + 1.7 * cycle;
    let x0 = rect.left() + rect.width() * t;
    let skew = rect.height() * 0.364;
    let pts = vec![
        egui::pos2(x0 + skew, rect.top()),
        egui::pos2(x0 + skew + shimmer_w, rect.top()),
        egui::pos2(x0 + shimmer_w, rect.bottom()),
        egui::pos2(x0, rect.bottom()),
    ];
    let clip = rect.shrink2(Vec2::new(1.0, 0.0));
    if clip.width() > 2.0 {
        painter
            .with_clip_rect(clip)
            .add(egui::Shape::convex_polygon(
                pts,
                Color32::WHITE.gamma_multiply(0.38),
                Stroke::NONE,
            ));
    }
}

fn status_cell(ui: &mut egui::Ui, status: DownloadStatus, lang: &str, palette: &Palette) {
    ui.add_space(8.0);
    let (en_label, fg) = match status {
        DownloadStatus::Downloading => ("Downloading", palette.sky),
        DownloadStatus::Queued => ("Queued", palette.amber_bright),
        DownloadStatus::Paused => ("Paused", palette.amber_bright),
        DownloadStatus::Done => ("Completed", palette.emerald_bright),
        DownloadStatus::Error => ("Error", palette.red_bright),
    };
    let label = crate::i18n::translate(lang, en_label).into_owned();
    let galley = ui
        .painter()
        .layout_no_wrap(label, FontId::proportional(9.5), fg);
    let pill_w = 7.0 + 10.0 + 5.0 + galley.size().x + 8.0;
    let (pill_rect, _) = ui.allocate_exact_size(Vec2::new(pill_w, 20.0), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(pill_rect, 10.0, fg.gamma_multiply(0.14));
    painter.rect_stroke(
        pill_rect,
        10.0,
        Stroke::new(1.0, fg.gamma_multiply(0.35)),
        egui::StrokeKind::Inside,
    );
    let icon_rect = Rect::from_center_size(
        egui::pos2(pill_rect.left() + 12.0, pill_rect.center().y),
        Vec2::splat(10.0),
    );
    paint_status_icon(ui, icon_rect, status, fg);
    painter.galley(
        egui::pos2(
            pill_rect.left() + 22.0,
            pill_rect.center().y - galley.size().y * 0.5,
        ),
        galley,
        fg,
    );
}

fn paint_status_icon(ui: &egui::Ui, rect: Rect, status: DownloadStatus, color: Color32) {
    let painter = ui.painter();
    let c = rect.center();
    match status {
        DownloadStatus::Downloading => {
            ui.ctx().request_repaint_after(Duration::from_millis(16));
            let t = ui.ctx().input(|i| i.time);
            painter.circle_stroke(c, 4.1, Stroke::new(1.2, color.gamma_multiply(0.25)));
            paint_arc(
                painter,
                c,
                4.1,
                (t * 4.2) as f32,
                100.0_f32.to_radians(),
                8,
                Stroke::new(1.4, color),
            );
            let phase = (t * 1.5).fract() as f32;
            painter.circle_filled(c, 1.3, color);
            painter.circle_stroke(
                c,
                1.3 + phase * 2.3,
                Stroke::new(1.0, color.gamma_multiply(0.75 * (1.0 - phase))),
            );
        }
        DownloadStatus::Queued => {
            ui.ctx().request_repaint_after(Duration::from_millis(30));
            let t = ui.ctx().input(|i| i.time);
            let phase = (t * 1.3).fract() as f32;
            painter.circle_filled(c, 2.0, color);
            painter.circle_stroke(
                c,
                2.0 + phase * 2.6,
                Stroke::new(1.0, color.gamma_multiply(0.7 * (1.0 - phase))),
            );
        }
        DownloadStatus::Paused => {
            painter.rect_filled(
                Rect::from_min_max(scale_pt(rect, 6.0, 4.0), scale_pt(rect, 10.0, 20.0)),
                0.8,
                color,
            );
            painter.rect_filled(
                Rect::from_min_max(scale_pt(rect, 14.0, 4.0), scale_pt(rect, 18.0, 20.0)),
                0.8,
                color,
            );
        }
        DownloadStatus::Done => {
            painter.circle_filled(c, 4.8, color.gamma_multiply(0.20));
            painter.line(
                vec![
                    scale_pt(rect, 20.0, 6.0),
                    scale_pt(rect, 9.0, 17.0),
                    scale_pt(rect, 4.0, 12.0),
                ],
                Stroke::new(1.5, color),
            );
        }
        DownloadStatus::Error => {
            painter.circle_filled(c, 4.8, color.gamma_multiply(0.20));
            painter.line_segment(
                [scale_pt(rect, 7.5, 7.5), scale_pt(rect, 16.5, 16.5)],
                Stroke::new(1.4, color),
            );
            painter.line_segment(
                [scale_pt(rect, 16.5, 7.5), scale_pt(rect, 7.5, 16.5)],
                Stroke::new(1.4, color),
            );
        }
    }
}

fn progress_cell(ui: &mut egui::Ui, download: &Download, lang: &str, palette: &Palette) {
    ui.add_space(6.0);
    let fraction = progress_fraction(download.size, download.downloaded, download.status);
    let is_downloading = download.status == DownloadStatus::Downloading;
    if is_downloading {
        ui.ctx().request_repaint_after(Duration::from_millis(16));
    }

    let (cell_rect, _) = ui.allocate_exact_size(Vec2::new(148.0, 20.0), Sense::hover());
    let cy = cell_rect.center().y;
    let painter = ui.painter();

    let ring_rect =
        Rect::from_center_size(egui::pos2(cell_rect.left() + 6.0, cy), Vec2::splat(12.0));
    let ring_center = ring_rect.center();
    let ring_radius = 4.8;
    painter.circle_stroke(ring_center, ring_radius, Stroke::new(1.5, palette.track));
    let ring_color = match download.status {
        DownloadStatus::Downloading => palette.sky,
        DownloadStatus::Done => palette.emerald_bright,
        DownloadStatus::Queued | DownloadStatus::Paused => palette.amber_bright,
        DownloadStatus::Error => palette.red_bright,
    };
    if fraction > 0.0 {
        paint_arc(
            painter,
            ring_center,
            ring_radius,
            -std::f32::consts::FRAC_PI_2,
            fraction * std::f32::consts::TAU,
            16,
            Stroke::new(1.6, ring_color),
        );
    }
    if download.status == DownloadStatus::Done {
        painter.line(
            vec![
                scale_pt(ring_rect, 18.0, 8.0),
                scale_pt(ring_rect, 10.5, 16.0),
                scale_pt(ring_rect, 6.5, 12.0),
            ],
            Stroke::new(1.3, ring_color),
        );
    }

    let track_rect = Rect::from_center_size(
        egui::pos2(cell_rect.left() + 18.0 + 42.0, cy),
        Vec2::new(84.0, 7.0),
    );
    painter.rect_filled(track_rect, 3.5, palette.track);
    painter.rect_stroke(
        track_rect,
        3.5,
        Stroke::new(1.0, palette.track_ring),
        egui::StrokeKind::Inside,
    );

    let inner_track = track_rect.shrink(1.0);
    if download.status == DownloadStatus::Done {
        painter.rect_filled(inner_track, 2.5, palette.emerald);
    } else if fraction > 0.0 {
        let fill_w = (inner_track.width() * fraction).clamp(5.0, inner_track.width());
        let fill_rect =
            Rect::from_min_size(inner_track.min, Vec2::new(fill_w, inner_track.height()));
        if is_downloading {
            paint_gradient_fill(
                painter,
                fill_rect,
                &[palette.blue, palette.indigo, palette.sky],
            );
            let t = ui.ctx().input(|i| i.time);
            let cycle = ease_in_out(((t / 1.6).fract()) as f32);
            paint_skewed_shimmer(painter, fill_rect, cycle);
        } else {
            let fill_color = match download.status {
                DownloadStatus::Queued | DownloadStatus::Paused => palette.amber_bright,
                DownloadStatus::Error => palette.red_bright,
                _ => palette.indigo,
            };
            painter.rect_filled(fill_rect, 2.5, fill_color);
        }
    } else if is_downloading {
        let t = ui.ctx().input(|i| i.time);
        let cycle = ease_in_out(((t / 1.6).fract()) as f32);
        let seg_w = 24.0_f32;
        let start_x = inner_track.left() - seg_w + (inner_track.width() + seg_w) * cycle;
        let seg_rect = Rect::from_min_max(
            egui::pos2(start_x.max(inner_track.left()), inner_track.top()),
            egui::pos2(
                (start_x + seg_w).min(inner_track.right()),
                inner_track.bottom(),
            ),
        );
        if seg_rect.width() > 1.0 {
            paint_gradient_fill(
                painter,
                seg_rect,
                &[palette.blue, palette.indigo, palette.sky],
            );
        }
    }

    let pct_color = match download.status {
        DownloadStatus::Downloading => palette.sky,
        DownloadStatus::Done => palette.emerald_bright,
        _ => palette.muted,
    };
    painter.text(
        egui::pos2(track_rect.right() + 7.0, cy),
        Align2::LEFT_CENTER,
        crate::i18n::format_percent_locale(fraction, lang),
        FontId::monospace(9.5),
        pct_color,
    );
}

fn speed_cell(
    ui: &mut egui::Ui,
    status: DownloadStatus,
    speed: Option<u64>,
    lang: &str,
    palette: &Palette,
) {
    let paused_text = crate::i18n::translate(lang, "Paused").into_owned();
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        ui.add_space(8.0);
        let active = status == DownloadStatus::Downloading && speed.unwrap_or(0) > 0;
        let (text, fg) = if active {
            (crate::i18n::format_speed_locale(speed, lang), palette.sky)
        } else if status == DownloadStatus::Paused {
            (paused_text, palette.amber_bright)
        } else if status == DownloadStatus::Done {
            ("—".to_string(), palette.emerald_bright)
        } else {
            ("—".to_string(), palette.faint)
        };
        let galley = ui
            .painter()
            .layout_no_wrap(text, FontId::monospace(9.0), fg);
        let badge_w = 6.0 + 10.0 + 4.0 + galley.size().x + 6.0;
        let (badge_rect, _) = ui.allocate_exact_size(Vec2::new(badge_w, 20.0), Sense::hover());
        let painter = ui.painter();
        let border = if active {
            palette.sky.gamma_multiply(0.35)
        } else {
            palette.track_ring
        };
        painter.rect_filled(badge_rect, 6.0, palette.speed_bg);
        painter.rect_stroke(
            badge_rect,
            6.0,
            Stroke::new(1.0, border),
            egui::StrokeKind::Inside,
        );
        let bolt_rect = Rect::from_center_size(
            egui::pos2(badge_rect.left() + 6.0 + 5.0, badge_rect.center().y),
            Vec2::splat(10.0),
        );
        if active {
            ui.ctx().request_repaint_after(Duration::from_millis(16));
            let t = ui.ctx().input(|i| i.time);
            let pulse = 0.5 + 0.5 * (t * 5.0).sin() as f32;
            painter.circle_filled(
                bolt_rect.center(),
                4.5,
                palette.sky.gamma_multiply(0.18 + 0.18 * pulse),
            );
        }
        let bolt_color = if active {
            palette.sky
        } else if status == DownloadStatus::Paused {
            palette.amber_bright.gamma_multiply(0.8)
        } else if status == DownloadStatus::Done {
            palette.emerald_bright.gamma_multiply(0.8)
        } else {
            palette.faint.gamma_multiply(0.65)
        };
        paint_lightning_bolt(painter, bolt_rect, bolt_color);
        painter.galley(
            egui::pos2(
                bolt_rect.right() + 4.0,
                badge_rect.center().y - galley.size().y * 0.5,
            ),
            galley,
            fg,
        );
    });
}

fn paint_lightning_bolt(painter: &egui::Painter, rect: Rect, color: Color32) {
    let p13_2 = scale_pt(rect, 13.0, 2.0);
    let p3_14 = scale_pt(rect, 3.0, 14.0);
    let p12_14 = scale_pt(rect, 12.0, 14.0);
    let p11_22 = scale_pt(rect, 11.0, 22.0);
    let p21_10 = scale_pt(rect, 21.0, 10.0);
    let p12_10 = scale_pt(rect, 12.0, 10.0);
    for tri in [
        vec![p13_2, p12_10, p3_14],
        vec![p12_10, p12_14, p3_14],
        vec![p12_10, p21_10, p12_14],
        vec![p12_14, p21_10, p11_22],
    ] {
        painter.add(egui::Shape::convex_polygon(tri, color, Stroke::NONE));
    }
    painter.line(
        vec![p13_2, p3_14, p12_14, p11_22, p21_10, p12_10, p13_2],
        Stroke::new(0.8, color),
    );
}

fn right_label(ui: &mut egui::Ui, text: String, color: Color32) {
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        ui.add_space(8.0);
        ui.label(RichText::new(text).size(11.0).color(color));
    });
}

fn host_of(download: &Download) -> String {
    let raw = download.final_url.as_deref().unwrap_or(&download.url);
    Url::parse(raw)
        .ok()
        .and_then(|url| url.host_str().map(str::to_string))
        .unwrap_or_else(|| "—".to_string())
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
    use super::{ease_in_out, extension_label, progress_fraction};
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
