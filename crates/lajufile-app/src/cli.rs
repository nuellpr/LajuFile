//! Lightweight CLI for forwarding URLs and inspecting the running LajuFile Desktop bridge.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

use lajufile_core::paths;
use lajufile_core::settings::Settings;
use lajufile_core::video::{
    DEFAULT_SELECTOR, compose_video_spec, is_valid_clip_range, is_valid_video_selector,
};
use serde_json::{Value, json};

use crate::bridge::{BRIDGE_HEADER, health_check};
use crate::ui::dialogs::is_video_site_url;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliCommand {
    Add {
        url: String,
        filename: Option<String>,
        video: bool,
        format: Option<String>,
        clip: Option<String>,
    },
    Status,
    Help,
    Version,
}

const HELP_TEXT: &str = "\
LajuFile CLI
Usage:
  lajufile                          Launch LajuFile Desktop
  lajufile add <url> [options]      Send a URL to running LajuFile Desktop
  lajufile status                   Check running LajuFile Desktop bridge
  lajufile --help | -h              Show this help
  lajufile --version | -V           Show version

Add options:
  --name, -n <file>     Override output file name
  --video, -v           Force video/yt-dlp mode
  --format, -f <sel>    yt-dlp selector (e.g. bv*[height<=1080]+ba/b, audio:mp3, subs:srt)
  --clip, -c <range>    Trim video section (e.g. 00:30-01:45)";

pub fn parse_args<I: IntoIterator<Item = String>>(args: I) -> Result<Option<CliCommand>, String> {
    let mut iter = args.into_iter();
    let Some(first) = iter.next() else {
        return Ok(None);
    };
    match first.as_str() {
        "-h" | "--help" | "help" => Ok(Some(CliCommand::Help)),
        "-V" | "--version" | "version" => Ok(Some(CliCommand::Version)),
        "status" => {
            if let Some(extra) = iter.next() {
                return Err(format!("Unexpected argument for 'status': {extra}"));
            }
            Ok(Some(CliCommand::Status))
        }
        "add" => {
            let mut url: Option<String> = None;
            let mut filename: Option<String> = None;
            let mut video = false;
            let mut format: Option<String> = None;
            let mut clip: Option<String> = None;

            while let Some(arg) = iter.next() {
                match arg.as_str() {
                    "-h" | "--help" => return Ok(Some(CliCommand::Help)),
                    "--video" | "-v" => video = true,
                    "--name" | "-n" => {
                        let val = iter
                            .next()
                            .filter(|s| !s.trim().is_empty())
                            .ok_or_else(|| "Missing value for --name".to_string())?;
                        filename = Some(val.trim().to_string());
                    }
                    "--format" | "-f" => {
                        let val = iter
                            .next()
                            .filter(|s| !s.trim().is_empty())
                            .ok_or_else(|| "Missing value for --format".to_string())?;
                        if !is_valid_video_selector(&val) {
                            return Err(format!("Invalid video format selector: {val}"));
                        }
                        format = Some(val.trim().to_string());
                    }
                    "--clip" | "-c" => {
                        let val = iter
                            .next()
                            .filter(|s| !s.trim().is_empty())
                            .ok_or_else(|| "Missing value for --clip".to_string())?;
                        if !is_valid_clip_range(&val) {
                            return Err(format!(
                                "Invalid clip range '{val}' (expected start-end, e.g. 00:30-01:45)"
                            ));
                        }
                        clip = Some(val.trim().to_string());
                    }
                    other if other.starts_with('-') => {
                        return Err(format!("Unknown option: {other}"));
                    }
                    positional => {
                        if url.is_some() {
                            return Err(format!("Unexpected argument: {positional}"));
                        }
                        url = Some(positional.to_string());
                    }
                }
            }

            let raw_url = url.ok_or_else(|| "Missing <url> for 'lajufile add'".to_string())?;
            let parsed =
                url::Url::parse(raw_url.trim()).map_err(|e| format!("Invalid URL: {e}"))?;
            if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
                return Err("Only http:// and https:// URLs are supported".to_string());
            }
            Ok(Some(CliCommand::Add {
                url: parsed.to_string(),
                filename,
                video,
                format,
                clip,
            }))
        }
        other => Err(format!(
            "Unknown command '{other}'. Run 'lajufile --help' for usage."
        )),
    }
}

pub fn build_add_request(
    url: &str,
    filename: Option<&str>,
    video: bool,
    format: Option<&str>,
    clip: Option<&str>,
) -> (&'static str, Value) {
    let is_video = video || format.is_some() || clip.is_some() || is_video_site_url(url);
    let mut payload = json!({ "url": url });
    if let Some(name) = filename.map(str::trim).filter(|s| !s.is_empty()) {
        payload["filename"] = Value::String(name.to_string());
    }
    if is_video {
        let selector = compose_video_spec(format.unwrap_or(DEFAULT_SELECTOR), clip);
        payload["video_format"] = Value::String(selector);
        ("video", payload)
    } else {
        ("downloads", payload)
    }
}

pub fn post_bridge_json(
    port: u16,
    token: &str,
    path: &str,
    body: &Value,
) -> Result<(u16, Value), String> {
    let payload = serde_json::to_vec(body).map_err(|e| format!("serialize error: {e}"))?;
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(3))
        .map_err(|e| format!("Could not reach LajuFile Desktop on 127.0.0.1:{port}: {e}"))?;
    let _ = stream.set_read_timeout(Some(Duration::from_secs(70)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));

    let header = format!(
        "POST /{path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n{BRIDGE_HEADER}: {token}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        payload.len()
    );
    stream
        .write_all(header.as_bytes())
        .and_then(|()| stream.write_all(&payload))
        .map_err(|e| format!("Bridge write failed: {e}"))?;

    let mut raw = Vec::new();
    stream
        .take(64 * 1024)
        .read_to_end(&mut raw)
        .map_err(|e| format!("Bridge read failed: {e}"))?;
    parse_http_response(&raw)
}

fn parse_http_response(raw: &[u8]) -> Result<(u16, Value), String> {
    let (head_bytes, body_bytes) = if let Some(pos) = raw.windows(4).position(|w| w == b"\r\n\r\n")
    {
        (&raw[..pos], &raw[pos + 4..])
    } else if let Some(pos) = raw.windows(2).position(|w| w == b"\n\n") {
        (&raw[..pos], &raw[pos + 2..])
    } else {
        return Err("Invalid HTTP response from LajuFile bridge".to_string());
    };
    let head = String::from_utf8_lossy(head_bytes);
    let status: u16 = head
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse().ok())
        .ok_or_else(|| "Missing HTTP status code from LajuFile bridge".to_string())?;
    let is_chunked = head.lines().any(|line| {
        let lower = line.to_ascii_lowercase();
        lower.starts_with("transfer-encoding:") && lower.contains("chunked")
    });
    let decoded = if is_chunked {
        decode_chunked(body_bytes).unwrap_or_else(|| body_bytes.to_vec())
    } else {
        body_bytes.to_vec()
    };
    let json = serde_json::from_slice::<Value>(&decoded).unwrap_or_else(|_| json!({}));
    Ok((status, json))
}

fn decode_chunked(mut data: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    loop {
        let line_end = data.windows(2).position(|w| w == b"\r\n")?;
        let size_str = std::str::from_utf8(&data[..line_end]).ok()?.trim();
        let size_hex = size_str.split(';').next()?.trim();
        let size = usize::from_str_radix(size_hex, 16).ok()?;
        data = &data[line_end + 2..];
        if size == 0 {
            return Some(out);
        }
        if data.len() < size + 2 {
            return None;
        }
        out.extend_from_slice(&data[..size]);
        data = &data[size + 2..];
    }
}

pub fn run_cli(cmd: CliCommand) -> i32 {
    match cmd {
        CliCommand::Help => {
            println!("{HELP_TEXT}");
            0
        }
        CliCommand::Version => {
            println!("LajuFile {}", env!("CARGO_PKG_VERSION"));
            0
        }
        CliCommand::Status => {
            let settings = Settings::load_readonly(&paths::data_dir()).unwrap_or_default();
            match health_check(settings.bridge_port, Duration::from_secs(2)) {
                Ok(()) => {
                    println!(
                        "LajuFile Desktop bridge online on 127.0.0.1:{}",
                        settings.bridge_port
                    );
                    0
                }
                Err(err) => {
                    eprintln!(
                        "LajuFile Desktop bridge offline on 127.0.0.1:{}: {err}",
                        settings.bridge_port
                    );
                    1
                }
            }
        }
        CliCommand::Add {
            url,
            filename,
            video,
            format,
            clip,
        } => {
            let Some(settings) = Settings::load_readonly(&paths::data_dir()) else {
                eprintln!(
                    "LajuFile settings not found. Launch LajuFile Desktop at least once first."
                );
                return 1;
            };
            if settings.bridge_token.trim().is_empty() {
                eprintln!("LajuFile bridge token is empty. Launch LajuFile Desktop first.");
                return 1;
            }
            let (path, payload) = build_add_request(
                &url,
                filename.as_deref(),
                video,
                format.as_deref(),
                clip.as_deref(),
            );
            match post_bridge_json(settings.bridge_port, &settings.bridge_token, path, &payload) {
                Ok((201, body)) => {
                    let name = body
                        .get("filename")
                        .and_then(Value::as_str)
                        .unwrap_or("download");
                    println!("Added to LajuFile: {name}");
                    0
                }
                Ok((409, _)) => {
                    println!("Already in LajuFile.");
                    0
                }
                Ok((204, _)) => {
                    println!("Canceled in LajuFile.");
                    0
                }
                Ok((code, body)) => {
                    let msg = body
                        .get("error")
                        .and_then(Value::as_str)
                        .unwrap_or("request failed");
                    eprintln!("LajuFile error (HTTP {code}): {msg}");
                    1
                }
                Err(err) => {
                    eprintln!("{err}");
                    1
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_args_handles_empty_help_status_and_add_options() {
        assert_eq!(parse_args(Vec::<String>::new()), Ok(None));
        assert_eq!(
            parse_args(["--help".to_string()]),
            Ok(Some(CliCommand::Help))
        );
        assert_eq!(
            parse_args(["-V".to_string()]),
            Ok(Some(CliCommand::Version))
        );
        assert_eq!(
            parse_args(["status".to_string()]),
            Ok(Some(CliCommand::Status))
        );

        let parsed = parse_args([
            "add".to_string(),
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ".to_string(),
            "--name".to_string(),
            "rick.mp3".to_string(),
            "--format".to_string(),
            "audio:mp3".to_string(),
            "--clip".to_string(),
            "00:10-00:30".to_string(),
        ])
        .unwrap()
        .unwrap();
        assert_eq!(
            parsed,
            CliCommand::Add {
                url: "https://www.youtube.com/watch?v=dQw4w9WgXcQ".to_string(),
                filename: Some("rick.mp3".to_string()),
                video: false,
                format: Some("audio:mp3".to_string()),
                clip: Some("00:10-00:30".to_string()),
            }
        );

        assert!(parse_args(["add".to_string(), "file:///C:/a.zip".to_string()]).is_err());
        assert!(
            parse_args([
                "add".to_string(),
                "https://youtu.be/abc".to_string(),
                "--clip".to_string(),
                "30-10".to_string()
            ])
            .is_err()
        );
        assert!(parse_args(["unknown".to_string()]).is_err());
    }

    #[test]
    fn build_add_request_selects_video_or_http_endpoint() {
        let (path1, body1) = build_add_request(
            "https://example.com/app.zip",
            Some("a.zip"),
            false,
            None,
            None,
        );
        assert_eq!(path1, "downloads");
        assert_eq!(body1["filename"], "a.zip");
        assert!(body1.get("video_format").is_none());

        let (path2, body2) = build_add_request(
            "https://www.tiktok.com/@user/video/123",
            None,
            false,
            Some("audio:mp3"),
            Some("00:05-00:15"),
        );
        assert_eq!(path2, "video");
        assert_eq!(body2["video_format"], "audio:mp3|clip=00:05-00:15");
    }
}
