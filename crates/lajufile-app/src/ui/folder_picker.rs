//! Small in-app folder picker, so the Add dialog can choose a save folder
//! without pulling in a native dialog dependency.

use std::path::{Path, PathBuf};

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
            Err(error) => self.error = Some(format!("create {target:?}: {error}")),
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

    pub fn show(&mut self, ctx: &egui::Context) -> Option<FolderChoice> {
        let mut choice = None;
        egui::Window::new("Choose folder")
            .collapsible(false)
            .resizable(false)
            .default_width(560.0)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.set_width(520.0);
                ui.label(egui::RichText::new(self.current.to_string_lossy()).strong());
                if let Some(error) = &self.error {
                    ui.label(
                        egui::RichText::new(error)
                            .size(11.0)
                            .color(ui.visuals().warn_fg_color),
                    );
                }

                ui.horizontal(|ui| {
                    if ui.button("Up").clicked() {
                        self.up();
                    }
                    for root in roots() {
                        if ui.button(root.display().to_string()).clicked() {
                            self.enter(root);
                        }
                    }
                });

                ui.separator();
                egui::ScrollArea::vertical()
                    .max_height(280.0)
                    .show(ui, |ui| {
                        for entry in self.entries.clone() {
                            let label = entry
                                .file_name()
                                .map(|name| name.to_string_lossy().into_owned())
                                .unwrap_or_else(|| entry.display().to_string());
                            if ui.selectable_label(false, format!("{label}/")).clicked() {
                                self.enter(entry);
                            }
                        }
                    });
                ui.separator();

                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.new_name)
                            .hint_text("New folder name")
                            .desired_width(220.0),
                    );
                    if ui.button("Create folder").clicked() {
                        self.create_folder();
                    }
                });

                ui.add_space(8.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Select").clicked() {
                        choice = Some(FolderChoice::Select(self.current.clone()));
                    }
                    if ui.button("Cancel").clicked() {
                        choice = Some(FolderChoice::Cancel);
                    }
                });
            });
        choice
    }
}

/// Existing drive roots, so the user can jump between drives.
pub fn roots() -> Vec<PathBuf> {
    ('A'..='Z')
        .map(|letter| PathBuf::from(format!("{letter}:\\")))
        .filter(|path| path.exists())
        .collect()
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
}
