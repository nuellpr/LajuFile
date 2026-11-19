#[cfg(test)]
use std::path::Path;
use std::time::Duration;

use egui::{Align, Align2, Color32, FontId, Layout, Rect, RichText, Sense, Stroke, Vec2};
use egui_extras::{Column, TableBuilder};
use lajufile_core::store::{Download, DownloadStatus, now_millis};
use url::Url;

use super::super::dialogs::lerp_color;
use super::super::{Action, SortOrder, format::*};
use super::Palette;

pub(super) fn draw_table(
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
        super::super::icons::draw_file_icon(
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
pub(super) fn extension_label(file_name: &str) -> String {
    let extension = Path::new(file_name)
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or("FILE")
        .to_ascii_uppercase();
    extension.chars().take(4).collect()
}

pub(super) fn progress_fraction(size: Option<u64>, downloaded: u64, status: DownloadStatus) -> f32 {
    match (size, status) {
        (_, DownloadStatus::Done) => 1.0,
        (Some(size), _) if size > 0 => (downloaded as f32 / size as f32).clamp(0.0, 1.0),
        _ => 0.0,
    }
}

pub(super) fn ease_in_out(t: f32) -> f32 {
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
