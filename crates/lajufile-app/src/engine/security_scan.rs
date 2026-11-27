//! Post-download executable security checks (Windows Defender + optional VirusTotal SHA-256 hash lookup).

use std::path::{Path, PathBuf};
use std::time::Duration;

use lajufile_core::checksum::sha256_file;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScanOutcome {
    Clean(String),
    Threat(String),
    Skipped(String),
}

pub fn is_scannable_executable(path: &Path) -> bool {
    let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
        return false;
    };
    matches!(
        ext.to_ascii_lowercase().as_str(),
        "exe"
            | "msi"
            | "msix"
            | "appx"
            | "apk"
            | "dmg"
            | "deb"
            | "rpm"
            | "bat"
            | "cmd"
            | "ps1"
            | "vbs"
            | "scr"
            | "dll"
            | "jar"
            | "sh"
            | "appimage"
    )
}

pub fn parse_virustotal_stats(body: &str) -> Result<ScanOutcome, String> {
    let doc: serde_json::Value =
        serde_json::from_str(body).map_err(|e| format!("Invalid VirusTotal JSON: {e}"))?;
    let stats = doc
        .pointer("/data/attributes/last_analysis_stats")
        .ok_or_else(|| "Missing VirusTotal analysis stats".to_string())?;
    let malicious = stats.get("malicious").and_then(|v| v.as_u64()).unwrap_or(0);
    let suspicious = stats
        .get("suspicious")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let undetected = stats
        .get("undetected")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let harmless = stats.get("harmless").and_then(|v| v.as_u64()).unwrap_or(0);

    if malicious > 0 || suspicious >= 3 {
        Ok(ScanOutcome::Threat(format!(
            "VirusTotal flagged {malicious} malicious / {suspicious} suspicious"
        )))
    } else {
        let total = malicious + suspicious + undetected + harmless;
        Ok(ScanOutcome::Clean(format!(
            "VirusTotal clean (0/{total} detections)"
        )))
    }
}

#[cfg(windows)]
fn defender_scan_blocking(path: &Path) -> ScanOutcome {
    use std::process::Stdio;
    use std::time::Instant;

    let candidates = [
        std::env::var_os("ProgramFiles")
            .map(|p| PathBuf::from(p).join(r"Windows Defender\MpCmdRun.exe")),
        Some(PathBuf::from(
            r"C:\Program Files\Windows Defender\MpCmdRun.exe",
        )),
    ];
    let Some(exe) = candidates.into_iter().flatten().find(|p| p.is_file()) else {
        return ScanOutcome::Skipped("Windows Defender CLI unavailable".to_string());
    };
    let mut command = std::process::Command::new(exe);
    command
        .arg("-Scan")
        .arg("-ScanType")
        .arg("3")
        .arg("-File")
        .arg(path)
        .arg("-DisableRemediation")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    crate::tools::no_window(&mut command);
    let mut child = match command.spawn() {
        Ok(c) => c,
        Err(err) => return ScanOutcome::Skipped(format!("Defender scan skipped: {err}")),
    };
    let deadline = Instant::now() + Duration::from_secs(45);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                return match status.code() {
                    Some(0) => ScanOutcome::Clean("Windows Defender clean".to_string()),
                    Some(2) => ScanOutcome::Threat(
                        "WARNING: Windows Defender detected a threat!".to_string(),
                    ),
                    Some(code) => ScanOutcome::Skipped(format!("Defender exit code {code}")),
                    None => ScanOutcome::Skipped("Defender terminated".to_string()),
                };
            }
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(100));
            }
            Ok(None) => {
                crate::tools::kill_tree(child.id());
                let _ = child.wait();
                return ScanOutcome::Skipped("Defender scan timed out".to_string());
            }
            Err(err) => return ScanOutcome::Skipped(format!("Defender wait failed: {err}")),
        }
    }
}

#[cfg(not(windows))]
fn defender_scan_blocking(_path: &Path) -> ScanOutcome {
    ScanOutcome::Skipped("Defender scan only available on Windows".to_string())
}

async fn lookup_virustotal_hash(
    client: &reqwest::Client,
    sha256: &str,
    api_key: &str,
) -> Result<ScanOutcome, String> {
    let url = format!("https://www.virustotal.com/api/v3/files/{sha256}");
    let resp = client
        .get(&url)
        .header("x-apikey", api_key)
        .timeout(Duration::from_secs(15))
        .send()
        .await
        .map_err(|e| format!("VirusTotal request failed: {e}"))?;
    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(ScanOutcome::Clean(
            "VirusTotal hash unknown (not previously flagged)".to_string(),
        ));
    }
    if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
        return Err("VirusTotal API key rejected".to_string());
    }
    if !resp.status().is_success() {
        return Err(format!("VirusTotal HTTP {}", resp.status().as_u16()));
    }
    let body = resp
        .text()
        .await
        .map_err(|e| format!("VirusTotal read failed: {e}"))?;
    parse_virustotal_stats(&body)
}

pub async fn scan_file_async(
    client: reqwest::Client,
    path: PathBuf,
    vt_api_key: Option<String>,
) -> String {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".to_string());

    let path_for_defender = path.clone();
    let defender = tokio::task::spawn_blocking(move || defender_scan_blocking(&path_for_defender))
        .await
        .unwrap_or_else(|e| ScanOutcome::Skipped(format!("Defender task failed: {e}")));

    if let ScanOutcome::Threat(msg) = defender {
        return format!("{name}: {msg}");
    }

    let mut notes = Vec::new();
    match defender {
        ScanOutcome::Clean(m) => notes.push(m),
        ScanOutcome::Skipped(m) => notes.push(m),
        ScanOutcome::Threat(_) => {}
    }

    if let Some(key) = vt_api_key
        .map(|k| k.trim().to_string())
        .filter(|k| !k.is_empty())
    {
        let path_for_hash = path.clone();
        match tokio::task::spawn_blocking(move || sha256_file(&path_for_hash)).await {
            Ok(Ok(hash)) => match lookup_virustotal_hash(&client, &hash, &key).await {
                Ok(ScanOutcome::Threat(msg)) => return format!("{name}: WARNING: {msg}"),
                Ok(ScanOutcome::Clean(msg) | ScanOutcome::Skipped(msg)) => notes.push(msg),
                Err(err) => notes.push(err),
            },
            Ok(Err(err)) => notes.push(format!("Hash failed: {err}")),
            Err(err) => notes.push(format!("Hash task failed: {err}")),
        }
    }

    format!("Security scan ({name}): {}", notes.join(" · "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn executable_classifier_covers_installers_and_skips_media() {
        assert!(is_scannable_executable(Path::new("setup.exe")));
        assert!(is_scannable_executable(Path::new("app.MSI")));
        assert!(is_scannable_executable(Path::new("package.apk")));
        assert!(!is_scannable_executable(Path::new("video.mp4")));
        assert!(!is_scannable_executable(Path::new("photo.png")));
        assert!(!is_scannable_executable(Path::new("README")));
    }

    #[test]
    fn virustotal_stats_parser_handles_clean_threat_and_invalid_json() {
        let clean = r#"{"data":{"attributes":{"last_analysis_stats":{"malicious":0,"suspicious":0,"undetected":68,"harmless":2}}}}"#;
        assert_eq!(
            parse_virustotal_stats(clean),
            Ok(ScanOutcome::Clean(
                "VirusTotal clean (0/70 detections)".to_string()
            ))
        );

        let threat = r#"{"data":{"attributes":{"last_analysis_stats":{"malicious":4,"suspicious":1,"undetected":60,"harmless":0}}}}"#;
        assert!(matches!(
            parse_virustotal_stats(threat),
            Ok(ScanOutcome::Threat(_))
        ));

        let suspicious = r#"{"data":{"attributes":{"last_analysis_stats":{"malicious":0,"suspicious":3,"undetected":60,"harmless":0}}}}"#;
        assert!(matches!(
            parse_virustotal_stats(suspicious),
            Ok(ScanOutcome::Threat(_))
        ));

        assert!(parse_virustotal_stats("{bad").is_err());
    }
}
