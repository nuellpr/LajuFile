//! HuggingFace model repository file discovery and host matching.

use percent_encoding::{AsciiSet, CONTROLS, utf8_percent_encode};
use reqwest::header::AUTHORIZATION;
use url::Url;

const PATH_SEGMENT_ENCODE: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'#')
    .add(b'%')
    .add(b'?')
    .add(b'[')
    .add(b']');

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HfModelFile {
    pub path: String,
    pub file_name: String,
    pub size: u64,
    pub sha256: Option<String>,
    pub download_url: String,
}

pub fn is_huggingface_host(url: &Url) -> bool {
    if url.scheme() != "https" {
        return false;
    }
    url.host_str().is_some_and(|host| {
        let h = host.to_ascii_lowercase();
        h == "huggingface.co" || h.ends_with(".huggingface.co")
    })
}

/// Parses a HuggingFace model repository URL into `(repo_id, revision)`.
///
/// Accepts `https://huggingface.co/{owner}/{repo}` and `/tree/{revision}` URLs.
/// Returns `None` for direct file links (`/resolve/`, `/blob/`) or non-model namespaces.
pub fn parse_hf_repo_url(raw: &str) -> Option<(String, String)> {
    let parsed = Url::parse(raw.trim()).ok()?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return None;
    }
    let host = parsed.host_str()?.to_ascii_lowercase();
    if host != "huggingface.co" && host != "www.huggingface.co" {
        return None;
    }
    let segments: Vec<&str> = parsed.path_segments()?.filter(|s| !s.is_empty()).collect();
    if segments.len() < 2 {
        return None;
    }
    if matches!(
        segments[0],
        "datasets" | "spaces" | "models" | "docs" | "api" | "blog" | "settings" | "organizations"
    ) {
        return None;
    }
    let repo_id = format!("{}/{}", segments[0], segments[1]);
    let revision = match segments.get(2).copied() {
        None => "main".to_string(),
        Some("tree") => segments
            .get(3)
            .copied()
            .filter(|s| !s.is_empty())
            .unwrap_or("main")
            .to_string(),
        Some(_) => return None,
    };
    Some((repo_id, revision))
}

fn encode_repo_path(path: &str) -> String {
    path.split('/')
        .map(|seg| utf8_percent_encode(seg, PATH_SEGMENT_ENCODE).to_string())
        .collect::<Vec<_>>()
        .join("/")
}

fn model_weight_rank(file_name: &str) -> u8 {
    let lower = file_name.to_ascii_lowercase();
    if lower.ends_with(".gguf") {
        0
    } else if lower.ends_with(".safetensors") {
        1
    } else if lower.ends_with(".onnx") {
        2
    } else if lower.ends_with(".bin") || lower.ends_with(".pt") || lower.ends_with(".pth") {
        3
    } else if lower.ends_with(".ckpt") || lower.ends_with(".zip") || lower.ends_with(".tar.gz") {
        4
    } else {
        9
    }
}

pub fn parse_tree_json(
    repo_id: &str,
    revision: &str,
    body: &str,
) -> Result<Vec<HfModelFile>, String> {
    let entries: Vec<serde_json::Value> =
        serde_json::from_str(body).map_err(|e| format!("Invalid HuggingFace API response: {e}"))?;
    let mut all = Vec::new();
    for entry in entries {
        if entry.get("type").and_then(|v| v.as_str()) != Some("file") {
            continue;
        }
        let Some(path) = entry.get("path").and_then(|v| v.as_str()) else {
            continue;
        };
        if path.is_empty() || path.starts_with('.') {
            continue;
        }
        let file_name = path.rsplit('/').next().unwrap_or(path).to_string();
        let lfs = entry.get("lfs");
        let size = lfs
            .and_then(|l| l.get("size"))
            .and_then(|v| v.as_u64())
            .or_else(|| entry.get("size").and_then(|v| v.as_u64()))
            .unwrap_or(0);
        let sha256 = lfs
            .and_then(|l| l.get("oid"))
            .and_then(|v| v.as_str())
            .map(str::to_ascii_lowercase)
            .filter(|h| h.len() == 64 && h.bytes().all(|b| b.is_ascii_hexdigit()));
        let download_url = format!(
            "https://huggingface.co/{repo_id}/resolve/{revision}/{}?download=true",
            encode_repo_path(path)
        );
        all.push(HfModelFile {
            path: path.to_string(),
            file_name,
            size,
            sha256,
            download_url,
        });
    }
    let weights: Vec<HfModelFile> = all
        .iter()
        .filter(|f| model_weight_rank(&f.file_name) < 9)
        .cloned()
        .collect();
    let mut chosen = if weights.is_empty() { all } else { weights };
    chosen.sort_by(|a, b| {
        model_weight_rank(&a.file_name)
            .cmp(&model_weight_rank(&b.file_name))
            .then_with(|| a.path.cmp(&b.path))
    });
    Ok(chosen)
}

pub async fn list_repo_files(
    client: &reqwest::Client,
    repo_id: &str,
    revision: &str,
    hf_token: Option<&str>,
) -> Result<Vec<HfModelFile>, String> {
    let api_url =
        format!("https://huggingface.co/api/models/{repo_id}/tree/{revision}?recursive=true");
    let mut req = client.get(&api_url);
    if let Some(token) = hf_token.map(str::trim).filter(|t| !t.is_empty()) {
        req = req.header(AUTHORIZATION, format!("Bearer {token}"));
    }
    let resp = req
        .send()
        .await
        .map_err(|e| format!("HuggingFace request failed: {e}"))?;
    let status = resp.status();
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return Err(
            "HuggingFace repo requires authentication — add your HF token in Settings.".to_string(),
        );
    }
    if !status.is_success() {
        return Err(format!("HuggingFace API returned HTTP {}", status.as_u16()));
    }
    let body = resp
        .text()
        .await
        .map_err(|e| format!("Failed reading HuggingFace response: {e}"))?;
    let files = parse_tree_json(repo_id, revision, &body)?;
    if files.is_empty() {
        return Err("No downloadable model files found in repository.".to_string());
    }
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hf_repo_urls_and_skips_direct_files() {
        assert_eq!(
            parse_hf_repo_url("https://huggingface.co/bartowski/Llama-3.2-3B-Instruct-GGUF"),
            Some((
                "bartowski/Llama-3.2-3B-Instruct-GGUF".to_string(),
                "main".to_string()
            ))
        );
        assert_eq!(
            parse_hf_repo_url("https://huggingface.co/org/model/tree/v2"),
            Some(("org/model".to_string(), "v2".to_string()))
        );
        assert_eq!(
            parse_hf_repo_url("https://huggingface.co/org/model/resolve/main/a.gguf"),
            None
        );
        assert_eq!(
            parse_hf_repo_url("https://huggingface.co/datasets/org/data"),
            None
        );
        assert_eq!(parse_hf_repo_url("https://example.com/org/model"), None);
    }

    #[test]
    fn parses_tree_json_prioritizing_model_weights_and_extracting_sha256() {
        let oid = "a".repeat(64);
        let raw = format!(
            r#"[
                {{"type":"file","path":"README.md","size":1200}},
                {{"type":"file","path":"model-Q4_K_M.gguf","size":200,"lfs":{{"oid":"{oid}","size":4294967296}}}},
                {{"type":"file","path":"weights/model.safetensors","size":5000000}}
            ]"#
        );
        let files = parse_tree_json("org/model", "main", &raw).unwrap();
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].file_name, "model-Q4_K_M.gguf");
        assert_eq!(files[0].size, 4_294_967_296);
        assert_eq!(files[0].sha256.as_deref(), Some(oid.as_str()));
        assert_eq!(
            files[0].download_url,
            "https://huggingface.co/org/model/resolve/main/model-Q4_K_M.gguf?download=true"
        );
        assert_eq!(files[1].file_name, "model.safetensors");
    }

    #[test]
    fn hf_host_check_matches_only_huggingface_domains() {
        assert!(is_huggingface_host(
            &Url::parse("https://huggingface.co/a/b").unwrap()
        ));
        assert!(is_huggingface_host(
            &Url::parse("https://cdn-lfs.huggingface.co/blob").unwrap()
        ));
        assert!(!is_huggingface_host(
            &Url::parse("http://huggingface.co/a/b").unwrap()
        ));
        assert!(!is_huggingface_host(
            &Url::parse("http://cdn-lfs.huggingface.co/blob").unwrap()
        ));
        assert!(!is_huggingface_host(
            &Url::parse("https://evil-huggingface.co/a").unwrap()
        ));
    }
}
