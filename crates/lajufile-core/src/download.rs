use std::path::PathBuf;

use reqwest::header::{CONTENT_ENCODING, CONTENT_RANGE};
use tokio::io::AsyncWriteExt;
use url::Url;

use crate::http::{RequestOptions, apply_headers, parse_content_range};

#[derive(Debug, Clone)]
pub struct DownloadSpec {
    pub url: Url,
    pub temp_path: PathBuf,
    pub final_path: PathBuf,
    pub expected_size: Option<u64>,
    pub options: RequestOptions,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadOutcome {
    pub final_path: PathBuf,
    pub bytes: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum DownloadError {
    #[error("HTTP {0}")]
    Status(u16),
    #[error("request failed: {0}")]
    Request(#[from] reqwest::Error),
    #[error("I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error("size mismatch: expected {expected}, got {actual}")]
    SizeMismatch { expected: u64, actual: u64 },
    #[error("unexpected Content-Range: {0}")]
    ContentRange(String),
    #[error("destination already exists: {0}")]
    DestinationExists(PathBuf),
    #[error("unsupported Content-Encoding: {0}")]
    UnsupportedEncoding(String),
    #[error("unsupported URL scheme: {0}")]
    UnsupportedScheme(String),
}

pub async fn download_single<F>(
    client: &reqwest::Client,
    spec: &DownloadSpec,
    mut on_progress: F,
) -> Result<DownloadOutcome, DownloadError>
where
    F: FnMut(u64) + Send,
{
    match spec.url.scheme() {
        "http" | "https" => {}
        other => return Err(DownloadError::UnsupportedScheme(other.to_string())),
    }

    if spec.final_path.exists() {
        return Err(DownloadError::DestinationExists(spec.final_path.clone()));
    }

    if let Some(parent) = spec.temp_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let mut file = tokio::fs::File::create(&spec.temp_path).await?;

    let response = apply_headers(client.get(spec.url.clone()), &spec.options)
        .send()
        .await?;
    let status = response.status();
    if status != reqwest::StatusCode::OK && status != reqwest::StatusCode::PARTIAL_CONTENT {
        return Err(DownloadError::Status(status.as_u16()));
    }

    if let Some(value) = response.headers().get(CONTENT_ENCODING) {
        let encoding = value.to_str().unwrap_or("<invalid>");
        if encoding != "identity" {
            return Err(DownloadError::UnsupportedEncoding(encoding.to_string()));
        }
    }

    let mut expected = spec.expected_size;
    if status == reqwest::StatusCode::PARTIAL_CONTENT {
        let raw = response
            .headers()
            .get(CONTENT_RANGE)
            .and_then(|value| value.to_str().ok())
            .ok_or_else(|| DownloadError::ContentRange("<missing>".to_string()))?;
        let range =
            parse_content_range(raw).ok_or_else(|| DownloadError::ContentRange(raw.to_string()))?;
        if range.start != 0 {
            return Err(DownloadError::ContentRange(raw.to_string()));
        }
        if let Some(total) = range.total {
            if let Some(known) = expected {
                if known != total {
                    return Err(DownloadError::SizeMismatch {
                        expected: known,
                        actual: total,
                    });
                }
            }
            expected = Some(total);
        }
    } else if let Some(length) = response.content_length() {
        if let Some(known) = expected {
            if known != length {
                return Err(DownloadError::SizeMismatch {
                    expected: known,
                    actual: length,
                });
            }
        }
        expected = Some(length);
    }

    let mut downloaded = 0_u64;
    let mut response = response;
    while let Some(chunk) = response.chunk().await? {
        let length = chunk.len() as u64;
        downloaded = downloaded.saturating_add(length);
        if let Some(known) = expected {
            if downloaded > known {
                return Err(DownloadError::SizeMismatch {
                    expected: known,
                    actual: downloaded,
                });
            }
        }
        file.write_all(&chunk).await?;
        on_progress(downloaded);
    }

    file.flush().await?;
    file.sync_all().await?;
    drop(file);

    if let Some(known) = expected {
        if downloaded != known {
            return Err(DownloadError::SizeMismatch {
                expected: known,
                actual: downloaded,
            });
        }
    }

    if spec.final_path.exists() {
        return Err(DownloadError::DestinationExists(spec.final_path.clone()));
    }
    match std::fs::rename(&spec.temp_path, &spec.final_path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            return Err(DownloadError::DestinationExists(spec.final_path.clone()));
        }
        Err(error) => return Err(error.into()),
    }

    Ok(DownloadOutcome {
        final_path: spec.final_path.clone(),
        bytes: downloaded,
    })
}
