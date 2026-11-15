//! Managed yt-dlp and ffmpeg binaries for video downloads.

use std::path::{Path, PathBuf};

pub const YTDLP: &str = "yt-dlp.exe";
pub const FFMPEG: &str = "ffmpeg.exe";
pub const YTDLP_URL: &str = "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp.exe";
pub const FFMPEG_ZIP_URL: &str = "https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip";

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

/// Downloads both tools with the Windows built-in curl and tar.
pub fn install_tools(dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|error| format!("create {dir:?}: {error}"))?;

    let ytdlp = tool_path(dir, YTDLP);
    run(
        "curl.exe",
        &["-L", "--fail", "-o", path_arg(&ytdlp)?, YTDLP_URL],
    )?;

    let zip = dir.join("ffmpeg.zip");
    run(
        "curl.exe",
        &["-L", "--fail", "-o", path_arg(&zip)?, FFMPEG_ZIP_URL],
    )?;

    let extract = dir.join("ffmpeg-extract");
    let _ = std::fs::remove_dir_all(&extract);
    run(
        "tar.exe",
        &["-xf", path_arg(&zip)?, "-C", path_arg(&extract)?],
    )?;

    let found = find_file(&extract, FFMPEG).ok_or("ffmpeg.exe not found in the archive")?;
    std::fs::copy(&found, tool_path(dir, FFMPEG))
        .map_err(|error| format!("copy ffmpeg: {error}"))?;

    let _ = std::fs::remove_file(&zip);
    let _ = std::fs::remove_dir_all(&extract);

    if tools_ready(dir) {
        Ok(())
    } else {
        Err("tools are still missing after install".to_string())
    }
}

fn path_arg(path: &Path) -> Result<&str, String> {
    path.to_str()
        .ok_or_else(|| format!("path is not valid UTF-8: {path:?}"))
}

fn run(program: &str, args: &[&str]) -> Result<(), String> {
    let output = std::process::Command::new(program)
        .args(args)
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
        let dir = Path::new("C:/tools");
        assert_eq!(tool_path(dir, YTDLP), PathBuf::from("C:/tools/yt-dlp.exe"));
    }

    #[test]
    fn find_file_searches_nested_directories() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("ffmpeg/bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join("ffmpeg.exe"), b"x").unwrap();

        assert_eq!(find_file(dir.path(), FFMPEG), Some(bin.join("ffmpeg.exe")));
        assert_eq!(find_file(dir.path(), "missing.exe"), None);
    }
}
