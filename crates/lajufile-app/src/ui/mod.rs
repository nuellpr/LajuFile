//! UI state shared by the main window and dialogs.

pub mod dialogs;
pub mod download_info;
pub mod folder_picker;
pub mod format;
pub mod main_window;
pub mod theme;

use lajufile_core::settings::{Settings, Theme};
use lajufile_core::store::{Category, Download, DownloadStatus};

use crate::engine::AddRequest;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Filter {
    All,
    Unfinished,
    Finished,
    Category(Category),
}

impl Filter {
    pub fn matches(self, download: &Download) -> bool {
        match self {
            Self::All => true,
            Self::Unfinished => matches!(
                download.status,
                DownloadStatus::Queued | DownloadStatus::Downloading | DownloadStatus::Paused
            ),
            Self::Finished => download.status == DownloadStatus::Done,
            Self::Category(category) => download.category == category,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::All => "All downloads",
            Self::Unfinished => "Unfinished",
            Self::Finished => "Finished",
            Self::Category(Category::Video) => "Video",
            Self::Category(Category::Music) => "Music",
            Self::Category(Category::Documents) => "Documents",
            Self::Category(Category::Programs) => "Programs",
            Self::Category(Category::Compressed) => "Compressed",
            Self::Category(Category::Other) => "Other",
        }
    }
}

#[derive(Debug, Clone)]
pub enum Action {
    Select(i64),
    Pause(i64),
    Resume(i64),
    Remove(i64),
    Open(i64),
    OpenFolder(i64),
    CopyUrl(String),
    OpenAdd,
    OpenSettings,
    OpenFolderPicker,
    Add(AddRequest),
    SaveSettings(Settings),
    SetTheme(Theme),
    PauseAll,
    ResumeAll,
    TestBridge(u16),
    RegenerateToken,
    Exit,
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn download(status: DownloadStatus, category: Category) -> Download {
        Download {
            id: 1,
            url: "https://example.com/file.bin".to_string(),
            final_url: None,
            file_name: "file.bin".to_string(),
            save_path: PathBuf::new(),
            temp_path: PathBuf::new(),
            size: Some(10),
            downloaded: 0,
            status,
            category,
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
            queue_order: 0,
            created_at: 0,
            completed_at: None,
        }
    }

    #[test]
    fn unfinished_matches_active_statuses_only() {
        assert!(Filter::Unfinished.matches(&download(DownloadStatus::Queued, Category::Other)));
        assert!(
            Filter::Unfinished.matches(&download(DownloadStatus::Downloading, Category::Other))
        );
        assert!(Filter::Unfinished.matches(&download(DownloadStatus::Paused, Category::Other)));
        assert!(!Filter::Unfinished.matches(&download(DownloadStatus::Done, Category::Other)));
        assert!(!Filter::Unfinished.matches(&download(DownloadStatus::Error, Category::Other)));
    }

    #[test]
    fn category_filter_matches_only_that_category() {
        assert!(
            Filter::Category(Category::Video)
                .matches(&download(DownloadStatus::Done, Category::Video))
        );
        assert!(
            !Filter::Category(Category::Video)
                .matches(&download(DownloadStatus::Done, Category::Music))
        );
    }
}
