//! Persistent application settings.

use crate::paths;
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};

pub const DEFAULT_CAPTURE_EXTENSIONS: &str = "zip rar 7z tar gz bz2 xz tgz exe msi msix apk dmg deb rpm iso img bin mp4 mkv webm avi mov flv wmv m4v mp3 m4a wav flac ogg aac opus pdf doc docx xls xlsx ppt pptx epub csv torrent";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DuplicatePolicy {
    #[default]
    Reject,
    RenameNumbered,
    ResumeExisting,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    Light,
    Dark,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    pub download_dir: PathBuf,
    pub max_connections: u32,
    pub max_concurrent: u32,
    pub global_speed_limit_kbps: u64,
    pub close_to_tray: bool,
    pub autostart: bool,
    pub auto_resume: bool,
    pub theme: Theme,
    pub bridge_port: u16,
    pub bridge_token: String,
    #[serde(default)]
    pub ask_where_to_save: bool,
    #[serde(default = "default_true")]
    pub clipboard_monitor: bool,
    #[serde(default)]
    pub category_dirs: std::collections::BTreeMap<String, PathBuf>,
    #[serde(default = "default_scale")]
    pub ui_scale_pct: u32,
    #[serde(default)]
    pub excluded_domains: String,
    #[serde(default = "default_capture_extensions")]
    pub captured_extensions: String,
    #[serde(default = "default_true")]
    pub show_complete_dialog: bool,
    #[serde(default)]
    pub show_progress_on_start: bool,
    #[serde(default)]
    pub duplicate_policy: DuplicatePolicy,
    #[serde(default)]
    pub video_subtitles: bool,
    #[serde(default = "default_true")]
    pub video_chapters: bool,
    #[serde(default)]
    pub browser_paired: bool,
    #[serde(default)]
    pub suppress_browser_wizard: bool,
    #[serde(default = "default_language")]
    pub language: String,
}

fn default_true() -> bool {
    true
}

fn default_scale() -> u32 {
    100
}

fn default_capture_extensions() -> String {
    DEFAULT_CAPTURE_EXTENSIONS.to_string()
}

fn default_language() -> String {
    "system".to_string()
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            download_dir: paths::default_download_dir(),
            max_connections: 8,
            max_concurrent: 3,
            global_speed_limit_kbps: 0,
            close_to_tray: true,
            autostart: false,
            auto_resume: false,
            theme: Theme::Light,
            bridge_port: 53841,
            bridge_token: String::new(),
            ask_where_to_save: false,
            clipboard_monitor: true,
            category_dirs: std::collections::BTreeMap::new(),
            ui_scale_pct: 100,
            excluded_domains: String::new(),
            captured_extensions: default_capture_extensions(),
            show_complete_dialog: true,
            show_progress_on_start: false,
            duplicate_policy: DuplicatePolicy::Reject,
            video_subtitles: false,
            video_chapters: true,
            browser_paired: false,
            suppress_browser_wizard: false,
            language: default_language(),
        }
    }
}

impl Settings {
    pub fn is_domain_excluded(&self, host: &str) -> bool {
        let host = host.trim().to_ascii_lowercase();
        if host.is_empty() {
            return false;
        }
        self.excluded_domains
            .split(|c: char| c.is_whitespace() || c == ',' || c == ';')
            .filter_map(|token| {
                let clean = token
                    .trim()
                    .trim_start_matches("*.")
                    .trim_start_matches('.')
                    .to_ascii_lowercase();
                (!clean.is_empty()).then_some(clean)
            })
            .any(|rule| host == rule || host.ends_with(&format!(".{rule}")))
    }

    pub fn is_extension_captured(&self, ext: &str) -> bool {
        let ext = ext.trim().trim_start_matches('.').to_ascii_lowercase();
        if ext.is_empty() {
            return false;
        }
        let source = if self.captured_extensions.trim().is_empty() {
            DEFAULT_CAPTURE_EXTENSIONS
        } else {
            self.captured_extensions.as_str()
        };
        source
            .split(|c: char| c.is_whitespace() || c == ',' || c == ';')
            .map(|token| token.trim().trim_start_matches('.').to_ascii_lowercase())
            .filter(|token| !token.is_empty())
            .any(|rule| rule == "*" || rule == ext)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SettingsError {
    #[error("settings I/O at {path:?}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("invalid settings JSON at {path:?}: {source}")]
    Json {
        path: PathBuf,
        source: serde_json::Error,
    },
}

pub fn generate_token() -> String {
    hex::encode(rand::random::<[u8; 32]>())
}

fn is_valid_token(token: &str) -> bool {
    token.len() == 64 && token.bytes().all(|b| b.is_ascii_hexdigit())
}

impl Settings {
    pub fn load_or_create(dir: &Path) -> Result<Self, SettingsError> {
        paths::ensure_dir(dir).map_err(|source| SettingsError::Io {
            path: dir.to_path_buf(),
            source,
        })?;

        let path = paths::settings_path(dir);
        if !path.exists() {
            let settings = Self {
                bridge_token: generate_token(),
                ..Self::default()
            };
            settings.save(dir)?;
            return Ok(settings);
        }

        let text = std::fs::read_to_string(&path).map_err(|source| SettingsError::Io {
            path: path.clone(),
            source,
        })?;
        let mut settings: Self =
            serde_json::from_str(&text).map_err(|source| SettingsError::Json {
                path: path.clone(),
                source,
            })?;

        if !is_valid_token(&settings.bridge_token) {
            settings.bridge_token = generate_token();
            settings.save(dir)?;
        }

        Ok(settings)
    }

    pub fn save(&self, dir: &Path) -> Result<(), SettingsError> {
        let path = paths::settings_path(dir);
        let tmp = dir.join("settings.json.tmp");
        let json = serde_json::to_string_pretty(self).map_err(|source| SettingsError::Json {
            path: path.clone(),
            source,
        })?;

        let mut file = std::fs::File::create(&tmp).map_err(|source| SettingsError::Io {
            path: tmp.clone(),
            source,
        })?;
        file.write_all(json.as_bytes())
            .map_err(|source| SettingsError::Io {
                path: tmp.clone(),
                source,
            })?;
        file.sync_all().map_err(|source| SettingsError::Io {
            path: tmp.clone(),
            source,
        })?;
        drop(file);

        std::fs::rename(&tmp, &path).map_err(|source| SettingsError::Io { path, source })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ask_where_to_save_defaults_off() {
        assert!(!Settings::default().ask_where_to_save);
    }

    #[test]
    fn old_settings_without_field_still_load() {
        let json = r#"{
            "download_dir": "C:/Downloads",
            "max_connections": 8,
            "max_concurrent": 3,
            "global_speed_limit_kbps": 0,
            "close_to_tray": true,
            "autostart": false,
            "auto_resume": false,
            "theme": "dark",
            "bridge_port": 53841,
            "bridge_token": ""
        }"#;
        let settings: Settings = serde_json::from_str(json).unwrap();
        assert!(!settings.ask_where_to_save);
        assert_eq!(settings.ui_scale_pct, 100);
        assert!(settings.show_complete_dialog);
        assert!(!settings.show_progress_on_start);
        assert_eq!(settings.duplicate_policy, DuplicatePolicy::Reject);
        assert!(!settings.browser_paired);
        assert!(!settings.suppress_browser_wizard);
        assert_eq!(settings.language, "system");
    }

    #[test]
    fn domain_and_extension_rules_match_cleanly() {
        let s = Settings {
            excluded_domains: "*.example.com, cdn.skip.org".to_string(),
            captured_extensions: "zip, .gguf, iso".to_string(),
            ..Settings::default()
        };
        assert!(s.is_domain_excluded("example.com"));
        assert!(s.is_domain_excluded("dl.example.com"));
        assert!(s.is_domain_excluded("CDN.SKIP.ORG"));
        assert!(!s.is_domain_excluded("other-example.com"));
        assert!(s.is_extension_captured("gguf"));
        assert!(s.is_extension_captured(".ZIP"));
        assert!(!s.is_extension_captured("html"));
    }

    #[test]
    fn save_and_load_roundtrips_ask_where_to_save() {
        let dir = tempfile::tempdir().unwrap();
        let settings = Settings {
            ask_where_to_save: true,
            ..Settings::default()
        };
        settings.save(dir.path()).unwrap();
        let loaded = Settings::load_or_create(dir.path()).unwrap();
        assert!(loaded.ask_where_to_save);
    }
}
