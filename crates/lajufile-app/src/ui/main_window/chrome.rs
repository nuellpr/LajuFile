use egui::{Align, Align2, Color32, FontId, Layout, Margin, Rect, RichText, Sense, Stroke, Vec2};
use lajufile_core::settings::Theme;
use lajufile_core::store::{Category, Download, DownloadStatus};

use super::super::dialogs::lerp_color;
use super::super::{Action, Filter};
use super::Palette;
use crate::app::LajuFileApp;

pub(super) fn draw_menu_bar(
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
    Category(super::super::icons::IconKey),
}

pub(super) fn draw_toolbar(
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

pub(super) fn draw_sidebar(
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
                    Some(SidebarIcon::Category(super::super::icons::category_key(
                        category,
                    ))),
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
                super::super::icons::paint_icon_at(ui, icon_rect, key);
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
