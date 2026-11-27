//! Discovery for HuggingFace models, GitHub Releases, and Civitai model files.

use std::collections::HashMap;

use reqwest::header::{ACCEPT, AUTHORIZATION};
use url::Url;

use crate::engine::huggingface::{self, HfModelFile};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SmartSource {
    HuggingFace {
        repo_id: String,
        revision: String,
    },
    GitHubRelease {
        owner: String,
        repo: String,
        tag: Option<String>,
    },
    Civitai {
        model_id: String,
        version_id: Option<String>,
    },
}

pub fn is_civitai_host(url: &Url) -> bool {
    if url.scheme() != "https" {
        return false;
    }
    url.host_str().is_some_and(|host| {
        let h = host.to_ascii_lowercase();
        h == "civitai.com" || h.ends_with(".civitai.com")
    })
}

pub fn parse_github_release_url(raw: &str) -> Option<(String, String, Option<String>)> {
    let parsed = Url::parse(raw.trim()).ok()?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return None;
    }
    let host = parsed.host_str()?.to_ascii_lowercase();
    if host != "github.com" && host != "www.github.com" {
        return None;
    }
    let segments: Vec<&str> = parsed.path_segments()?.filter(|s| !s.is_empty()).collect();
    if segments.len() < 2 {
        return None;
    }
    if matches!(
        segments[0].to_ascii_lowercase().as_str(),
        "features"
            | "marketplace"
            | "explore"
            | "topics"
            | "trending"
            | "collections"
            | "events"
            | "sponsors"
            | "settings"
            | "notifications"
            | "login"
            | "signup"
            | "orgs"
            | "users"
            | "search"
            | "about"
            | "pricing"
            | "security"
            | "site"
    ) {
        return None;
    }
    let owner = segments[0].to_string();
    let repo = segments[1].trim_end_matches(".git").to_string();
    if owner.is_empty() || repo.is_empty() {
        return None;
    }
    let tag = match segments.get(2).copied() {
        None => None,
        Some("releases") => match segments.get(3).copied() {
            None | Some("latest") if segments.len() <= 4 => None,
            Some("tag") => {
                let t = segments.get(4).copied().filter(|s| !s.is_empty())?;
                if segments.len() > 5 {
                    return None;
                }
                Some(t.to_string())
            }
            _ => return None,
        },
        Some(_) => return None,
    };
    Some((owner, repo, tag))
}

pub fn parse_civitai_model_url(raw: &str) -> Option<(String, Option<String>)> {
    let parsed = Url::parse(raw.trim()).ok()?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return None;
    }
    let host = parsed.host_str()?.to_ascii_lowercase();
    if host != "civitai.com" && host != "www.civitai.com" {
        return None;
    }
    let segments: Vec<&str> = parsed.path_segments()?.filter(|s| !s.is_empty()).collect();
    if segments.len() < 2 || segments[0] != "models" {
        return None;
    }
    let model_id = segments[1];
    if model_id.is_empty() || !model_id.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let version_id = parsed
        .query_pairs()
        .find(|(k, _)| k == "modelVersionId")
        .map(|(_, v)| v.into_owned())
        .filter(|v| !v.is_empty() && v.bytes().all(|b| b.is_ascii_digit()));
    Some((model_id.to_string(), version_id))
}

pub fn detect_source(raw: &str) -> Option<SmartSource> {
    if let Some((repo_id, revision)) = huggingface::parse_hf_repo_url(raw) {
        return Some(SmartSource::HuggingFace { repo_id, revision });
    }
    if let Some((owner, repo, tag)) = parse_github_release_url(raw) {
        return Some(SmartSource::GitHubRelease { owner, repo, tag });
    }
    if let Some((model_id, version_id)) = parse_civitai_model_url(raw) {
        return Some(SmartSource::Civitai {
            model_id,
            version_id,
        });
    }
    None
}

fn is_checksum_manifest_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower == "sha256sums"
        || lower == "sha256sums.txt"
        || lower == "checksums.txt"
        || lower.ends_with(".sha256")
        || lower.ends_with(".sha256sum")
        || lower.ends_with(".sha256.txt")
}

pub fn parse_sha256_checksums_text(manifest: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for line in manifest.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let mut parts = trimmed.splitn(2, char::is_whitespace);
        let (Some(digest), Some(file)) = (parts.next(), parts.next()) else {
            continue;
        };
        let digest = digest.trim();
        let file = file.trim_start().trim_start_matches('*');
        let file = file.strip_prefix("./").unwrap_or(file).trim();
        let file_base = file.rsplit(['/', '\\']).next().unwrap_or(file);
        if digest.len() == 64
            && digest.bytes().all(|b| b.is_ascii_hexdigit())
            && !file_base.is_empty()
        {
            map.insert(file_base.to_string(), digest.to_ascii_lowercase());
        }
    }
    map
}

fn github_asset_rank_for(name: &str, os: &str, arch: &str) -> u8 {
    let lower = name.to_ascii_lowercase();
    if is_checksum_manifest_name(&lower)
        || lower.ends_with(".sig")
        || lower.ends_with(".asc")
        || lower.ends_with(".minisig")
    {
        return 9;
    }
    let os_match = match os {
        "windows" => {
            lower.contains("windows")
                || lower.contains("win64")
                || lower.contains("win32")
                || lower.ends_with(".exe")
                || lower.ends_with(".msi")
        }
        "macos" => {
            lower.contains("macos")
                || lower.contains("darwin")
                || lower.contains("osx")
                || lower.ends_with(".dmg")
                || lower.ends_with(".pkg")
        }
        _ => {
            lower.contains("linux")
                || lower.ends_with(".appimage")
                || lower.ends_with(".deb")
                || lower.ends_with(".rpm")
                || lower.ends_with(".run")
        }
    };
    let arch_tokens: &[&str] = match arch {
        "x86_64" => &["x86_64", "amd64", "x64", "win64"],
        "aarch64" => &["aarch64", "arm64"],
        "x86" => &["i686", "x86", "win32"],
        _ => &[],
    };
    let arch_match = arch_tokens.iter().any(|t| lower.contains(t));
    match (os_match, arch_match) {
        (true, true) => 0,
        (true, false) => 1,
        (false, true) => 2,
        (false, false) => 3,
    }
}

pub fn parse_github_release_json(
    body: &str,
    checksums_map: &HashMap<String, String>,
) -> Result<Vec<HfModelFile>, String> {
    parse_github_release_json_for_target(
        body,
        checksums_map,
        std::env::consts::OS,
        std::env::consts::ARCH,
    )
}

pub fn parse_github_release_json_for_target(
    body: &str,
    checksums_map: &HashMap<String, String>,
    os: &str,
    arch: &str,
) -> Result<Vec<HfModelFile>, String> {
    let doc: serde_json::Value =
        serde_json::from_str(body).map_err(|e| format!("Invalid GitHub API response: {e}"))?;
    let Some(assets) = doc.get("assets").and_then(|v| v.as_array()) else {
        return Err("No downloadable assets found in this GitHub release.".to_string());
    };
    let tag = doc
        .get("tag_name")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let mut out = Vec::new();
    for asset in assets {
        let Some(name) = asset.get("name").and_then(|v| v.as_str()) else {
            continue;
        };
        let Some(dl_url) = asset.get("browser_download_url").and_then(|v| v.as_str()) else {
            continue;
        };
        if !dl_url.starts_with("https://") {
            continue;
        }
        let size = asset.get("size").and_then(|v| v.as_u64()).unwrap_or(0);
        let sha256 = checksums_map.get(name).cloned();
        let path = if tag.is_empty() {
            name.to_string()
        } else {
            format!("{tag}/{name}")
        };
        out.push(HfModelFile {
            path,
            file_name: name.to_string(),
            size,
            sha256,
            download_url: dl_url.to_string(),
        });
    }
    out.sort_by(|a, b| {
        github_asset_rank_for(&a.file_name, os, arch)
            .cmp(&github_asset_rank_for(&b.file_name, os, arch))
            .then_with(|| a.file_name.cmp(&b.file_name))
    });
    if out.is_empty() {
        return Err("No downloadable assets found in this GitHub release.".to_string());
    }
    Ok(out)
}

pub fn parse_civitai_model_json(
    body: &str,
    version_id: Option<&str>,
) -> Result<Vec<HfModelFile>, String> {
    let doc: serde_json::Value =
        serde_json::from_str(body).map_err(|e| format!("Invalid Civitai API response: {e}"))?;
    let versions = doc
        .get("modelVersions")
        .and_then(|v| v.as_array())
        .filter(|arr| !arr.is_empty())
        .ok_or_else(|| "No model versions found on Civitai.".to_string())?;
    let chosen_version = version_id
        .and_then(|wanted| {
            versions.iter().find(|v| {
                v.get("id")
                    .and_then(|id| id.as_u64())
                    .is_some_and(|n| n.to_string() == wanted)
            })
        })
        .unwrap_or(&versions[0]);
    let version_name = chosen_version
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let files = chosen_version
        .get("files")
        .and_then(|v| v.as_array())
        .ok_or_else(|| "No downloadable files found in Civitai model version.".to_string())?;

    let mut out = Vec::new();
    for file in files {
        let Some(name) = file.get("name").and_then(|v| v.as_str()) else {
            continue;
        };
        let Some(dl_url) = file.get("downloadUrl").and_then(|v| v.as_str()) else {
            continue;
        };
        if !dl_url.starts_with("https://") {
            continue;
        }
        let size = file
            .get("sizeKB")
            .and_then(|v| v.as_f64())
            .filter(|kb| kb.is_finite() && *kb > 0.0)
            .map(|kb| (kb * 1024.0).round() as u64)
            .unwrap_or(0);
        let sha256 = file
            .get("hashes")
            .and_then(|h| h.get("SHA256"))
            .and_then(|v| v.as_str())
            .map(str::to_ascii_lowercase)
            .filter(|h| h.len() == 64 && h.bytes().all(|b| b.is_ascii_hexdigit()));
        let path = if version_name.is_empty() {
            name.to_string()
        } else {
            format!("{version_name}/{name}")
        };
        out.push(HfModelFile {
            path,
            file_name: name.to_string(),
            size,
            sha256,
            download_url: dl_url.to_string(),
        });
    }
    if out.is_empty() {
        return Err("No downloadable files found in Civitai model version.".to_string());
    }
    Ok(out)
}

pub async fn list_source_files(
    client: &reqwest::Client,
    source: &SmartSource,
    hf_token: Option<&str>,
    civitai_token: Option<&str>,
) -> Result<Vec<HfModelFile>, String> {
    match source {
        SmartSource::HuggingFace { repo_id, revision } => {
            huggingface::list_repo_files(client, repo_id, revision, hf_token).await
        }
        SmartSource::GitHubRelease { owner, repo, tag } => {
            let api_url = match tag {
                Some(t) => format!("https://api.github.com/repos/{owner}/{repo}/releases/tags/{t}"),
                None => format!("https://api.github.com/repos/{owner}/{repo}/releases/latest"),
            };
            let resp = client
                .get(&api_url)
                .header(ACCEPT, "application/vnd.github+json")
                .send()
                .await
                .map_err(|e| format!("GitHub request failed: {e}"))?;
            if !resp.status().is_success() {
                return Err(format!(
                    "GitHub API returned HTTP {}",
                    resp.status().as_u16()
                ));
            }
            let body = resp
                .text()
                .await
                .map_err(|e| format!("Failed reading GitHub response: {e}"))?;

            let mut checksums_map = HashMap::new();
            if let Ok(doc) = serde_json::from_str::<serde_json::Value>(&body)
                && let Some(assets) = doc.get("assets").and_then(|v| v.as_array())
            {
                let manifest_url = assets.iter().find_map(|a| {
                    let name = a.get("name")?.as_str()?;
                    let size = a.get("size").and_then(|v| v.as_u64()).unwrap_or(0);
                    let dl = a.get("browser_download_url")?.as_str()?;
                    (is_checksum_manifest_name(name)
                        && size > 0
                        && size <= 256 * 1024
                        && dl.starts_with("https://"))
                    .then(|| dl.to_string())
                });
                if let Some(url) = manifest_url
                    && let Ok(cs_resp) = client.get(&url).send().await
                    && cs_resp.status().is_success()
                    && let Ok(text) = cs_resp.text().await
                {
                    checksums_map = parse_sha256_checksums_text(&text);
                }
            }
            parse_github_release_json(&body, &checksums_map)
        }
        SmartSource::Civitai {
            model_id,
            version_id,
        } => {
            let api_url = format!("https://civitai.com/api/v1/models/{model_id}");
            let mut req = client.get(&api_url);
            if let Some(token) = civitai_token.map(str::trim).filter(|t| !t.is_empty()) {
                req = req.header(AUTHORIZATION, format!("Bearer {token}"));
            }
            let resp = req
                .send()
                .await
                .map_err(|e| format!("Civitai request failed: {e}"))?;
            if resp.status() == reqwest::StatusCode::UNAUTHORIZED
                || resp.status() == reqwest::StatusCode::FORBIDDEN
            {
                return Err(
                    "Civitai model requires authentication — add your Civitai token in Settings."
                        .to_string(),
                );
            }
            if !resp.status().is_success() {
                return Err(format!(
                    "Civitai API returned HTTP {}",
                    resp.status().as_u16()
                ));
            }
            let body = resp
                .text()
                .await
                .map_err(|e| format!("Failed reading Civitai response: {e}"))?;
            parse_civitai_model_json(&body, version_id.as_deref())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_hf_github_and_civitai_sources_and_skips_direct_files() {
        assert!(matches!(
            detect_source("https://huggingface.co/bartowski/Llama-3.2-3B-Instruct-GGUF"),
            Some(SmartSource::HuggingFace { .. })
        ));
        assert_eq!(
            detect_source("https://github.com/nuellpr/LajuFile/releases/latest"),
            Some(SmartSource::GitHubRelease {
                owner: "nuellpr".to_string(),
                repo: "LajuFile".to_string(),
                tag: None,
            })
        );
        assert_eq!(
            detect_source("https://github.com/nuellpr/LajuFile/releases/tag/v2.0.2"),
            Some(SmartSource::GitHubRelease {
                owner: "nuellpr".to_string(),
                repo: "LajuFile".to_string(),
                tag: Some("v2.0.2".to_string()),
            })
        );
        assert_eq!(
            detect_source("https://github.com/nuellpr/LajuFile/releases/download/v2.0.2/a.exe"),
            None
        );
        assert_eq!(
            detect_source("https://github.com/nuellpr/LajuFile/blob/main/README.md"),
            None
        );
        assert_eq!(
            detect_source("https://civitai.com/models/12345/my-lora?modelVersionId=98765"),
            Some(SmartSource::Civitai {
                model_id: "12345".to_string(),
                version_id: Some("98765".to_string()),
            })
        );
        assert!(is_civitai_host(
            &Url::parse("https://civitai.com/api/download/models/98765").unwrap()
        ));
        assert!(!is_civitai_host(
            &Url::parse("http://civitai.com/api/download/models/98765").unwrap()
        ));
    }

    #[test]
    fn parses_github_release_assets_ranks_os_arch_and_attaches_checksums() {
        let sha = "b".repeat(64);
        let checksums =
            parse_sha256_checksums_text(&format!("{sha}  LajuFile-v2.0.2-windows-x64-setup.exe\n"));
        let body = r#"{
            "tag_name": "v2.0.2",
            "assets": [
                {"name": "SHA256SUMS.txt", "size": 120, "browser_download_url": "https://github.com/a/b/releases/download/v2.0.2/SHA256SUMS.txt"},
                {"name": "LajuFile-v2.0.2-linux-x64-setup.run", "size": 5000000, "browser_download_url": "https://github.com/a/b/releases/download/v2.0.2/LajuFile-v2.0.2-linux-x64-setup.run"},
                {"name": "LajuFile-v2.0.2-windows-x64-setup.exe", "size": 6000000, "browser_download_url": "https://github.com/a/b/releases/download/v2.0.2/LajuFile-v2.0.2-windows-x64-setup.exe"}
            ]
        }"#;
        let files =
            parse_github_release_json_for_target(body, &checksums, "windows", "x86_64").unwrap();
        assert_eq!(files[0].file_name, "LajuFile-v2.0.2-windows-x64-setup.exe");
        assert_eq!(files[0].sha256.as_deref(), Some(sha.as_str()));
        assert_eq!(files.last().unwrap().file_name, "SHA256SUMS.txt");
    }

    #[test]
    fn parses_civitai_model_json_selecting_requested_version() {
        let hash = "c".repeat(64);
        let body = format!(
            r#"{{
                "modelVersions": [
                    {{"id": 100, "name": "v1", "files": [{{"name": "old.safetensors", "sizeKB": 100.0, "downloadUrl": "https://civitai.com/api/download/models/100"}}]}},
                    {{"id": 200, "name": "v2", "files": [{{"name": "new.safetensors", "sizeKB": 2048.0, "hashes": {{"SHA256": "{}"}}, "downloadUrl": "https://civitai.com/api/download/models/200"}}]}}
                ]
            }}"#,
            hash.to_ascii_uppercase()
        );
        let files = parse_civitai_model_json(&body, Some("200")).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].file_name, "new.safetensors");
        assert_eq!(files[0].size, 2048 * 1024);
        assert_eq!(files[0].sha256.as_deref(), Some(hash.as_str()));
    }
}
