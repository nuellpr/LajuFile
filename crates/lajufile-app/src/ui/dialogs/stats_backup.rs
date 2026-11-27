//! Download statistics and safe settings backup/restore dialog.

use egui::{Align, Color32, Layout, Margin, RichText, Stroke, Vec2};
use lajufile_core::settings::Settings;
use lajufile_core::store::{Category, Download, DownloadStatus};

use super::super::Action;
use super::super::format::{format_size, format_speed};
use super::{
    SettingsPalette, card_kicker, draw_h_divider, figma_button, modal_header, settings_card,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadStats {
    pub completed_count: usize,
    pub total_bytes: u64,
    pub today_bytes: u64,
    pub month_bytes: u64,
    pub peak_speed_bps: u64,
    pub by_category: Vec<(&'static str, usize, u64)>,
}

impl DownloadStats {
    pub fn compute(downloads: &[Download], peak_speed_bps: u64, now_ms: i64) -> Self {
        const DAY_MS: i64 = 24 * 60 * 60 * 1000;
        const MONTH_MS: i64 = 30 * DAY_MS;

        let mut completed_count = 0usize;
        let mut total_bytes = 0u64;
        let mut today_bytes = 0u64;
        let mut month_bytes = 0u64;

        let cats = [
            Category::Video,
            Category::Music,
            Category::Images,
            Category::Documents,
            Category::Programs,
            Category::Compressed,
            Category::Other,
        ];
        let mut counts = [0usize; 7];
        let mut bytes = [0u64; 7];

        for d in downloads {
            if d.status != DownloadStatus::Done {
                continue;
            }
            let sz = d.size.unwrap_or(d.downloaded).max(d.downloaded);
            completed_count += 1;
            total_bytes = total_bytes.saturating_add(sz);
            let done_at = d.completed_at.unwrap_or(d.created_at);
            let age = now_ms.saturating_sub(done_at);
            if age <= DAY_MS {
                today_bytes = today_bytes.saturating_add(sz);
            }
            if age <= MONTH_MS {
                month_bytes = month_bytes.saturating_add(sz);
            }
            if let Some(pos) = cats.iter().position(|c| *c == d.category) {
                counts[pos] += 1;
                bytes[pos] = bytes[pos].saturating_add(sz);
            }
        }

        let by_category = cats
            .iter()
            .enumerate()
            .filter(|(idx, _)| counts[*idx] > 0)
            .map(|(idx, cat)| (cat.label(), counts[idx], bytes[idx]))
            .collect();

        Self {
            completed_count,
            total_bytes,
            today_bytes,
            month_bytes,
            peak_speed_bps,
            by_category,
        }
    }
}

pub fn stats_backup_dialog(
    ctx: &egui::Context,
    open: &mut bool,
    downloads: &[Download],
    settings: &Settings,
    import_buf: &mut String,
    feedback: Option<&(String, bool)>,
    dark: bool,
) -> Option<Action> {
    let mut keep_open = *open;
    let mut action = None;
    let sp = SettingsPalette::from_dark(dark);
    let stats = DownloadStats::compute(
        downloads,
        settings.peak_speed_bps,
        lajufile_core::store::now_millis(),
    );

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

    egui::Window::new("Statistics & Backup")
        .id(egui::Id::unique("lajufile_stats_backup_dialog"))
        .title_bar(false)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
        .fixed_size(Vec2::new(620.0, 540.0))
        .frame(frame)
        .show(ctx, |ui| {
            ui.spacing_mut().item_spacing = Vec2::ZERO;
            if modal_header(
                ui,
                "Statistics & Settings Backup",
                "Download activity summary and portable JSON settings backup (tokens omitted).",
                &sp,
            ) {
                keep_open = false;
            }
            draw_h_divider(ui, sp.border);

            egui::ScrollArea::vertical()
                .max_height(400.0)
                .show(ui, |ui| {
                    egui::Frame::new()
                        .inner_margin(Margin::symmetric(22, 16))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            settings_card(ui, &sp, |ui| {
                                card_kicker(ui, "DOWNLOAD SUMMARY", &sp);
                                ui.add_space(8.0);
                                egui::Grid::new("lajufile_stats_grid")
                                    .num_columns(2)
                                    .spacing([28.0, 6.0])
                                    .show(ui, |ui| {
                                        for (label, val) in [
                                            ("Completed files", stats.completed_count.to_string()),
                                            ("Total downloaded", format_size(stats.total_bytes)),
                                            ("Last 24 hours", format_size(stats.today_bytes)),
                                            ("Last 30 days", format_size(stats.month_bytes)),
                                            (
                                                "Peak speed",
                                                if stats.peak_speed_bps == 0 {
                                                    "—".to_string()
                                                } else {
                                                    format_speed(Some(stats.peak_speed_bps))
                                                },
                                            ),
                                        ] {
                                            ui.label(
                                                RichText::new(label).size(11.5).color(sp.muted),
                                            );
                                            ui.label(
                                                RichText::new(val)
                                                    .size(11.5)
                                                    .strong()
                                                    .color(sp.text),
                                            );
                                            ui.end_row();
                                        }
                                    });
                                if !stats.by_category.is_empty() {
                                    ui.add_space(10.0);
                                    draw_h_divider(ui, sp.border);
                                    ui.add_space(8.0);
                                    card_kicker(ui, "BY CATEGORY", &sp);
                                    ui.add_space(6.0);
                                    for (cat, count, bytes) in &stats.by_category {
                                        let frac = if stats.total_bytes > 0 {
                                            (*bytes as f32 / stats.total_bytes as f32).clamp(0.03, 1.0)
                                        } else {
                                            0.05
                                        };
                                        ui.horizontal(|ui| {
                                            ui.set_width(ui.available_width());
                                            ui.allocate_ui_with_layout(
                                                Vec2::new(96.0, 18.0),
                                                Layout::left_to_right(Align::Center),
                                                |ui| {
                                                    ui.label(
                                                        RichText::new(*cat)
                                                            .size(11.0)
                                                            .color(sp.text),
                                                    );
                                                },
                                            );
                                            let bar_w = (ui.available_width() - 140.0).max(80.0);
                                            let (bar_rect, _) = ui.allocate_exact_size(
                                                Vec2::new(bar_w, 7.0),
                                                egui::Sense::hover(),
                                            );
                                            ui.painter().rect_filled(
                                                bar_rect,
                                                4.0,
                                                sp.toggle_off.gamma_multiply(0.55),
                                            );
                                            let mut fill = bar_rect;
                                            fill.set_width(bar_rect.width() * frac);
                                            ui.painter().rect_filled(fill, 4.0, sp.accent);
                                            ui.with_layout(
                                                Layout::right_to_left(Align::Center),
                                                |ui| {
                                                    ui.label(
                                                        RichText::new(format!(
                                                            "{count} · {}",
                                                            format_size(*bytes)
                                                        ))
                                                        .size(10.5)
                                                        .color(sp.muted),
                                                    );
                                                },
                                            );
                                        });
                                        ui.add_space(4.0);
                                    }
                                }
                            });

                            ui.add_space(12.0);
                            settings_card(ui, &sp, |ui| {
                                card_kicker(ui, "BACKUP & RESTORE SETTINGS", &sp);
                                ui.add_space(6.0);
                                ui.label(
                                    RichText::new(
                                        "Export saves a safe JSON copy (excluding API keys, bridge tokens, and proxy passwords).",
                                    )
                                    .size(10.5)
                                    .color(sp.faint),
                                );
                                ui.add_space(8.0);
                                if figma_button(
                                    ui,
                                    "Export safe backup (.json + clipboard)",
                                    240.0,
                                    30.0,
                                    false,
                                    &sp,
                                ) {
                                    action = Some(Action::ExportSettingsBackup);
                                }
                                ui.add_space(10.0);
                                ui.label(
                                    RichText::new("Paste backup JSON below to restore preferences:")
                                        .size(11.0)
                                        .color(sp.muted),
                                );
                                ui.add_space(4.0);
                                egui::Frame::new()
                                    .fill(sp.field_bg)
                                    .stroke(Stroke::new(1.0, sp.border))
                                    .corner_radius(8)
                                    .inner_margin(Margin::symmetric(8, 6))
                                    .show(ui, |ui| {
                                        egui::ScrollArea::vertical()
                                            .id_salt("lajufile_backup_import_scroll")
                                            .max_height(74.0)
                                            .show(ui, |ui| {
                                                ui.add_sized(
                                                    [ui.available_width(), 64.0],
                                                    egui::TextEdit::multiline(import_buf)
                                                        .frame(egui::Frame::NONE)
                                                        .background_color(Color32::TRANSPARENT)
                                                        .hint_text("{\"download_dir\": ...}"),
                                                );
                                            });
                                    });
                                ui.add_space(8.0);
                                ui.horizontal(|ui| {
                                    if figma_button(
                                        ui,
                                        "Import JSON backup",
                                        154.0,
                                        30.0,
                                        true,
                                        &sp,
                                    ) {
                                        action =
                                            Some(Action::ImportSettingsBackup(import_buf.clone()));
                                    }
                                    if let Some((msg, is_ok)) = feedback {
                                        ui.add_space(10.0);
                                        ui.label(
                                            RichText::new(msg)
                                                .size(11.0)
                                                .color(if *is_ok { sp.green } else { sp.red }),
                                        );
                                    }
                                });
                            });
                        });
                });

            draw_h_divider(ui, sp.border);
            egui::Frame::new()
                .fill(sp.dialog_bg)
                .inner_margin(Margin::symmetric(22, 12))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if figma_button(ui, "Close", 88.0, 32.0, false, &sp) {
                            keep_open = false;
                        }
                    });
                });
        });

    *open = keep_open;
    action
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn dl(
        id: i64,
        cat: Category,
        size: u64,
        status: DownloadStatus,
        completed_at: Option<i64>,
    ) -> Download {
        Download {
            id,
            url: "https://example.com/a".to_string(),
            final_url: None,
            file_name: "a".to_string(),
            save_path: PathBuf::from("a"),
            temp_path: PathBuf::from("a.lajufile"),
            size: Some(size),
            downloaded: size,
            status,
            category: cat,
            kind: "http".to_string(),
            video_format: None,
            connections: 1,
            ranged: true,
            etag: None,
            last_modified: None,
            cookie_enc: None,
            referrer: None,
            user_agent: None,
            error: None,
            queue_order: id,
            created_at: 0,
            completed_at,
            speed_limit_kbps: 0,
            expected_sha256: None,
            checksum_status: None,
        }
    }

    #[test]
    fn compute_stats_aggregates_completed_windows_and_categories() {
        let now = 100 * 24 * 60 * 60 * 1000i64;
        let list = vec![
            dl(
                1,
                Category::Video,
                1000,
                DownloadStatus::Done,
                Some(now - 1000),
            ),
            dl(
                2,
                Category::Images,
                500,
                DownloadStatus::Done,
                Some(now - 5 * 24 * 60 * 60 * 1000),
            ),
            dl(
                3,
                Category::Documents,
                300,
                DownloadStatus::Done,
                Some(now - 45 * 24 * 60 * 60 * 1000),
            ),
            dl(4, Category::Video, 9999, DownloadStatus::Paused, None),
        ];
        let stats = DownloadStats::compute(&list, 250_000, now);
        assert_eq!(stats.completed_count, 3);
        assert_eq!(stats.total_bytes, 1800);
        assert_eq!(stats.today_bytes, 1000);
        assert_eq!(stats.month_bytes, 1500);
        assert_eq!(stats.peak_speed_bps, 250_000);
        assert_eq!(stats.by_category.len(), 3);
    }
}
