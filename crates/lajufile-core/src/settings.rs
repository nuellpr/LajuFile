//! Persistent application settings.

use crate::paths;
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};

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
        }
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
