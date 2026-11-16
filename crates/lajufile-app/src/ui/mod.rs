//! UI state shared by the main window and dialogs.

pub mod dialogs;
pub mod download_info;
pub mod folder_picker;
pub mod format;
pub mod icons;
pub mod main_window;
pub mod theme;

use std::collections::HashSet;
use std::path::PathBuf;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortOrder {
    #[default]
    Newest,
    Oldest,
    NameAsc,
    NameDesc,
    SizeDesc,
    SizeAsc,
    Progress,
}

impl SortOrder {
    pub const ALL: [Self; 7] = [
        Self::Newest,
        Self::Oldest,
        Self::NameAsc,
        Self::NameDesc,
        Self::SizeDesc,
        Self::SizeAsc,
        Self::Progress,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Newest => "Newest",
            Self::Oldest => "Oldest",
            Self::NameAsc => "Name (A-Z)",
            Self::NameDesc => "Name (Z-A)",
            Self::SizeDesc => "Largest",
            Self::SizeAsc => "Smallest",
            Self::Progress => "Progress",
        }
    }

    pub fn sort_downloads(self, rows: &mut [&Download]) {
        rows.sort_by(|a, b| {
            let ord = match self {
                Self::Newest => b.created_at.cmp(&a.created_at),
                Self::Oldest => a.created_at.cmp(&b.created_at),
                Self::NameAsc => a
                    .file_name
                    .to_ascii_lowercase()
                    .cmp(&b.file_name.to_ascii_lowercase()),
                Self::NameDesc => b
                    .file_name
                    .to_ascii_lowercase()
                    .cmp(&a.file_name.to_ascii_lowercase()),
                Self::SizeDesc => b.size.unwrap_or(0).cmp(&a.size.unwrap_or(0)),
                Self::SizeAsc => a.size.unwrap_or(0).cmp(&b.size.unwrap_or(0)),
                Self::Progress => progress_sort_key(b).cmp(&progress_sort_key(a)),
            };
            ord.then_with(|| b.id.cmp(&a.id))
        });
    }
}

fn progress_sort_key(download: &Download) -> u64 {
    match download.status {
        DownloadStatus::Done => 10_000,
        _ => match download.size {
            Some(size) if size > 0 => {
                ((download.downloaded as f64 / size as f64).clamp(0.0, 1.0) * 9_999.0) as u64
            }
            _ => 0,
        },
    }
}

pub fn matches_search(download: &Download, query: &str) -> bool {
    let q = query.trim().to_ascii_lowercase();
    if q.is_empty() {
        return true;
    }
    let status_label = match download.status {
        DownloadStatus::Queued => "queued",
        DownloadStatus::Downloading => "downloading",
        DownloadStatus::Paused => "stopped",
        DownloadStatus::Done => "completed",
        DownloadStatus::Error => "failed",
    };
    download.file_name.to_ascii_lowercase().contains(&q)
        || download.url.to_ascii_lowercase().contains(&q)
        || download.category.as_str().to_ascii_lowercase().contains(&q)
        || download.status.as_str().to_ascii_lowercase().contains(&q)
        || status_label.contains(&q)
        || download
            .error
            .as_deref()
            .is_some_and(|err| err.to_ascii_lowercase().contains(&q))
}

/// Extracts unique valid HTTP/HTTPS URLs from `.url` shortcuts or plain/markdown text lists.
pub fn extract_urls_from_text(text: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut urls = Vec::new();
    for raw_line in text.lines() {
        let trimmed = raw_line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let candidate = trimmed
            .strip_prefix("URL=")
            .or_else(|| trimmed.strip_prefix("url="))
            .unwrap_or(trimmed);
        for token in candidate.split(|ch: char| {
            ch.is_whitespace() || matches!(ch, '<' | '>' | '"' | '\'' | '(' | ')')
        }) {
            let clean = token.trim_end_matches(['.', ',', ';']);
            if let Ok(parsed) = url::Url::parse(clean)
                && matches!(parsed.scheme(), "http" | "https")
                && parsed.host_str().is_some()
            {
                let normalized = parsed.to_string();
                if seen.insert(normalized.clone()) {
                    urls.push(normalized);
                }
            }
        }
    }
    urls
}

/// Extracts unique downloadable `http`/`https` links from HTML `href` attributes resolved against `base_url`.
pub fn extract_links_from_html(base_url: &str, html: &str) -> Vec<String> {
    const IGNORED_EXTS: &[&str] = &[
        "html", "htm", "php", "asp", "aspx", "jsp", "css", "js", "mjs", "map", "ico", "svg",
        "woff", "woff2", "ttf", "eot", "png", "jpg", "jpeg", "gif", "webp",
    ];
    let Ok(base) = url::Url::parse(base_url.trim()) else {
        return Vec::new();
    };
    let lower = html.to_ascii_lowercase();
    let mut seen = HashSet::new();
    let mut urls = Vec::new();
    let mut cursor = 0;

    while let Some(rel) = lower[cursor..].find("href") {
        let mut idx = cursor + rel + 4;
        while idx < html.len() && html.as_bytes()[idx].is_ascii_whitespace() {
            idx += 1;
        }
        if idx >= html.len() || html.as_bytes()[idx] != b'=' {
            cursor = idx.max(cursor + rel + 4);
            continue;
        }
        idx += 1;
        while idx < html.len() && html.as_bytes()[idx].is_ascii_whitespace() {
            idx += 1;
        }
        if idx >= html.len() {
            break;
        }
        let quote = html.as_bytes()[idx];
        let (raw, next_cursor) = if quote == b'"' || quote == b'\'' {
            let start = idx + 1;
            let Some(end_rel) = html[start..].find(quote as char) else {
                break;
            };
            (&html[start..start + end_rel], start + end_rel + 1)
        } else {
            let start = idx;
            let end = html[start..]
                .find(|c: char| c.is_ascii_whitespace() || c == '>')
                .map_or(html.len(), |off| start + off);
            (&html[start..end], end)
        };
        cursor = next_cursor;

        let trimmed = raw.trim();
        if trimmed.is_empty()
            || trimmed.starts_with('#')
            || trimmed.starts_with("javascript:")
            || trimmed.starts_with("mailto:")
        {
            continue;
        }
        let Ok(mut resolved) = base.join(trimmed) else {
            continue;
        };
        if !matches!(resolved.scheme(), "http" | "https") || resolved.host_str().is_none() {
            continue;
        }
        resolved.set_fragment(None);
        let seg = resolved
            .path_segments()
            .and_then(|mut s| s.next_back())
            .unwrap_or("");
        let ext = std::path::Path::new(seg)
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase());
        let skip_asset = match ext.as_deref() {
            Some(e) => IGNORED_EXTS.contains(&e),
            None => !resolved.as_str().to_ascii_lowercase().contains("download"),
        };
        if skip_asset {
            continue;
        }
        let normalized = resolved.to_string();
        if seen.insert(normalized.clone()) {
            urls.push(normalized);
        }
    }
    urls
}

#[derive(Debug, Clone)]
pub enum Action {
    Select(i64),
    SelectRow {
        id: i64,
        ctrl: bool,
        shift: bool,
    },
    SelectAll,
    Pause(i64),
    Resume(i64),
    UpdateUrlAndResume {
        id: i64,
        url: String,
    },
    MoveRename {
        id: i64,
        file_name: String,
        dir: Option<PathBuf>,
    },
    BrowseMoveDir(i64),
    BrowseCategoryDir(String),
    VerifyChecksum(i64),
    CopyChecksum(String),
    Remove(i64),
    ConfirmRemove(Vec<i64>, bool),
    RemoveWithFile {
        ids: Vec<i64>,
        delete_file: bool,
    },
    RemoveCompleted,
    ShowProgress(i64),
    HideProgress(i64),
    OpenUpdateChecker,
    CheckUpdatesNow,
    StartSelfUpdate {
        url: String,
        file_name: String,
    },
    OpenProperties(i64),
    Redownload(i64),
    Open(i64),
    OpenWith(i64),
    OpenFolder(i64),
    CopyUrl(String),
    CopyPath(PathBuf),
    OpenAdd,
    OpenBatch,
    OpenSettings,
    OpenBrowserWizard,
    OpenExtensionFolder,
    LaunchBrowser {
        browser_name: String,
        extensions_url: String,
    },
    OpenLanguagePicker,
    SetLanguage(String),
    FocusSearch,
    ExportLinks,
    OpenFolderPicker,
    Add(AddRequest),
    AddBatch(Vec<AddRequest>),
    LoadFormats(String),
    GrabPageLinks(String),
    SaveSettings(Settings),
    SetSpeedLimit(u64),
    SetScale(u32),
    SetTheme(Theme),
    PauseAll,
    ResumeAll,
    TestBridge(u16),
    UpdateTools,
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

    #[test]
    fn search_matches_name_url_and_status() {
        let mut d = download(DownloadStatus::Error, Category::Video);
        d.file_name = "Ubuntu_26_ISO.iso".to_string();
        d.error = Some("HTTP 403 Forbidden".to_string());
        assert!(matches_search(&d, ""));
        assert!(matches_search(&d, "ubuntu"));
        assert!(matches_search(&d, "example.com"));
        assert!(matches_search(&d, "403"));
        assert!(matches_search(&d, "video"));
        assert!(!matches_search(&d, "nonexistent"));
    }

    #[test]
    fn sort_order_orders_downloads_consistently() {
        let mut a = download(DownloadStatus::Done, Category::Other);
        a.id = 1;
        a.file_name = "beta.zip".to_string();
        a.size = Some(200);
        a.created_at = 10;

        let mut b = download(DownloadStatus::Downloading, Category::Other);
        b.id = 2;
        b.file_name = "alpha.zip".to_string();
        b.size = Some(500);
        b.downloaded = 250;
        b.created_at = 20;

        let mut rows = vec![&a, &b];
        SortOrder::Newest.sort_downloads(&mut rows);
        assert_eq!(rows.iter().map(|d| d.id).collect::<Vec<_>>(), vec![2, 1]);

        SortOrder::NameAsc.sort_downloads(&mut rows);
        assert_eq!(rows.iter().map(|d| d.id).collect::<Vec<_>>(), vec![2, 1]);

        SortOrder::SizeAsc.sort_downloads(&mut rows);
        assert_eq!(rows.iter().map(|d| d.id).collect::<Vec<_>>(), vec![1, 2]);

        SortOrder::Progress.sort_downloads(&mut rows);
        assert_eq!(rows.iter().map(|d| d.id).collect::<Vec<_>>(), vec![1, 2]);
    }

    #[test]
    fn extract_urls_parses_url_shortcut_and_dedupes() {
        let text = "\
[InternetShortcut]
URL=https://example.com/archive.zip
# comment https://ignored.example/x.zip
Download <https://cdn.example.org/video.mp4> and https://example.com/archive.zip
";
        assert_eq!(
            extract_urls_from_text(text),
            vec![
                "https://example.com/archive.zip".to_string(),
                "https://cdn.example.org/video.mp4".to_string()
            ]
        );
    }

    #[test]
    fn extract_links_from_html_resolves_relative_and_filters_assets() {
        let html = r#"
            <link rel="stylesheet" href="/style.css">
            <a href="/about.html">About</a>
            <a href="files/app-v1.zip">ZIP</a>
            <a href='https://dl.example.org/release.exe#mirror'>EXE</a>
            <a href="https://example.com/download?id=42">Signed Download</a>
            <a href="javascript:void(0)">JS</a>
            <a href="files/app-v1.zip">Dup</a>
        "#;
        assert_eq!(
            extract_links_from_html("https://example.com/releases/index.html", html),
            vec![
                "https://example.com/releases/files/app-v1.zip".to_string(),
                "https://dl.example.org/release.exe".to_string(),
                "https://example.com/download?id=42".to_string(),
            ]
        );
    }
}
