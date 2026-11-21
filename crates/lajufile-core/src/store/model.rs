//! Download records and enums shared by the store and engine.

use super::StoreError;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DownloadStatus {
    Queued,
    Downloading,
    Paused,
    Done,
    Error,
}

impl DownloadStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Downloading => "downloading",
            Self::Paused => "paused",
            Self::Done => "done",
            Self::Error => "error",
        }
    }

    pub fn from_db(value: &str) -> Result<Self, StoreError> {
        match value {
            "queued" => Ok(Self::Queued),
            "downloading" => Ok(Self::Downloading),
            "paused" => Ok(Self::Paused),
            "done" => Ok(Self::Done),
            "error" => Ok(Self::Error),
            other => Err(StoreError::InvalidStatus(other.to_string())),
        }
    }

    pub fn is_active(self) -> bool {
        matches!(self, Self::Queued | Self::Downloading | Self::Paused)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Video,
    Music,
    Documents,
    Programs,
    Compressed,
    Other,
}

impl Category {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Video => "video",
            Self::Music => "music",
            Self::Documents => "documents",
            Self::Programs => "programs",
            Self::Compressed => "compressed",
            Self::Other => "other",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Video => "Video",
            Self::Music => "Music",
            Self::Documents => "Documents",
            Self::Programs => "Programs",
            Self::Compressed => "Compressed",
            Self::Other => "Other",
        }
    }

    pub fn from_db(value: &str) -> Result<Self, StoreError> {
        match value {
            "video" => Ok(Self::Video),
            "music" => Ok(Self::Music),
            "documents" => Ok(Self::Documents),
            "programs" => Ok(Self::Programs),
            "compressed" => Ok(Self::Compressed),
            "other" => Ok(Self::Other),
            other => Err(StoreError::InvalidCategory(other.to_string())),
        }
    }

    pub fn from_file_name(name: &str) -> Self {
        let extension = std::path::Path::new(name)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();

        match extension.as_str() {
            "mp4" | "mkv" | "avi" | "mov" | "webm" | "wmv" | "flv" | "m4v" | "mpg" | "mpeg"
            | "ts" => Self::Video,
            "mp3" | "flac" | "wav" | "aac" | "ogg" | "m4a" | "wma" | "opus" => Self::Music,
            "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "txt" | "rtf" | "csv"
            | "epub" | "md" => Self::Documents,
            "exe" | "msi" | "msix" | "appx" | "dll" | "bat" | "cmd" | "ps1" | "dmg" | "deb"
            | "rpm" | "apk" | "appimage" => Self::Programs,
            "zip" | "rar" | "7z" | "tar" | "gz" | "bz2" | "xz" | "zst" | "tgz" => Self::Compressed,
            _ => Self::Other,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    pub idx: u32,
    pub start: u64,
    pub end: u64,
    pub done: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Download {
    pub id: i64,
    pub url: String,
    pub final_url: Option<String>,
    pub file_name: String,
    pub save_path: PathBuf,
    pub temp_path: PathBuf,
    pub size: Option<u64>,
    pub downloaded: u64,
    pub status: DownloadStatus,
    pub category: Category,
    pub kind: String,
    pub video_format: Option<String>,
    pub connections: u32,
    pub ranged: bool,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub cookie_enc: Option<Vec<u8>>,
    pub referrer: Option<String>,
    pub user_agent: Option<String>,
    pub error: Option<String>,
    pub queue_order: i64,
    pub created_at: i64,
    pub completed_at: Option<i64>,
    pub speed_limit_kbps: u64,
    pub expected_sha256: Option<String>,
    pub checksum_status: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewDownload {
    pub url: String,
    pub final_url: Option<String>,
    pub file_name: String,
    pub save_path: PathBuf,
    pub temp_path: PathBuf,
    pub size: Option<u64>,
    pub status: DownloadStatus,
    pub category: Category,
    pub kind: String,
    pub video_format: Option<String>,
    pub connections: u32,
    pub ranged: bool,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub cookie_enc: Option<Vec<u8>>,
    pub referrer: Option<String>,
    pub user_agent: Option<String>,
    pub queue_order: i64,
    pub created_at: i64,
    pub speed_limit_kbps: u64,
    pub expected_sha256: Option<String>,
    pub segments: Vec<Segment>,
}

pub fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}
