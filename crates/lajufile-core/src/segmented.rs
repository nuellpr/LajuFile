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

const MIN_SPLIT_BYTES: u64 = 512 * 1024;

pub struct SegmentedSpec {
    pub url: Url,
    pub temp_path: PathBuf,
    pub final_path: PathBuf,
    pub size: u64,
    pub segments: Vec<Segment>,
    pub options: RequestOptions,
    pub if_range: Option<String>,
    pub limiter: Arc<RateLimiter>,
    pub item_limiter: Option<Arc<RateLimiter>>,
    pub retry_delays: Vec<Duration>,
}

struct LiveSegment {
    idx: u32,
    start: u64,
    end: Arc<AtomicU64>,
    done: Arc<AtomicU64>,
    finished: bool,
}

type SegmentWork = (Segment, Arc<AtomicU64>, Arc<AtomicU64>);

struct SplitCoordinator {
    segments: Vec<LiveSegment>,
    next_idx: u32,
}

impl SplitCoordinator {
    fn new(segments: &[Segment]) -> (Self, Vec<SegmentWork>) {
        let mut live = Vec::with_capacity(segments.len());
        let mut initial_work = Vec::new();
        let mut next_idx = 0u32;
        for seg in segments {
            next_idx = next_idx.max(seg.idx.saturating_add(1));
            let length = seg.end - seg.start + 1;
            let finished = seg.done == length;
            let end_atomic = Arc::new(AtomicU64::new(seg.end));
            let done_atomic = Arc::new(AtomicU64::new(seg.done));
            live.push(LiveSegment {
                idx: seg.idx,
                start: seg.start,
                end: Arc::clone(&end_atomic),
                done: Arc::clone(&done_atomic),
                finished,
            });
            if !finished {
                initial_work.push((seg.clone(), end_atomic, done_atomic));
            }
        }
        (
            Self {
                segments: live,
                next_idx,
            },
            initial_work,
        )
    }

    /// Marks `finished_idx` complete and, if another active segment still has at
    /// least `2 * MIN_SPLIT_BYTES` remaining, splits its trailing half for the idle worker.
    fn finish_and_steal(&mut self, finished_idx: u32) -> Option<(SegmentWork, Vec<Segment>)> {
        if let Some(entry) = self.segments.iter_mut().find(|s| s.idx == finished_idx) {
            entry.finished = true;
        }
        let mut best: Option<(usize, u64, u64, u64)> = None;
        for (pos, seg) in self.segments.iter().enumerate() {
            if seg.finished {
                continue;
            }
            let done = seg.done.load(Ordering::Acquire);
            let end = seg.end.load(Ordering::Acquire);
            let cur_pos = seg.start.saturating_add(done);
            let remaining = end.saturating_add(1).saturating_sub(cur_pos);
            if remaining >= MIN_SPLIT_BYTES * 2 && best.is_none_or(|(_, rem, _, _)| remaining > rem)
            {
                best = Some((pos, remaining, cur_pos, end));
            }
        }
        let (donor_pos, remaining, cur_pos, old_end) = best?;
        let mid = cur_pos + remaining / 2;
        if mid <= cur_pos || mid > old_end {
            return None;
        }
        self.segments[donor_pos]
            .end
            .store(mid - 1, Ordering::Release);
        let idx = self.next_idx;
        self.next_idx = self.next_idx.saturating_add(1);
        let new_seg = Segment {
            idx,
            start: mid,
            end: old_end,
            done: 0,
        };
        let end_atomic = Arc::new(AtomicU64::new(old_end));
        let done_atomic = Arc::new(AtomicU64::new(0));
        self.segments.push(LiveSegment {
            idx,
            start: mid,
            end: Arc::clone(&end_atomic),
            done: Arc::clone(&done_atomic),
            finished: false,
        });
        let snapshot = self
            .segments
            .iter()
            .map(|s| Segment {
                idx: s.idx,
                start: s.start,
                end: s.end.load(Ordering::Acquire),
                done: s.done.load(Ordering::Acquire),
            })
            .collect();
        Some(((new_seg, end_atomic, done_atomic), snapshot))
    }
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
    download_segmented_with_splits(client, spec, on_progress, |_| {}).await
}

pub async fn download_segmented_with_splits<F, S>(
    client: &reqwest::Client,
    spec: &SegmentedSpec,
    on_progress: F,
    on_split: S,
) -> Result<DownloadOutcome, SegmentedError>
where
    F: FnMut(u32, u64, u64) + Send + 'static,
    S: FnMut(Vec<Segment>) + Send + 'static,
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

    if initial_done > 0 {
        let min_existing_len = spec
            .segments
            .iter()
            .filter(|segment| segment.done > 0)
            .map(|segment| segment.start.saturating_add(segment.done))
            .max()
            .unwrap_or(0);
        let temp_ok = tokio::fs::metadata(&spec.temp_path)
            .await
            .is_ok_and(|meta| {
                meta.is_file() && meta.len() >= min_existing_len && meta.len() <= spec.size
            });
        if !temp_ok {
            return Err(SegmentedError::RestartRequired);
        }
        if spec.if_range.is_none()
            && !prefix_matches(client, &spec.url, &spec.temp_path, spec.size, &spec.options).await
        {
            return Err(SegmentedError::RestartRequired);
        }
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
    let on_split = Arc::new(Mutex::new(on_split));
    let (coordinator, initial_work) = SplitCoordinator::new(&spec.segments);
    let coordinator = Arc::new(Mutex::new(coordinator));
    let mut tasks = JoinSet::new();

    for (segment, seg_end, seg_done) in initial_work {
        let client = client.clone();
        let url = spec.url.clone();
        let temp_path = spec.temp_path.clone();
        let options = spec.options.clone();
        let if_range = spec.if_range.clone();
        let limiter = spec.limiter.clone();
        let item_limiter = spec.item_limiter.clone();
        let retry_delays = spec.retry_delays.clone();
        let total_done = total_done.clone();
        let on_progress = on_progress.clone();
        let on_split = on_split.clone();
        let coordinator = coordinator.clone();
        let size = spec.size;

        tasks.spawn(async move {
            let mut current = (segment, seg_end, seg_done);
            loop {
                let finished_idx = current.0.idx;
                download_segment(
                    &client,
                    &url,
                    &temp_path,
                    &options,
                    if_range.as_deref(),
                    &limiter,
                    item_limiter.as_deref(),
                    current.0,
                    current.1,
                    current.2,
                    size,
                    &retry_delays,
                    &total_done,
                    &on_progress,
                )
                .await?;
                let stolen = coordinator
                    .lock()
                    .expect("split coordinator mutex poisoned")
                    .finish_and_steal(finished_idx);
                let Some((next_work, snapshot)) = stolen else {
                    break;
                };
                on_split.lock().expect("split callback mutex poisoned")(snapshot);
                current = next_work;
            }
            Ok::<(), SegmentedError>(())
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
    client: &reqwest::Client,
    url: &Url,
    temp_path: &Path,
    options: &RequestOptions,
    if_range: Option<&str>,
    limiter: &RateLimiter,
    item_limiter: Option<&RateLimiter>,
    segment: Segment,
    seg_end: Arc<AtomicU64>,
    seg_done: Arc<AtomicU64>,
    size: u64,
    retry_delays: &[Duration],
    total_done: &AtomicU64,
    on_progress: &Mutex<F>,
) -> Result<(), SegmentedError>
where
    F: FnMut(u32, u64, u64) + Send,
{
    let mut file = tokio::fs::OpenOptions::new()
        .write(true)
        .open(temp_path)
        .await
        .map_err(DownloadError::Io)?;
    let mut done = segment.done;

    for attempt in 0..=retry_delays.len() {
        let result = download_segment_attempt(
            client,
            url,
            options,
            if_range,
            limiter,
            item_limiter,
            &segment,
            &seg_end,
            &seg_done,
            size,
            &mut file,
            &mut done,
            total_done,
            on_progress,
        )
        .await;

        match result {
            Ok(()) => {
                file.flush().await.map_err(DownloadError::Io)?;
                file.sync_all().await.map_err(DownloadError::Io)?;
                let target_len = seg_end
                    .load(Ordering::Acquire)
                    .saturating_sub(segment.start)
                    .saturating_add(1);
                debug_assert_eq!(done, target_len);
                return Ok(());
            }
            Err(SegmentedError::Download(ref err))
                if attempt < retry_delays.len() && crate::download::is_retryable_error(err) =>
            {
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
    item_limiter: Option<&RateLimiter>,
    segment: &Segment,
    seg_end: &AtomicU64,
    seg_done: &AtomicU64,
    size: u64,
    file: &mut tokio::fs::File,
    done: &mut u64,
    total_done: &AtomicU64,
    on_progress: &Mutex<F>,
) -> Result<(), SegmentedError>
where
    F: FnMut(u32, u64, u64),
{
    let requested_end = seg_end.load(Ordering::Acquire);
    let requested_len = requested_end
        .saturating_sub(segment.start)
        .saturating_add(1);
    if *done >= requested_len {
        return Ok(());
    }
    let expected_start = segment.start + *done;

    file.seek(std::io::SeekFrom::Start(expected_start))
        .await
        .map_err(DownloadError::Io)?;

    let mut request = client
        .get(url.clone())
        .header(RANGE, format!("bytes={expected_start}-{requested_end}"));
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
    if range.total != Some(size) || range.start != expected_start || range.end != requested_end {
        return Err(DownloadError::ContentRange(raw.to_string()).into());
    }

    const WRITE_BUF_CAP: usize = 128 * 1024;
    let mut write_buf: Vec<u8> = Vec::with_capacity(WRITE_BUF_CAP);
    let mut last_notify = std::time::Instant::now();
    let mut response = response;
    while let Some(chunk) = response.chunk().await.map_err(DownloadError::Request)? {
        let current_end = seg_end.load(Ordering::Acquire);
        let target_len = current_end.saturating_sub(segment.start).saturating_add(1);
        let chunk_len = chunk.len() as u64;
        let next_done = done
            .saturating_add(write_buf.len() as u64)
            .saturating_add(chunk_len);
        if next_done > requested_len {
            return Err(DownloadError::SizeMismatch {
                expected: requested_len,
                actual: next_done,
            }
            .into());
        }
        let already = done.saturating_add(write_buf.len() as u64);
        let remaining_target = target_len.saturating_sub(already);
        if remaining_target == 0 {
            break;
        }
        let take = (chunk.len() as u64).min(remaining_target) as usize;
        limiter.throttle(take as u64).await;
        if let Some(item_lim) = item_limiter {
            item_lim.throttle(take as u64).await;
        }
        write_buf.extend_from_slice(&chunk[..take]);
        let current_target = seg_end
            .load(Ordering::Acquire)
            .saturating_sub(segment.start)
            .saturating_add(1);
        let reached_end = done.saturating_add(write_buf.len() as u64) >= current_target;
        if !write_buf.is_empty()
            && (*done == 0
                || write_buf.len() >= WRITE_BUF_CAP
                || last_notify.elapsed() >= std::time::Duration::from_millis(50)
                || reached_end)
        {
            let allowed = current_target.saturating_sub(*done) as usize;
            if write_buf.len() > allowed {
                write_buf.truncate(allowed);
            }
            if !write_buf.is_empty() {
                let flushed = write_buf.len() as u64;
                file.write_all(&write_buf)
                    .await
                    .map_err(DownloadError::Io)?;
                write_buf.clear();
                *done += flushed;
                seg_done.store(*done, Ordering::Release);
                let total = total_done.fetch_add(flushed, Ordering::Relaxed) + flushed;
                let mut callback = on_progress
                    .lock()
                    .expect("progress callback mutex poisoned");
                callback(segment.idx, *done, total);
                last_notify = std::time::Instant::now();
            }
        }
        if reached_end {
            break;
        }
    }
    let final_target = seg_end
        .load(Ordering::Acquire)
        .saturating_sub(segment.start)
        .saturating_add(1);
    if !write_buf.is_empty() {
        let allowed = final_target.saturating_sub(*done) as usize;
        if write_buf.len() > allowed {
            write_buf.truncate(allowed);
        }
        if !write_buf.is_empty() {
            let flushed = write_buf.len() as u64;
            file.write_all(&write_buf)
                .await
                .map_err(DownloadError::Io)?;
            write_buf.clear();
            *done += flushed;
            seg_done.store(*done, Ordering::Release);
            let total = total_done.fetch_add(flushed, Ordering::Relaxed) + flushed;
            let mut callback = on_progress
                .lock()
                .expect("progress callback mutex poisoned");
            callback(segment.idx, *done, total);
        }
    }

    if *done != final_target {
        return Err(DownloadError::SizeMismatch {
            expected: final_target,
            actual: *done,
        }
        .into());
    }
    Ok(())
}
