//! Resolve on-disk locations for LajuFile data and downloads.

use directories::{BaseDirs, UserDirs};
use std::path::{Path, PathBuf};

/// `%APPDATA%\LajuFile` on Windows, with a portable fallback.
pub fn data_dir() -> PathBuf {
    BaseDirs::new()
        .map(|b| b.data_dir().join("LajuFile"))
        .unwrap_or_else(|| {
            std::env::current_dir()
                .unwrap_or_default()
                .join(".lajufile")
        })
}

/// Returns the user's system Downloads directory when available.
pub fn user_downloads_dir() -> Option<PathBuf> {
    UserDirs::new().and_then(|u| u.download_dir().map(Path::to_path_buf))
}

/// The user's Downloads folder plus `LajuFile`, or a data-dir fallback.
pub fn default_download_dir() -> PathBuf {
    UserDirs::new()
        .and_then(|u| u.download_dir().map(|p| p.join("LajuFile")))
        .unwrap_or_else(|| data_dir().join("downloads"))
}

pub fn ensure_dir(path: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(path)
}

/// Create `path` and restrict permissions on Unix so only the owner can access secrets/state.
pub fn ensure_private_dir(path: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

pub fn settings_path(data_dir: &Path) -> PathBuf {
    data_dir.join("settings.json")
}

pub fn db_path(data_dir: &Path) -> PathBuf {
    data_dir.join("lajufile.db")
}
