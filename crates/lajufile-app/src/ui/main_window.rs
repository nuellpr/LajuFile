//! Main window: menu, toolbar, sidebar, download table, and status bar.

#[cfg(test)]
use std::path::Path;
use std::time::Duration;

use egui::{Align, Align2, Color32, FontId, Layout, Margin, Rect, RichText, Sense, Stroke, Vec2};
use egui_extras::{Column, TableBuilder};
use lajufile_core::settings::Theme;
use lajufile_core::store::{Category, Download, DownloadStatus, now_millis};
use url::Url;

use super::{Action, Filter, format::*};
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
    green: Color32,
    green_soft: Color32,
    amber: Color32,
    amber_soft: Color32,
    red: Color32,
    red_soft: Color32,
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
                green: Color32::from_rgb(0x17, 0x93, 0x6A),
                green_soft: Color32::from_rgb(0xE6, 0xF6, 0xEF),
                amber: Color32::from_rgb(0xB4, 0x53, 0x09),
                amber_soft: Color32::from_rgb(0xFE, 0xF3, 0xE2),
                red: Color32::from_rgb(0xC2, 0x41, 0x3A),
                red_soft: Color32::from_rgb(0xFD, 0xEC, 0xEC),
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
                green: Color32::from_rgb(0x3D, 0xDC, 0x97),
                green_soft: Color32::from_rgb(0x12, 0x32, 0x27),
                amber: Color32::from_rgb(0xF5, 0xB4, 0x4A),
                amber_soft: Color32::from_rgb(0x33, 0x26, 0x0F),
                red: Color32::from_rgb(0xFF, 0x6B, 0x6B),
                red_soft: Color32::from_rgb(0x3A, 0x1A, 0x1A),
            },
        }
    }
}

pub fn draw(app: &mut LajuFileApp, ui: &mut egui::Ui) -> Vec<Action> {
    let theme = app
        .snapshot
        .as_ref()
        .map(|snapshot| snapshot.settings.theme.clone())
        .unwrap_or_else(|| app.theme.clone());
    let palette = Palette::for_theme(&theme);
    let downloads = app
        .snapshot
        .as_ref()
        .map(|snapshot| snapshot.downloads.clone())
        .unwrap_or_default();
    let speeds = app
        .snapshot
        .as_ref()
        .map(|snapshot| snapshot.speeds.clone())
        .unwrap_or_default();
    let mut actions = Vec::new();
    ui.painter().rect_filled(ui.max_rect(), 0.0, palette.window);
    ui.spacing_mut().item_spacing = Vec2::ZERO;

    draw_menu_bar(ui, &palette, &mut actions);
    draw_toolbar(ui, &palette, app.selected, &downloads, &mut actions);

    egui::Panel::bottom("lajufile_status")
        .exact_size(32.0)
        .frame(
            egui::Frame::new()
                .fill(palette.bar)
                .stroke(Stroke::new(1.0, palette.border))
                .inner_margin(Margin::symmetric(12, 0)),
        )
        .show(ui, |ui| {
            draw_status_bar(ui, &downloads, &speeds, &palette);
        });

    let body_height = ui.available_height().max(100.0);
    ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
        ui.allocate_ui_with_layout(
            Vec2::new(232.0, body_height),
            Layout::top_down(Align::Min),
            |ui| {
                ui.set_min_height(body_height);
                draw_sidebar(ui, app, &downloads, &palette);
            },
        );
        ui.allocate_ui_with_layout(
            Vec2::new(ui.available_width(), body_height),
            Layout::top_down(Align::Min),
            |ui| {
                ui.set_min_height(body_height);
                draw_content(ui, app, &downloads, &speeds, &palette, &mut actions);
            },
        );
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

fn draw_menu_bar(ui: &mut egui::Ui, palette: &Palette, actions: &mut Vec<Action>) {
    egui::Frame::new()
        .fill(palette.bar)
        .stroke(Stroke::new(1.0, palette.border))
        .inner_margin(Margin::symmetric(12, 0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.set_height(28.0);
            ui.horizontal_centered(|ui| {
                ui.set_width(ui.available_width());
                ui.menu_button("File", |ui| {
                    if ui.button("Add URL...").clicked() {
                        actions.push(Action::OpenAdd);
                        ui.close();
                    }
                    if ui.button("Settings...").clicked() {
                        actions.push(Action::OpenSettings);
                        ui.close();
                    }
                    ui.separator();
                    if ui.button("Exit").clicked() {
                        actions.push(Action::Exit);
                        ui.close();
                    }
                });
                ui.menu_button("Tasks", |ui| {
                    if ui.button("Add new download...").clicked() {
                        actions.push(Action::OpenAdd);
                        ui.close();
                    }
                    if ui.button("Add batch download...").clicked() {
                        actions.push(Action::OpenBatch);
                        ui.close();
                    }
                    ui.separator();
                    if ui.button("Resume all").clicked() {
                        actions.push(Action::ResumeAll);
                        ui.close();
                    }
                    if ui.button("Stop all").clicked() {
                        actions.push(Action::PauseAll);
                        ui.close();
                    }
                    if ui.button("Delete completed").clicked() {
                        actions.push(Action::RemoveCompleted);
                        ui.close();
                    }
                });
                ui.menu_button("View", |ui| {
                    if ui.button("Light").clicked() {
                        actions.push(Action::SetTheme(Theme::Light));
                        ui.close();
                    }
                    if ui.button("Dark").clicked() {
                        actions.push(Action::SetTheme(Theme::Dark));
                        ui.close();
                    }
                });
                ui.menu_button("Help", |ui| {
                    ui.label("LajuFile 0.1.0");
                    ui.label("Segmented download manager for Windows");
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(RichText::new("v0.1.0").size(10.0).color(palette.faint));
                });
            });
        });
}

fn draw_toolbar(
    ui: &mut egui::Ui,
    palette: &Palette,
    selected: Option<i64>,
    downloads: &[Download],
    actions: &mut Vec<Action>,
) {
    let selected_download =
        selected.and_then(|id| downloads.iter().find(|download| download.id == id));
    let can_pause = selected_download.is_some_and(|download| {
        matches!(
            download.status,
            DownloadStatus::Downloading | DownloadStatus::Queued
        )
    });
    let can_resume = selected_download.is_some_and(|download| {
        matches!(
            download.status,
            DownloadStatus::Paused | DownloadStatus::Error
        )
    });
    let has_active = downloads.iter().any(|download| {
        matches!(
            download.status,
            DownloadStatus::Downloading | DownloadStatus::Queued
        )
    });
    let has_completed = downloads
        .iter()
        .any(|download| download.status == DownloadStatus::Done);

    egui::Frame::new()
        .fill(palette.bar)
        .stroke(Stroke::new(1.0, palette.border))
        .inner_margin(Margin::symmetric(12, 0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.set_height(60.0);
            ui.horizontal_centered(|ui| {
                ui.set_width(ui.available_width());
                if toolbar_button(ui, "Add URL", true, true, palette) {
                    actions.push(Action::OpenAdd);
                }
                if toolbar_button(ui, "Batch", true, false, palette) {
                    actions.push(Action::OpenBatch);
                }
                ui.add_space(4.0);
                ui.separator();
                ui.add_space(4.0);
                if toolbar_button(ui, "Resume", can_resume, false, palette) {
                    if let Some(id) = selected {
                        actions.push(Action::Resume(id));
                    }
                }
                if toolbar_button(ui, "Stop", can_pause, false, palette) {
                    if let Some(id) = selected {
                        actions.push(Action::Pause(id));
                    }
                }
                if toolbar_button(ui, "Stop All", has_active, false, palette) {
                    actions.push(Action::PauseAll);
                }
                if toolbar_button(ui, "Delete", selected.is_some(), false, palette) {
                    if let Some(id) = selected {
                        actions.push(Action::Remove(id));
                    }
                }
                if toolbar_button(ui, "Delete Completed", has_completed, false, palette) {
                    actions.push(Action::RemoveCompleted);
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if toolbar_button(ui, "Options", true, false, palette) {
                        actions.push(Action::OpenSettings);
                    }
                });
            });
        });
}

fn toolbar_button(
    ui: &mut egui::Ui,
    label: &str,
    enabled: bool,
    primary: bool,
    palette: &Palette,
) -> bool {
    let text = RichText::new(label).size(13.0).color(if primary {
        Color32::WHITE
    } else {
        palette.text
    });
    let mut button = egui::Button::new(text)
        .min_size(Vec2::new(0.0, 32.0))
        .corner_radius(6);
    if primary {
        button = button.fill(palette.accent);
    } else {
        button = button
            .fill(palette.surface)
            .stroke(Stroke::new(1.0, palette.border));
    }
    ui.add_enabled(enabled, button).clicked()
}

fn draw_sidebar(
    ui: &mut egui::Ui,
    app: &mut LajuFileApp,
    downloads: &[Download],
    palette: &Palette,
) {
    let (bridge_state, bridge_port, bridge_message) = app
        .snapshot
        .as_ref()
        .map(|snapshot| {
            (
                snapshot.bridge.state,
                snapshot.bridge.port,
                snapshot.bridge.message.clone(),
            )
        })
        .unwrap_or((BridgeState::Starting, 0, None));
    egui::Frame::new()
        .fill(palette.sidebar)
        .stroke(Stroke::new(1.0, palette.border))
        .inner_margin(Margin::symmetric(12, 16))
        .show(ui, |ui| {
            ui.set_width(232.0);
            section_label(ui, "LIBRARY", palette);

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
                "All downloads",
                all,
                Filter::All,
                app.filter,
                None,
                palette,
            ) {
                app.filter = Filter::All;
            }
            if sidebar_row(
                ui,
                "Unfinished",
                unfinished,
                Filter::Unfinished,
                app.filter,
                None,
                palette,
            ) {
                app.filter = Filter::Unfinished;
            }
            if sidebar_row(
                ui,
                "Finished",
                finished,
                Filter::Finished,
                app.filter,
                None,
                palette,
            ) {
                app.filter = Filter::Finished;
            }

            ui.add_space(12.0);
            section_label(ui, "CATEGORIES", palette);
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
                    filter.label(),
                    count,
                    filter,
                    app.filter,
                    Some(super::icons::category_key(category)),
                    palette,
                ) {
                    app.filter = filter;
                }
            }

            let remaining = ui.available_height();
            ui.add_space(remaining.clamp(8.0, 24.0));
            egui::Frame::new()
                .fill(palette.surface)
                .stroke(Stroke::new(1.0, palette.border))
                .corner_radius(8)
                .inner_margin(Margin::symmetric(10, 10))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(
                        RichText::new("BROWSER CAPTURE")
                            .size(9.0)
                            .strong()
                            .color(palette.faint),
                    );
                    ui.add_space(4.0);
                    let (status, color, detail) = match bridge_state {
                        BridgeState::Bound => (
                            "Connected",
                            palette.green,
                            format!("127.0.0.1:{bridge_port}"),
                        ),
                        BridgeState::Error => (
                            "Bridge unavailable",
                            palette.red,
                            bridge_message
                                .clone()
                                .unwrap_or_else(|| "Check Settings".to_string()),
                        ),
                        BridgeState::Starting => (
                            "Starting bridge",
                            palette.muted,
                            format!("Port {bridge_port}"),
                        ),
                    };
                    ui.label(RichText::new(status).size(12.0).color(color));
                    ui.label(RichText::new(detail).size(10.0).color(palette.faint));
                    if ui.small_button("Manage").clicked() {
                        if let Some(snapshot) = &app.snapshot {
                            app.settings_draft = snapshot.settings.clone();
                        }
                        app.show_settings = true;
                    }
                });
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
    icon: Option<super::icons::IconKey>,
    palette: &Palette,
) -> bool {
    let selected = filter == active;
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 32.0), Sense::click());
    let fill = if selected {
        palette.selected
    } else if response.hovered() {
        palette.surface_alt
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, 6.0, fill);
    if selected {
        ui.painter().rect_filled(
            Rect::from_min_size(rect.left_top(), Vec2::new(3.0, rect.height())),
            2,
            palette.accent,
        );
    }
    let mut x = rect.left() + 10.0;
    if let Some(key) = icon {
        let icon_rect =
            Rect::from_center_size(egui::pos2(x + 10.0, rect.center().y), Vec2::splat(20.0));
        super::icons::paint_icon_at(ui, icon_rect, key);
        x += 26.0;
    }
    ui.painter().text(
        egui::pos2(x, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        FontId::proportional(13.0),
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

fn draw_content(
    ui: &mut egui::Ui,
    app: &mut LajuFileApp,
    downloads: &[Download],
    speeds: &std::collections::HashMap<i64, u64>,
    palette: &Palette,
    actions: &mut Vec<Action>,
) {
    let visible: Vec<&Download> = downloads
        .iter()
        .filter(|download| app.filter.matches(download))
        .collect();
    let selected = app.selected;
    let now = now_millis();

    egui::Frame::new()
        .fill(palette.window)
        .inner_margin(Margin::symmetric(20, 16))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.set_width(ui.available_width());
                ui.label(
                    RichText::new(app.filter.label())
                        .size(22.0)
                        .strong()
                        .color(palette.text),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(
                        RichText::new("Sort: Newest")
                            .size(11.0)
                            .color(palette.muted),
                    );
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

            draw_table(ui, &visible, speeds, selected, now, palette, actions);

            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.set_width(ui.available_width());
                ui.label(
                    RichText::new(format!(
                        "Showing {} of {} downloads",
                        visible.len(),
                        downloads.len()
                    ))
                    .size(10.0)
                    .color(palette.faint),
                );
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

fn draw_table(
    ui: &mut egui::Ui,
    visible: &[&Download],
    speeds: &std::collections::HashMap<i64, u64>,
    selected: Option<i64>,
    now: i64,
    palette: &Palette,
    actions: &mut Vec<Action>,
) {
    let table_height = (ui.available_height() - 34.0).max(120.0);
    TableBuilder::new(ui)
        .striped(false)
        .sense(Sense::click())
        .cell_layout(Layout::left_to_right(Align::Center))
        .column(Column::remainder())
        .column(Column::exact(80.0))
        .column(Column::exact(100.0))
        .column(Column::exact(170.0))
        .column(Column::exact(76.0))
        .column(Column::exact(64.0))
        .column(Column::exact(86.0))
        .auto_shrink([false, false])
        .max_scroll_height(table_height)
        .header(34.0, |mut header| {
            header.col(|ui| header_cell(ui, "NAME", false, palette));
            header.col(|ui| header_cell(ui, "SIZE", true, palette));
            header.col(|ui| header_cell(ui, "STATUS", false, palette));
            header.col(|ui| header_cell(ui, "PROGRESS", false, palette));
            header.col(|ui| header_cell(ui, "SPEED", true, palette));
            header.col(|ui| header_cell(ui, "ETA", true, palette));
            header.col(|ui| header_cell(ui, "ADDED", true, palette));
        })
        .body(|body| {
            body.rows(62.0, visible.len(), |mut row| {
                let index = row.index();
                let download = visible[index];
                row.col(|ui| name_cell(ui, download, selected == Some(download.id), palette));
                row.col(|ui| {
                    right_label(
                        ui,
                        download
                            .size
                            .map(format_size)
                            .unwrap_or_else(|| "—".to_string()),
                        palette.text,
                    );
                });
                row.col(|ui| status_cell(ui, download.status, palette));
                row.col(|ui| progress_cell(ui, download, palette));
                row.col(|ui| {
                    right_label(
                        ui,
                        format_speed(speeds.get(&download.id).copied()),
                        palette.muted,
                    );
                });
                row.col(|ui| {
                    let eta = eta_seconds(
                        download.size,
                        download.downloaded,
                        speeds.get(&download.id).copied().unwrap_or(0),
                    );
                    right_label(ui, format_eta(eta), palette.muted);
                });
                row.col(|ui| {
                    right_label(
                        ui,
                        format_relative_time(now, download.created_at),
                        palette.faint,
                    );
                });

                let response = row.response();
                if response.clicked() {
                    actions.push(Action::Select(download.id));
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
                    if ui.button("Open folder").clicked() {
                        actions.push(Action::OpenFolder(download.id));
                        ui.close();
                    }
                    if ui.button("Download progress").clicked() {
                        actions.push(Action::ShowProgress(download.id));
                        ui.close();
                    }
                    if ui.button("Copy URL").clicked() {
                        actions.push(Action::CopyUrl(download.url.clone()));
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
                    if ui.button("Remove").clicked() {
                        actions.push(Action::Remove(download.id));
                        ui.close();
                    }
                });
            });
        });
}

fn header_cell(ui: &mut egui::Ui, label: &str, right: bool, palette: &Palette) {
    ui.add_space(8.0);
    if right {
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.add_space(8.0);
            ui.label(RichText::new(label).size(9.0).strong().color(palette.faint));
        });
    } else {
        ui.label(RichText::new(label).size(9.0).strong().color(palette.faint));
    }
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

fn status_cell(ui: &mut egui::Ui, status: DownloadStatus, palette: &Palette) {
    ui.add_space(8.0);
    let (label, fg, bg) = match status {
        DownloadStatus::Downloading => ("Downloading", palette.accent, palette.selected),
        DownloadStatus::Queued => ("Queued", palette.amber, palette.amber_soft),
        DownloadStatus::Paused => ("Paused", palette.muted, palette.surface_alt),
        DownloadStatus::Done => ("Completed", palette.green, palette.green_soft),
        DownloadStatus::Error => ("Error", palette.red, palette.red_soft),
    };
    egui::Frame::new()
        .fill(bg)
        .corner_radius(12)
        .inner_margin(Margin::symmetric(8, 3))
        .show(ui, |ui| {
            ui.label(RichText::new(label).size(10.0).color(fg));
        });
}

fn progress_cell(ui: &mut egui::Ui, download: &Download, palette: &Palette) {
    ui.add_space(8.0);
    let fraction = match (download.size, download.status) {
        (_, DownloadStatus::Done) => 1.0,
        (Some(size), _) if size > 0 => (download.downloaded as f32 / size as f32).clamp(0.0, 1.0),
        _ => 0.0,
    };
    ui.horizontal(|ui| {
        ui.add(
            egui::ProgressBar::new(fraction)
                .desired_width(120.0)
                .fill(palette.accent)
                .text(""),
        );
        ui.label(
            RichText::new(format!("{:.0}%", fraction * 100.0))
                .size(9.0)
                .color(palette.muted),
        );
    });
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
    downloads: &[Download],
    speeds: &std::collections::HashMap<i64, u64>,
    palette: &Palette,
) {
    let active = downloads
        .iter()
        .filter(|download| download.status == DownloadStatus::Downloading)
        .count();
    let queued = downloads
        .iter()
        .filter(|download| download.status == DownloadStatus::Queued)
        .count();
    let speed: u64 = speeds.values().sum();
    let speed_text = if speed > 0 {
        format!("{}/s", format_size(speed))
    } else {
        "0 B/s".to_string()
    };

    ui.set_width(ui.available_width());
    ui.set_height(ui.available_height());
    ui.horizontal_centered(|ui| {
        ui.label(
            RichText::new(format!("{active} active · {queued} queued · {speed_text}"))
                .size(10.0)
                .color(palette.muted),
        );
    });
}

#[cfg(test)]
mod tests {
    use super::extension_label;

    #[test]
    fn extension_label_handles_unicode() {
        assert_eq!(extension_label("report.文档"), "文档");
        assert_eq!(extension_label("archive.backup"), "BACK");
    }
}
