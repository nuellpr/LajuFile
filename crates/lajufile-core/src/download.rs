use std::path::{Path, PathBuf};
use std::sync::Arc;

use reqwest::header::CONTENT_RANGE;
use tokio::io::AsyncWriteExt;
use url::Url;

use crate::http::{RequestOptions, apply_headers, ensure_identity_encoding, parse_content_range};
use crate::speed::RateLimiter;

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
    on_progress: F,
) -> Result<DownloadOutcome, DownloadError>
where
    F: FnMut(u64) + Send,
{
    download_single_with_limiter(client, spec, None, on_progress).await
}

pub async fn download_single_with_limiter<F>(
    client: &reqwest::Client,
    spec: &DownloadSpec,
    limiter: Option<Arc<RateLimiter>>,
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

    if let Err(encoding) = ensure_identity_encoding(response.headers()) {
        return Err(DownloadError::UnsupportedEncoding(encoding));
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
        let total = range
            .total
            .ok_or_else(|| DownloadError::ContentRange(raw.to_string()))?;
        if range.start != 0 || range.end.checked_add(1) != Some(total) {
            return Err(DownloadError::ContentRange(raw.to_string()));
        }
        if let Some(known) = expected {
            if known != total {
                return Err(DownloadError::SizeMismatch {
                    expected: known,
                    actual: total,
                });
            }
        }
        expected = Some(total);
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

    const WRITE_BUF_CAP: usize = 256 * 1024;
    let mut downloaded = 0_u64;
    let mut write_buf: Vec<u8> = Vec::with_capacity(WRITE_BUF_CAP);
    let mut last_notify = std::time::Instant::now();
    let mut response = response;
    while let Some(chunk) = response.chunk().await? {
        let length = chunk.len() as u64;
        let next_total = downloaded
            .saturating_add(write_buf.len() as u64)
            .saturating_add(length);
        if let Some(known) = expected
            && next_total > known
        {
            return Err(DownloadError::SizeMismatch {
                expected: known,
                actual: next_total,
            });
        }
        if let Some(limiter) = &limiter {
            limiter.throttle(length).await;
        }
        write_buf.extend_from_slice(&chunk);
        if write_buf.len() >= WRITE_BUF_CAP
            || last_notify.elapsed() >= std::time::Duration::from_millis(80)
        {
            let flushed = write_buf.len() as u64;
            file.write_all(&write_buf).await?;
            write_buf.clear();
            downloaded = downloaded.saturating_add(flushed);
            if last_notify.elapsed() >= std::time::Duration::from_millis(50)
                || expected.is_some_and(|known| downloaded == known)
            {
                on_progress(downloaded);
                last_notify = std::time::Instant::now();
            }
        }
    }
    if !write_buf.is_empty() {
        let flushed = write_buf.len() as u64;
        file.write_all(&write_buf).await?;
        write_buf.clear();
        downloaded = downloaded.saturating_add(flushed);
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
    finalize_temp(&spec.temp_path, &spec.final_path)?;

    Ok(DownloadOutcome {
        final_path: spec.final_path.clone(),
        bytes: downloaded,
    })
}

pub(crate) fn finalize_temp(temp: &Path, final_path: &Path) -> Result<(), DownloadError> {
    // shortcut: hard links need NTFS-like support; exFAT/FAT32 destinations fail
    // with Io instead of overwriting. Upgrade to MoveFileExW without REPLACE_EXISTING.
    match std::fs::hard_link(temp, final_path) {
        Ok(()) => {
            let _ = std::fs::remove_file(temp);
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            Err(DownloadError::DestinationExists(final_path.to_path_buf()))
        }
        Err(error) => Err(error.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finalize_refuses_existing_destination_without_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        let temp = dir.path().join("file.bin.lajufile");
        let final_path = dir.path().join("file.bin");
        std::fs::write(&temp, b"new").unwrap();
        std::fs::write(&final_path, b"keep").unwrap();

        let error = finalize_temp(&temp, &final_path).unwrap_err();

        assert!(matches!(
            error,
            DownloadError::DestinationExists(ref path) if path == &final_path
        ));
        assert_eq!(std::fs::read(&final_path).unwrap(), b"keep");
        assert_eq!(std::fs::read(&temp).unwrap(), b"new");
    }

    #[test]
    fn finalize_moves_temp_when_destination_is_free() {
        let dir = tempfile::tempdir().unwrap();
        let temp = dir.path().join("file.bin.lajufile");
        let final_path = dir.path().join("file.bin");
        std::fs::write(&temp, b"new").unwrap();

        finalize_temp(&temp, &final_path).unwrap();

        assert_eq!(std::fs::read(&final_path).unwrap(), b"new");
        assert!(!temp.exists());
    }
}
