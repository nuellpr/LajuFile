//! Small in-app folder picker, so the Add dialog can choose a save folder
//! without pulling in a native dialog dependency.

use std::path::{Component, Path, PathBuf};

use egui::{Align, Align2, Color32, FontId, Layout, Margin, Rect, RichText, Sense, Stroke, Vec2};

use super::dialogs::{SettingsPalette, figma_button, lerp_color, modal_header};

#[derive(Debug, Clone)]
pub enum FolderChoice {
    Select(PathBuf),
    Cancel,
}

#[derive(Debug, Clone)]
pub struct FolderPicker {
    current: PathBuf,
    entries: Vec<PathBuf>,
    error: Option<String>,
    new_name: String,
}

impl FolderPicker {
    pub fn new(start: PathBuf) -> Self {
        let mut picker = Self {
            current: start,
            entries: Vec::new(),
            error: None,
            new_name: String::new(),
        };
        picker.reload();
        picker
    }

    pub fn current(&self) -> &Path {
        &self.current
    }

    pub fn enter(&mut self, path: PathBuf) {
        self.current = path;
        self.reload();
    }

    pub fn up(&mut self) {
        if let Some(parent) = self.current.parent() {
            self.enter(parent.to_path_buf());
        }
    }

    pub fn create_folder(&mut self) {
        let name = self.new_name.trim();
        if name.is_empty() {
            return;
        }
        let target = self.current.join(name);
        match std::fs::create_dir(&target) {
            Ok(()) => {
                self.new_name.clear();
                self.reload();
            }
            Err(error) => self.error = Some(format!("Could not create folder: {error}")),
        }
    }

    fn reload(&mut self) {
        match visible_dirs(&self.current) {
            Ok(entries) => {
                self.entries = entries;
                self.error = None;
            }
            Err(error) => {
                self.entries.clear();
                self.error = Some(error);
            }
        }
    }

    pub fn show(&mut self, ctx: &egui::Context, dark: bool) -> Option<FolderChoice> {
        let sp = SettingsPalette::from_dark(dark);
        let style = ctx.style_of(if dark {
            egui::Theme::Dark
        } else {
            egui::Theme::Light
        });
        let mut choice = None;

        egui::Window::new("Choose folder")
            .id(egui::Id::unique("lajufile_folder_picker"))
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .movable(true)
            .default_width(600.0)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .frame(
                egui::Frame::window(&style)
                    .fill(sp.dialog_bg)
                    .stroke(Stroke::new(1.0, sp.border))
                    .corner_radius(14)
                    .inner_margin(Margin::ZERO),
            )
            .show(ctx, |ui| {
                ui.set_width(590.0);

                if modal_header(
                    ui,
                    "Choose folder",
                    "Pick a destination folder or create a new directory",
                    &sp,
                ) {
                    choice = Some(FolderChoice::Cancel);
                }
                paint_divider(ui, sp.border);

                egui::Frame::new()
                    .fill(sp.dialog_bg)
                    .inner_margin(Margin::symmetric(20, 16))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());

                        // Quick navigation chips: Up, Downloads, and drive/root jumps.
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing = Vec2::new(6.0, 6.0);
                            let has_parent = self.current.parent().is_some();
                            if pill_chip(ui, "↑ Up", false, has_parent, &sp) && has_parent {
                                self.up();
                            }
                            if let Some(downloads) = lajufile_core::paths::user_downloads_dir()
                                && downloads.is_dir()
                            {
                                let active = self.current == downloads;
                                if pill_chip(ui, "Downloads", active, true, &sp) {
                                    self.enter(downloads);
                                }
                            }
                            for root in roots() {
                                let label = root.display().to_string();
                                let active = self.current.starts_with(&root) && roots().len() > 1;
                                if pill_chip(ui, &label, active, true, &sp) {
                                    self.enter(root);
                                }
                            }
                        });

                        ui.add_space(10.0);

                        // Interactive breadcrumb path bar.
                        let crumbs = path_breadcrumbs(&self.current);
                        egui::Frame::new()
                            .fill(sp.field_bg)
                            .stroke(Stroke::new(1.0, sp.border))
                            .corner_radius(9)
                            .inner_margin(Margin::symmetric(10, 7))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                egui::ScrollArea::horizontal()
                                    .id_salt("folder_breadcrumbs")
                                    .scroll_bar_visibility(
                                        egui::scroll_area::ScrollBarVisibility::AlwaysHidden,
                                    )
                                    .show(ui, |ui| {
                                        ui.horizontal(|ui| {
                                            ui.spacing_mut().item_spacing.x = 4.0;
                                            let last_idx = crumbs.len().saturating_sub(1);
                                            for (idx, (label, path)) in
                                                crumbs.into_iter().enumerate()
                                            {
                                                let is_current = idx == last_idx;
                                                let resp = ui
                                                    .add(
                                                        egui::Button::new(
                                                            RichText::new(label).size(11.5).color(
                                                                if is_current {
                                                                    sp.text
                                                                } else {
                                                                    sp.muted
                                                                },
                                                            ),
                                                        )
                                                        .fill(if is_current {
                                                            sp.nav_selected
                                                        } else {
                                                            Color32::TRANSPARENT
                                                        })
                                                        .stroke(Stroke::NONE)
                                                        .corner_radius(5),
                                                    )
                                                    .on_hover_cursor(
                                                        egui::CursorIcon::PointingHand,
                                                    );
                                                if resp.clicked() && !is_current {
                                                    self.enter(path);
                                                }
                                                if !is_current {
                                                    ui.label(
                                                        RichText::new("›")
                                                            .size(12.0)
                                                            .color(sp.faint),
                                                    );
                                                }
                                            }
                                        });
                                    });
                            });

                        if let Some(error) = &self.error {
                            ui.add_space(8.0);
                            egui::Frame::new()
                                .fill(sp.red.gamma_multiply(0.12))
                                .stroke(Stroke::new(1.0, sp.red.gamma_multiply(0.45)))
                                .corner_radius(8)
                                .inner_margin(Margin::symmetric(10, 6))
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    ui.label(RichText::new(error).size(11.0).color(sp.red));
                                });
                        }

                        ui.add_space(10.0);

                        // Directory browser card.
                        egui::Frame::new()
                            .fill(sp.field_bg)
                            .stroke(Stroke::new(1.0, sp.border))
                            .corner_radius(10)
                            .inner_margin(Margin::symmetric(8, 8))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                if self.entries.is_empty() {
                                    ui.allocate_ui_with_layout(
                                        Vec2::new(ui.available_width(), 220.0),
                                        Layout::centered_and_justified(egui::Direction::TopDown),
                                        |ui| {
                                            ui.label(
                                                RichText::new("No subfolders in this directory")
                                                    .size(12.0)
                                                    .color(sp.faint),
                                            );
                                        },
                                    );
                                } else {
                                    egui::ScrollArea::vertical()
                                        .id_salt("folder_entries_scroll")
                                        .max_height(250.0)
                                        .show(ui, |ui| {
                                            ui.set_width(ui.available_width());
                                            for entry in self.entries.clone() {
                                                let label = entry
                                                    .file_name()
                                                    .map(|n| n.to_string_lossy().into_owned())
                                                    .unwrap_or_else(|| entry.display().to_string());
                                                if folder_row(ui, &label, &sp) {
                                                    self.enter(entry);
                                                }
                                            }
                                        });
                                }
                            });

                        ui.add_space(12.0);

                        // Inline "Create new folder" bar.
                        egui::Frame::new()
                            .fill(sp.card_bg)
                            .stroke(Stroke::new(1.0, sp.border))
                            .corner_radius(9)
                            .inner_margin(Margin::symmetric(10, 8))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.horizontal(|ui| {
                                    let field_w = (ui.available_width() - 122.0).max(160.0);
                                    let edit = ui.add_sized(
                                        [field_w, 30.0],
                                        egui::TextEdit::singleline(&mut self.new_name)
                                            .hint_text("New folder name...")
                                            .frame(egui::Frame::NONE),
                                    );
                                    let enter_create = edit.lost_focus()
                                        && ui.input(|i| i.key_pressed(egui::Key::Enter));
                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        let can_create = !self.new_name.trim().is_empty();
                                        if figma_button(
                                            ui,
                                            "+ Create folder",
                                            112.0,
                                            30.0,
                                            false,
                                            &sp,
                                        ) && can_create
                                            || enter_create && can_create
                                        {
                                            self.create_folder();
                                        }
                                    });
                                });
                            });
                    });

                paint_divider(ui, sp.border);

                // Footer action bar.
                egui::Frame::new()
                    .fill(sp.nav_bg)
                    .inner_margin(Margin::symmetric(20, 14))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            let target_label = self
                                .current
                                .file_name()
                                .map(|n| n.to_string_lossy().into_owned())
                                .filter(|s| !s.is_empty())
                                .unwrap_or_else(|| self.current.display().to_string());
                            ui.label(
                                RichText::new(format!("Target: {target_label}"))
                                    .size(11.0)
                                    .color(sp.muted),
                            );
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if figma_button(ui, "Select folder", 116.0, 34.0, true, &sp) {
                                    choice = Some(FolderChoice::Select(self.current.clone()));
                                }
                                ui.add_space(8.0);
                                if figma_button(ui, "Cancel", 86.0, 34.0, false, &sp) {
                                    choice = Some(FolderChoice::Cancel);
                                }
                            });
                        });
                    });
            });
        choice
    }
}

fn paint_divider(ui: &mut egui::Ui, color: Color32) {
    let rect = ui
        .allocate_exact_size(Vec2::new(ui.available_width(), 1.0), Sense::hover())
        .0;
    ui.painter().rect_filled(rect, 0.0, color);
}

fn pill_chip(
    ui: &mut egui::Ui,
    label: &str,
    active: bool,
    enabled: bool,
    sp: &SettingsPalette,
) -> bool {
    let font = FontId::proportional(11.5);
    let text_w = ui
        .fonts_mut(|f| f.layout_no_wrap(label.to_string(), font.clone(), sp.text))
        .size()
        .x;
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, mut resp) = ui.allocate_exact_size(Vec2::new(text_w + 20.0, 26.0), sense);
    if enabled {
        resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    }
    let fill = if active {
        sp.nav_selected
    } else if enabled && resp.hovered() {
        lerp_color(sp.card_bg, sp.accent, 0.12)
    } else {
        sp.card_bg
    };
    let border = if active { sp.accent } else { sp.border };
    ui.painter().rect_filled(rect, 7.0, fill);
    ui.painter().rect_stroke(
        rect,
        7.0,
        Stroke::new(1.0, border),
        egui::StrokeKind::Inside,
    );
    let text_color = if !enabled {
        sp.faint.gamma_multiply(0.55)
    } else if active {
        sp.accent
    } else {
        sp.text
    };
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        label,
        font,
        text_color,
    );
    enabled && resp.clicked()
}

fn folder_row(ui: &mut egui::Ui, name: &str, sp: &SettingsPalette) -> bool {
    let (rect, resp) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 34.0), Sense::click());
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    if resp.hovered() {
        ui.painter()
            .rect_filled(rect, 7.0, lerp_color(sp.field_bg, sp.nav_selected, 0.85));
        ui.painter().rect_stroke(
            rect,
            7.0,
            Stroke::new(1.0, sp.accent.gamma_multiply(0.35)),
            egui::StrokeKind::Inside,
        );
    }

    // Custom vector folder glyph on the left.
    let icon_rect = Rect::from_min_size(
        egui::pos2(rect.left() + 10.0, rect.center().y - 7.0),
        Vec2::new(18.0, 14.0),
    );
    paint_folder_glyph(ui.painter(), icon_rect, sp.amber);

    ui.painter().text(
        egui::pos2(icon_rect.right() + 10.0, rect.center().y),
        Align2::LEFT_CENTER,
        name,
        FontId::proportional(12.5),
        sp.text,
    );

    if resp.hovered() {
        ui.painter().text(
            egui::pos2(rect.right() - 12.0, rect.center().y),
            Align2::RIGHT_CENTER,
            "›",
            FontId::proportional(14.0),
            sp.muted,
        );
    }

    resp.clicked()
}

fn paint_folder_glyph(painter: &egui::Painter, rect: Rect, color: Color32) {
    let tab = Rect::from_min_size(rect.min, Vec2::new(rect.width() * 0.45, 4.5));
    painter.rect_filled(tab, 2.0, color);
    let body = Rect::from_min_max(egui::pos2(rect.left(), rect.top() + 3.0), rect.max);
    painter.rect_filled(body, 2.5, color);
}

/// Breaks `path` into clickable ancestor `(label, path)` breadcrumbs.
pub fn path_breadcrumbs(path: &Path) -> Vec<(String, PathBuf)> {
    let mut crumbs = Vec::new();
    let mut running = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::Prefix(prefix) => {
                running.push(comp.as_os_str());
                crumbs.push((
                    prefix.as_os_str().to_string_lossy().into_owned(),
                    running.clone(),
                ));
            }
            Component::RootDir => {
                running.push(comp.as_os_str());
                if crumbs.is_empty() {
                    crumbs.push(("/".to_string(), running.clone()));
                } else if let Some(last) = crumbs.last_mut() {
                    last.1 = running.clone();
                }
            }
            Component::Normal(part) => {
                running.push(part);
                crumbs.push((part.to_string_lossy().into_owned(), running.clone()));
            }
            Component::CurDir | Component::ParentDir => {}
        }
    }
    crumbs
}

/// Existing drive/home roots, so the user can jump quickly across volumes.
pub fn roots() -> Vec<PathBuf> {
    #[cfg(windows)]
    {
        ('A'..='Z')
            .map(|letter| PathBuf::from(format!("{letter}:\\")))
            .filter(|path| path.exists())
            .collect()
    }
    #[cfg(not(windows))]
    {
        let mut out = Vec::new();
        if let Some(home) = std::env::var_os("HOME") {
            out.push(PathBuf::from(home));
        }
        out.push(PathBuf::from("/"));
        out
    }
}

/// Sub-directories of `path`, sorted by name, files skipped.
pub fn visible_dirs(path: &Path) -> Result<Vec<PathBuf>, String> {
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(path)
        .map_err(|error| format!("read {path:?}: {error}"))?
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false))
        .map(|entry| entry.path())
        .collect();
    dirs.sort_by_key(|dir| {
        dir.file_name()
            .map(|name| name.to_string_lossy().to_lowercase())
    });
    Ok(dirs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visible_dirs_lists_only_directories_sorted() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("zeta")).unwrap();
        std::fs::create_dir(dir.path().join("Alpha")).unwrap();
        std::fs::write(dir.path().join("file.txt"), b"x").unwrap();

        let names: Vec<String> = visible_dirs(dir.path())
            .unwrap()
            .iter()
            .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec!["Alpha".to_string(), "zeta".to_string()]);
    }

    #[test]
    fn visible_dirs_reports_missing_path() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("does-not-exist");
        assert!(visible_dirs(&missing).is_err());
    }

    #[test]
    fn path_breadcrumbs_builds_clean_ancestor_chain() {
        let crumbs = path_breadcrumbs(Path::new("/tmp/lajufile/downloads"));
        let labels: Vec<&str> = crumbs.iter().map(|(l, _)| l.as_str()).collect();
        assert_eq!(labels, vec!["/", "tmp", "lajufile", "downloads"]);
        assert_eq!(
            crumbs.last().unwrap().1,
            Path::new("/tmp/lajufile/downloads")
        );
    }
}
