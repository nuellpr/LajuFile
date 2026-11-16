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
pub const YTDLP_URL: &str = "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp.exe";
#[cfg(target_os = "macos")]
pub const YTDLP_URL: &str =
    "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_macos";
#[cfg(all(not(windows), not(target_os = "macos"), target_arch = "aarch64"))]
pub const YTDLP_URL: &str =
    "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_linux_aarch64";
#[cfg(all(not(windows), not(target_os = "macos"), not(target_arch = "aarch64")))]
pub const YTDLP_URL: &str =
    "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_linux";

#[cfg(windows)]
pub const FFMPEG_ZIP_URL: &str = "https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip";
#[cfg(target_os = "macos")]
pub const FFMPEG_ZIP_URL: &str = "https://evermeet.cx/ffmpeg/getrelease/zip";
#[cfg(all(not(windows), not(target_os = "macos"), target_arch = "aarch64"))]
pub const FFMPEG_ZIP_URL: &str =
    "https://johnvansickle.com/ffmpeg/releases/ffmpeg-release-arm64-static.tar.xz";
#[cfg(all(not(windows), not(target_os = "macos"), not(target_arch = "aarch64")))]
pub const FFMPEG_ZIP_URL: &str =
    "https://johnvansickle.com/ffmpeg/releases/ffmpeg-release-amd64-static.tar.xz";

#[cfg(windows)]
const CURL_BIN: &str = "curl.exe";
#[cfg(not(windows))]
const CURL_BIN: &str = "curl";

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
            extensions_url: "edge://extensions",
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

/// Downloads both tools with curl and tar.
pub fn install_tools(dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|error| format!("create {dir:?}: {error}"))?;

    let ytdlp = tool_path(dir, YTDLP);
    download(&ytdlp, YTDLP_URL)?;
    make_executable(&ytdlp);

    let archive = dir.join("ffmpeg-archive");
    download(&archive, FFMPEG_ZIP_URL)?;

    let extract = dir.join("ffmpeg-extract");
    let _ = std::fs::remove_dir_all(&extract);
    // tar refuses an extraction directory that does not exist yet.
    std::fs::create_dir_all(&extract).map_err(|error| format!("create {extract:?}: {error}"))?;
    run(
        TAR_BIN,
        &["-xf", path_arg(&archive)?, "-C", path_arg(&extract)?],
    )?;

    let found = find_file(&extract, FFMPEG).ok_or("ffmpeg not found in the archive")?;
    let ffmpeg = tool_path(dir, FFMPEG);
    std::fs::copy(&found, &ffmpeg).map_err(|error| format!("copy ffmpeg: {error}"))?;
    make_executable(&ffmpeg);

    let _ = std::fs::remove_file(&archive);
    let _ = std::fs::remove_dir_all(&extract);

    if tools_ready(dir) {
        Ok(())
    } else {
        Err("tools are still missing after install".to_string())
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

/// Downloads to `<target>.part` and renames on success, so a truncated file never
/// passes `tools_ready`.
fn download(target: &Path, url: &str) -> Result<(), String> {
    let mut part = target.as_os_str().to_owned();
    part.push(".part");
    let part = PathBuf::from(part);
    run(
        CURL_BIN,
        &[
            "-L",
            "--fail",
            "--proto",
            "=https",
            "-o",
            path_arg(&part)?,
            "--",
            url,
        ],
    )?;
    std::fs::rename(&part, target).map_err(|error| format!("rename {target:?}: {error}"))
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
}
