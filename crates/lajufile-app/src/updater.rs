//! GitHub Releases update checker and verified self-updater.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde::Deserialize;
use url::Url;

#[cfg(not(target_os = "macos"))]
use crate::installer::APP_BIN;

pub const GITHUB_LATEST_API: &str = "https://api.github.com/repos/nuellpr/LajuFile/releases/latest";
pub const GITHUB_RELEASES_PAGE: &str = "https://github.com/nuellpr/LajuFile/releases";

#[derive(Debug, Clone, Deserialize)]
pub struct ReleaseAsset {
    pub name: String,
    pub browser_download_url: String,
    #[serde(default)]
    pub size: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    #[serde(default)]
    html_url: String,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    assets: Vec<ReleaseAsset>,
}

#[derive(Debug, Clone)]
pub struct UpdateInfo {
    pub tag: String,
    pub version: String,
    pub notes: String,
    pub release_url: String,
    pub asset_name: Option<String>,
    pub asset_url: Option<String>,
    pub asset_size: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct UpdateCheck {
    pub current: String,
    pub latest: UpdateInfo,
    pub has_update: bool,
}

#[derive(Debug, Clone)]
pub enum UpdateEvent {
    Checked(Result<UpdateCheck, String>),
    DownloadProgress { downloaded: u64, total: Option<u64> },
    ReadyToApply(Result<PathBuf, String>),
}

pub fn parse_semver(raw: &str) -> Option<(u64, u64, u64)> {
    let clean = raw.trim().trim_start_matches(['v', 'V']);
    let core = clean.split(['-', '+']).next()?.trim();
    let mut parts = core.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next().unwrap_or("0").parse().ok()?;
    let patch = parts.next().unwrap_or("0").parse().ok()?;
    Some((major, minor, patch))
}

pub fn is_newer_version(current: &str, latest: &str) -> bool {
    match (parse_semver(current), parse_semver(latest)) {
        (Some(cur), Some(lat)) => lat > cur,
        _ => false,
    }
}

pub fn validate_update_download_url(raw: &str) -> Result<Url, String> {
    let url = Url::parse(raw.trim()).map_err(|e| format!("Invalid update URL: {e}"))?;
    if url.scheme() != "https" {
        return Err("Update URL must use HTTPS".to_string());
    }
    let host = url
        .host_str()
        .map(str::to_ascii_lowercase)
        .ok_or_else(|| "Update URL missing host".to_string())?;
    let allowed = matches!(
        host.as_str(),
        "github.com"
            | "api.github.com"
            | "objects.githubusercontent.com"
            | "release-assets.githubusercontent.com"
            | "codeload.github.com"
    );
    if !allowed {
        return Err(format!("Untrusted update host: {host}"));
    }
    Ok(url)
}

pub fn select_asset_for_platform(assets: &[ReleaseAsset]) -> Option<&ReleaseAsset> {
    let matches = |asset: &ReleaseAsset| -> bool {
        let lower = asset.name.to_ascii_lowercase();
        if cfg!(windows) {
            lower.contains("windows") && lower.ends_with(".exe")
        } else if cfg!(target_os = "macos") {
            lower.contains("macos") && lower.ends_with(".dmg")
        } else {
            lower.contains("linux") && lower.ends_with(".run")
        }
    };
    assets.iter().find(|a| matches(a))
}

fn build_update_client(total_timeout: Duration) -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(total_timeout)
        .user_agent(concat!("LajuFile/", env!("CARGO_PKG_VERSION")))
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() >= 8 {
                return attempt.stop();
            }
            if let Err(msg) = validate_update_download_url(attempt.url().as_str()) {
                return attempt.error(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    msg,
                ));
            }
            attempt.follow()
        }))
        .build()
        .map_err(|e| format!("HTTP client error: {e}"))
}

pub fn spawn_check(tx: mpsc::Sender<UpdateEvent>) {
    std::thread::spawn(move || {
        let rt = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(rt) => rt,
            Err(e) => {
                let _ = tx.send(UpdateEvent::Checked(Err(e.to_string())));
                return;
            }
        };
        let res = rt.block_on(fetch_latest_release());
        let _ = tx.send(UpdateEvent::Checked(res));
    });
}

async fn fetch_latest_release() -> Result<UpdateCheck, String> {
    let url = validate_update_download_url(GITHUB_LATEST_API)?;
    let client = build_update_client(Duration::from_secs(20))?;
    let resp = client
        .get(url)
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| format!("Network error: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("GitHub returned HTTP {}", resp.status().as_u16()));
    }
    let bytes = resp.bytes().await.map_err(|e| format!("Read error: {e}"))?;
    let release: GitHubRelease =
        serde_json::from_slice(&bytes).map_err(|e| format!("Invalid GitHub JSON: {e}"))?;

    let current = env!("CARGO_PKG_VERSION").to_string();
    let version = release
        .tag_name
        .trim()
        .trim_start_matches(['v', 'V'])
        .to_string();
    let has_update = is_newer_version(&current, &release.tag_name);
    let asset = select_asset_for_platform(&release.assets);
    let release_url = if release.html_url.is_empty() {
        GITHUB_RELEASES_PAGE.to_string()
    } else {
        release.html_url
    };
    let notes = release
        .body
        .unwrap_or_default()
        .lines()
        .take(16)
        .collect::<Vec<_>>()
        .join("\n");

    Ok(UpdateCheck {
        current,
        latest: UpdateInfo {
            tag: release.tag_name,
            version,
            notes,
            release_url,
            asset_name: asset.map(|a| a.name.clone()),
            asset_url: asset.map(|a| a.browser_download_url.clone()),
            asset_size: asset.and_then(|a| a.size),
        },
        has_update,
    })
}

pub fn spawn_download(asset_url: String, asset_name: String, tx: mpsc::Sender<UpdateEvent>) {
    std::thread::spawn(move || {
        let rt = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(rt) => rt,
            Err(e) => {
                let _ = tx.send(UpdateEvent::ReadyToApply(Err(e.to_string())));
                return;
            }
        };
        let res = rt.block_on(download_installer(&asset_url, &asset_name, &tx));
        let _ = tx.send(UpdateEvent::ReadyToApply(res));
    });
}

async fn download_installer(
    asset_url: &str,
    asset_name: &str,
    tx: &mpsc::Sender<UpdateEvent>,
) -> Result<PathBuf, String> {
    let url = validate_update_download_url(asset_url)?;
    let safe_name = lajufile_core::naming::sanitize_file_name(asset_name);
    let target = std::env::temp_dir().join(format!("lajufile-update-{safe_name}"));
    let part = std::env::temp_dir().join(format!("lajufile-update-{safe_name}.part"));

    let client = build_update_client(Duration::from_secs(600))?;
    let mut resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("Download failed: {e}"))?
        .error_for_status()
        .map_err(|e| format!("Download failed: {e}"))?;

    let total = resp.content_length();
    let mut file = fs::File::create(&part).map_err(|e| format!("Temp file error: {e}"))?;
    let mut downloaded = 0_u64;
    let mut last_send = Instant::now();
    while let Some(chunk) = resp
        .chunk()
        .await
        .map_err(|e| format!("Download stream error: {e}"))?
    {
        file.write_all(&chunk)
            .map_err(|e| format!("Write error: {e}"))?;
        downloaded = downloaded.saturating_add(chunk.len() as u64);
        if last_send.elapsed() >= Duration::from_millis(80) {
            let _ = tx.send(UpdateEvent::DownloadProgress { downloaded, total });
            last_send = Instant::now();
        }
    }
    file.flush().map_err(|e| format!("Flush error: {e}"))?;
    drop(file);
    let _ = tx.send(UpdateEvent::DownloadProgress { downloaded, total });

    if downloaded < 1024 {
        let _ = fs::remove_file(&part);
        return Err("Downloaded installer is unexpectedly small or empty".to_string());
    }
    if cfg!(windows) {
        let header = fs::read(&part).map_err(|e| e.to_string())?;
        if !header.starts_with(b"MZ") || !header.ends_with(b"LAJUFILE_PK1") {
            let _ = fs::remove_file(&part);
            return Err("Downloaded file failed Windows installer verification".to_string());
        }
    }
    let _ = fs::remove_file(&target);
    fs::rename(&part, &target).map_err(|e| format!("Finalize error: {e}"))?;
    Ok(target)
}

/// Launches the downloaded installer and returns `true` when LajuFile should exit
/// so the silent upgrader can overwrite the running executable cleanly.
pub fn launch_installer_and_update(installer_path: &Path) -> Result<bool, String> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(installer_path)
            .spawn()
            .map_err(|e| format!("Could not open disk image: {e}"))?;
        Ok(false)
    }

    #[cfg(not(target_os = "macos"))]
    {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Ok(meta) = fs::metadata(installer_path) {
                let mut perms = meta.permissions();
                perms.set_mode(0o755);
                let _ = fs::set_permissions(installer_path, perms);
            }
        }
        let install_dir = std::env::current_exe()
            .ok()
            .and_then(|p| {
                let is_installed = p
                    .file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.eq_ignore_ascii_case(APP_BIN));
                is_installed
                    .then(|| p.parent().map(Path::to_path_buf))
                    .flatten()
            })
            .unwrap_or_else(crate::installer::default_install_dir);

        std::process::Command::new(installer_path)
            .args(["--silent", "--relaunch", "--dir"])
            .arg(&install_dir)
            .spawn()
            .map_err(|e| format!("Failed to start installer: {e}"))?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semver_parsing_and_comparison_work() {
        assert_eq!(parse_semver("v1.0.1"), Some((1, 0, 1)));
        assert_eq!(parse_semver("1.2.3-beta.1"), Some((1, 2, 3)));
        assert!(is_newer_version("1.0.0", "v1.0.1"));
        assert!(is_newer_version("1.0.1", "v1.1.0"));
        assert!(!is_newer_version("1.0.1", "v1.0.1"));
        assert!(!is_newer_version("1.1.0", "v1.0.9"));
    }

    #[test]
    fn update_url_rejects_untrusted_or_http_hosts() {
        assert!(validate_update_download_url(GITHUB_LATEST_API).is_ok());
        assert!(
            validate_update_download_url(
                "https://objects.githubusercontent.com/github-production-release-asset/foo.exe"
            )
            .is_ok()
        );
        assert!(validate_update_download_url("http://github.com/nuellpr/LajuFile").is_err());
        assert!(validate_update_download_url("https://evil.example.com/payload.exe").is_err());
    }

    #[test]
    fn select_asset_matches_current_platform() {
        let assets = vec![
            ReleaseAsset {
                name: "LajuFile-v1.0.1-windows-x64-setup.exe".into(),
                browser_download_url: "https://github.com/x.exe".into(),
                size: Some(100),
            },
            ReleaseAsset {
                name: "LajuFile-v1.0.1-linux-x64-setup.run".into(),
                browser_download_url: "https://github.com/x.run".into(),
                size: Some(100),
            },
            ReleaseAsset {
                name: "LajuFile-v1.0.1-macos-arm64-setup.dmg".into(),
                browser_download_url: "https://github.com/x.dmg".into(),
                size: Some(100),
            },
        ];
        let picked = select_asset_for_platform(&assets).expect("matching asset");
        if cfg!(windows) {
            assert!(picked.name.ends_with(".exe"));
        } else if cfg!(target_os = "macos") {
            assert!(picked.name.ends_with(".dmg"));
        } else {
            assert!(picked.name.ends_with(".run"));
        }
    }
}
