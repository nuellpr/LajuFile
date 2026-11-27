//! Managed yt-dlp and ffmpeg binaries for video downloads.

use std::path::{Path, PathBuf};

#[cfg(windows)]
pub const YTDLP: &str = "yt-dlp.exe";
#[cfg(not(windows))]
pub const YTDLP: &str = "yt-dlp";

#[cfg(windows)]
pub const FFMPEG: &str = "ffmpeg.exe";
#[cfg(not(windows))]
pub const FFMPEG: &str = "ffmpeg";

#[cfg(windows)]
pub const DENO: &str = "deno.exe";
#[cfg(not(windows))]
pub const DENO: &str = "deno";

#[cfg(windows)]
pub const NODE: &str = "node.exe";
#[cfg(not(windows))]
pub const NODE: &str = "node";

#[cfg(all(windows, target_arch = "aarch64"))]
pub const DENO_ZIP_URL: &str =
    "https://github.com/denoland/deno/releases/latest/download/deno-aarch64-pc-windows-msvc.zip";
#[cfg(all(windows, target_arch = "aarch64"))]
pub const DENO_SHA256_URL: &str = "https://github.com/denoland/deno/releases/latest/download/deno-aarch64-pc-windows-msvc.zip.sha256sum";

#[cfg(all(windows, not(target_arch = "aarch64")))]
pub const DENO_ZIP_URL: &str =
    "https://github.com/denoland/deno/releases/latest/download/deno-x86_64-pc-windows-msvc.zip";
#[cfg(all(windows, not(target_arch = "aarch64")))]
pub const DENO_SHA256_URL: &str = "https://github.com/denoland/deno/releases/latest/download/deno-x86_64-pc-windows-msvc.zip.sha256sum";

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub const DENO_ZIP_URL: &str =
    "https://github.com/denoland/deno/releases/latest/download/deno-aarch64-apple-darwin.zip";
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub const DENO_SHA256_URL: &str = "https://github.com/denoland/deno/releases/latest/download/deno-aarch64-apple-darwin.zip.sha256sum";

#[cfg(all(target_os = "macos", not(target_arch = "aarch64")))]
pub const DENO_ZIP_URL: &str =
    "https://github.com/denoland/deno/releases/latest/download/deno-x86_64-apple-darwin.zip";
#[cfg(all(target_os = "macos", not(target_arch = "aarch64")))]
pub const DENO_SHA256_URL: &str = "https://github.com/denoland/deno/releases/latest/download/deno-x86_64-apple-darwin.zip.sha256sum";

#[cfg(all(not(windows), not(target_os = "macos"), target_arch = "aarch64"))]
pub const DENO_ZIP_URL: &str =
    "https://github.com/denoland/deno/releases/latest/download/deno-aarch64-unknown-linux-gnu.zip";
#[cfg(all(not(windows), not(target_os = "macos"), target_arch = "aarch64"))]
pub const DENO_SHA256_URL: &str = "https://github.com/denoland/deno/releases/latest/download/deno-aarch64-unknown-linux-gnu.zip.sha256sum";

#[cfg(all(not(windows), not(target_os = "macos"), not(target_arch = "aarch64")))]
pub const DENO_ZIP_URL: &str =
    "https://github.com/denoland/deno/releases/latest/download/deno-x86_64-unknown-linux-gnu.zip";
#[cfg(all(not(windows), not(target_os = "macos"), not(target_arch = "aarch64")))]
pub const DENO_SHA256_URL: &str = "https://github.com/denoland/deno/releases/latest/download/deno-x86_64-unknown-linux-gnu.zip.sha256sum";

pub const YTDLP_SHA256SUMS_URL: &str =
    "https://github.com/yt-dlp/yt-dlp/releases/latest/download/SHA2-256SUMS";

#[cfg(windows)]
pub const YTDLP_URL: &str = "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp.exe";
#[cfg(windows)]
const YTDLP_ASSET: &str = "yt-dlp.exe";

#[cfg(target_os = "macos")]
pub const YTDLP_URL: &str =
    "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_macos";
#[cfg(target_os = "macos")]
const YTDLP_ASSET: &str = "yt-dlp_macos";

#[cfg(all(not(windows), not(target_os = "macos"), target_arch = "aarch64"))]
pub const YTDLP_URL: &str =
    "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_linux_aarch64";
#[cfg(all(not(windows), not(target_os = "macos"), target_arch = "aarch64"))]
const YTDLP_ASSET: &str = "yt-dlp_linux_aarch64";

#[cfg(all(not(windows), not(target_os = "macos"), not(target_arch = "aarch64")))]
pub const YTDLP_URL: &str =
    "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_linux";
#[cfg(all(not(windows), not(target_os = "macos"), not(target_arch = "aarch64")))]
const YTDLP_ASSET: &str = "yt-dlp_linux";

#[cfg(windows)]
pub const FFMPEG_ZIP_URL: &str = "https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip";
#[cfg(windows)]
const FFMPEG_SHA256_URL: &str =
    "https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip.sha256";
#[cfg(windows)]
const FFMPEG_ASSET: Option<&str> = None;

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub const FFMPEG_ZIP_URL: &str =
    "https://ffmpeg.martin-riedl.de/redirect/latest/macos/arm64/release/ffmpeg.zip";
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
const FFMPEG_SHA256_URL: &str =
    "https://ffmpeg.martin-riedl.de/redirect/latest/macos/arm64/release/ffmpeg.zip.sha256";
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
const FFMPEG_ASSET: Option<&str> = None;

#[cfg(all(target_os = "macos", not(target_arch = "aarch64")))]
pub const FFMPEG_ZIP_URL: &str =
    "https://ffmpeg.martin-riedl.de/redirect/latest/macos/amd64/release/ffmpeg.zip";
#[cfg(all(target_os = "macos", not(target_arch = "aarch64")))]
const FFMPEG_SHA256_URL: &str =
    "https://ffmpeg.martin-riedl.de/redirect/latest/macos/amd64/release/ffmpeg.zip.sha256";
#[cfg(all(target_os = "macos", not(target_arch = "aarch64")))]
const FFMPEG_ASSET: Option<&str> = None;

#[cfg(all(not(windows), not(target_os = "macos"), target_arch = "aarch64"))]
pub const FFMPEG_ZIP_URL: &str = "https://github.com/yt-dlp/FFmpeg-Builds/releases/download/latest/ffmpeg-master-latest-linuxarm64-gpl.tar.xz";
#[cfg(all(not(windows), not(target_os = "macos"), target_arch = "aarch64"))]
const FFMPEG_SHA256_URL: &str =
    "https://github.com/yt-dlp/FFmpeg-Builds/releases/download/latest/checksums.sha256";
#[cfg(all(not(windows), not(target_os = "macos"), target_arch = "aarch64"))]
const FFMPEG_ASSET: Option<&str> = Some("ffmpeg-master-latest-linuxarm64-gpl.tar.xz");

#[cfg(all(not(windows), not(target_os = "macos"), not(target_arch = "aarch64")))]
pub const FFMPEG_ZIP_URL: &str = "https://github.com/yt-dlp/FFmpeg-Builds/releases/download/latest/ffmpeg-master-latest-linux64-gpl.tar.xz";
#[cfg(all(not(windows), not(target_os = "macos"), not(target_arch = "aarch64")))]
const FFMPEG_SHA256_URL: &str =
    "https://github.com/yt-dlp/FFmpeg-Builds/releases/download/latest/checksums.sha256";
#[cfg(all(not(windows), not(target_os = "macos"), not(target_arch = "aarch64")))]
const FFMPEG_ASSET: Option<&str> = Some("ffmpeg-master-latest-linux64-gpl.tar.xz");

#[cfg(windows)]
const TAR_BIN: &str = "tar.exe";
#[cfg(not(windows))]
const TAR_BIN: &str = "tar";

pub fn tools_dir() -> PathBuf {
    // LAJUFILE_TOOLS_DIR keeps the tool install out of the real profile in tests.
    if let Some(dir) = std::env::var_os("LAJUFILE_TOOLS_DIR") {
        return PathBuf::from(dir);
    }
    lajufile_core::paths::data_dir().join("tools")
}

/// Resolves the Chromium extension folder next to the executable, bundle Resources, cwd, or workspace root.
pub fn extension_dir() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(exe) = std::env::current_exe()
        && let Some(parent) = exe.parent()
    {
        candidates.push(parent.join("extension"));
        if let Some(bundle_contents) = parent.parent() {
            candidates.push(bundle_contents.join("Resources").join("extension"));
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd.join("extension"));
    }
    if let Some(root) = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
    {
        candidates.push(root.join("extension"));
    }
    candidates
        .into_iter()
        .find(|dir| dir.join("manifest.json").is_file())
}

pub const EDGE_ADDONS_STORE_URL: &str = "https://microsoftedge.microsoft.com/addons/detail/lajufile-capture/ohmgghfohddiikbgiaocmpckjggccime";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetectedBrowser {
    pub id: &'static str,
    pub name: &'static str,
    pub extensions_url: &'static str,
    pub is_firefox: bool,
    pub executable: Option<PathBuf>,
}

/// Detects common browsers installed on the user's machine so the onboarding dialog
/// prioritizes browsers actually present on this system.
pub fn detect_installed_browsers() -> Vec<DetectedBrowser> {
    struct Spec {
        id: &'static str,
        name: &'static str,
        extensions_url: &'static str,
        is_firefox: bool,
        rel_paths: &'static [&'static str],
    }

    const SPECS: &[Spec] = &[
        Spec {
            id: "chrome",
            name: "Google Chrome",
            extensions_url: "chrome://extensions",
            is_firefox: false,
            #[cfg(windows)]
            rel_paths: &[r"Google\Chrome\Application\chrome.exe"],
            #[cfg(target_os = "macos")]
            rel_paths: &["Google Chrome.app/Contents/MacOS/Google Chrome"],
            #[cfg(all(not(windows), not(target_os = "macos")))]
            rel_paths: &["google-chrome", "google-chrome-stable", "chromium"],
        },
        Spec {
            id: "edge",
            name: "Microsoft Edge",
            extensions_url: EDGE_ADDONS_STORE_URL,
            is_firefox: false,
            #[cfg(windows)]
            rel_paths: &[r"Microsoft\Edge\Application\msedge.exe"],
            #[cfg(target_os = "macos")]
            rel_paths: &["Microsoft Edge.app/Contents/MacOS/Microsoft Edge"],
            #[cfg(all(not(windows), not(target_os = "macos")))]
            rel_paths: &["microsoft-edge", "microsoft-edge-stable"],
        },
        Spec {
            id: "brave",
            name: "Brave",
            extensions_url: "brave://extensions",
            is_firefox: false,
            #[cfg(windows)]
            rel_paths: &[r"BraveSoftware\Brave-Browser\Application\brave.exe"],
            #[cfg(target_os = "macos")]
            rel_paths: &["Brave Browser.app/Contents/MacOS/Brave Browser"],
            #[cfg(all(not(windows), not(target_os = "macos")))]
            rel_paths: &["brave-browser", "brave"],
        },
        Spec {
            id: "opera",
            name: "Opera",
            extensions_url: "opera://extensions",
            is_firefox: false,
            #[cfg(windows)]
            rel_paths: &[r"Programs\Opera\opera.exe", r"Opera\opera.exe"],
            #[cfg(target_os = "macos")]
            rel_paths: &["Opera.app/Contents/MacOS/Opera"],
            #[cfg(all(not(windows), not(target_os = "macos")))]
            rel_paths: &["opera"],
        },
        Spec {
            id: "vivaldi",
            name: "Vivaldi",
            extensions_url: "vivaldi://extensions",
            is_firefox: false,
            #[cfg(windows)]
            rel_paths: &[r"Vivaldi\Application\vivaldi.exe"],
            #[cfg(target_os = "macos")]
            rel_paths: &["Vivaldi.app/Contents/MacOS/Vivaldi"],
            #[cfg(all(not(windows), not(target_os = "macos")))]
            rel_paths: &["vivaldi", "vivaldi-stable"],
        },
        Spec {
            id: "firefox",
            name: "Mozilla Firefox",
            extensions_url: "about:addons",
            is_firefox: true,
            #[cfg(windows)]
            rel_paths: &[r"Mozilla Firefox\firefox.exe"],
            #[cfg(target_os = "macos")]
            rel_paths: &["Firefox.app/Contents/MacOS/firefox"],
            #[cfg(all(not(windows), not(target_os = "macos")))]
            rel_paths: &["firefox"],
        },
    ];

    #[cfg(windows)]
    let roots: Vec<PathBuf> = [
        std::env::var_os("PROGRAMFILES"),
        std::env::var_os("PROGRAMFILES(X86)"),
        std::env::var_os("LOCALAPPDATA"),
    ]
    .into_iter()
    .flatten()
    .map(PathBuf::from)
    .collect();

    #[cfg(target_os = "macos")]
    let roots: Vec<PathBuf> = {
        let mut r = vec![PathBuf::from("/Applications")];
        if let Some(home) = std::env::var_os("HOME") {
            r.push(PathBuf::from(home).join("Applications"));
        }
        r
    };

    #[cfg(all(not(windows), not(target_os = "macos")))]
    let roots: Vec<PathBuf> = {
        let mut r: Vec<PathBuf> = std::env::var_os("PATH")
            .map(|p| std::env::split_paths(&p).collect())
            .unwrap_or_default();
        r.extend([
            PathBuf::from("/usr/bin"),
            PathBuf::from("/usr/local/bin"),
            PathBuf::from("/snap/bin"),
        ]);
        r
    };

    let mut found = Vec::new();
    for spec in SPECS {
        let exe = roots
            .iter()
            .flat_map(|root| spec.rel_paths.iter().map(move |rel| root.join(rel)))
            .find(|candidate| candidate.is_file());
        if exe.is_some() {
            found.push(DetectedBrowser {
                id: spec.id,
                name: spec.name,
                extensions_url: spec.extensions_url,
                is_firefox: spec.is_firefox,
                executable: exe,
            });
        }
    }

    if found.is_empty() {
        for spec in SPECS.iter().take(3) {
            found.push(DetectedBrowser {
                id: spec.id,
                name: spec.name,
                extensions_url: spec.extensions_url,
                is_firefox: spec.is_firefox,
                executable: None,
            });
        }
    }
    found
}

pub fn tool_path(dir: &Path, name: &str) -> PathBuf {
    dir.join(name)
}

pub fn tools_ready(dir: &Path) -> bool {
    tool_path(dir, YTDLP).is_file() && tool_path(dir, FFMPEG).is_file()
}

fn find_executable_in_dirs(name: &str, dirs: &[PathBuf]) -> Option<PathBuf> {
    dirs.iter()
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
}

fn find_on_path_or(name: &str, extra_dirs: &[PathBuf]) -> Option<PathBuf> {
    if let Some(path_var) = std::env::var_os("PATH") {
        let path_dirs: Vec<PathBuf> = std::env::split_paths(&path_var).collect();
        if let Some(found) = find_executable_in_dirs(name, &path_dirs) {
            return Some(found);
        }
    }
    find_executable_in_dirs(name, extra_dirs)
}

fn standard_node_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    #[cfg(windows)]
    {
        for key in ["PROGRAMFILES", "PROGRAMFILES(X86)"] {
            if let Some(base) = std::env::var_os(key) {
                dirs.push(PathBuf::from(base).join("nodejs"));
            }
        }
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            dirs.push(PathBuf::from(local).join("Programs").join("nodejs"));
        }
    }
    #[cfg(not(windows))]
    {
        dirs.extend([
            PathBuf::from("/opt/homebrew/bin"),
            PathBuf::from("/usr/local/bin"),
            PathBuf::from("/usr/bin"),
        ]);
    }
    dirs
}

/// Resolves an available JS runtime (`deno` or `node`) for yt-dlp's YouTube EJS extractor.
pub fn js_runtime_arg(dir: &Path) -> Option<String> {
    let bundled_deno = tool_path(dir, DENO);
    if bundled_deno.is_file() {
        return Some(format!("deno:{}", bundled_deno.display()));
    }
    if let Some(deno) = find_on_path_or(DENO, &[]) {
        return Some(format!("deno:{}", deno.display()));
    }
    if let Some(node) = find_on_path_or(NODE, &standard_node_dirs()) {
        return Some(format!("node:{}", node.display()));
    }
    None
}

/// Downloads yt-dlp, ffmpeg (if missing), and deno (when no JS runtime is installed yet),
/// verifying SHA-256 checksums for every downloaded binary.
pub fn install_tools(dir: &Path) -> Result<(), String> {
    install_tools_with_proxy(dir, None)
}

pub fn install_tools_with_proxy(dir: &Path, proxy: Option<&str>) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|error| format!("create {dir:?}: {error}"))?;
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| format!("tokio runtime: {error}"))?;
    let client = build_tool_client(std::time::Duration::from_secs(300), proxy)?;

    let ytdlp = tool_path(dir, YTDLP);
    let ytdlp_sha = rt.block_on(download_verified(
        &client,
        &ytdlp,
        YTDLP_URL,
        YTDLP_SHA256SUMS_URL,
        Some(YTDLP_ASSET),
        Some("LAJUFILE_YTDLP_SHA256"),
    ))?;
    make_executable(&ytdlp);

    let mut ffmpeg_sha = None;
    if !tool_path(dir, FFMPEG).is_file() {
        let archive = dir.join("ffmpeg-archive");
        let extract = dir.join("ffmpeg-extract");
        let _cleanup = TempCleanup(vec![archive.clone(), extract.clone()]);

        let sha = rt.block_on(download_verified(
            &client,
            &archive,
            FFMPEG_ZIP_URL,
            FFMPEG_SHA256_URL,
            FFMPEG_ASSET,
            Some("LAJUFILE_FFMPEG_SHA256"),
        ))?;
        crate::engine::extract::validate_archive_before_extract(&archive)?;
        ffmpeg_sha = Some(sha);

        let _ = std::fs::remove_dir_all(&extract);
        // tar refuses an extraction directory that does not exist yet.
        std::fs::create_dir_all(&extract)
            .map_err(|error| format!("create {extract:?}: {error}"))?;
        run(
            TAR_BIN,
            &["-xf", path_arg(&archive)?, "-C", path_arg(&extract)?],
        )?;

        let found = find_file(&extract, FFMPEG).ok_or("ffmpeg not found in the archive")?;
        let ffmpeg = tool_path(dir, FFMPEG);
        std::fs::copy(&found, &ffmpeg).map_err(|error| format!("copy ffmpeg: {error}"))?;
        make_executable(&ffmpeg);
    }

    let mut deno_err = None;
    let mut deno_sha = None;
    if js_runtime_arg(dir).is_none() {
        match install_deno(&rt, &client, dir) {
            Ok(sha) => deno_sha = Some(sha),
            Err(err) => deno_err = Some(err),
        }
    }

    let lock = serde_json::json!({
        "yt_dlp_sha256": ytdlp_sha,
        "ffmpeg_sha256": ffmpeg_sha,
        "deno_sha256": deno_sha,
        "updated_at_ms": lajufile_core::store::now_millis(),
    });
    let _ = std::fs::write(
        dir.join("tools-lock.json"),
        serde_json::to_string_pretty(&lock).unwrap_or_default(),
    );

    if !tools_ready(dir) {
        return Err("tools are still missing after install".to_string());
    }
    if let Some(err) = deno_err
        && js_runtime_arg(dir).is_none()
    {
        return Err(format!(
            "yt-dlp and ffmpeg installed, but JS runtime (deno) failed: {err}"
        ));
    }
    Ok(())
}

fn install_deno(
    rt: &tokio::runtime::Runtime,
    client: &reqwest::Client,
    dir: &Path,
) -> Result<String, String> {
    let archive = dir.join("deno-archive.zip");
    let extract = dir.join("deno-extract");
    let _cleanup = TempCleanup(vec![archive.clone(), extract.clone()]);

    let deno_sha = rt.block_on(download_verified(
        client,
        &archive,
        DENO_ZIP_URL,
        DENO_SHA256_URL,
        None,
        Some("LAJUFILE_DENO_SHA256"),
    ))?;
    crate::engine::extract::validate_archive_before_extract(&archive)?;

    let _ = std::fs::remove_dir_all(&extract);
    std::fs::create_dir_all(&extract).map_err(|error| format!("create {extract:?}: {error}"))?;

    #[cfg(any(windows, target_os = "macos"))]
    {
        run(
            TAR_BIN,
            &["-xf", path_arg(&archive)?, "-C", path_arg(&extract)?],
        )?;
    }
    #[cfg(all(not(windows), not(target_os = "macos")))]
    {
        if run(
            "unzip",
            &["-oq", path_arg(&archive)?, "-d", path_arg(&extract)?],
        )
        .is_err()
        {
            run(
                "python3",
                &[
                    "-m",
                    "zipfile",
                    "-e",
                    path_arg(&archive)?,
                    path_arg(&extract)?,
                ],
            )?;
        }
    }

    let found = find_file(&extract, DENO).ok_or("deno not found in the archive")?;
    let deno = tool_path(dir, DENO);
    std::fs::copy(&found, &deno).map_err(|error| format!("copy deno: {error}"))?;
    make_executable(&deno);
    Ok(deno_sha)
}

struct TempCleanup(Vec<PathBuf>);

impl Drop for TempCleanup {
    fn drop(&mut self) {
        for path in &self.0 {
            if path.is_dir() {
                let _ = std::fs::remove_dir_all(path);
            } else {
                let _ = std::fs::remove_file(path);
            }
        }
    }
}

#[cfg(unix)]
fn make_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755));
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) {}

fn path_arg(path: &Path) -> Result<&str, String> {
    path.to_str()
        .ok_or_else(|| format!("path is not valid UTF-8: {path:?}"))
}

pub fn parse_tool_sha256(raw: &str, asset_name: Option<&str>) -> Option<String> {
    if let Some(name) = asset_name
        && let Some(found) = crate::updater::parse_sha256_manifest_entry(raw, name)
    {
        return Some(found);
    }
    let lines: Vec<&str> = raw
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect();
    if lines.len() == 1
        && let Some(token) = lines[0].split_whitespace().next()
        && token.len() == 64
        && token.bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Some(token.to_ascii_lowercase());
    }
    None
}

pub fn verify_file_sha256(path: &Path, expected_hex: &str) -> Result<(), String> {
    let actual = lajufile_core::checksum::sha256_file(path)
        .map_err(|e| format!("checksum read {path:?}: {e}"))?;
    if !actual.eq_ignore_ascii_case(expected_hex.trim()) {
        return Err(format!(
            "SHA-256 mismatch for {}: expected {}, got {}",
            path.display(),
            expected_hex.trim(),
            actual
        ));
    }
    Ok(())
}

fn is_trusted_tool_host(domain: &str) -> bool {
    let host = domain.trim_end_matches('.').to_ascii_lowercase();
    matches!(
        host.as_str(),
        "github.com"
            | "api.github.com"
            | "objects.githubusercontent.com"
            | "release-assets.githubusercontent.com"
            | "codeload.github.com"
            | "gyan.dev"
            | "www.gyan.dev"
            | "ffmpeg.martin-riedl.de"
            | "microsoftedge.microsoft.com"
    ) || host.ends_with(".githubusercontent.com")
}

pub fn validate_tool_https_url(raw: &str) -> Result<reqwest::Url, String> {
    let url = reqwest::Url::parse(raw.trim()).map_err(|e| format!("invalid tool URL: {e}"))?;
    if url.scheme() != "https" {
        return Err(format!("tool URL must use HTTPS: {raw}"));
    }
    let Some(url::Host::Domain(domain)) = url.host() else {
        return Err(format!("tool URL must use a trusted hostname: {raw}"));
    };
    if !is_trusted_tool_host(domain) {
        return Err(format!("untrusted tool host: {domain}"));
    }
    Ok(url)
}

fn build_tool_client(
    total_timeout: std::time::Duration,
    proxy: Option<&str>,
) -> Result<reqwest::Client, String> {
    let mut builder = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(15))
        .read_timeout(std::time::Duration::from_secs(30))
        .timeout(total_timeout)
        .user_agent(concat!("LajuFile/", env!("CARGO_PKG_VERSION")))
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() >= 10 {
                return attempt.stop();
            }
            if let Err(msg) = validate_tool_https_url(attempt.url().as_str()) {
                return attempt.error(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    msg,
                ));
            }
            attempt.follow()
        }));
    if let Some(proxy_url) = proxy.map(str::trim).filter(|s| !s.is_empty()) {
        let parsed = reqwest::Proxy::all(proxy_url).map_err(|e| format!("invalid proxy: {e}"))?;
        builder = builder.proxy(parsed);
    }
    builder
        .build()
        .map_err(|error| format!("HTTP client error: {error}"))
}

async fn fetch_https_text(client: &reqwest::Client, raw_url: &str) -> Result<String, String> {
    let url = validate_tool_https_url(raw_url)?;
    let resp = client
        .get(url)
        .timeout(std::time::Duration::from_secs(20))
        .send()
        .await
        .map_err(|error| format!("checksum fetch failed: {error}"))?
        .error_for_status()
        .map_err(|error| format!("checksum fetch failed: {error}"))?;
    let bytes = resp
        .bytes()
        .await
        .map_err(|error| format!("checksum read failed: {error}"))?;
    if bytes.len() > 512 * 1024 {
        return Err("checksum manifest is unexpectedly large".to_string());
    }
    String::from_utf8(bytes.to_vec())
        .map_err(|_| "checksum manifest is not valid UTF-8".to_string())
}

/// Downloads to `<target>.part`, verifies mandatory SHA-256, and atomically renames on success.
async fn download_verified(
    client: &reqwest::Client,
    target: &Path,
    raw_url: &str,
    checksum_url: &str,
    asset_name: Option<&str>,
    pinned_env: Option<&str>,
) -> Result<String, String> {
    use std::io::Write;

    let expected = if let Some(var) = pinned_env
        && let Ok(val) = std::env::var(var)
        && !val.trim().is_empty()
    {
        let clean = val.trim().to_ascii_lowercase();
        if clean.len() != 64 || !clean.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(format!("{var} must be a 64-character hex SHA-256 digest"));
        }
        clean
    } else {
        let text = fetch_https_text(client, checksum_url).await?;
        parse_tool_sha256(&text, asset_name).ok_or_else(|| {
            format!(
                "SHA-256 digest for {} missing from checksum manifest",
                asset_name.unwrap_or(raw_url)
            )
        })?
    };

    let url = validate_tool_https_url(raw_url)?;
    let mut part = target.as_os_str().to_owned();
    part.push(".part");
    let part = PathBuf::from(part);
    let _cleanup = TempCleanup(vec![part.clone()]);

    let mut resp = client
        .get(url)
        .send()
        .await
        .map_err(|error| format!("download {raw_url}: {error}"))?
        .error_for_status()
        .map_err(|error| format!("download {raw_url}: {error}"))?;

    let file = std::fs::File::create(&part).map_err(|error| format!("create {part:?}: {error}"))?;
    let mut writer = std::io::BufWriter::new(file);
    while let Some(chunk) = resp
        .chunk()
        .await
        .map_err(|error| format!("stream {raw_url}: {error}"))?
    {
        writer
            .write_all(&chunk)
            .map_err(|error| format!("write {part:?}: {error}"))?;
    }
    writer
        .flush()
        .map_err(|error| format!("flush {part:?}: {error}"))?;
    drop(writer);

    verify_file_sha256(&part, &expected)?;
    std::fs::rename(&part, target).map_err(|error| format!("rename {target:?}: {error}"))?;
    Ok(expected)
}

/// Keeps a console window from flashing: the app has no console in release.
pub fn no_window(_command: &mut std::process::Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        _command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
}

/// Kills a process and its children: yt-dlp spawns ffmpeg, which `kill()` leaves running.
pub fn kill_tree(pid: u32) {
    #[cfg(windows)]
    {
        let mut command = std::process::Command::new("taskkill");
        command
            .args(["/T", "/F", "/PID", &pid.to_string()])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        no_window(&mut command);
        let _ = command.status();
    }
    #[cfg(not(windows))]
    {
        let pid_str = pid.to_string();
        let _ = std::process::Command::new("pkill")
            .args(["-KILL", "-P", &pid_str])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
        let _ = std::process::Command::new("kill")
            .args(["-KILL", &pid_str])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
}

fn run(program: &str, args: &[&str]) -> Result<(), String> {
    let mut command = std::process::Command::new(program);
    command.args(args);
    no_window(&mut command);
    let output = command
        .output()
        .map_err(|error| format!("{program}: {error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "{program} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

fn find_file(dir: &Path, name: &str) -> Option<PathBuf> {
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(found) = find_file(&path, name) {
                return Some(found);
            }
        } else if path
            .file_name()
            .map(|file| file.to_string_lossy().eq_ignore_ascii_case(name))
            .unwrap_or(false)
        {
            return Some(path);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tools_ready_requires_both_files() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!tools_ready(dir.path()));

        std::fs::write(tool_path(dir.path(), YTDLP), b"x").unwrap();
        assert!(!tools_ready(dir.path()));

        std::fs::write(tool_path(dir.path(), FFMPEG), b"x").unwrap();
        assert!(tools_ready(dir.path()));
    }

    #[test]
    fn tool_path_joins_the_name() {
        let dir = Path::new("/tmp/tools");
        assert_eq!(tool_path(dir, YTDLP), dir.join(YTDLP));
    }

    #[test]
    fn find_file_searches_nested_directories() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("ffmpeg/bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join(FFMPEG), b"x").unwrap();

        assert_eq!(find_file(dir.path(), FFMPEG), Some(bin.join(FFMPEG)));
        assert_eq!(find_file(dir.path(), "missing.bin"), None);
    }

    #[test]
    fn extension_dir_finds_workspace_extension_folder() {
        let dir = extension_dir().expect("workspace extension directory");
        assert!(dir.join("manifest.json").is_file());
    }

    #[test]
    fn js_runtime_arg_prefers_bundled_deno() {
        let dir = tempfile::tempdir().unwrap();
        let bundled = tool_path(dir.path(), DENO);
        std::fs::write(&bundled, b"deno").unwrap();
        let runtime = js_runtime_arg(dir.path()).expect("bundled deno detected");
        assert_eq!(runtime, format!("deno:{}", bundled.display()));
    }

    #[test]
    fn parses_multi_file_and_single_hash_manifests_and_verifies_files() {
        let multi = "\
# Checksums
bad-line-no-sep
0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef  yt-dlp.exe
fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210 *deno-x86_64-pc-windows-msvc.zip
";
        assert_eq!(
            parse_tool_sha256(multi, Some("yt-dlp.exe")).as_deref(),
            Some("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef")
        );
        assert_eq!(
            parse_tool_sha256(
                "AABBCCDDEEFF00112233445566778899AABBCCDDEEFF00112233445566778899\n",
                None
            )
            .as_deref(),
            Some("aabbccddeeff00112233445566778899aabbccddeeff00112233445566778899")
        );
        assert_eq!(parse_tool_sha256("not-a-hash", None), None);

        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("bin.part");
        std::fs::write(&file, b"abc").unwrap();
        assert!(
            verify_file_sha256(
                &file,
                "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
            )
            .is_ok()
        );
        assert!(
            verify_file_sha256(
                &file,
                "0000000000000000000000000000000000000000000000000000000000000000"
            )
            .is_err()
        );
    }

    #[test]
    fn tool_urls_and_checksums_require_https() {
        assert!(validate_tool_https_url(YTDLP_URL).is_ok());
        assert!(validate_tool_https_url(YTDLP_SHA256SUMS_URL).is_ok());
        assert!(validate_tool_https_url(FFMPEG_ZIP_URL).is_ok());
        assert!(validate_tool_https_url(FFMPEG_SHA256_URL).is_ok());
        assert!(validate_tool_https_url(DENO_ZIP_URL).is_ok());
        assert!(validate_tool_https_url(DENO_SHA256_URL).is_ok());
        assert!(validate_tool_https_url(EDGE_ADDONS_STORE_URL).is_ok());
        assert!(validate_tool_https_url("http://example.com/ffmpeg.zip").is_err());
        assert!(validate_tool_https_url("https://evil.example/yt-dlp.exe").is_err());
        assert!(validate_tool_https_url("https://127.0.0.1/yt-dlp.exe").is_err());
        assert!(validate_tool_https_url("file:///tmp/ffmpeg.zip").is_err());
        assert!(validate_tool_https_url("https://").is_err());
    }
}
