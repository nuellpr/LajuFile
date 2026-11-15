//! IDM-style "Download File Info" dialog, used when a browser capture needs
//! a save folder before it is queued.

use std::path::PathBuf;

use egui::{Align, Layout, RichText};
use lajufile_core::store::Category;

#[derive(Debug, Clone)]
pub struct DownloadInfoDraft {
    pub url: String,
    pub category: Category,
    pub folder: String,
    pub remember: bool,
}

#[derive(Debug, Clone)]
pub enum DownloadInfoChoice {
    Start { folder: PathBuf, remember: bool },
    Browse,
    Cancel,
}

pub fn category_label(category: Category) -> &'static str {
    match category {
        Category::Video => "Video",
        Category::Music => "Music",
        Category::Documents => "Documents",
        Category::Programs => "Programs",
        Category::Compressed => "Compressed",
        Category::Other => "Other",
    }
}

pub fn download_info_dialog(
    ctx: &egui::Context,
    draft: &mut DownloadInfoDraft,
) -> Option<DownloadInfoChoice> {
    let mut choice = None;
    egui::Window::new("Download File Info")
        .collapsible(false)
        .resizable(false)
        .default_width(620.0)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .show(ctx, |ui| {
            ui.set_width(580.0);

            ui.label(RichText::new("URL").strong());
            let mut url = draft.url.clone();
            ui.add_sized(
                [ui.available_width(), 26.0],
                egui::TextEdit::singleline(&mut url).interactive(false),
            );

            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("Category").strong());
                ui.add_space(8.0);
                ui.label(category_label(draft.category));
            });

            ui.add_space(6.0);
            ui.label(RichText::new("Save As").strong());
            ui.horizontal(|ui| {
                ui.add_sized(
                    [ui.available_width() - 96.0, 28.0],
                    egui::TextEdit::singleline(&mut draft.folder),
                );
                if ui.button("Browse...").clicked() {
                    choice = Some(DownloadInfoChoice::Browse);
                }
            });

            ui.add_space(6.0);
            ui.checkbox(
                &mut draft.remember,
                format!(
                    "Remember this path for the {} category",
                    category_label(draft.category)
                ),
            );

            ui.add_space(10.0);
            ui.separator();
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.button("Start download").clicked() {
                    let folder = PathBuf::from(draft.folder.trim());
                    choice = Some(DownloadInfoChoice::Start {
                        folder,
                        remember: draft.remember,
                    });
                }
                if ui.button("Cancel").clicked() {
                    choice = Some(DownloadInfoChoice::Cancel);
                }
            });
        });
    choice
}
