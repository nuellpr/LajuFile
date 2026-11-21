//! Modern "Download File Info" dialog shown when a browser download is captured.

use std::path::PathBuf;

use egui::{Align, Color32, Layout, Margin, RichText, Stroke, Vec2};
use lajufile_core::store::Category;

use super::dialogs::{
    SettingsPalette, badge_pill, card_kicker, draw_h_divider, figma_button, modal_header,
    settings_card, toggle_row,
};

#[derive(Debug, Clone)]
pub struct DownloadInfoDraft {
    pub url: String,
    pub file_name: String,
    pub size: Option<u64>,
    pub ranged: Option<bool>,
    pub category: Category,
    pub folder: String,
    pub remember: bool,
    pub user_edited_name: bool,
    pub user_edited_folder: bool,
}

#[derive(Debug, Clone)]
pub enum DownloadInfoChoice {
    Start {
        folder: PathBuf,
        file_name: String,
        remember: bool,
        start_now: bool,
    },
    Browse,
    Cancel,
}

pub fn download_info_dialog(
    ctx: &egui::Context,
    draft: &mut DownloadInfoDraft,
    dark: bool,
) -> Option<DownloadInfoChoice> {
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

    egui::Window::new("Download File Info")
        .id(egui::Id::unique("lajufile_download_info_modal"))
        .title_bar(false)
        .collapsible(false)
        .resizable(false)
        .frame(frame)
        .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
        .show(ctx, |ui| {
            ui.set_width(580.0);
            ui.spacing_mut().item_spacing = Vec2::ZERO;

            if modal_header(
                ui,
                &crate::i18n::t_ctx(ctx, "Download File Info"),
                "Captured from browser — choose destination or start immediately.",
                &sp,
            ) {
                choice = Some(DownloadInfoChoice::Cancel);
            }
            draw_h_divider(ui, sp.border);

            egui::Frame::new()
                .fill(sp.dialog_bg)
                .inner_margin(Margin::symmetric(24, 18))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());

                    // File overview hero card
                    settings_card(ui, &sp, |ui| {
                        ui.horizontal(|ui| {
                            super::icons::draw_file_icon(
                                ui,
                                &draft.file_name,
                                draft.category,
                                false,
                                46.0,
                            );
                            ui.add_space(12.0);
                            ui.vertical(|ui| {
                                let title = if draft.file_name.trim().is_empty() {
                                    "Pending filename"
                                } else {
                                    draft.file_name.trim()
                                };
                                ui.label(RichText::new(title).size(14.0).strong().color(sp.text));
                                ui.add_space(5.0);
                                ui.horizontal(|ui| {
                                    badge_pill(
                                        ui,
                                        &crate::i18n::t_ctx(ctx, draft.category.label()),
                                        sp.accent,
                                        &sp,
                                    );
                                    ui.add_space(6.0);
                                    let lang = crate::i18n::ctx_lang(ctx);
                                    let size_text = draft
                                        .size
                                        .map(|s| crate::i18n::format_size_locale(s, &lang))
                                        .unwrap_or_else(|| "Size checking...".to_string());
                                    badge_pill(ui, &size_text, sp.muted, &sp);
                                    ui.add_space(6.0);
                                    let (resume_text, resume_color) = match draft.ranged {
                                        Some(true) => ("Resume supported", sp.green),
                                        Some(false) => ("Single stream", sp.amber),
                                        None => ("Probing server...", sp.muted),
                                    };
                                    badge_pill(ui, resume_text, resume_color, &sp);
                                });
                            });
                        });
                    });

                    ui.add_space(12.0);
                    settings_card(ui, &sp, |ui| {
                        card_kicker(ui, "SOURCE URL", &sp);
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            let mut url = draft.url.clone();
                            egui::Frame::new()
                                .fill(sp.field_bg)
                                .stroke(Stroke::new(1.0, sp.border))
                                .corner_radius(8)
                                .inner_margin(Margin::symmetric(10, 6))
                                .show(ui, |ui| {
                                    ui.set_width((ui.available_width() - 86.0).max(160.0));
                                    ui.add_sized(
                                        [ui.available_width(), 20.0],
                                        egui::TextEdit::singleline(&mut url)
                                            .frame(egui::Frame::NONE)
                                            .background_color(Color32::TRANSPARENT)
                                            .interactive(false),
                                    );
                                });
                            ui.add_space(8.0);
                            if figma_button(ui, "Copy", 64.0, 32.0, false, &sp) {
                                ui.ctx().copy_text(draft.url.clone());
                            }
                        });

                        ui.add_space(12.0);
                        card_kicker(ui, "FILE NAME", &sp);
                        ui.add_space(6.0);
                        egui::Frame::new()
                            .fill(sp.field_bg)
                            .stroke(Stroke::new(1.0, sp.border))
                            .corner_radius(8)
                            .inner_margin(Margin::symmetric(10, 6))
                            .show(ui, |ui| {
                                ui.set_width((ui.available_width() - 22.0).max(160.0));
                                if ui
                                    .add_sized(
                                        [ui.available_width(), 20.0],
                                        egui::TextEdit::singleline(&mut draft.file_name)
                                            .frame(egui::Frame::NONE)
                                            .background_color(Color32::TRANSPARENT),
                                    )
                                    .changed()
                                {
                                    draft.user_edited_name = true;
                                    draft.category =
                                        Category::from_file_name(draft.file_name.trim());
                                }
                            });

                        ui.add_space(12.0);
                        card_kicker(ui, "SAVE DESTINATION", &sp);
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            egui::Frame::new()
                                .fill(sp.field_bg)
                                .stroke(Stroke::new(1.0, sp.border))
                                .corner_radius(8)
                                .inner_margin(Margin::symmetric(10, 6))
                                .show(ui, |ui| {
                                    ui.set_width((ui.available_width() - 108.0).max(160.0));
                                    if ui
                                        .add_sized(
                                            [ui.available_width(), 20.0],
                                            egui::TextEdit::singleline(&mut draft.folder)
                                                .frame(egui::Frame::NONE)
                                                .background_color(Color32::TRANSPARENT),
                                        )
                                        .changed()
                                    {
                                        draft.user_edited_folder = true;
                                    }
                                });
                            ui.add_space(8.0);
                            if figma_button(ui, "Browse...", 86.0, 32.0, false, &sp) {
                                choice = Some(DownloadInfoChoice::Browse);
                            }
                        });

                        ui.add_space(12.0);
                        toggle_row(
                            ui,
                            &format!("Remember folder for {} category", draft.category.label()),
                            "download_info_remember_cat",
                            &mut draft.remember,
                            &sp,
                        );
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
                            &crate::i18n::t_ctx(ctx, "Start download"),
                            126.0,
                            34.0,
                            true,
                            &sp,
                        ) {
                            choice = Some(DownloadInfoChoice::Start {
                                folder: PathBuf::from(draft.folder.trim()),
                                file_name: draft.file_name.trim().to_string(),
                                remember: draft.remember,
                                start_now: true,
                            });
                        }
                        ui.add_space(8.0);
                        if figma_button(
                            ui,
                            &crate::i18n::t_ctx(ctx, "Download later"),
                            120.0,
                            34.0,
                            false,
                            &sp,
                        ) {
                            choice = Some(DownloadInfoChoice::Start {
                                folder: PathBuf::from(draft.folder.trim()),
                                file_name: draft.file_name.trim().to_string(),
                                remember: draft.remember,
                                start_now: false,
                            });
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
                            choice = Some(DownloadInfoChoice::Cancel);
                        }
                    });
                });
        });
    choice
}
