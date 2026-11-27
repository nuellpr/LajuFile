//! Persistent application settings.

use crate::paths;
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};

pub const DEFAULT_CAPTURE_EXTENSIONS: &str = "zip rar 7z tar gz bz2 xz tgz exe msi msix apk dmg deb rpm iso img bin mp4 mkv webm avi mov flv wmv m4v mp3 m4a wav flac ogg aac opus pdf doc docx xls xlsx ppt pptx epub csv torrent";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueueCompleteAction {
    #[default]
    None,
    ExitApp,
    Sleep,
    Shutdown,
}

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
    #[serde(default)]
    pub domain_dirs: std::collections::BTreeMap<String, PathBuf>,
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
    #[serde(default)]
    pub proxy_url: String,
    #[serde(default = "default_true")]
    pub auto_check_updates: bool,
    #[serde(default)]
    pub last_update_check_ms: i64,
    #[serde(default)]
    pub schedule_enabled: bool,
    #[serde(default = "default_schedule_start")]
    pub schedule_start: String,
    #[serde(default = "default_schedule_stop")]
    pub schedule_stop: String,
    #[serde(default)]
    pub on_queue_complete: QueueCompleteAction,
    #[serde(default)]
    pub hf_token: String,
    #[serde(default)]
    pub civitai_token: String,
    #[serde(default)]
    pub vt_api_key: String,
    #[serde(default)]
    pub auto_extract_archives: bool,
    #[serde(default)]
    pub auto_throttle_fullscreen: bool,
    #[serde(default = "default_auto_throttle_kbps")]
    pub auto_throttle_kbps: u64,
    #[serde(default)]
    pub auto_scan_downloads: bool,
    #[serde(default)]
    pub peak_speed_bps: u64,
}

fn default_auto_throttle_kbps() -> u64 {
    512
}

fn default_schedule_start() -> String {
    "00:00".to_string()
}

fn default_schedule_stop() -> String {
    "06:00".to_string()
}

fn default_true() -> bool {
    true
}

fn default_scale() -> u32 {
    100
}

fn default_capture_extensions() -> String {
    String::new()
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
            domain_dirs: std::collections::BTreeMap::new(),
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
            proxy_url: String::new(),
            auto_check_updates: true,
            last_update_check_ms: 0,
            schedule_enabled: false,
            schedule_start: default_schedule_start(),
            schedule_stop: default_schedule_stop(),
            on_queue_complete: QueueCompleteAction::None,
            hf_token: String::new(),
            civitai_token: String::new(),
            vt_api_key: String::new(),
            auto_extract_archives: false,
            auto_throttle_fullscreen: false,
            auto_throttle_kbps: default_auto_throttle_kbps(),
            auto_scan_downloads: false,
            peak_speed_bps: 0,
        }
    }
}

/// Parses `"HH:MM"` (24-hour clock) into minutes since midnight (`0..1440`).
pub fn parse_hhmm(raw: &str) -> Option<u16> {
    let (h, m) = raw.trim().split_once(':')?;
    let hour: u16 = h.trim().parse().ok()?;
    let minute: u16 = m.trim().parse().ok()?;
    (hour < 24 && minute < 60).then_some(hour * 60 + minute)
}

/// Returns true when `now_mins` falls inside `[start_mins, stop_mins)`, including
/// overnight spans like `23:00` → `06:00`. Equal start and stop means always active.
pub fn is_within_schedule(now_mins: u16, start_mins: u16, stop_mins: u16) -> bool {
    if start_mins == stop_mins {
        return true;
    }
    if start_mins < stop_mins {
        now_mins >= start_mins && now_mins < stop_mins
    } else {
        now_mins >= start_mins || now_mins < stop_mins
    }
}

pub fn normalize_domain_rule(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let extracted = if let Ok(parsed) = url::Url::parse(trimmed)
        && let Some(host) = parsed.host_str()
    {
        host.to_string()
    } else {
        let without_scheme = trimmed
            .strip_prefix("https://")
            .or_else(|| trimmed.strip_prefix("http://"))
            .unwrap_or(trimmed);
        let host_port = without_scheme
            .split(['/', '?', '#'])
            .next()
            .unwrap_or(without_scheme);
        host_port.split(':').next().unwrap_or(host_port).to_string()
    };
    extracted
        .trim()
        .trim_start_matches("*.")
        .trim_start_matches('.')
        .trim_end_matches('.')
        .to_ascii_lowercase()
}

impl Settings {
    pub fn domain_dir_for_url(&self, raw_url: &str) -> Option<&PathBuf> {
        let parsed = url::Url::parse(raw_url.trim()).ok()?;
        let host = parsed
            .host_str()?
            .trim_end_matches('.')
            .to_ascii_lowercase();
        if host.is_empty() {
            return None;
        }
        self.domain_dirs
            .iter()
            .filter_map(|(rule, dir)| {
                if dir.as_os_str().is_empty() {
                    return None;
                }
                let clean = normalize_domain_rule(rule);
                if clean.is_empty() {
                    return None;
                }
                let matches = host == clean || host.ends_with(&format!(".{clean}"));
                matches.then_some((clean.len(), dir))
            })
            .max_by_key(|(len, _)| *len)
            .map(|(_, dir)| dir)
    }

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

pub fn is_valid_token(token: &str) -> bool {
    token.len() == 64 && token.bytes().all(|b| b.is_ascii_hexdigit())
}

const SEALED_SECRET_PREFIX: &str = "enc:v1:";

fn proxy_has_credentials(raw: &str) -> bool {
    url::Url::parse(raw.trim())
        .is_ok_and(|parsed| !parsed.username().is_empty() || parsed.password().is_some())
}

fn seal_secret(plain: &str, path: &Path) -> Result<String, SettingsError> {
    let trimmed = plain.trim();
    if trimmed.is_empty() || trimmed.starts_with(SEALED_SECRET_PREFIX) {
        return Ok(trimmed.to_string());
    }
    let encrypted = crate::cookie::protect_cookie(trimmed).map_err(|err| SettingsError::Io {
        path: path.to_path_buf(),
        source: std::io::Error::other(format!("protect secret: {err}")),
    })?;
    Ok(format!("{SEALED_SECRET_PREFIX}{}", hex::encode(encrypted)))
}

fn unseal_secret(stored: &str) -> String {
    let trimmed = stored.trim();
    let Some(hex_body) = trimmed.strip_prefix(SEALED_SECRET_PREFIX) else {
        return trimmed.to_string();
    };
    let Ok(bytes) = hex::decode(hex_body) else {
        return String::new();
    };
    crate::cookie::unprotect_cookie(&bytes).unwrap_or_default()
}

impl Settings {
    /// Reads and unseals settings from disk without creating or modifying the file.
    pub fn load_readonly(dir: &Path) -> Option<Self> {
        let text = std::fs::read_to_string(paths::settings_path(dir)).ok()?;
        let mut settings: Self = serde_json::from_str(&text).ok()?;
        settings.bridge_token = unseal_secret(&settings.bridge_token);
        settings.hf_token = unseal_secret(&settings.hf_token);
        settings.civitai_token = unseal_secret(&settings.civitai_token);
        settings.vt_api_key = unseal_secret(&settings.vt_api_key);
        settings.proxy_url = unseal_secret(&settings.proxy_url);
        Some(settings)
    }

    pub fn to_safe_backup_json(&self) -> Result<String, SettingsError> {
        let mut exported = self.clone();
        exported.bridge_token.clear();
        exported.hf_token.clear();
        exported.civitai_token.clear();
        exported.vt_api_key.clear();
        if proxy_has_credentials(&exported.proxy_url) {
            exported.proxy_url.clear();
        }
        serde_json::to_string_pretty(&exported).map_err(|source| SettingsError::Json {
            path: PathBuf::from("settings-backup.json"),
            source,
        })
    }

    pub fn merge_from_backup_json(&self, raw: &str) -> Result<Self, SettingsError> {
        let mut imported: Self =
            serde_json::from_str(raw.trim()).map_err(|source| SettingsError::Json {
                path: PathBuf::from("settings-backup.json"),
                source,
            })?;
        if imported.download_dir.as_os_str().is_empty() {
            imported.download_dir = self.download_dir.clone();
        }
        imported.max_concurrent = imported.max_concurrent.clamp(1, 10);
        imported.max_connections = imported.max_connections.clamp(1, 32);
        imported.ui_scale_pct = imported.ui_scale_pct.clamp(80, 150);
        imported.bridge_port = imported.bridge_port.clamp(1024, 65_535);
        imported.global_speed_limit_kbps = imported.global_speed_limit_kbps.min(1_000_000);
        imported.auto_throttle_kbps = imported.auto_throttle_kbps.min(1_000_000);
        imported.bridge_token = self.bridge_token.clone();
        imported.browser_paired = self.browser_paired;
        imported.peak_speed_bps = self.peak_speed_bps.max(imported.peak_speed_bps);
        if imported.hf_token.trim().is_empty() {
            imported.hf_token = self.hf_token.clone();
        }
        if imported.civitai_token.trim().is_empty() {
            imported.civitai_token = self.civitai_token.clone();
        }
        if imported.vt_api_key.trim().is_empty() {
            imported.vt_api_key = self.vt_api_key.clone();
        }
        if imported.proxy_url.trim().is_empty() {
            imported.proxy_url = self.proxy_url.clone();
        }
        Ok(imported)
    }

    pub fn load_or_create(dir: &Path) -> Result<Self, SettingsError> {
        paths::ensure_private_dir(dir).map_err(|source| SettingsError::Io {
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

        let mut needs_resave = false;
        let raw_bridge = settings.bridge_token.trim().to_string();
        if !raw_bridge.is_empty() && !raw_bridge.starts_with(SEALED_SECRET_PREFIX) {
            needs_resave = true;
        }
        settings.bridge_token = unseal_secret(&raw_bridge);
        if !is_valid_token(&settings.bridge_token) {
            settings.bridge_token = generate_token();
            needs_resave = true;
        }
        let raw_hf = settings.hf_token.trim().to_string();
        if !raw_hf.is_empty() && !raw_hf.starts_with(SEALED_SECRET_PREFIX) {
            needs_resave = true;
        }
        settings.hf_token = unseal_secret(&raw_hf);

        let raw_civitai = settings.civitai_token.trim().to_string();
        if !raw_civitai.is_empty() && !raw_civitai.starts_with(SEALED_SECRET_PREFIX) {
            needs_resave = true;
        }
        settings.civitai_token = unseal_secret(&raw_civitai);

        let raw_vt = settings.vt_api_key.trim().to_string();
        if !raw_vt.is_empty() && !raw_vt.starts_with(SEALED_SECRET_PREFIX) {
            needs_resave = true;
        }
        settings.vt_api_key = unseal_secret(&raw_vt);

        let raw_proxy = settings.proxy_url.trim().to_string();
        if !raw_proxy.is_empty()
            && !raw_proxy.starts_with(SEALED_SECRET_PREFIX)
            && proxy_has_credentials(&raw_proxy)
        {
            needs_resave = true;
        }
        settings.proxy_url = unseal_secret(&raw_proxy);

        if settings.captured_extensions.trim() == DEFAULT_CAPTURE_EXTENSIONS {
            settings.captured_extensions.clear();
            needs_resave = true;
        }

        if needs_resave {
            settings.save(dir)?;
        }

        Ok(settings)
    }

    pub fn save(&self, dir: &Path) -> Result<(), SettingsError> {
        static TMP_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        paths::ensure_private_dir(dir).map_err(|source| SettingsError::Io {
            path: dir.to_path_buf(),
            source,
        })?;
        let path = paths::settings_path(dir);
        let seq = TMP_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let tmp = dir.join(format!("settings.json.tmp.{}.{seq}", std::process::id()));
        let mut persisted = self.clone();
        persisted.bridge_token = seal_secret(&persisted.bridge_token, &path)?;
        persisted.hf_token = seal_secret(&persisted.hf_token, &path)?;
        persisted.civitai_token = seal_secret(&persisted.civitai_token, &path)?;
        persisted.vt_api_key = seal_secret(&persisted.vt_api_key, &path)?;
        if proxy_has_credentials(&persisted.proxy_url) {
            persisted.proxy_url = seal_secret(&persisted.proxy_url, &path)?;
        }
        let json =
            serde_json::to_string_pretty(&persisted).map_err(|source| SettingsError::Json {
                path: path.clone(),
                source,
            })?;

        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut file = opts.open(&tmp).map_err(|source| SettingsError::Io {
            path: tmp.clone(),
            source,
        })?;
        if let Err(source) = file
            .write_all(json.as_bytes())
            .and_then(|()| file.sync_all())
        {
            drop(file);
            let _ = std::fs::remove_file(&tmp);
            return Err(SettingsError::Io { path: tmp, source });
        }
        drop(file);

        if let Err(source) = std::fs::rename(&tmp, &path) {
            let _ = std::fs::remove_file(&tmp);
            return Err(SettingsError::Io { path, source });
        }
        Ok(())
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
        assert!(settings.proxy_url.is_empty());
        assert!(settings.auto_check_updates);
        assert_eq!(settings.last_update_check_ms, 0);
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

    #[test]
    fn schedule_parsing_and_overnight_windows_work() {
        assert_eq!(parse_hhmm("00:00"), Some(0));
        assert_eq!(parse_hhmm("23:59"), Some(23 * 60 + 59));
        assert_eq!(parse_hhmm("24:00"), None);
        assert_eq!(parse_hhmm("12:60"), None);
        assert_eq!(parse_hhmm("nope"), None);

        // Daytime window 09:00..17:00
        assert!(!is_within_schedule(8 * 60 + 59, 9 * 60, 17 * 60));
        assert!(is_within_schedule(9 * 60, 9 * 60, 17 * 60));
        assert!(is_within_schedule(16 * 60 + 59, 9 * 60, 17 * 60));
        assert!(!is_within_schedule(17 * 60, 9 * 60, 17 * 60));

        // Overnight window 23:00..06:00
        assert!(!is_within_schedule(22 * 60 + 59, 23 * 60, 6 * 60));
        assert!(is_within_schedule(23 * 60, 23 * 60, 6 * 60));
        assert!(is_within_schedule(2 * 60, 23 * 60, 6 * 60));
        assert!(!is_within_schedule(6 * 60, 23 * 60, 6 * 60));
    }

    #[test]
    fn hf_token_and_proxy_credentials_are_sealed_on_disk_and_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let settings = Settings {
            bridge_token: generate_token(),
            hf_token: "hf_secret_token_12345".to_string(),
            proxy_url: "http://alice:s3cr3t@127.0.0.1:8080".to_string(),
            ..Settings::default()
        };
        settings.save(dir.path()).unwrap();

        let raw = std::fs::read_to_string(paths::settings_path(dir.path())).unwrap();
        assert!(!raw.contains(&settings.bridge_token));
        assert!(!raw.contains("hf_secret_token_12345"));
        assert!(!raw.contains("s3cr3t"));
        assert!(raw.contains(SEALED_SECRET_PREFIX));

        let loaded = Settings::load_or_create(dir.path()).unwrap();
        assert_eq!(loaded.bridge_token, settings.bridge_token);
        assert_eq!(loaded.hf_token, "hf_secret_token_12345");
        assert_eq!(loaded.proxy_url, "http://alice:s3cr3t@127.0.0.1:8080");
        let ro = Settings::load_readonly(dir.path()).unwrap();
        assert_eq!(ro.bridge_token, settings.bridge_token);
        assert_eq!(ro.hf_token, "hf_secret_token_12345");

        // Legacy plaintext settings migrate seamlessly and get sealed on load.
        let mut legacy: serde_json::Value = serde_json::from_str(&raw).unwrap();
        legacy["hf_token"] = serde_json::Value::String("hf_legacy_plain".to_string());
        legacy["proxy_url"] = serde_json::Value::String("http://127.0.0.1:7890".to_string());
        std::fs::write(
            paths::settings_path(dir.path()),
            serde_json::to_string_pretty(&legacy).unwrap(),
        )
        .unwrap();
        let migrated = Settings::load_or_create(dir.path()).unwrap();
        assert_eq!(migrated.hf_token, "hf_legacy_plain");
        assert_eq!(migrated.proxy_url, "http://127.0.0.1:7890");
        let after = std::fs::read_to_string(paths::settings_path(dir.path())).unwrap();
        assert!(!after.contains("hf_legacy_plain"));
        assert!(after.contains("http://127.0.0.1:7890"));
    }

    #[test]
    fn default_capture_extensions_is_blank_and_migrates_legacy_default() {
        let def = Settings::default();
        assert!(def.captured_extensions.is_empty());
        assert!(def.is_extension_captured("zip"));
        assert!(!def.is_extension_captured("html"));

        let dir = tempfile::tempdir().unwrap();
        let legacy = Settings {
            bridge_token: generate_token(),
            captured_extensions: DEFAULT_CAPTURE_EXTENSIONS.to_string(),
            ..Settings::default()
        };
        legacy.save(dir.path()).unwrap();

        let migrated = Settings::load_or_create(dir.path()).unwrap();
        assert!(migrated.captured_extensions.is_empty());

        let reloaded = Settings::load_readonly(dir.path()).unwrap();
        assert!(reloaded.captured_extensions.is_empty());
    }

    #[test]
    fn domain_dir_matching_prefers_longest_specific_rule() {
        assert_eq!(
            normalize_domain_rule("https://*.GitHub.com:443/nuellpr"),
            "github.com"
        );
        assert_eq!(normalize_domain_rule("  .TikTok.com/ "), "tiktok.com");

        let mut s = Settings::default();
        s.domain_dirs
            .insert("github.com".to_string(), PathBuf::from("D:/Dev"));
        s.domain_dirs.insert(
            "release-assets.githubusercontent.com".to_string(),
            PathBuf::from("D:/Assets"),
        );
        s.domain_dirs
            .insert("tiktok.com".to_string(), PathBuf::from("D:/TikTok"));

        assert_eq!(
            s.domain_dir_for_url("https://github.com/a/b.zip"),
            Some(&PathBuf::from("D:/Dev"))
        );
        assert_eq!(
            s.domain_dir_for_url("https://www.tiktok.com/@user/video/123"),
            Some(&PathBuf::from("D:/TikTok"))
        );
        assert_eq!(
            s.domain_dir_for_url("https://release-assets.githubusercontent.com/file.exe"),
            Some(&PathBuf::from("D:/Assets"))
        );
        assert_eq!(s.domain_dir_for_url("https://notgithub.com/file.zip"), None);
    }

    #[test]
    fn backup_strips_secrets_and_merge_preserves_local_tokens() {
        let dir = tempfile::tempdir().unwrap();
        let original = Settings {
            bridge_token: generate_token(),
            hf_token: "hf_secret".to_string(),
            civitai_token: "civ_secret".to_string(),
            vt_api_key: "vt_secret".to_string(),
            proxy_url: "http://alice:pass@127.0.0.1:8080".to_string(),
            browser_paired: true,
            peak_speed_bps: 12_345_678,
            ..Settings::default()
        };
        original.save(dir.path()).unwrap();
        let loaded = Settings::load_or_create(dir.path()).unwrap();
        assert_eq!(loaded.civitai_token, "civ_secret");
        assert_eq!(loaded.vt_api_key, "vt_secret");

        let backup = loaded.to_safe_backup_json().unwrap();
        assert!(!backup.contains(&loaded.bridge_token));
        assert!(!backup.contains("hf_secret"));
        assert!(!backup.contains("civ_secret"));
        assert!(!backup.contains("vt_secret"));
        assert!(!backup.contains("alice:pass"));

        let mut modified: Settings = serde_json::from_str(&backup).unwrap();
        modified.max_connections = 99;
        modified.auto_throttle_fullscreen = true;
        modified.auto_throttle_kbps = 256;
        modified
            .domain_dirs
            .insert("github.com".to_string(), PathBuf::from("D:/GitHub"));
        let modified_json = serde_json::to_string_pretty(&modified).unwrap();

        let merged = loaded.merge_from_backup_json(&modified_json).unwrap();
        assert_eq!(merged.max_connections, 32);
        assert!(merged.auto_throttle_fullscreen);
        assert_eq!(merged.auto_throttle_kbps, 256);
        assert_eq!(
            merged.domain_dirs.get("github.com"),
            Some(&PathBuf::from("D:/GitHub"))
        );
        assert_eq!(merged.bridge_token, loaded.bridge_token);
        assert!(merged.browser_paired);
        assert_eq!(merged.hf_token, "hf_secret");
        assert_eq!(merged.civitai_token, "civ_secret");
        assert_eq!(merged.vt_api_key, "vt_secret");
        assert_eq!(merged.proxy_url, "http://alice:pass@127.0.0.1:8080");
        assert_eq!(merged.peak_speed_bps, 12_345_678);
    }
}
