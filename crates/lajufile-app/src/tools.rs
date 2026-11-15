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
#[cfg(not(windows))]
pub const YTDLP_URL: &str =
    "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_linux";

#[cfg(windows)]
pub const FFMPEG_ZIP_URL: &str = "https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip";
#[cfg(not(windows))]
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
    run(CURL_BIN, &["-L", "--fail", "-o", path_arg(&part)?, url])?;
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
}
