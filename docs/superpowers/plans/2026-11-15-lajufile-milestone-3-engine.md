# LajuFile Milestone 3 — Segmented Downloads, Resume, Speed Limit Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make LajuFile download large files with multiple byte-range connections, resume from the exact bytes already written, and enforce a global KB/s speed limit without ever finalizing a corrupt file.

**Architecture:** `lajufile-core` gains a segment planner, an async segmented downloader, a resume validator, and a shared token-bucket rate limiter. The app engine chooses segmented vs single-stream from the stored probe metadata, persists per-segment progress, falls back to single-stream when a server ignores ranges, and passes one global limiter to every active connection.

**Tech Stack:** Rust 2024, Tokio 1, reqwest 0.13, rusqlite 0.42, eframe/egui 0.37, `lajufile-core`.

**Spec:** `docs/superpowers/specs/2026-11-14-lajufile-design.md` — sections 8, 9, 10, 14, 15. GUI reference: https://www.figma.com/design/PASDzAEqJlF35dkv4L9eIx

## Global Constraints

- Windows 10/11 x64 only; build with the installed MSVC toolchain (`x86_64-pc-windows-msvc`).
- Never finalize a partial or mixed file: data goes to `<final>.lajufile`; finalization uses the existing no-replace hard-link path.
- Segment count is `clamp(size / 1 MiB, 1, max_connections)` only when the server supports ranges and the size is known. The last segment receives the remainder.
- Pause persists bytes actually written; resume validates the resource before appending. Without a validator, compare the first 64 KiB of the temp file with the server prefix; mismatch or unsupported ranges restarts from zero.
- If a server ignores `Range` during a segmented/resume request, fall back to a single stream from zero. Never keep writing at stale offsets.
- One global token bucket at `global_speed_limit_kbps` (0 = unlimited) is shared by every active connection; settings changes apply live.
- M3 scope only: segmented downloads, true resume, speed limit, and the UI controls for them. Out of scope: browser bridge/Edge capture, autostart, installer, scheduler, FTP, and GUI redesign.
- Verification for every task: `cargo test -p lajufile-core`, `cargo test -p lajufile-app`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --check`. Full workspace before handoff.

## Review Focus

These are the failure modes most likely to corrupt a file or mislead a real user, and the tests that pin them:

1. **Server ignores Range on resume** — a 200 response must abort segmented writes and restart single-stream; the final file must contain one complete response, never a mix. Covered in Task 3 (`rejects_range_ignored_response`) and Task 5 (`range_ignored_falls_back_to_single`).
2. **Stale validator or prefix mismatch** — `If-Range` returning 200, or a 64 KiB prefix that differs, must restart from zero instead of appending. Covered in Task 4 (`if_range_mismatch_returns_range_ignored`, `prefix_mismatch_returns_restart`).
3. **Pause mid-segment** — aggregate `downloaded` equals the sum of segment `done`; resume requests start at `start + done`, not at zero. Covered in Task 1 (`update_progress_aggregates_segments`) and Task 4 (`resume_uses_done_offsets`), plus Task 5 (`pause_and_resume_continue_from_offset`).
4. **Retry exhaustion** — after the configured retries a segment failure leaves the temp file, no final file, and an Error row; no retry loop. Covered in Task 3 (`retry_exhaustion_leaves_temp_and_no_final`).
5. **Speed limit zero/live change** — 0 KB/s never sleeps; a changed limit applies to subsequent throttles without deadlock. Covered in Task 2 (`unlimited_never_waits`, `set_kbps_applies_live`).

---

## File Structure

```
crates/lajufile-core/
  src/segment.rs             segment planner
  src/speed.rs               global token-bucket rate limiter
  src/segmented.rs           multi-connection downloader, resume validator
  src/download.rs            single-stream wrapper + shared limiter hook
  src/store/mod.rs           aggregate progress + replace segments
  src/lib.rs                 module exports
  tests/segment.rs           planner tests
  tests/segmented.rs         segmented/resume tests
  tests/support/mod.rs       ranged test server

crates/lajufile-app/
  src/engine.rs              segmented choice, fallback, progress, limiter
  src/ui/dialogs.rs          Add connections, Settings limits
  src/ui/mod.rs              AddDraft/AddRequest wiring
  tests/engine.rs            segmented, fallback, resume, connection tests
  tests/support/mod.rs       ranged app test server

README.md                    M3 status and limitations
```

---

### Task 1: Segment planner and aggregate progress

**Files:**
- Create: `crates/lajufile-core/src/segment.rs`
- Create: `crates/lajufile-core/tests/segment.rs`
- Modify: `crates/lajufile-core/src/lib.rs`
- Modify: `crates/lajufile-core/src/store/mod.rs`
- Test: `crates/lajufile-core/tests/store.rs`

**Interfaces:**
- Consumes: `lajufile_core::store::Segment`.
- Produces:
  - `pub const SEGMENT_BYTES: u64 = 1024 * 1024;`
  - `pub fn plan_segments(size: u64, ranged: bool, max_connections: u32) -> Vec<Segment>`
  - `Store::update_progress(&mut self, id: i64, segment_idx: u32, done: u64) -> Result<u64, StoreError>` (returns aggregate bytes)
  - `Store::replace_segments(&mut self, id: i64, segments: &[Segment], connections: u32, ranged: bool) -> Result<u64, StoreError>` (returns aggregate bytes)

- [ ] **Step 1: Write the failing planner tests**

```rust
// crates/lajufile-core/tests/segment.rs
use lajufile_core::segment::{SEGMENT_BYTES, plan_segments};

#[test]
fn plans_one_segment_for_small_or_non_ranged() {
    let small = plan_segments(512 * 1024, true, 8);
    assert_eq!(small.len(), 1);
    assert_eq!((small[0].start, small[0].end), (0, 512 * 1024 - 1));

    let plain = plan_segments(10 * SEGMENT_BYTES, false, 8);
    assert_eq!(plain.len(), 1);
    assert_eq!((plain[0].start, plain[0].end), (0, 10 * SEGMENT_BYTES - 1));
}

#[test]
fn splits_evenly_and_gives_remainder_to_last() {
    let segments = plan_segments(4 * SEGMENT_BYTES + SEGMENT_BYTES / 2, true, 8);
    assert_eq!(segments.len(), 4);
    assert_eq!((segments[0].start, segments[0].end), (0, 1_179_647));
    assert_eq!((segments[3].start, segments[3].end), (3_538_944, 4_718_591));
    assert!(segments.iter().all(|segment| segment.done == 0));
}

#[test]
fn clamps_connection_count_to_one_and_limit() {
    assert_eq!(plan_segments(8 * SEGMENT_BYTES, true, 0).len(), 1);
    assert_eq!(plan_segments(8 * SEGMENT_BYTES, true, 3).len(), 3);
    assert_eq!(plan_segments(8 * SEGMENT_BYTES, true, 64).len(), 8);
}

#[test]
fn zero_size_produces_one_empty_range() {
    let segments = plan_segments(0, true, 8);
    assert_eq!(segments.len(), 1);
    assert_eq!((segments[0].start, segments[0].end), (0, 0));
}
```

- [ ] **Step 2: Run the planner tests and verify they fail**

Run: `cargo test -p lajufile-core --test segment`
Expected: FAIL — unresolved import `lajufile_core::segment`.

- [ ] **Step 3: Implement `plan_segments` and export it**

Create `src/segment.rs` with the constant and function. Use `count = (size / SEGMENT_BYTES).clamp(1, max_connections.max(1) as u64) as u32`; `chunk = size / count as u64`; last `end = size - 1`; every other `end = start + chunk - 1`. Add `pub mod segment;` to `src/lib.rs`.

- [ ] **Step 4: Run the planner tests and verify they pass**

Run: `cargo test -p lajufile-core --test segment`
Expected: PASS, 4 tests.

- [ ] **Step 5: Write the failing store aggregate tests**

Append to `tests/store.rs`:

```rust
#[test]
fn update_progress_aggregates_segments() {
    let mut store = Store::open_in_memory().unwrap();
    let id = store.insert_download(&sample_download(vec![
        Segment { idx: 0, start: 0, end: 99, done: 0 },
        Segment { idx: 1, start: 100, end: 199, done: 0 },
    ])).unwrap();

    assert_eq!(store.update_progress(id, 0, 100).unwrap(), 100);
    assert_eq!(store.update_progress(id, 1, 50).unwrap(), 150);
    assert_eq!(store.get_download(id).unwrap().downloaded, 150);
    assert_eq!(store.segments(id).unwrap()[1].done, 50);
}

#[test]
fn replace_segments_resets_progress_and_metadata() {
    let mut store = Store::open_in_memory().unwrap();
    let id = store.insert_download(&sample_download(vec![
        Segment { idx: 0, start: 0, end: 99, done: 40 },
    ])).unwrap();

    let total = store.replace_segments(id, &[
        Segment { idx: 0, start: 0, end: 49, done: 10 },
        Segment { idx: 1, start: 50, end: 99, done: 5 },
    ], 2, true).unwrap();

    assert_eq!(total, 15);
    let download = store.get_download(id).unwrap();
    assert_eq!(download.connections, 2);
    assert!(download.ranged);
    assert_eq!(download.downloaded, 15);
    assert_eq!(store.segments(id).unwrap().len(), 2);
}
```

`sample_download` is the existing helper in `tests/store.rs`; reuse it and add a `segments` argument or build the `NewDownload` inline if the helper does not accept segments.

- [ ] **Step 6: Run the store tests and verify they fail**

Run: `cargo test -p lajufile-core --test store`
Expected: FAIL — `replace_segments` missing; the aggregate assertion fails while `update_progress` still writes only one segment's bytes.

- [ ] **Step 7: Implement aggregate progress and segment replacement**

In `Store::update_progress`, after updating the segment row, sum `done` for all segments of the id and write that sum to `downloads.downloaded`; return the sum. In `Store::replace_segments`, use one transaction: verify the download exists, delete old segments, insert the new ones, compute the sum, and update `connections`, `ranged`, and `downloaded`. Return `StoreError::NotFound` for an unknown id.

- [ ] **Step 8: Run the store and planner tests**

Run: `cargo test -p lajufile-core --test segment --test store`
Expected: PASS, including all previous store tests.

- [ ] **Step 9: Commit**

```bash
git add crates/lajufile-core/src/segment.rs crates/lajufile-core/src/lib.rs crates/lajufile-core/src/store/mod.rs crates/lajufile-core/tests/segment.rs crates/lajufile-core/tests/store.rs
git commit -m "feat: plan segments and aggregate download progress"
```

---

### Task 2: Global rate limiter

**Files:**
- Create: `crates/lajufile-core/src/speed.rs`
- Modify: `crates/lajufile-core/src/lib.rs`
- Modify: `crates/lajufile-core/src/download.rs`
- Modify: `crates/lajufile-core/Cargo.toml`
- Test: unit tests in `src/speed.rs`

**Interfaces:**
- Produces:
  - `pub struct RateLimiter`
  - `RateLimiter::new(kbps: u64) -> Self`
  - `RateLimiter::set_kbps(&self, kbps: u64)`
  - `RateLimiter::kbps(&self) -> u64`
  - `RateLimiter::throttle(&self, bytes: u64) -> impl Future<Output = ()>`
  - `pub async fn download_single_with_limiter<F>(client: &reqwest::Client, spec: &DownloadSpec, limiter: Option<Arc<RateLimiter>>, on_progress: F) -> Result<DownloadOutcome, DownloadError>`
- `download_single` remains unchanged as a wrapper passing `None`.

- [ ] **Step 1: Write the failing limiter tests**

In `src/speed.rs`:

```rust
#[tokio::test(start_paused = true)]
async fn unlimited_never_waits() {
    let limiter = RateLimiter::new(0);
    let start = tokio::time::Instant::now();
    limiter.throttle(10 * 1024 * 1024).await;
    assert!(start.elapsed().is_zero());
}

#[tokio::test(start_paused = true)]
async fn throttle_waits_for_tokens() {
    let limiter = RateLimiter::new(1); // 1 KB/s, one second of burst
    let start = tokio::time::Instant::now();
    limiter.throttle(1024).await; // consumes the initial burst immediately
    limiter.throttle(512).await;
    assert!(start.elapsed() >= Duration::from_millis(400));
}

#[test]
fn set_kbps_applies_live() {
    let limiter = RateLimiter::new(64);
    assert_eq!(limiter.kbps(), 64);
    limiter.set_kbps(0);
    assert_eq!(limiter.kbps(), 0);
}
```

Add `test-util` to the core dev-dependency Tokio features so `start_paused` is available:
`tokio = { version = "1", features = ["macros", "net", "rt-multi-thread", "test-util"] }`.

- [ ] **Step 2: Run the limiter tests and verify they fail**

Run: `cargo test -p lajufile-core speed`
Expected: FAIL — `RateLimiter` not found.

- [ ] **Step 3: Implement the token bucket**

Use `AtomicU64` for the live KB/s value and `std::sync::Mutex<Bucket>` for `{ tokens: f64, last: tokio::time::Instant }`. `throttle` returns immediately when `kbps == 0`; otherwise refill at `kbps * 1024` bytes per second, subtract `bytes`, and if the balance is negative sleep for `-tokens / rate`; repeat until the balance reaches zero. Never hold the mutex across `.await`.

- [ ] **Step 4: Add the single-stream limiter hook**

Add `download_single_with_limiter`; move the existing function body into it, call `limiter.throttle(chunk.len() as u64).await` before each `write_all`, and make `download_single` call it with `None`. Export `pub mod speed;` from `lib.rs`.

- [ ] **Step 5: Run the limiter and existing download tests**

Run: `cargo test -p lajufile-core speed download`
Expected: PASS; existing single-stream tests remain green.

- [ ] **Step 6: Commit**

```bash
git add crates/lajufile-core/src/speed.rs crates/lajufile-core/src/download.rs crates/lajufile-core/src/lib.rs crates/lajufile-core/Cargo.toml
git commit -m "feat: add global download rate limiter"
```

---

### Task 3: Segmented downloader

**Files:**
- Create: `crates/lajufile-core/src/segmented.rs`
- Create: `crates/lajufile-core/tests/segmented.rs`
- Modify: `crates/lajufile-core/src/lib.rs`
- Modify: `crates/lajufile-core/src/download.rs` (`finalize_temp` → `pub(crate)`)
- Modify: `crates/lajufile-core/tests/support/mod.rs`
- Test: `crates/lajufile-core/tests/segmented.rs`

**Interfaces:**
- Consumes: `Segment`, `DownloadOutcome`, `DownloadError`, `RateLimiter`, `RequestOptions`, `apply_headers`, `ensure_identity_encoding`, `parse_content_range`.
- Produces:
  - `pub struct SegmentedSpec { pub url: Url, pub temp_path: PathBuf, pub final_path: PathBuf, pub size: u64, pub segments: Vec<Segment>, pub options: RequestOptions, pub if_range: Option<String>, pub limiter: Arc<RateLimiter>, pub retry_delays: Vec<Duration> }`
  - `pub enum SegmentedError { Download(DownloadError), RangeIgnored, RestartRequired, InvalidSegments(String) }`
  - `pub async fn download_segmented<F>(client: &reqwest::Client, spec: &SegmentedSpec, on_progress: F) -> Result<DownloadOutcome, SegmentedError>`
    where `F: FnMut(u32, u64, u64) + Send + 'static`; arguments are `(segment_idx, segment_done, total_done)`.
- Task 3 rejects any resumed segment (`done > 0`) with `RestartRequired`; Task 4 adds validation.

- [ ] **Step 1: Write the failing segmented tests**

Use a 4 MiB deterministic payload and four equal segments. Add these tests to `tests/segmented.rs`:

```rust
#[tokio::test]
async fn downloads_all_segments_and_finalizes() {
    // /seg returns 206 + Content-Range + ETag for each range.
    // Verify final bytes equal payload, temp is gone, and progress ends at size.
}

#[tokio::test]
async fn rejects_range_ignored_response() {
    // /seg-ignore returns 200 for a Range request.
    // Expect SegmentedError::RangeIgnored; final absent, temp exists.
}

#[tokio::test]
async fn rejects_invalid_content_range() {
    // /seg-bad-range returns 206 with the wrong start.
    // Expect an error; final absent.
}

#[tokio::test]
async fn retries_segment_and_succeeds() {
    // /seg-flaky fails the first request for segment 1 with 500.
    // retry_delays = vec![Duration::ZERO]; expect success and one retry.
}

#[tokio::test]
async fn retry_exhaustion_leaves_temp_and_no_final() {
    // /seg-always-500; retry_delays = vec![Duration::ZERO, Duration::ZERO];
    // Expect error, temp exists, final absent.
}

#[tokio::test]
async fn refuses_existing_final() {
    // Create final before calling; expect DestinationExists and unchanged final.
}

#[tokio::test]
async fn resumed_segments_require_validation() {
    // One segment has done > 0, if_range = None; expect RestartRequired.
}
```

- [ ] **Step 2: Run the segmented tests and verify they fail**

Run: `cargo test -p lajufile-core --test segmented`
Expected: FAIL — unresolved import `segmented`.

- [ ] **Step 3: Add ranged routes to the core test server**

Extend `tests/support/mod.rs` with a 4 MiB payload and routes `/seg`, `/seg-ignore`, `/seg-bad-range`, `/seg-flaky`, `/seg-always-500`. `/seg` parses `Range: bytes=start-end`, returns 206 with `Content-Range`, `Content-Length`, and ETag `"seg-v1"`; `/seg-ignore` always returns 200; `/seg-bad-range` returns a 206 whose start is one byte late; `/seg-flaky` fails the first request for `start > 0` with 500; `/seg-always-500` returns 500. Keep existing routes.

- [ ] **Step 4: Implement the segmented downloader**

Validate segments are contiguous and cover `0..=size-1`; reject empty/zero-size plans with `InvalidSegments`. Reject an existing final path. Open the temp with create/write and `set_len(size)`. Return `RestartRequired` if any `done > 0` (Task 4 replaces this check). Use a `JoinSet`; each segment task opens its own file handle, seeks to `start + done`, loops retries with `retry_delays`, sends `Range: bytes={start+done}-{end}`, accepts only 206 with matching `start`, `end`, and `total == size`, rejects non-identity encoding, writes chunks at the current offset, calls the shared progress callback, and throttles through the limiter. On any task error abort all tasks and return the error. After all tasks finish, verify total bytes, finalize with the existing `finalize_temp`, and return `DownloadOutcome`.

Progress is shared with `Arc<Mutex<F>>`; total bytes use `AtomicU64`. Do not use `std::fs::rename`; reuse `finalize_temp`.

- [ ] **Step 5: Run the segmented tests and verify they pass**

Run: `cargo test -p lajufile-core --test segmented`
Expected: PASS for all seven tests. The retry tests must finish quickly (`Duration::ZERO`).

- [ ] **Step 6: Run the full core suite**

Run: `cargo test -p lajufile-core`
Expected: PASS, no regressions.

- [ ] **Step 7: Commit**

```bash
git add crates/lajufile-core/src/segmented.rs crates/lajufile-core/src/download.rs crates/lajufile-core/src/lib.rs crates/lajufile-core/tests/segmented.rs crates/lajufile-core/tests/support/mod.rs
git commit -m "feat: download segments concurrently"
```

---

### Task 4: Resume validation

**Files:**
- Modify: `crates/lajufile-core/src/segmented.rs`
- Modify: `crates/lajufile-core/tests/segmented.rs`
- Modify: `crates/lajufile-core/tests/support/mod.rs`

**Interfaces:**
- Produces:
  - `pub async fn prefix_matches(client: &reqwest::Client, url: &Url, temp: &Path, size: u64, options: &RequestOptions) -> bool`
  - `SegmentedSpec.if_range: Option<String>` is now honored on every range request.
- `download_segmented` behavior:
  - `done == 0`: no validation.
  - `done > 0` and `if_range = Some`: send `If-Range`; a 200 response is `RangeIgnored`.
  - `done > 0` and `if_range = None`: call `prefix_matches`; false is `RestartRequired`.

- [ ] **Step 1: Add failing resume tests**

Add to `tests/segmented.rs`:

```rust
#[tokio::test]
async fn resume_uses_done_offsets() {
    // Pre-fill the temp with the first 512 KiB of the payload.
    // Segment 0 has done = 512 KiB; segments 1..3 have done = 0.
    // Expect the server to receive Range bytes=524288-... and the final bytes to match.
}

#[tokio::test]
async fn prefix_mismatch_returns_restart() {
    // Pre-fill the temp with wrong bytes; if_range = None.
    // Expect RestartRequired; final absent.
}

#[tokio::test]
async fn if_range_mismatch_returns_range_ignored() {
    // /seg returns 200 when If-Range does not equal "seg-v1".
    // Spec if_range = Some("\"old\""); expect RangeIgnored.
}

#[tokio::test]
async fn prefix_matches_accepts_matching_and_rejects_plain_200() {
    // Direct calls: matching prefix true; /seg-ignore false.
}
```

- [ ] **Step 2: Run the resume tests and verify they fail**

Run: `cargo test -p lajufile-core --test segmented`
Expected: FAIL — `prefix_matches` missing; `restart`/`if_range` behavior not implemented.

- [ ] **Step 3: Replace the Task 3 validation placeholder**

The Task 3 test `resumed_segments_require_validation` asserted `RestartRequired` for any `done > 0`. Replace it with `resume_uses_done_offsets` (matching prefix) and `prefix_mismatch_returns_restart` (mismatching prefix); Task 4 must not leave a test that contradicts the new resume behavior.

- [ ] **Step 4: Implement `prefix_matches` and `If-Range`**

`prefix_matches` reads `min(64 KiB, size)` bytes from the temp file, sends `Range: bytes=0-{n-1}`, requires 206, `Content-Range` exactly `0..n-1` with total `size`, identity encoding, and a body equal to the file prefix; any other status, malformed range, encoding, I/O, or request error returns `false`. In `download_segmented`, replace the Task 3 `done > 0` rejection: if `done > 0` and `if_range` is `None`, call `prefix_matches` and return `RestartRequired` on false; if `if_range` is `Some`, add the `If-Range` header to every segment request and map a 200 response to `RangeIgnored`.

- [ ] **Step 5: Run the resume and segmented tests**

Run: `cargo test -p lajufile-core --test segmented`
Expected: PASS for all resume, segmented, retry, and range-ignored tests.

- [ ] **Step 6: Run the full core suite**

Run: `cargo test -p lajufile-core`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add crates/lajufile-core/src/segmented.rs crates/lajufile-core/tests/segmented.rs crates/lajufile-core/tests/support/mod.rs
git commit -m "feat: validate and resume segmented downloads"
```

---

### Task 5: Engine integration

**Files:**
- Modify: `crates/lajufile-app/src/engine.rs`
- Modify: `crates/lajufile-app/src/ui/mod.rs` (AddRequest field only)
- Modify: `crates/lajufile-app/src/ui/dialogs.rs` (compile-only field until Task 6)
- Modify: `crates/lajufile-app/tests/engine.rs`
- Modify: `crates/lajufile-app/tests/support/mod.rs`

**Interfaces:**
- Consumes: `plan_segments`, `download_segmented`, `SegmentedSpec`, `SegmentedError`, `RateLimiter`, `download_single_with_limiter`, `Store::replace_segments`.
- Produces:
  - `AddRequest { url, file_name, save_dir, start_now, connections: Option<u32> }`
  - `EngineState.limiter: Arc<RateLimiter>`
  - engine `Event::SegmentProgress { id, generation, idx, segment_done, total }`
  - engine `Event::Fallback { id, generation, reason }`
- Engine rules:
  - Add: `connections = request.connections.unwrap_or(settings.max_connections).clamp(1, 32)`; `segments = plan_segments(size, ranged, connections)`.
  - Start: segmented when `ranged && size > 0 && !segments.is_empty()`; otherwise single.
  - Resume: pass `if_range = download.etag.or(download.last_modified)`; let core handle prefix validation.
  - Fallback: on `RangeIgnored` or `RestartRequired`, remove the active entry, replace segments with one full range, set `connections=1`, `ranged=false`, status Queued, message, and pump. Retry once as a single stream; never fall back in a loop.
  - Settings: clamp `max_concurrent` 1..=10, `max_connections` 1..=32, `global_speed_limit_kbps` to 0..=1_000_000; save first, then apply; call `limiter.set_kbps`.
  - Remove the dead `cancelled` set; the active-generation guard is the real race guard. On Remove, abort and await the task before deleting the temp so Windows releases the file handle.

- [ ] **Step 1: Add failing engine tests**

Extend `tests/support/mod.rs` with `/seg.bin` (4 MiB, ranged, ETag), `/range0-only.bin` (206 only for `bytes=0-0`, 200 otherwise), and `/slow-seg.bin` (ranged, 4 MiB, 20 ms per 64 KiB chunk). Record every requested range start in a thread-safe vector and expose it from `TestServer`.

Add to `tests/engine.rs`:

```rust
#[test]
fn segmented_download_uses_multiple_ranges_and_completes() {
    // settings.max_connections = 4, add /seg.bin start_now, wait Done.
    // Assert final bytes, download.connections == 4, and at least 4 range requests.
}

#[test]
fn range_ignored_falls_back_to_single() {
    // Add /range0-only.bin start_now; wait Done.
    // Assert final bytes, ranged == false, connections == 1.
}

#[test]
fn pause_and_resume_continue_from_offset() {
    // Add /slow-seg.bin start_now; wait downloaded >= 256 KiB; Pause; wait Paused;
    // Resume; wait Done; assert final bytes and a later range start > 0.
}

#[test]
fn add_records_requested_connections() {
    // AddRequest connections = Some(2), /seg.bin; assert connections == 2,
    // segments == 2, and both rows are within the file.
}
```

Update existing `AddRequest` literals with `connections: None`.

- [ ] **Step 2: Run the engine tests and verify they fail**

Run: `cargo test -p lajufile-app --test engine`
Expected: FAIL to compile because `connections` and the segmented/fallback behavior do not exist.

- [ ] **Step 3: Implement engine segmented choice and fallback**

Build `SegmentedSpec` with `retry_delays = vec![2s, 5s, 10s]`, the shared limiter, and `if_range` from the row. Spawn `download_segmented`; on `RangeIgnored`/`RestartRequired` send `Event::Fallback`, on other errors convert to `DownloadError` and send `Finished`. Implement `reset_to_single` with `replace_segments` and one full segment. Persist `Event::SegmentProgress` through `update_progress` at most once per second; use the event total for speed. Use `download_single_with_limiter(..., Some(limiter))` for the single path.

- [ ] **Step 4: Implement settings and cleanup**

Create the limiter from startup settings; clamp startup values before the first `pump_queue`; update the limiter after a successful save. Remove `cancelled` and its `Finished` branch. In Remove, abort and `runtime.block_on(handle)` before removing the temp.

- [ ] **Step 5: Run the engine tests and verify they pass**

Run: `cargo test -p lajufile-app --test engine`
Expected: PASS for all old and new tests. The slow resume test must complete within the test timeout.

- [ ] **Step 6: Run the app and core suites**

Run: `cargo test -p lajufile-app && cargo test -p lajufile-core`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add crates/lajufile-app/src/engine.rs crates/lajufile-app/src/ui/mod.rs crates/lajufile-app/src/ui/dialogs.rs crates/lajufile-app/tests/engine.rs crates/lajufile-app/tests/support/mod.rs
git commit -m "feat: run segmented downloads and true resume in the engine"
```

---

### Task 6: UI controls, end-to-end verification, README

**Files:**
- Modify: `crates/lajufile-app/src/ui/dialogs.rs`
- Modify: `crates/lajufile-app/src/ui/mod.rs`
- Modify: `crates/lajufile-app/src/app.rs`
- Modify: `crates/lajufile-app/tests/engine.rs` (if any fixture needs the new field)
- Modify: `README.md`

**Interfaces:**
- `AddDraft` gains `connections: u32` (default 8); `into_request` sets `connections: Some(self.connections.clamp(1, 32))`.
- Settings dialog enables `max_connections` (1..=32) and `global_speed_limit_kbps` (0..=1_000_000, 0 = unlimited).
- `normalize_settings` clamps both; engine repeats the clamp.

- [ ] **Step 1: Write the failing UI draft/settings tests**

Update the existing `add_draft_keeps_non_empty_values` test to assert `request.connections == Some(8)`, and add:

```rust
#[test]
fn add_draft_clamps_connections() {
    let request = AddDraft { connections: 99, ..AddDraft::default() }.into_request();
    assert_eq!(request.connections, Some(32));
}

#[test]
fn normalize_settings_clamps_connections_and_speed() {
    let mut settings = Settings { max_connections: 0, global_speed_limit_kbps: 2_000_000, ..Settings::default() };
    normalize_settings(&mut settings);
    assert_eq!(settings.max_connections, 1);
    assert_eq!(settings.global_speed_limit_kbps, 1_000_000);
}
```

- [ ] **Step 2: Run the UI tests and verify they fail**

Run: `cargo test -p lajufile-app --lib`
Expected: FAIL to compile because `AddDraft.connections` does not exist.

- [ ] **Step 3: Implement the controls**

Add a `Connections` slider to the Add dialog and set `AddDraft.connections`; remove the “arrives in M3” notes; enable the Settings connections slider and speed-limit drag value; update the explanatory text to say ranges are used when the server supports them. Keep the dialog compact (`default_height(0.0)`).

- [ ] **Step 4: Update README for M3**

State: segmented downloads, true resume with `If-Range`/64 KiB prefix validation, global speed limit, M2/M3 done, M4 capture next; list the remaining limits (no bridge, no autostart, no installer, pause granularity is about one second). Link this plan.

- [ ] **Step 5: Run the full verification**

```powershell
cargo fmt
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --check
cargo build --release -p lajufile-app
```

Expected: all tests pass; clippy/fmt clean; release binary builds.

- [ ] **Step 6: Manual smoke test**

Run the app and a local ranged 4 MiB server. Add the file, verify it completes and the final bytes are correct; pause around 50%, resume, and confirm the server logs a range start greater than zero; set a 256 KB/s limit and confirm the speed drops; set 0 and confirm it recovers. Close to tray and reopen from the tray.

- [ ] **Step 7: Commit**

```bash
git add crates/lajufile-app/src crates/lajufile-app/tests README.md
git commit -m "feat: expose segmented downloads and speed limit in the UI"
```

---

## Self-Review

- **Spec coverage:** segment planning, concurrent ranges, validation of 206/Content-Range, `If-Range`, 64 KiB prefix fallback, retry/backoff, pause progress, true resume, range-ignore fallback, global speed limit, Add/Settings UI, and final no-replace finalization are each assigned to a task.
- **Step scan:** every implementation step names the file and signature; every test step names the behavior and expected result. No TBD or “handle edge cases” lines.
- **Type consistency:** `Segment` uses `end` inclusive throughout; `update_progress` returns `u64`; `SegmentedSpec` field names are used unchanged in engine and UI tasks; `AddRequest.connections` is `Option<u32>` in both producer and consumer.
- **Review Focus:** each of the five failure modes has a named test in Task 1–5.
- **Proportion:** code blocks are limited to tests and signatures; the implementation body is described only where the algorithm is not determined by the signature.

## Execution Handoff

Plan saved to `docs/superpowers/plans/2026-11-15-lajufile-milestone-3-engine.md`. Review it, then choose Native (recommended: the segment/store/engine interfaces are tightly coupled and the core tests are the fastest feedback loop) or Subagent-driven. Do not merge or push M2 before that decision.
