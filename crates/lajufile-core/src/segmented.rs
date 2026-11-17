//! Concurrent ranged downloads over a preallocated temporary file.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use reqwest::header::{CONTENT_RANGE, IF_RANGE, RANGE};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use tokio::task::JoinSet;
use url::Url;

use crate::download::{DownloadError, DownloadOutcome, finalize_temp};
use crate::http::{RequestOptions, apply_headers, ensure_identity_encoding, parse_content_range};
use crate::speed::RateLimiter;
use crate::store::Segment;

pub struct SegmentedSpec {
    pub url: Url,
    pub temp_path: PathBuf,
    pub final_path: PathBuf,
    pub size: u64,
    pub segments: Vec<Segment>,
    pub options: RequestOptions,
    pub if_range: Option<String>,
    pub limiter: Arc<RateLimiter>,
    pub retry_delays: Vec<Duration>,
}

#[derive(Debug, thiserror::Error)]
pub enum SegmentedError {
    #[error(transparent)]
    Download(#[from] DownloadError),
    #[error("server ignored Range and sent the full body")]
    RangeIgnored,
    #[error("resume validation required")]
    RestartRequired,
    #[error("invalid segments: {0}")]
    InvalidSegments(String),
}

/// Compare a local temp prefix with the first 64 KiB of the remote file.
/// Any request, status, range, encoding, or I/O problem counts as no match.
pub async fn prefix_matches(
    client: &reqwest::Client,
    url: &Url,
    temp: &Path,
    size: u64,
    options: &RequestOptions,
) -> bool {
    let length = size.min(64 * 1024);
    if length == 0 {
        return false;
    }

    let mut file = match tokio::fs::File::open(temp).await {
        Ok(file) => file,
        Err(_) => return false,
    };
    let mut prefix = vec![0_u8; length as usize];
    if file.read_exact(&mut prefix).await.is_err() {
        return false;
    }

    let request = client
        .get(url.clone())
        .header(RANGE, format!("bytes=0-{}", length - 1));
    let response = match apply_headers(request, options).send().await {
        Ok(response) => response,
        Err(_) => return false,
    };
    if response.status() != reqwest::StatusCode::PARTIAL_CONTENT {
        return false;
    }
    if ensure_identity_encoding(response.headers()).is_err() {
        return false;
    }
    let Some(raw) = response
        .headers()
        .get(CONTENT_RANGE)
        .and_then(|value| value.to_str().ok())
    else {
        return false;
    };
    let Some(range) = parse_content_range(raw) else {
        return false;
    };
    if range.start != 0 || range.end.checked_add(1) != Some(length) || range.total != Some(size) {
        return false;
    }

    let mut body = Vec::with_capacity(length as usize);
    let mut response = response;
    loop {
        match response.chunk().await {
            Ok(Some(chunk)) => {
                if body.len() + chunk.len() > length as usize {
                    return false;
                }
                body.extend_from_slice(&chunk);
            }
            Ok(None) => break,
            Err(_) => return false,
        }
    }
    body == prefix
}

pub async fn download_segmented<F>(
    client: &reqwest::Client,
    spec: &SegmentedSpec,
    on_progress: F,
) -> Result<DownloadOutcome, SegmentedError>
where
    F: FnMut(u32, u64, u64) + Send + 'static,
{
    match spec.url.scheme() {
        "http" | "https" => {}
        other => {
            return Err(DownloadError::UnsupportedScheme(other.to_string()).into());
        }
    }

    if spec.final_path.exists() {
        return Err(DownloadError::DestinationExists(spec.final_path.clone()).into());
    }

    validate_segments(spec.size, &spec.segments)?;
    let initial_done: u64 = spec.segments.iter().map(|segment| segment.done).sum();
    crate::download::ensure_disk_space(&spec.temp_path, spec.size.saturating_sub(initial_done))?;

    if initial_done > 0
        && spec.if_range.is_none()
        && !prefix_matches(client, &spec.url, &spec.temp_path, spec.size, &spec.options).await
    {
        return Err(SegmentedError::RestartRequired);
    }

    if let Some(parent) = spec.temp_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(DownloadError::Io)?;
    }
    let file = tokio::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(&spec.temp_path)
        .await
        .map_err(DownloadError::Io)?;
    file.set_len(spec.size).await.map_err(DownloadError::Io)?;
    drop(file);

    let total_done = Arc::new(AtomicU64::new(initial_done));
    let on_progress = Arc::new(Mutex::new(on_progress));
    let mut tasks = JoinSet::new();

    for segment in spec.segments.iter().cloned() {
        let length = segment.end - segment.start + 1;
        if segment.done == length {
            continue;
        }
        let client = client.clone();
        let url = spec.url.clone();
        let temp_path = spec.temp_path.clone();
        let options = spec.options.clone();
        let if_range = spec.if_range.clone();
        let limiter = spec.limiter.clone();
        let retry_delays = spec.retry_delays.clone();
        let total_done = total_done.clone();
        let on_progress = on_progress.clone();
        let size = spec.size;

        tasks.spawn(async move {
            download_segment(
                client,
                url,
                temp_path,
                options,
                if_range,
                limiter,
                segment,
                size,
                retry_delays,
                total_done,
                on_progress,
            )
            .await
        });
    }

    while let Some(joined) = tasks.join_next().await {
        match joined {
            Ok(Ok(())) => {}
            Ok(Err(error)) => {
                tasks.abort_all();
                while tasks.join_next().await.is_some() {}
                return Err(error);
            }
            Err(error) => {
                tasks.abort_all();
                while tasks.join_next().await.is_some() {}
                return Err(DownloadError::Io(std::io::Error::other(format!(
                    "segment task failed: {error}"
                )))
                .into());
            }
        }
    }

    let actual = tokio::fs::metadata(&spec.temp_path)
        .await
        .map_err(DownloadError::Io)?
        .len();
    if actual != spec.size {
        return Err(DownloadError::SizeMismatch {
            expected: spec.size,
            actual,
        }
        .into());
    }

    if spec.final_path.exists() {
        return Err(DownloadError::DestinationExists(spec.final_path.clone()).into());
    }
    finalize_temp(&spec.temp_path, &spec.final_path)?;

    Ok(DownloadOutcome {
        final_path: spec.final_path.clone(),
        bytes: spec.size,
    })
}

fn validate_segments(size: u64, segments: &[Segment]) -> Result<(), SegmentedError> {
    if size == 0 {
        return Err(SegmentedError::InvalidSegments("size is zero".to_string()));
    }
    if segments.is_empty() {
        return Err(SegmentedError::InvalidSegments(
            "no segments provided".to_string(),
        ));
    }

    let mut expected_start = 0_u64;
    for (position, segment) in segments.iter().enumerate() {
        if segment.idx != position as u32 {
            return Err(SegmentedError::InvalidSegments(format!(
                "segment index {} at position {position}",
                segment.idx
            )));
        }
        if segment.start != expected_start || segment.end < segment.start {
            return Err(SegmentedError::InvalidSegments(format!(
                "segment {position} has range {}-{}",
                segment.start, segment.end
            )));
        }
        let length = segment.end - segment.start + 1;
        if segment.done > length {
            return Err(SegmentedError::InvalidSegments(format!(
                "segment {position} has {} done of {length}",
                segment.done
            )));
        }
        expected_start = segment
            .end
            .checked_add(1)
            .ok_or_else(|| SegmentedError::InvalidSegments("range overflow".to_string()))?;
    }

    if expected_start != size {
        return Err(SegmentedError::InvalidSegments(format!(
            "segments cover up to {expected_start}, expected {size}"
        )));
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn download_segment<F>(
    client: reqwest::Client,
    url: Url,
    temp_path: PathBuf,
    options: RequestOptions,
    if_range: Option<String>,
    limiter: Arc<RateLimiter>,
    segment: Segment,
    size: u64,
    retry_delays: Vec<Duration>,
    total_done: Arc<AtomicU64>,
    on_progress: Arc<Mutex<F>>,
) -> Result<(), SegmentedError>
where
    F: FnMut(u32, u64, u64) + Send,
{
    let length = segment.end - segment.start + 1;
    let mut file = tokio::fs::OpenOptions::new()
        .write(true)
        .open(&temp_path)
        .await
        .map_err(DownloadError::Io)?;
    let mut done = segment.done;

    for attempt in 0..=retry_delays.len() {
        let result = download_segment_attempt(
            &client,
            &url,
            &options,
            if_range.as_deref(),
            &limiter,
            &segment,
            size,
            &mut file,
            &mut done,
            &total_done,
            &on_progress,
        )
        .await;

        match result {
            Ok(()) => {
                file.flush().await.map_err(DownloadError::Io)?;
                file.sync_all().await.map_err(DownloadError::Io)?;
                debug_assert_eq!(done, length);
                return Ok(());
            }
            Err(SegmentedError::Download(_)) if attempt < retry_delays.len() => {
                let delay = retry_delays[attempt];
                if !delay.is_zero() {
                    tokio::time::sleep(delay).await;
                }
            }
            Err(error) => return Err(error),
        }
    }

    Err(SegmentedError::Download(DownloadError::Status(0)))
}

#[allow(clippy::too_many_arguments)]
async fn download_segment_attempt<F>(
    client: &reqwest::Client,
    url: &Url,
    options: &RequestOptions,
    if_range: Option<&str>,
    limiter: &RateLimiter,
    segment: &Segment,
    size: u64,
    file: &mut tokio::fs::File,
    done: &mut u64,
    total_done: &AtomicU64,
    on_progress: &Mutex<F>,
) -> Result<(), SegmentedError>
where
    F: FnMut(u32, u64, u64),
{
    let length = segment.end - segment.start + 1;
    let expected_start = segment.start + *done;

    file.seek(std::io::SeekFrom::Start(expected_start))
        .await
        .map_err(DownloadError::Io)?;

    let mut request = client
        .get(url.clone())
        .header(RANGE, format!("bytes={expected_start}-{}", segment.end));
    if let Some(value) = if_range {
        request = request.header(IF_RANGE, value);
    }
    let response = apply_headers(request, options)
        .send()
        .await
        .map_err(DownloadError::Request)?;

    let status = response.status();
    if status == reqwest::StatusCode::OK {
        return Err(SegmentedError::RangeIgnored);
    }
    if status != reqwest::StatusCode::PARTIAL_CONTENT {
        return Err(DownloadError::Status(status.as_u16()).into());
    }
    if let Err(encoding) = ensure_identity_encoding(response.headers()) {
        return Err(DownloadError::UnsupportedEncoding(encoding).into());
    }

    let raw = response
        .headers()
        .get(CONTENT_RANGE)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| DownloadError::ContentRange("<missing>".to_string()))?;
    let range =
        parse_content_range(raw).ok_or_else(|| DownloadError::ContentRange(raw.to_string()))?;
    if range.total != Some(size) || range.start != expected_start || range.end != segment.end {
        return Err(DownloadError::ContentRange(raw.to_string()).into());
    }

    const WRITE_BUF_CAP: usize = 128 * 1024;
    let mut write_buf: Vec<u8> = Vec::with_capacity(WRITE_BUF_CAP);
    let mut last_notify = std::time::Instant::now();
    let mut response = response;
    while let Some(chunk) = response.chunk().await.map_err(DownloadError::Request)? {
        let chunk_len = chunk.len() as u64;
        let next_done = done
            .saturating_add(write_buf.len() as u64)
            .saturating_add(chunk_len);
        if next_done > length {
            return Err(DownloadError::SizeMismatch {
                expected: length,
                actual: next_done,
            }
            .into());
        }
        limiter.throttle(chunk_len).await;
        write_buf.extend_from_slice(&chunk);
        if !write_buf.is_empty()
            && (*done == 0
                || write_buf.len() >= WRITE_BUF_CAP
                || last_notify.elapsed() >= std::time::Duration::from_millis(50)
                || *done + (write_buf.len() as u64) >= length)
        {
            let flushed = write_buf.len() as u64;
            file.write_all(&write_buf)
                .await
                .map_err(DownloadError::Io)?;
            write_buf.clear();
            *done += flushed;
            let total = total_done.fetch_add(flushed, Ordering::Relaxed) + flushed;
            let mut callback = on_progress
                .lock()
                .expect("progress callback mutex poisoned");
            callback(segment.idx, *done, total);
            last_notify = std::time::Instant::now();
        }
    }
    if !write_buf.is_empty() {
        let flushed = write_buf.len() as u64;
        file.write_all(&write_buf)
            .await
            .map_err(DownloadError::Io)?;
        write_buf.clear();
        *done += flushed;
        let total = total_done.fetch_add(flushed, Ordering::Relaxed) + flushed;
        let mut callback = on_progress
            .lock()
            .expect("progress callback mutex poisoned");
        callback(segment.idx, *done, total);
    }

    if *done != length {
        return Err(DownloadError::SizeMismatch {
            expected: length,
            actual: *done,
        }
        .into());
    }
    Ok(())
}
