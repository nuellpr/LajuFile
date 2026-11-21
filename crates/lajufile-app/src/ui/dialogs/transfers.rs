use std::path::PathBuf;

use egui::{
    Align, Align2, Color32, FontId, Layout, Margin, Rect, RichText, Sense, Stroke, Ui, Vec2,
};
use lajufile_core::checksum::checksums_match;
use lajufile_core::settings::Settings;
use lajufile_core::store::{Download, DownloadStatus, Segment, now_millis};

use super::super::Action;
use super::super::format::{
    eta_seconds, format_eta, format_relative_time, format_size, format_speed,
};
use super::{
    SettingsPalette, badge_pill, card_kicker, draw_h_divider, figma_button, lerp_color,
    modal_header, non_empty, settings_card,
};

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
                        super::super::icons::draw_file_icon(
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
    pub speed_limit_kbps: u64,
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
            checksum_input: download.expected_sha256.clone().unwrap_or_default(),
            speed_limit_kbps: download.speed_limit_kbps,
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
                        super::super::icons::draw_file_icon(
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
                                            crate::i18n::format_grouped_int(
                                                s,
                                                crate::i18n::NumberStyle::CommaDot
                                            )
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
                                        if let Some(cs) = download.checksum_status.as_deref() {
                                            ui.label(
                                                RichText::new("SHA-256")
                                                    .size(11.5)
                                                    .color(sp.muted),
                                            );
                                            let (text, color) = match cs {
                                                "verified" => ("Verified ✓", sp.green),
                                                _ => ("Mismatch ✗", sp.red),
                                            };
                                            ui.label(
                                                RichText::new(text)
                                                    .size(11.5)
                                                    .strong()
                                                    .color(color),
                                            );
                                            ui.end_row();
                                        }
                                    });

                                ui.add_space(8.0);
                                draw_h_divider(ui, sp.border);
                                ui.add_space(8.0);
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new("Per-download speed limit (KB/s, 0 = unlimited)")
                                            .size(11.0)
                                            .color(sp.muted),
                                    );
                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        if figma_button(ui, "Apply", 64.0, 26.0, false, &sp) {
                                            action = Some(Action::SetItemSpeedLimit {
                                                id: download.id,
                                                kbps: draft.speed_limit_kbps,
                                            });
                                        }
                                        ui.add_space(6.0);
                                        egui::Frame::new()
                                            .fill(sp.field_bg)
                                            .stroke(Stroke::new(1.0, sp.border))
                                            .corner_radius(6)
                                            .inner_margin(Margin::symmetric(8, 3))
                                            .show(ui, |ui| {
                                                ui.set_width(86.0);
                                                ui.add(
                                                    egui::DragValue::new(
                                                        &mut draft.speed_limit_kbps,
                                                    )
                                                    .range(0..=1_000_000)
                                                    .speed(32),
                                                );
                                            });
                                    });
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

pub(super) fn format_utc_timestamp(millis: i64) -> String {
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
                            super::super::icons::draw_file_icon(
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
