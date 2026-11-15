# LajuFile M6 - YouTube download Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Download YouTube videos into LajuFile with an IDM-like quality list, driven by managed `yt-dlp` + `ffmpeg`.

**Architecture:** LajuFile downloads two tools into `%APPDATA%\LajuFile\tools` on first use. Format listing and downloads shell out to `yt-dlp`, which reports progress on stdout. Video jobs are rows in the existing downloads table with `kind = video`, so the queue, pause and cancel reuse the current engine. The Edge extension gains a content script that asks the bridge for formats and posts the chosen one.

**Tech Stack:** Rust 2024, eframe/egui 0.37, tokio 1, reqwest 0.13, rusqlite 0.42, axum 0.8, yt-dlp + ffmpeg binaries (GPL, downloaded at runtime).

**Spec:** `docs/superpowers/specs/2026-11-15-lajufile-m5-m6-design.md`

## Global Constraints

- Windows 10/11 x64 only; no tool binaries committed to the repo.
- Tools live in `%APPDATA%\LajuFile\tools`; downloaded once, updatable from Settings.
- UI copy stays English, like the rest of the app.
- No new Cargo dependency unless it is already in `Cargo.lock`.
- `/video` and `/video/formats` use the same `x-lajufile-token` check, 64 KiB body limit and CORS layer as `/downloads`.
- FFmpeg merges video+audio above 720p; the selector must always add audio.

## Review Focus

- Missing or half-downloaded tools: the app shows a clear error and stays usable, never panics.
- Invalid yt-dlp JSON: 422 with no ghost row.
- Cancel or pause during a yt-dlp run: the child dies, `.part` files are cleaned, no final file appears.
- A requested height that does not exist (e.g. 1440p on a 1080p video): fall back to the best lower height, never fail.
- The page button must never cancel a browser download; only a queued row is created.

---

### Task 1: Managed tools

**Files:**
- Create: `crates/lajufile-app/src/tools.rs`
- Modify: `crates/lajufile-app/src/lib.rs`
- Modify: `crates/lajufile-app/src/engine.rs` (Command, progress message)
- Test: `crates/lajufile-app/src/tools.rs` unit tests

**Interfaces:**
- Produces: `pub fn tools_dir() -> PathBuf`; `pub fn tool_path(name: Tool) -> PathBuf`;
  `pub enum Tool { YtDlp, Ffmpeg }`; `pub fn tools_ready() -> bool`;
  `pub async fn download_tool(client: &reqwest::Client, tool: Tool, dir: &Path) -> Result<PathBuf, String>`.
- Produces: `Command::UpdateTools { reply: std::sync::mpsc::Sender<Result<(), String>> }`.

- [ ] **Step 1: Write the failing test** in `tools.rs`: `tools_ready_is_false_on_empty_dir(tmp)` and `true` after creating both files with any bytes.
- [ ] **Step 2: Run** `cargo test -p lajufile-app tools`; expect failure "cannot find function tools_ready".
- [ ] **Step 3: Implement** paths (`%APPDATA%\LajuFile\tools\yt-dlp.exe`, `ffmpeg.exe`), `tools_ready`, and `download_tool` (GET to `<path>.part`, stream to disk, rename on success, delete `.part` on error).
- [ ] **Step 4: Run** `cargo test -p lajufile-app tools`; expect PASS.
- [ ] **Step 5: Wire** `Command::UpdateTools` in the engine: message "Downloading tools..." then "Tools ready" or the error; no download when `tools_ready()`.
- [ ] **Step 6: Commit** `git add crates/lajufile-app/src/tools.rs crates/lajufile-app/src/lib.rs crates/lajufile-app/src/engine.rs && git commit -m "feat: manage yt-dlp and ffmpeg tools"`.

Pinned URLs (verify with a HEAD request before hardcoding):
- `https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp.exe`
- `https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip` (extract `bin/ffmpeg.exe`)

### Task 2: Format listing

**Files:**
- Create: `crates/lajufile-core/src/video.rs`
- Modify: `crates/lajufile-core/src/lib.rs`
- Test: `crates/lajufile-core/tests/video.rs`

**Interfaces:**
- Produces: `pub struct VideoFormat { pub id: String, pub label: String, pub ext: String, pub height: Option<u32>, pub filesize: Option<u64>, pub audio_only: bool }`
- Produces: `pub fn parse_formats(json: &str) -> Result<Vec<VideoFormat>, String>` (reads `title`, `formats[]`)
- Produces: `pub fn quality_choices(formats: &[VideoFormat]) -> Vec<(String, String)>` returning `(selector, label)` for best, 1440, 1080, 720, 480, 360, 240, audio only
- Produces: `pub fn selector_for_height(height: u32) -> String` = `bv*[height<=H]+ba/b[height<=H]`

- [ ] **Step 1: Write failing tests** with a small fixture JSON: parse two video formats + one audio format; `quality_choices` returns 8 entries; `selector_for_height(1080)` contains `height<=1080` and `+ba`.
- [ ] **Step 2: Run** `cargo test -p lajufile-core video`; expect failure.
- [ ] **Step 3: Implement** the parser with `serde_json::Value`; sort by height desc; audio only uses `bestaudio[ext=m4a]/bestaudio`.
- [ ] **Step 4: Run** the test; expect PASS.
- [ ] **Step 5: Commit** `feat: parse yt-dlp formats`.

### Task 3: Video job path

**Files:**
- Modify: `crates/lajufile-core/src/store.rs` (schema v2: `kind TEXT NOT NULL DEFAULT 'http'`, `video_format TEXT`; `NewDownload` and `Download` fields)
- Modify: `crates/lajufile-app/src/engine.rs` (`start_download` branch, yt-dlp child, progress, cancel)
- Modify: `crates/lajufile-app/src/bridge.rs` (`BridgeAddRequest.kind`, `video_format`)
- Test: `crates/lajufile-core/tests/store.rs`, `crates/lajufile-app/tests/engine.rs`

**Interfaces:**
- Produces: `Download.kind: String` (`http` | `video`), `Download.video_format: Option<String>`.
- Produces: `Command::VideoFormats { url: String, reply: tokio::sync::oneshot::Sender<Result<Vec<VideoFormat>, String>> }`.
- Produces: `VideoFormats` bridge endpoint in Task 5.

- [ ] **Step 1: Write failing store test:** opening an old v1 database adds the columns and defaults existing rows to `http`.
- [ ] **Step 2: Run** `cargo test -p lajufile-core store`; expect failure.
- [ ] **Step 3: Implement** the migration (`PRAGMA user_version`, `ALTER TABLE downloads ADD COLUMN ...`).
- [ ] **Step 4: Write failing engine test:** a row with `kind = video` and `video_format = bv*[height<=360]+ba` runs yt-dlp (use a stub executable script in a temp dir via `LAJUFILE_TOOLS_DIR`), reports progress and finishes at the target path.
- [ ] **Step 5: Implement** the video branch: spawn `yt-dlp.exe -f <selector> --merge-output-format mp4 --newline -o <temp> <url>`; parse `[download]  xx.x%` into `Event::Progress`; on exit 0 rename temp to `save_path`; on cancel kill the child and remove `.part`.
- [ ] **Step 6: Run** `cargo test -p lajufile-app --test engine`; expect PASS.
- [ ] **Step 7: Commit** `feat: download video jobs with yt-dlp`.

### Task 4: Quality dropdown in Add

**Files:**
- Modify: `crates/lajufile-app/src/ui/dialogs.rs` (`AddDraft.quality: Option<String>`, `AddDraft.formats`)
- Modify: `crates/lajufile-app/src/app.rs` (`Action::LoadFormats`, engine command, dialog wiring)
- Test: `crates/lajufile-app/src/ui/dialogs.rs` unit test for `youtube_url`

**Interfaces:**
- Produces: `pub fn youtube_url(url: &str) -> bool` (hosts `youtube.com`, `youtu.be`, `m.youtube.com`).
- Produces: `AddRequest.kind` (`http` default).

- [ ] **Step 1: Write failing test** for `youtube_url`.
- [ ] **Step 2: Run** it; expect failure.
- [ ] **Step 3: Implement** `youtube_url`, the dropdown (visible only for YouTube URLs), and `Action::LoadFormats` that sends `Command::VideoFormats`.
- [ ] **Step 4: Verify** `cargo test -p lajufile-app --lib` passes and the dialog compiles.
- [ ] **Step 5: Commit** `feat: choose a video quality in Add`.

### Task 5: Extension page button and bridge endpoints

**Files:**
- Create: `extension/content.js`
- Modify: `extension/manifest.json` (content script on `https://www.youtube.com/*`)
- Modify: `extension/background.js` (formats request, video post)
- Modify: `crates/lajufile-app/src/bridge.rs` (`POST /video/formats`, `POST /video`)
- Test: `crates/lajufile-app/tests/bridge.rs`

**Interfaces:**
- Consumes: `Command::VideoFormats`, `BridgeAddRequest.kind`.
- Produces: `POST /video/formats` body `{ url }` -> `200 { formats: [{ selector, label }] }`; `POST /video` body `{ url, video_format }` -> 201/409/422.

- [ ] **Step 1: Write failing bridge tests:** formats endpoint returns 200 with selectors for a stub yt-dlp; `/video` queues a `kind = video` row.
- [ ] **Step 2: Implement** the endpoints and the content script button next to the player (position absolute, small "Download with LajuFile" button) with a dropdown of formats.
- [ ] **Step 3: Run** `cargo test -p lajufile-app --test bridge` and `node --check extension/content.js extension/background.js`.
- [ ] **Step 4: Commit** `feat: add the youtube page button`.

### Task 6: Docs and verification

- [ ] **Step 1:** README section "Video downloads": tools download, first-use prompt, quality list, limitations (no subtitles, no playlists, no DRM, login-only videos fail).
- [ ] **Step 2:** `cargo fmt`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo build --release -p lajufile-app`.
- [ ] **Step 3:** Manual check in Edge: button appears on a watch page, a 360p download completes, pause/cancel work, Cancel leaves no file.
- [ ] **Step 4: Commit** `docs: document video downloads`.

## Open decisions for the builder

- Validate the tool URLs and whether ffmpeg ships as a zip (extract) or a single exe.
- Whether to send cookies from the browser for age-restricted videos (out of scope v1; document the failure).
- Timeout for a stalled yt-dlp run (suggest 10 minutes without progress).
