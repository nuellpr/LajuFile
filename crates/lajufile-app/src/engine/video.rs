use std::ffi::OsString;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use lajufile_core::naming::sanitize_file_name;
use lajufile_core::paths::ensure_dir;
use lajufile_core::video::{self, VideoListing, VideoMode, parse_video_spec};
use url::Url;

use crate::tools;

/// A running `yt-dlp` child; the engine polls it for exit instead of awaiting it,
/// so pause and cancel can kill the process.
pub(super) struct VideoRun {
    pub(super) child: std::process::Child,
    pub(super) save_path: PathBuf,
    pub(super) mode: VideoMode,
    pub(super) started_at: std::time::SystemTime,
    pub(super) last_error: Arc<Mutex<String>>,
    pub(super) stdout_join: Option<JoinHandle<()>>,
    pub(super) stderr_join: Option<JoinHandle<()>>,
    // Dropped automatically when the yt-dlp process exits, stops, or is aborted.
    pub(super) _cookie_file: Option<TempCookieFile>,
}

#[derive(Default)]
pub(super) struct VideoStreamTracker {
    seeded_size: Option<u64>,
    min_done: u64,
    current_file: Option<String>,
    completed_bytes: u64,
    current_peak: u64,
    current_total: u64,
}

impl VideoStreamTracker {
    pub(super) fn from_seeded(seeded_size: Option<u64>, previous_done: u64) -> Self {
        Self {
            seeded_size: seeded_size.filter(|s| *s > 0),
            min_done: previous_done,
            ..Self::default()
        }
    }

    pub(super) fn record(
        &mut self,
        done: u64,
        total: Option<u64>,
        filename: Option<&str>,
    ) -> (u64, Option<u64>) {
        if let Some(fname) = filename {
            if self.current_file.as_deref() != Some(fname) {
                if self.current_file.is_some() {
                    let finished = self.current_total.max(self.current_peak);
                    self.completed_bytes = self.completed_bytes.saturating_add(finished);
                    self.current_peak = 0;
                    self.current_total = 0;
                }
                self.current_file = Some(fname.to_string());
            }
        } else if self.current_peak > 0 && done < self.current_peak / 2 {
            // Filename omitted by yt-dlp; treat a sharp drop back toward zero as a new stream.
            let finished = self.current_total.max(self.current_peak);
            self.completed_bytes = self.completed_bytes.saturating_add(finished);
            self.current_peak = 0;
            self.current_total = 0;
        }
        self.current_peak = self.current_peak.max(done);
        if let Some(t) = total
            && t > 0
        {
            self.current_total = t;
        }
        let cumulative_done = self.completed_bytes.saturating_add(done).max(self.min_done);
        let observed_total = self
            .completed_bytes
            .saturating_add(self.current_total.max(done));
        let expected_size = if self.current_total > 0 {
            Some(
                self.seeded_size
                    .map_or(observed_total, |seeded| seeded.max(observed_total))
                    .max(cumulative_done),
            )
        } else {
            self.seeded_size
        };
        (cumulative_done, expected_size)
    }
}

pub(super) fn video_file_name(url: &str) -> String {
    let id = Url::parse(url)
        .ok()
        .and_then(|url| {
            url.query_pairs()
                .find(|(key, _)| key == "v")
                .map(|(_, value)| value.into_owned())
                .or_else(|| {
                    url.path_segments()
                        .and_then(|mut segments| segments.next_back().map(str::to_string))
                })
        })
        .unwrap_or_default();
    let safe: String = id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_') {
                c
            } else {
                '-'
            }
        })
        .collect();
    let safe = safe.trim_matches('-');
    if safe.is_empty() {
        "video.mp4".to_string()
    } else {
        format!("{safe}.mp4")
    }
}

fn strip_media_ext(clean: &str) -> &str {
    let lower = clean.to_ascii_lowercase();
    for ext in [
        ".mp4", ".mkv", ".webm", ".m4a", ".mp3", ".opus", ".aac", ".flac", ".ogg", ".wav", ".srt",
        ".vtt",
    ] {
        if lower.ends_with(ext) {
            return &clean[..clean.len() - ext.len()];
        }
    }
    clean
}

pub(super) fn resolve_video_file_name(
    requested: Option<&str>,
    url: &str,
    selector: Option<&str>,
) -> String {
    let mode = parse_video_spec(selector.unwrap_or(video::DEFAULT_SELECTOR)).mode;
    let raw = requested
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| video_file_name(url));
    let clean = sanitize_file_name(&raw);
    let lower = clean.to_ascii_lowercase();
    match mode {
        VideoMode::AudioMp3 => {
            if lower.ends_with(".mp3") {
                return clean;
            }
            format!("{}.mp3", strip_media_ext(&clean))
        }
        VideoMode::AudioM4a => {
            if [".m4a", ".mp3", ".opus", ".aac", ".flac", ".ogg", ".wav"]
                .iter()
                .any(|ext| lower.ends_with(ext))
            {
                return clean;
            }
            format!("{}.m4a", strip_media_ext(&clean))
        }
        VideoMode::SubtitlesOnly => {
            if lower.ends_with(".srt") || lower.ends_with(".vtt") {
                return clean;
            }
            format!("{}.srt", strip_media_ext(&clean))
        }
        VideoMode::Video => {
            if lower.ends_with(".mp4") || lower.ends_with(".mkv") || lower.ends_with(".webm") {
                clean
            } else {
                format!("{clean}.mp4")
            }
        }
    }
}

/// Short-lived Netscape cookie file for `yt-dlp --cookies`, deleted automatically on drop.
pub(super) struct TempCookieFile {
    path: PathBuf,
}

impl TempCookieFile {
    pub(super) fn from_netscape_in(raw: &str, base_dir: &Path) -> Option<Self> {
        let trimmed = raw.trim();
        if !trimmed.starts_with("# Netscape HTTP Cookie File") {
            return None;
        }
        let has_entries = trimmed
            .lines()
            .any(|line| !line.is_empty() && !line.starts_with('#') && line.contains('\t'));
        if !has_entries {
            return None;
        }

        static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let pid = std::process::id();
        let dir = base_dir.join("cookies-tmp");
        lajufile_core::paths::ensure_private_dir(&dir).ok()?;
        for _ in 0..8 {
            let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let path = dir.join(format!("lajufile-yt-{pid}-{seq}.cookies"));
            let mut opts = std::fs::OpenOptions::new();
            opts.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                opts.mode(0o600);
            }
            match opts.open(&path) {
                Ok(mut file) => {
                    use std::io::Write;
                    if file.write_all(trimmed.as_bytes()).is_err() || file.write_all(b"\n").is_err()
                    {
                        drop(file);
                        let _ = std::fs::remove_file(&path);
                        return None;
                    }
                    return Some(Self { path });
                }
                Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(_) => return None,
            }
        }
        None
    }

    pub(super) fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempCookieFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Removes leftover `lajufile-yt-*.cookies` files in `dir` from crashed/killed previous runs.
pub(super) fn cleanup_stale_cookie_files(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let own_prefix = format!("lajufile-yt-{}-", std::process::id());
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with("lajufile-yt-")
            && name.ends_with(".cookies")
            && !name.starts_with(&own_prefix)
        {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

pub(super) fn format_ytdlp_error(stderr: &str) -> String {
    let trimmed = stderr.trim();
    if trimmed.contains("Sign in to confirm you're not a bot") {
        return "yt-dlp: YouTube requires a signed-in browser session on this network. Log in to YouTube in your browser and retry from the LajuFile extension (paired with this app).".to_string();
    }
    if let Some(err_line) = trimmed
        .lines()
        .rev()
        .find(|line| line.trim_start().starts_with("ERROR:"))
    {
        return format!("yt-dlp: {}", err_line.trim());
    }
    if trimmed.is_empty() {
        "yt-dlp failed".to_string()
    } else {
        format!("yt-dlp: {trimmed}")
    }
}

/// Resolves a writable working directory for `yt-dlp` child processes so they never
/// inherit a protected OS directory (such as `C:\Windows\System32`).
pub(super) fn ytdlp_work_dir(preferred: Option<&Path>) -> PathBuf {
    if let Some(dir) = preferred.filter(|p| !p.as_os_str().is_empty() && p.is_absolute())
        && ensure_dir(dir).is_ok()
        && dir.is_dir()
    {
        return dir.to_path_buf();
    }
    let tmp = std::env::temp_dir();
    let _ = std::fs::create_dir_all(&tmp);
    tmp
}

/// Validates an HTTP(S) referrer URL before forwarding it to `yt-dlp --referer`.
pub(super) fn valid_http_referrer(raw: Option<&str>) -> Option<String> {
    let trimmed = raw?.trim();
    if trimmed.is_empty() || trimmed.starts_with('-') {
        return None;
    }
    let parsed = Url::parse(trimmed).ok()?;
    (matches!(parsed.scheme(), "http" | "https") && parsed.host_str().is_some())
        .then(|| parsed.to_string())
}

/// `yt-dlp -J`, killed if it stalls: a hung run must not outlive the request.
pub(super) fn ytdlp_listing(
    dir: &Path,
    data_dir: &Path,
    url: &str,
    cookie: Option<&str>,
    referrer: Option<&str>,
    proxy: Option<&str>,
) -> Result<VideoListing, String> {
    let cookie_file = cookie.and_then(|c| TempCookieFile::from_netscape_in(c, data_dir));
    let js_runtime = tools::js_runtime_arg(dir);
    let mut command = std::process::Command::new(tools::tool_path(dir, tools::YTDLP));
    command
        .current_dir(ytdlp_work_dir(None))
        .args(["--no-playlist", "-J"]);
    if let Some(proxy_url) = lajufile_core::http::validate_proxy_url(proxy.unwrap_or(""))? {
        command.args(["--proxy", &proxy_url]);
    }
    if let Some(runtime) = js_runtime.as_deref() {
        command.args(["--js-runtimes", runtime]);
    }
    if let Some(cookies) = cookie_file.as_ref() {
        command.arg("--cookies").arg(cookies.path());
    }
    if let Some(ref_url) = valid_http_referrer(referrer) {
        command.args(["--referer", &ref_url]);
    }
    command
        .args(["--", url])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    tools::no_window(&mut command);
    let mut child = command
        .spawn()
        .map_err(|error| format!("yt-dlp: {error}"))?;
    let stdout = read_all(child.stdout.take());
    let stderr = read_all(child.stderr.take());
    let deadline = Instant::now() + Duration::from_secs(30);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(50)),
            Ok(None) => {
                tools::kill_tree(child.id());
                let _ = child.wait();
                return Err("yt-dlp did not answer in time".to_string());
            }
            Err(error) => return Err(format!("yt-dlp: {error}")),
        }
    };
    let output = stdout.join().unwrap_or_default();
    if !status.success() {
        let error = String::from_utf8_lossy(&stderr.join().unwrap_or_default()).to_string();
        return Err(format_ytdlp_error(&error));
    }
    let json: serde_json::Value =
        serde_json::from_slice(&output).map_err(|error| format!("yt-dlp: {error}"))?;
    let listing = video::listing(&json);
    if listing.choices.is_empty() {
        return Err("this link has no downloadable formats".to_string());
    }
    Ok(listing)
}

/// Expands a supported video playlist URL via `yt-dlp --flat-playlist -J` (up to 200 items).
pub(super) fn ytdlp_playlist_urls(
    dir: &Path,
    url: &str,
    proxy: Option<&str>,
) -> Result<Vec<String>, String> {
    let js_runtime = tools::js_runtime_arg(dir);
    let mut command = std::process::Command::new(tools::tool_path(dir, tools::YTDLP));
    command.current_dir(ytdlp_work_dir(None)).args([
        "--flat-playlist",
        "--playlist-end",
        "200",
        "-J",
    ]);
    if let Some(proxy_url) = lajufile_core::http::validate_proxy_url(proxy.unwrap_or(""))? {
        command.args(["--proxy", &proxy_url]);
    }
    if let Some(runtime) = js_runtime.as_deref() {
        command.args(["--js-runtimes", runtime]);
    }
    command
        .args(["--", url])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    tools::no_window(&mut command);
    let mut child = command
        .spawn()
        .map_err(|error| format!("yt-dlp: {error}"))?;
    let stdout = read_all(child.stdout.take());
    let stderr = read_all(child.stderr.take());
    let deadline = Instant::now() + Duration::from_secs(45);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(50)),
            Ok(None) => {
                tools::kill_tree(child.id());
                let _ = child.wait();
                return Err("yt-dlp did not answer in time".to_string());
            }
            Err(error) => return Err(format!("yt-dlp: {error}")),
        }
    };
    let output = stdout.join().unwrap_or_default();
    if !status.success() {
        let error = String::from_utf8_lossy(&stderr.join().unwrap_or_default()).to_string();
        return Err(format_ytdlp_error(&error));
    }
    let json: serde_json::Value =
        serde_json::from_slice(&output).map_err(|error| format!("yt-dlp: {error}"))?;
    let urls = video::parse_playlist_urls(&json);
    if urls.is_empty() {
        return Err("no downloadable videos found in playlist".to_string());
    }
    Ok(urls)
}

fn read_all<R: Read + Send + 'static>(reader: Option<R>) -> thread::JoinHandle<Vec<u8>> {
    thread::spawn(move || {
        let mut buffer = Vec::new();
        if let Some(mut reader) = reader {
            let _ = reader.read_to_end(&mut buffer);
        }
        buffer
    })
}

fn ytdlp_output_template(path: &Path) -> OsString {
    match path.to_str() {
        Some(text) => OsString::from(text.replace('%', "%%")),
        None => path.as_os_str().to_owned(),
    }
}

/// yt-dlp arguments for one video job: merge to mp4, extract audio, or download subtitles.
#[allow(clippy::too_many_arguments)]
pub(super) fn video_args(
    selector: &str,
    save_path: &Path,
    url: &str,
    tools_dir: &Path,
    subtitles: bool,
    embed_chapters: bool,
    js_runtime: Option<&str>,
    cookie_file: Option<&Path>,
    referrer: Option<&str>,
    proxy: Option<&str>,
    limit_kbps: u64,
) -> Vec<OsString> {
    let spec = parse_video_spec(selector);
    let mut args: Vec<OsString> = vec!["--no-playlist".into(), "--newline".into()];
    match spec.mode {
        VideoMode::AudioMp3 => {
            args.extend(["-x".into(), "--audio-format".into(), "mp3".into()]);
        }
        VideoMode::AudioM4a => {
            args.extend(["-x".into(), "--audio-format".into(), "m4a".into()]);
        }
        VideoMode::SubtitlesOnly => {
            args.extend([
                "--skip-download".into(),
                "--write-subs".into(),
                "--write-auto-subs".into(),
                "--sub-format".into(),
                "srt/best".into(),
                "--convert-subs".into(),
                "srt".into(),
            ]);
        }
        VideoMode::Video => {
            args.extend([
                "--merge-output-format".into(),
                "mp4".into(),
                // Prefer native H.264 MP4 + AAC M4A so ffmpeg stream-copies losslessly without re-encoding.
                "-S".into(),
                "ext:mp4:m4a,vcodec:h264,acodec:aac".into(),
            ]);
        }
    }
    args.extend(["--ffmpeg-location".into(), tools_dir.as_os_str().to_owned()]);
    if let Ok(Some(proxy_url)) = lajufile_core::http::validate_proxy_url(proxy.unwrap_or("")) {
        args.extend(["--proxy".into(), proxy_url.into()]);
    }
    if limit_kbps > 0 {
        args.extend(["--limit-rate".into(), format!("{limit_kbps}K").into()]);
    }
    if let Some(runtime) = js_runtime {
        args.push("--js-runtimes".into());
        args.push(runtime.into());
    }
    if let Some(cookies) = cookie_file {
        args.push("--cookies".into());
        args.push(cookies.as_os_str().to_owned());
    }
    if let Some(ref_url) = valid_http_referrer(referrer) {
        args.push("--referer".into());
        args.push(ref_url.into());
    }
    if subtitles && spec.mode == VideoMode::Video {
        args.extend([
            "--write-subs".into(),
            "--write-auto-subs".into(),
            "--sub-format".into(),
            "srt/best".into(),
            "--embed-subs".into(),
        ]);
    }
    if embed_chapters && spec.mode != VideoMode::SubtitlesOnly {
        args.push("--embed-chapters".into());
    }
    if let Some(range) = spec.clip.as_deref()
        && spec.mode != VideoMode::SubtitlesOnly
    {
        args.extend([
            "--download-sections".into(),
            format!("*{range}").into(),
            "--force-keyframes-at-cuts".into(),
        ]);
    }
    args.extend([
        // Balance DASH/HLS fragment concurrency against YouTube throttling, keep
        // 10M HTTP chunks for single-stream throttling, and throttle stdout progress lines.
        "-N".into(),
        "4".into(),
        "--http-chunk-size".into(),
        "10M".into(),
        "--buffer-size".into(),
        "64K".into(),
        "--progress-delta".into(),
        "0.25".into(),
        // Machine-readable progress, one line per update:
        // "LF <downloaded> <total> <estimate> <filename>", either total may be NA.
        "--progress-template".into(),
        "download:LF %(progress.downloaded_bytes)s %(progress.total_bytes)s %(progress.total_bytes_estimate)s %(progress.filename)s".into(),
    ]);
    if spec.mode != VideoMode::SubtitlesOnly {
        args.extend(["-f".into(), spec.selector.into()]);
    }
    let output_path = if spec.mode == VideoMode::SubtitlesOnly {
        save_path.with_extension("")
    } else {
        save_path.to_path_buf()
    };
    args.extend([
        "-o".into(),
        ytdlp_output_template(&output_path),
        "--".into(),
        url.into(),
    ]);
    args
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct ParsedVideoProgress {
    pub(super) done: u64,
    pub(super) total: Option<u64>,
    pub(super) filename: Option<String>,
}

pub(super) fn is_auxiliary_media_file(filename: Option<&str>) -> bool {
    let Some(name) = filename else {
        return false;
    };
    let lower = name.trim().to_ascii_lowercase();
    let base = lower.strip_suffix(".part").unwrap_or(&lower);
    [
        ".vtt",
        ".srt",
        ".ass",
        ".ssa",
        ".ttml",
        ".lrc",
        ".srv1",
        ".srv2",
        ".srv3",
        ".json3",
        ".description",
        ".info.json",
        ".webp",
        ".jpg",
        ".jpeg",
        ".png",
        ".ytdl",
    ]
    .iter()
    .any(|ext| base.ends_with(ext))
}

pub(super) fn parse_progress(line: &str) -> Option<ParsedVideoProgress> {
    let rest = line.strip_prefix("LF ")?;
    let mut parts = rest.splitn(4, ' ');
    let done = parts.next()?.parse().ok()?;
    let total_opt = parts.next().and_then(|text| text.parse().ok());
    let estimate_opt = parts.next().and_then(|text| text.parse().ok());
    let filename = parts
        .next()
        .map(str::trim)
        .filter(|s| !s.is_empty() && *s != "NA")
        .map(str::to_string);
    Some(ParsedVideoProgress {
        done,
        total: total_opt.or(estimate_opt),
        filename,
    })
}

pub(super) fn stop_video(video: &mut Option<VideoRun>) {
    if let Some(mut run) = video.take() {
        // yt-dlp's ffmpeg child survives a plain kill and would finish the merge.
        tools::kill_tree(run.child.id());
        let _ = run.child.wait();
        if let Some(join) = run.stdout_join.take() {
            let _ = join.join();
        }
        if let Some(join) = run.stderr_join.take() {
            let _ = join.join();
        }
    }
}

pub(super) fn resolve_video_output(
    expected: &Path,
    mode: VideoMode,
    started_at: std::time::SystemTime,
) -> Option<PathBuf> {
    if expected.is_file() {
        return Some(expected.to_path_buf());
    }
    let dir = expected.parent()?;
    let stem = expected.file_stem()?.to_str()?;
    let cutoff = started_at.checked_sub(Duration::from_secs(2));
    let is_fresh = |path: &Path| {
        cutoff.is_none_or(|min_time| {
            std::fs::metadata(path)
                .and_then(|m| m.modified())
                .is_ok_and(|mtime| mtime >= min_time)
        })
    };

    if mode == VideoMode::SubtitlesOnly {
        let prefix = format!("{stem}.");
        let entries = std::fs::read_dir(dir).ok()?;
        let mut fallback_vtt = None;
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if !name.starts_with(&prefix) || !is_fresh(&path) {
                continue;
            }
            let lower = name.to_ascii_lowercase();
            if lower.ends_with(".srt") {
                return Some(path);
            }
            if lower.ends_with(".vtt") && fallback_vtt.is_none() {
                fallback_vtt = Some(path);
            }
        }
        return fallback_vtt;
    }

    let exts: &[&str] = match mode {
        VideoMode::AudioMp3 => &["mp3", "m4a", "opus"],
        VideoMode::AudioM4a => &["m4a", "mp3", "opus", "aac", "flac", "ogg", "wav"],
        VideoMode::Video => &["mp4", "mkv", "webm"],
        VideoMode::SubtitlesOnly => unreachable!(),
    };
    for ext in exts {
        let candidate = dir.join(format!("{stem}.{ext}"));
        if candidate.is_file() && is_fresh(&candidate) {
            return Some(candidate);
        }
    }
    None
}

fn is_ytdlp_format_suffix(rest: &str) -> bool {
    let Some(after_dot) = rest.strip_prefix(".f") else {
        return false;
    };
    let digits = after_dot.bytes().take_while(|b| b.is_ascii_digit()).count();
    digits > 0 && after_dot.as_bytes().get(digits) == Some(&b'.')
}

fn is_video_part_file(save_name: &str, stem: &str, file_name: &str) -> bool {
    if file_name == save_name {
        return false;
    }
    let matches_name = file_name
        .strip_prefix(save_name)
        .is_some_and(|rest| rest.starts_with('.'));
    let matches_stem_ytdlp = file_name
        .strip_prefix(stem)
        .is_some_and(|rest| rest.starts_with(".temp.") || is_ytdlp_format_suffix(rest));
    let temp = file_name.ends_with(".part")
        || file_name.contains(".part-Frag")
        || file_name.ends_with(".ytdl")
        || file_name.ends_with(".temp.mp4");
    (matches_name || matches_stem_ytdlp) && temp
}

pub(super) fn has_parts(save_path: &Path) -> bool {
    let (Some(dir), Some(name)) = (
        save_path.parent(),
        save_path.file_name().and_then(|name| name.to_str()),
    ) else {
        return false;
    };
    let stem = save_path
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| name.to_string());
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    entries
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .any(|file_name| is_video_part_file(name, &stem, &file_name))
}

/// Drops the temp files yt-dlp leaves behind: `name.part`, `name.part-Frag<N>`,
/// `name.ytdl`, `name.f137.mp4.part-Frag<N>` and `name.temp.mp4`.
pub(super) fn remove_parts(save_path: &Path) {
    let (Some(dir), Some(name)) = (
        save_path.parent(),
        save_path.file_name().and_then(|name| name.to_str()),
    ) else {
        return;
    };
    let stem = save_path
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| name.to_string());
    let sentinel = format!("{name}.lajufile");
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let file_name = entry.file_name().to_string_lossy().into_owned();
        if file_name == sentinel || is_video_part_file(name, &stem, &file_name) {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_lines_carry_downloaded_bytes_and_total() {
        assert_eq!(
            parse_progress("LF 1234 5678 5678"),
            Some(ParsedVideoProgress {
                done: 1234,
                total: Some(5678),
                filename: None,
            })
        );
        assert_eq!(
            parse_progress("LF 99 NA 4321 C:/dl/clip with spaces.f137.mp4"),
            Some(ParsedVideoProgress {
                done: 99,
                total: Some(4321),
                filename: Some("C:/dl/clip with spaces.f137.mp4".to_string()),
            })
        );
        assert_eq!(
            parse_progress("LF 99 NA NA"),
            Some(ParsedVideoProgress {
                done: 99,
                total: None,
                filename: None,
            })
        );
        assert_eq!(parse_progress("[youtube] Extracting URL"), None);
        assert_eq!(parse_progress(""), None);
    }

    #[test]
    fn video_stream_tracker_skips_subtitles_and_accumulates_video_and_audio() {
        assert!(is_auxiliary_media_file(Some("clip.en.vtt")));
        assert!(is_auxiliary_media_file(Some("clip.id.srt.part")));
        assert!(!is_auxiliary_media_file(Some("clip.f137.mp4")));

        let mut tracker = VideoStreamTracker::from_seeded(Some(150_000_000), 0);
        let (done1, sz1) = tracker.record(20_000_000, Some(135_000_000), Some("clip.f137.mp4"));
        assert_eq!(done1, 20_000_000);
        assert_eq!(sz1, Some(150_000_000));

        let (done2, _) = tracker.record(135_000_000, Some(135_000_000), Some("clip.f137.mp4"));
        assert_eq!(done2, 135_000_000);

        let (done3, sz3) = tracker.record(5_000_000, Some(18_000_000), Some("clip.f140.m4a"));
        assert_eq!(done3, 140_000_000);
        assert_eq!(sz3, Some(153_000_000));

        // Resuming directly on the audio stream must not jump progress backwards.
        let mut resumed = VideoStreamTracker::from_seeded(Some(150_000_000), 140_000_000);
        let (resumed_done, resumed_sz) =
            resumed.record(5_000_000, Some(18_000_000), Some("clip.f140.m4a"));
        assert_eq!(resumed_done, 140_000_000);
        assert_eq!(resumed_sz, Some(150_000_000));
    }

    #[test]
    fn video_args_select_quality_and_target() {
        let args = video_args(
            "bv*[height<=360]+ba",
            Path::new("C:/dl/100% clip.mp4"),
            "https://youtu.be/x",
            Path::new("C:/tools"),
            false,
            false,
            Some("node:/usr/bin/node"),
            Some(Path::new("C:/tmp/cookies.txt")),
            Some("https://www.tiktok.com/@creator/video/123"),
            Some("http://127.0.0.1:8080"),
            512,
        );
        let args: Vec<String> = args
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();

        assert_eq!(
            args[args.iter().position(|a| a == "-f").unwrap() + 1],
            "bv*[height<=360]+ba"
        );
        assert_eq!(
            args[args.iter().position(|a| a == "-o").unwrap() + 1],
            "C:/dl/100%% clip.mp4"
        );
        assert_eq!(
            args[args.iter().position(|a| a == "--proxy").unwrap() + 1],
            "http://127.0.0.1:8080/"
        );
        assert_eq!(
            args[args.iter().position(|a| a == "--limit-rate").unwrap() + 1],
            "512K"
        );
        assert_eq!(
            args[args
                .iter()
                .position(|a| a == "--merge-output-format")
                .unwrap()
                + 1],
            "mp4"
        );
        assert_eq!(
            args[args.iter().position(|a| a == "-S").unwrap() + 1],
            "ext:mp4:m4a,vcodec:h264,acodec:aac"
        );
        assert!(!args.iter().any(|arg| arg == "--ppa"));
        assert_eq!(
            args[args.iter().position(|a| a == "--js-runtimes").unwrap() + 1],
            "node:/usr/bin/node"
        );
        assert_eq!(
            args[args.iter().position(|a| a == "--cookies").unwrap() + 1],
            "C:/tmp/cookies.txt"
        );
        assert_eq!(
            args[args.iter().position(|a| a == "--referer").unwrap() + 1],
            "https://www.tiktok.com/@creator/video/123"
        );
        assert!(!args.iter().any(|arg| arg == "--embed-subs"));
        assert!(!args.iter().any(|arg| arg == "--embed-chapters"));
        assert_eq!(args[args.iter().position(|a| a == "-N").unwrap() + 1], "4");
        assert!(args.iter().any(|arg| arg == "--http-chunk-size"));
        assert_eq!(
            args[args.iter().position(|a| a == "--buffer-size").unwrap() + 1],
            "64K"
        );
        assert_eq!(
            args[args.iter().position(|a| a == "--progress-delta").unwrap() + 1],
            "0.25"
        );
        assert!(args.iter().any(|arg| arg == "--progress-template"));
        assert_eq!(args[args.len() - 2], "--");
        assert_eq!(args.last().unwrap(), "https://youtu.be/x");
    }

    #[test]
    fn video_args_include_subtitles_and_chapters_when_enabled() {
        let args: Vec<String> = video_args(
            "best",
            Path::new("C:/dl/clip.mp4"),
            "https://youtu.be/x",
            Path::new("C:/tools"),
            true,
            true,
            None,
            None,
            Some("--exec calc"),
            None,
            0,
        )
        .iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
        for flag in [
            "--write-subs",
            "--write-auto-subs",
            "--embed-subs",
            "--embed-chapters",
        ] {
            assert!(args.iter().any(|arg| arg == flag), "missing {flag}");
        }
        assert!(!args.iter().any(|arg| arg == "--referer"));
        assert_eq!(valid_http_referrer(Some("file:///etc/passwd")), None);
        assert_eq!(valid_http_referrer(Some("--exec calc")), None);
        assert!(!args.iter().any(|arg| arg == "--proxy"));
        assert!(!args.iter().any(|arg| arg == "--limit-rate"));
    }

    #[test]
    fn video_file_name_uses_the_youtube_id() {
        assert_eq!(
            video_file_name("https://www.youtube.com/watch?v=dQw4w9WgXcQ"),
            "dQw4w9WgXcQ.mp4"
        );
        assert_eq!(video_file_name("https://youtu.be/abc123"), "abc123.mp4");
        assert_eq!(video_file_name("not a url"), "video.mp4");
        assert_eq!(
            resolve_video_file_name(
                Some("Song.m4a"),
                "https://youtu.be/abc123",
                Some("ba[ext=m4a]/ba")
            ),
            "Song.m4a"
        );
        assert_eq!(
            resolve_video_file_name(
                Some("Song.mp4"),
                "https://youtu.be/abc123",
                Some("ba[ext=m4a]/ba")
            ),
            "Song.m4a"
        );
        assert_eq!(
            resolve_video_file_name(None, "https://youtu.be/abc123", Some("ba[ext=m4a]/ba")),
            "abc123.m4a"
        );
    }

    #[test]
    fn video_args_audio_only_extracts_m4a_without_mp4_merge() {
        assert!(lajufile_core::video::is_audio_only_selector(
            "ba[ext=m4a]/ba"
        ));
        assert!(!lajufile_core::video::is_audio_only_selector(
            "bv*[height<=1080]+ba/b"
        ));

        let args: Vec<String> = video_args(
            "ba[ext=m4a]/ba",
            Path::new("C:/dl/song.m4a"),
            "https://youtu.be/x",
            Path::new("C:/tools"),
            true,
            true,
            None,
            None,
            None,
            None,
            0,
        )
        .iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();

        assert!(!args.iter().any(|arg| arg == "--merge-output-format"));
        assert!(!args.iter().any(|arg| arg == "--write-subs"));
        assert!(args.windows(2).any(|w| w == ["-x", "--audio-format"]));
        assert!(args.windows(2).any(|w| w == ["--audio-format", "m4a"]));
        assert!(args.windows(2).any(|w| w == ["-o", "C:/dl/song.m4a"]));
    }

    #[test]
    fn video_args_support_mp3_clip_sections_and_subtitles_only() {
        let mp3_args: Vec<String> = video_args(
            "audio:mp3|clip=00:30-01:45",
            Path::new("C:/dl/track.mp3"),
            "https://youtu.be/x",
            Path::new("C:/tools"),
            true,
            true,
            None,
            None,
            None,
            None,
            0,
        )
        .iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
        assert!(mp3_args.windows(2).any(|w| w == ["--audio-format", "mp3"]));
        assert!(mp3_args.windows(2).any(|w| w == ["-f", "ba/b"]));
        assert!(
            mp3_args
                .windows(2)
                .any(|w| w == ["--download-sections", "*00:30-01:45"])
        );
        assert!(mp3_args.iter().any(|a| a == "--force-keyframes-at-cuts"));

        let sub_args: Vec<String> = video_args(
            lajufile_core::video::SUBTITLES_ONLY_SELECTOR,
            Path::new("C:/dl/talk.srt"),
            "https://youtu.be/x",
            Path::new("C:/tools"),
            false,
            true,
            None,
            None,
            None,
            None,
            0,
        )
        .iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
        assert!(sub_args.iter().any(|a| a == "--skip-download"));
        assert!(sub_args.windows(2).any(|w| w == ["--convert-subs", "srt"]));
        assert!(!sub_args.iter().any(|a| a == "-f"));
        assert!(sub_args.windows(2).any(|w| w == ["-o", "C:/dl/talk"]));

        assert_eq!(
            resolve_video_file_name(Some("Clip.mp4"), "https://youtu.be/x", Some("audio:mp3")),
            "Clip.mp3"
        );
        assert_eq!(
            resolve_video_file_name(Some("Clip.mp4"), "https://youtu.be/x", Some("subs:srt")),
            "Clip.srt"
        );
    }

    #[test]
    fn remove_parts_keeps_the_target_file() {
        let dir = tempfile::tempdir().unwrap();
        let save = dir.path().join("clip.mp4");
        std::fs::write(&save, b"x").unwrap();
        let leftovers = [
            "clip.mp4.part",
            "clip.mp4.f137.mp4.part",
            "clip.mp4.part-Frag12",
            "clip.mp4.ytdl",
            "clip.mp4.temp.mp4",
            "clip.mp4.lajufile",
        ];
        for name in leftovers {
            std::fs::write(dir.path().join(name), b"x").unwrap();
        }
        std::fs::write(dir.path().join("other.mp4.part"), b"x").unwrap();
        std::fs::write(dir.path().join("clip.1080p.mp4.lajufile"), b"x").unwrap();
        std::fs::write(dir.path().join("clip.mp4"), b"final").unwrap();

        remove_parts(&save);

        assert_eq!(std::fs::read(&save).unwrap(), b"final");
        assert!(dir.path().join("other.mp4.part").exists());
        assert!(dir.path().join("clip.1080p.mp4.lajufile").exists());
        for name in leftovers {
            assert!(!dir.path().join(name).exists(), "{name} should be removed");
        }
    }

    #[test]
    fn resolve_video_output_falls_back_by_media_kind_and_freshness() {
        let dir = tempfile::tempdir().unwrap();
        let started = std::time::SystemTime::now();
        let expected = dir.path().join("movie.mp4");
        let mkv = dir.path().join("movie.mkv");
        std::fs::write(&mkv, b"mkv-bytes").unwrap();
        assert_eq!(
            resolve_video_output(&expected, VideoMode::Video, started),
            Some(mkv)
        );
        assert_eq!(
            resolve_video_output(&dir.path().join("movie.m4a"), VideoMode::AudioM4a, started),
            None,
            "audio-only job must not hijack an existing video file with the same stem"
        );
        assert_eq!(
            resolve_video_output(&dir.path().join("absent.mp4"), VideoMode::Video, started),
            None
        );

        let sub_path = dir.path().join("talk.en.srt");
        std::fs::write(&sub_path, b"1\n00:00:00,000 --> 00:00:01,000\nhi\n").unwrap();
        assert_eq!(
            resolve_video_output(
                &dir.path().join("talk.srt"),
                VideoMode::SubtitlesOnly,
                started
            ),
            Some(sub_path)
        );

        let stale_check = started + Duration::from_secs(60);
        assert_eq!(
            resolve_video_output(&expected, VideoMode::Video, stale_check),
            None,
            "older unrelated file must not be picked as a fresh output"
        );
    }

    #[test]
    fn cleanup_stale_cookie_files_removes_other_pid_leftovers() {
        let dir = tempfile::tempdir().unwrap();
        let stale = dir.path().join("lajufile-yt-99999999-0.cookies");
        let keep = dir.path().join("unrelated.cookies");
        std::fs::write(&stale, b"secret").unwrap();
        std::fs::write(&keep, b"keep").unwrap();

        cleanup_stale_cookie_files(dir.path());

        assert!(!stale.exists());
        assert!(keep.exists());
    }

    #[test]
    fn temp_cookie_file_writes_netscape_cookies_and_cleans_up_on_drop() {
        let dir = tempfile::tempdir().unwrap();
        assert!(TempCookieFile::from_netscape_in("SID=abc; HSID=xyz", dir.path()).is_none());
        assert!(
            TempCookieFile::from_netscape_in("# Netscape HTTP Cookie File\n", dir.path()).is_none()
        );

        let netscape =
            "# Netscape HTTP Cookie File\n.youtube.com\tTRUE\t/\tTRUE\t1900000000\tSID\tvalue123\n";
        let path = {
            let file =
                TempCookieFile::from_netscape_in(netscape, dir.path()).expect("valid cookie file");
            assert!(file.path().starts_with(dir.path().join("cookies-tmp")));
            let saved = std::fs::read_to_string(file.path()).unwrap();
            assert!(saved.contains("SID\tvalue123"));
            file.path().to_path_buf()
        };
        assert!(!path.exists(), "cookie file must be deleted on drop");
    }

    #[test]
    fn format_ytdlp_error_clarifies_bot_check_and_picks_error_line() {
        let bot = "WARNING: foo\nERROR: [youtube] xyz: Sign in to confirm you're not a bot.\nAlso see wiki";
        assert!(format_ytdlp_error(bot).contains("signed-in browser session"));

        let other = "WARNING: minor\nERROR: [youtube] xyz: Video unavailable";
        assert_eq!(
            format_ytdlp_error(other),
            "yt-dlp: ERROR: [youtube] xyz: Video unavailable"
        );
    }

    #[test]
    fn ytdlp_work_dir_prefers_writable_absolute_dir_and_falls_back_to_temp() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("videos/sub");
        assert_eq!(ytdlp_work_dir(Some(&nested)), nested);
        assert!(nested.is_dir());

        assert_eq!(ytdlp_work_dir(Some(Path::new(""))), std::env::temp_dir());
        assert_eq!(
            ytdlp_work_dir(Some(Path::new("relative-dir"))),
            std::env::temp_dir()
        );
        assert_eq!(ytdlp_work_dir(None), std::env::temp_dir());
    }

    #[test]
    fn has_parts_detects_ytdlp_partial_files_until_removed() {
        let dir = tempfile::tempdir().unwrap();
        let save = dir.path().join("clip.mp4");
        std::fs::write(dir.path().join("clip.mp4.lajufile"), b"").unwrap();
        assert!(!has_parts(&save));

        std::fs::write(dir.path().join("clip.mp4.part"), b"partial").unwrap();
        assert!(has_parts(&save));

        remove_parts(&save);
        assert!(!has_parts(&save));

        std::fs::write(dir.path().join("clip.f137.mp4.part"), b"partial").unwrap();
        assert!(has_parts(&save));

        remove_parts(&save);
        assert!(!has_parts(&save));
    }
}
