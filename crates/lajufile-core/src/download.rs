use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

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
    #[error("insufficient disk space: need {required} bytes, {available} bytes available")]
    InsufficientDiskSpace { required: u64, available: u64 },
}

pub(crate) fn allocated_file_bytes(path: &Path) -> u64 {
    let Ok(meta) = std::fs::metadata(path) else {
        return 0;
    };
    if !meta.is_file() {
        return 0;
    }
    #[cfg(windows)]
    {
        // Non-sparse NTFS set_len reserves full logical length in GetDiskFreeSpaceExW.
        meta.len()
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        meta.blocks().saturating_mul(512).min(meta.len())
    }
    #[cfg(not(any(windows, unix)))]
    {
        0
    }
}

pub fn ensure_disk_space(target: &Path, required: u64) -> Result<(), DownloadError> {
    if required == 0 {
        return Ok(());
    }
    let mut probe = target.parent().unwrap_or(target);
    while !probe.exists() {
        match probe.parent() {
            Some(parent) if !parent.as_os_str().is_empty() => probe = parent,
            _ => break,
        }
    }
    if !probe.exists() {
        return Ok(());
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        let wide: Vec<u16> = probe
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let mut free_avail = 0u64;
        let ok = unsafe {
            windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW(
                wide.as_ptr(),
                &mut free_avail,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        if ok != 0 && free_avail < required {
            return Err(DownloadError::InsufficientDiskSpace {
                required,
                available: free_avail,
            });
        }
    }
    #[cfg(not(windows))]
    {
        if let Ok(out) = std::process::Command::new("df")
            .arg("-Pk")
            .arg(probe)
            .output()
            && out.status.success()
            && let Some(line) = String::from_utf8_lossy(&out.stdout).lines().nth(1)
            && let Some(kib_str) = line.split_whitespace().nth(3)
            && let Ok(kib) = kib_str.parse::<u64>()
        {
            let free_avail = kib.saturating_mul(1024);
            if free_avail < required {
                return Err(DownloadError::InsufficientDiskSpace {
                    required,
                    available: free_avail,
                });
            }
        }
    }
    Ok(())
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

pub(crate) fn is_retryable_error(err: &DownloadError) -> bool {
    match err {
        DownloadError::Request(_) => true,
        DownloadError::Status(status) => *status == 429 || (500..600).contains(status),
        _ => false,
    }
}

pub async fn download_single_with_retry<F>(
    client: &reqwest::Client,
    spec: &DownloadSpec,
    limiter: Option<Arc<RateLimiter>>,
    retry_delays: &[Duration],
    mut on_progress: F,
) -> Result<DownloadOutcome, DownloadError>
where
    F: FnMut(u64) + Send,
{
    for attempt in 0..=retry_delays.len() {
        match download_single_with_limiter(client, spec, limiter.clone(), &mut on_progress).await {
            Ok(outcome) => return Ok(outcome),
            Err(err) if attempt < retry_delays.len() && is_retryable_error(&err) => {
                let delay = retry_delays[attempt];
                if !delay.is_zero() {
                    tokio::time::sleep(delay).await;
                }
            }
            Err(err) => return Err(err),
        }
    }
    Err(DownloadError::Status(0))
}

pub async fn download_single_from_response_or_retry<F>(
    client: &reqwest::Client,
    initial_response: Option<reqwest::Response>,
    spec: &DownloadSpec,
    limiter: Option<Arc<RateLimiter>>,
    item_limiter: Option<Arc<RateLimiter>>,
    retry_delays: &[Duration],
    mut on_progress: F,
) -> Result<DownloadOutcome, DownloadError>
where
    F: FnMut(u64) + Send,
{
    if let Some(response) = initial_response {
        match download_single_from_response_with_item(
            response,
            spec,
            limiter.clone(),
            item_limiter.clone(),
            &mut on_progress,
        )
        .await
        {
            Ok(outcome) => return Ok(outcome),
            Err(err) if !retry_delays.is_empty() && is_retryable_error(&err) => {
                let delay = retry_delays[0];
                if !delay.is_zero() {
                    tokio::time::sleep(delay).await;
                }
                return download_single_with_retry_inner(
                    client,
                    spec,
                    limiter,
                    item_limiter,
                    &retry_delays[1..],
                    on_progress,
                )
                .await;
            }
            Err(err) => return Err(err),
        }
    }
    download_single_with_retry_inner(
        client,
        spec,
        limiter,
        item_limiter,
        retry_delays,
        on_progress,
    )
    .await
}

async fn download_single_with_retry_inner<F>(
    client: &reqwest::Client,
    spec: &DownloadSpec,
    limiter: Option<Arc<RateLimiter>>,
    item_limiter: Option<Arc<RateLimiter>>,
    retry_delays: &[Duration],
    mut on_progress: F,
) -> Result<DownloadOutcome, DownloadError>
where
    F: FnMut(u64) + Send,
{
    for attempt in 0..=retry_delays.len() {
        match download_single_with_limiters(
            client,
            spec,
            limiter.clone(),
            item_limiter.clone(),
            &mut on_progress,
        )
        .await
        {
            Ok(outcome) => return Ok(outcome),
            Err(err) if attempt < retry_delays.len() && is_retryable_error(&err) => {
                let delay = retry_delays[attempt];
                if !delay.is_zero() {
                    tokio::time::sleep(delay).await;
                }
            }
            Err(err) => return Err(err),
        }
    }
    Err(DownloadError::Status(0))
}

pub async fn download_single_from_response<F>(
    response: reqwest::Response,
    spec: &DownloadSpec,
    limiter: Option<Arc<RateLimiter>>,
    mut on_progress: F,
) -> Result<DownloadOutcome, DownloadError>
where
    F: FnMut(u64) + Send,
{
    download_single_from_response_with_item(response, spec, limiter, None, &mut on_progress).await
}

async fn download_single_from_response_with_item<F>(
    response: reqwest::Response,
    spec: &DownloadSpec,
    limiter: Option<Arc<RateLimiter>>,
    item_limiter: Option<Arc<RateLimiter>>,
    on_progress: &mut F,
) -> Result<DownloadOutcome, DownloadError>
where
    F: FnMut(u64) + Send,
{
    if spec.final_path.exists() {
        return Err(DownloadError::DestinationExists(spec.final_path.clone()));
    }
    if let Some(expected) = spec.expected_size {
        ensure_disk_space(&spec.temp_path, expected)?;
    }
    if let Some(parent) = spec.temp_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let file = tokio::fs::File::create(&spec.temp_path).await?;
    write_single_response(response, file, spec, limiter, item_limiter, on_progress).await
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
    download_single_with_limiters(client, spec, limiter, None, &mut on_progress).await
}

async fn download_single_with_limiters<F>(
    client: &reqwest::Client,
    spec: &DownloadSpec,
    limiter: Option<Arc<RateLimiter>>,
    item_limiter: Option<Arc<RateLimiter>>,
    on_progress: &mut F,
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
    if let Some(expected) = spec.expected_size {
        ensure_disk_space(&spec.temp_path, expected)?;
    }

    if let Some(parent) = spec.temp_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let file = tokio::fs::File::create(&spec.temp_path).await?;

    let response = apply_headers(client.get(spec.url.clone()), &spec.options)
        .send()
        .await?;
    write_single_response(response, file, spec, limiter, item_limiter, on_progress).await
}

async fn write_single_response<F>(
    response: reqwest::Response,
    mut file: tokio::fs::File,
    spec: &DownloadSpec,
    limiter: Option<Arc<RateLimiter>>,
    item_limiter: Option<Arc<RateLimiter>>,
    on_progress: &mut F,
) -> Result<DownloadOutcome, DownloadError>
where
    F: FnMut(u64) + Send,
{
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

    const WRITE_BUF_CAP: usize = 128 * 1024;
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
        if let Some(item_limiter) = &item_limiter {
            item_limiter.throttle(length).await;
        }
        write_buf.extend_from_slice(&chunk);
        if !write_buf.is_empty()
            && (downloaded == 0
                || write_buf.len() >= WRITE_BUF_CAP
                || last_notify.elapsed() >= std::time::Duration::from_millis(50)
                || expected.is_some_and(|known| {
                    downloaded.saturating_add(write_buf.len() as u64) >= known
                }))
        {
            let flushed = write_buf.len() as u64;
            file.write_all(&write_buf).await?;
            write_buf.clear();
            downloaded = downloaded.saturating_add(flushed);
            on_progress(downloaded);
            last_notify = std::time::Instant::now();
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

pub(crate) fn finalize_copy_new(temp: &Path, final_path: &Path) -> Result<(), DownloadError> {
    let mut dst = match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(final_path)
    {
        Ok(file) => file,
        Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
            return Err(DownloadError::DestinationExists(final_path.to_path_buf()));
        }
        Err(err) => return Err(err.into()),
    };
    let copy_res = (|| -> std::io::Result<()> {
        let mut src = std::fs::File::open(temp)?;
        std::io::copy(&mut src, &mut dst)?;
        dst.sync_all()?;
        Ok(())
    })();
    drop(dst);
    if let Err(err) = copy_res {
        let _ = std::fs::remove_file(final_path);
        return Err(err.into());
    }
    let _ = std::fs::remove_file(temp);
    Ok(())
}

/// Atomically moves `src` to `dst` without overwriting an existing destination,
/// falling back to a `create_new` copy + sync + cleanup across filesystems.
pub fn move_file_no_overwrite(src: &Path, dst: &Path) -> Result<(), DownloadError> {
    finalize_temp(src, dst)
}

pub(crate) fn finalize_temp(temp: &Path, final_path: &Path) -> Result<(), DownloadError> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        let src: Vec<u16> = temp
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let dst: Vec<u16> = final_path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let ok = unsafe {
            windows_sys::Win32::Storage::FileSystem::MoveFileExW(src.as_ptr(), dst.as_ptr(), 0)
        };
        if ok != 0 {
            return Ok(());
        }
        let err = std::io::Error::last_os_error();
        match err.raw_os_error() {
            Some(80 | 183) => Err(DownloadError::DestinationExists(final_path.to_path_buf())),
            Some(17) => finalize_copy_new(temp, final_path),
            _ => Err(err.into()),
        }
    }
    #[cfg(not(windows))]
    {
        match std::fs::hard_link(temp, final_path) {
            Ok(()) => {
                let _ = std::fs::remove_file(temp);
                Ok(())
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                Err(DownloadError::DestinationExists(final_path.to_path_buf()))
            }
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::CrossesDevices | std::io::ErrorKind::Unsupported
                ) || matches!(error.raw_os_error(), Some(1 | 18 | 95)) =>
            {
                finalize_copy_new(temp, final_path)
            }
            Err(error) => Err(error.into()),
        }
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

    #[test]
    fn fallback_copy_moves_without_overwriting_existing() {
        let dir = tempfile::tempdir().unwrap();
        let temp = dir.path().join("file.bin.lajufile");
        let final_path = dir.path().join("file.bin");
        std::fs::write(&temp, b"fresh-data").unwrap();
        finalize_copy_new(&temp, &final_path).unwrap();
        assert_eq!(std::fs::read(&final_path).unwrap(), b"fresh-data");
        assert!(!temp.exists());

        std::fs::write(&temp, b"second").unwrap();
        let err = finalize_copy_new(&temp, &final_path).unwrap_err();
        assert!(matches!(err, DownloadError::DestinationExists(_)));
        assert_eq!(std::fs::read(&final_path).unwrap(), b"fresh-data");
        assert!(temp.exists());
    }

    #[test]
    fn ensure_disk_space_rejects_excessive_requirement() {
        let dir = tempfile::tempdir().unwrap();
        let err =
            ensure_disk_space(&dir.path().join("missing-sub/file.bin"), u64::MAX).unwrap_err();
        assert!(matches!(err, DownloadError::InsufficientDiskSpace { .. }));
        ensure_disk_space(&dir.path().join("file.bin"), 1024).unwrap();
    }
}
