# LajuFile Milestone 2 — GUI + Tray Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship a usable Windows desktop app for LajuFile: a windowed download list backed by a headless engine thread, with Add URL, queueing, pause/resume/remove, settings, tray, and close-to-tray.

**Architecture:** `crates/lajufile-app` is a new eframe/egui binary on top of `lajufile-core`. The GUI stays on the main thread. A dedicated engine thread owns `Store`, `Settings`, the reqwest client, and Tokio downloads; the GUI sends `Command`s and receives `Snapshot`s over `std::sync::mpsc`. M2 stays single-stream: `connections` is displayed but segmented downloading and true resume arrive in M3.

**Tech Stack:** Rust 2024, eframe/egui 0.37 (glow), egui_extras 0.37, tray-icon 0.26, Tokio 1, reqwest 0.13, rusqlite 0.42, `lajufile-core`.

**Spec:** `docs/superpowers/specs/2026-11-14-lajufile-design.md` — sections 8, 10, 13, 14, 15. GUI reference: https://www.figma.com/design/PASDzAEqJlF35dkv4L9eIx

## Global Constraints

- Windows 10/11 x64 only; build with the installed MSVC toolchain (`x86_64-pc-windows-msvc`).
- Never leave a half-written final file: downloads go to `<final>.lajufile` and core finalizes with no-replace semantics.
- The engine thread is the only owner of `Store`; the GUI never opens SQLite.
- Data stays in `%APPDATA%\LajuFile`: `lajufile.db`, `settings.json`.
- UI language is English. Light theme is the default; dark theme is a toggle.
- M2 scope only: window, sidebar/table, Add URL, settings, queue with `max_concurrent`, pause/resume (restart from zero), remove, tray, close-to-tray, light/dark.
- Out of scope for M2: segmented/multi-connection downloads, true byte-range resume, speed limiting, bridge/Edge capture, autostart, single-instance mutex, custom title bar, installer, logging crate.
- Verification for every task: `cargo test -p lajufile-app`, `cargo clippy -p lajufile-app --all-targets -- -D warnings`, `cargo fmt --check`. Full workspace before handoff.

## Review Focus

These are the failure modes most likely to hurt a real user, and the tests that pin them:

1. **Probe failure must not create a ghost download** — a dead/404/timeout URL leaves zero records. Covered in Task 2 (`add_missing_url_keeps_store_empty`).
2. **Pause/remove race with completion** — a task that finishes just after Pause/Remove must not flip the row back to Done or resurrect a deleted record. Covered in Task 3 (`pause_wins_over_late_finish_and_resume_completes`).
3. **Queue over-subscription** — with `max_concurrent = 1`, two adds must produce one `Downloading` and one `Queued`, never two `Downloading`. Covered in Task 3 (`queue_never_exceeds_max_concurrent`).
4. **Close-to-tray with no tray** — if tray creation fails, closing must exit instead of hiding an unreachable window. Covered in Task 6 (`should_hide_on_close` unit test).
5. **Active duplicate** — the same URL already queued/downloading/paused must not create a second record or temp file. Covered in Task 2 (`add_duplicate_active_is_rejected`).
6. **Invalid settings values** — `max_concurrent = 0` would stall the queue forever; the UI clamps values and Task 5 tests `normalize_settings`. Settings file writes remain atomic in core; a failed engine save surfaces as a toast and keeps the previous in-memory settings.

---

## File Structure

```
crates/lajufile-app/
  Cargo.toml
  src/main.rs              entry point, window options, icon
  src/lib.rs               module exports (binary uses the crate)
  src/app.rs               LajuFileApp state, update loop, actions
  src/engine.rs            engine thread, Command/Snapshot, queue, downloads
  src/tray.rs              tray icon, menu, TrayEvent
  src/ui/mod.rs            Filter, Action, shared UI helpers
  src/ui/main_window.rs    menu bar, toolbar, sidebar, table, status bar
  src/ui/dialogs.rs        Add download and Settings windows
  src/ui/format.rs         bytes/ETA/time formatting, status labels
  src/ui/theme.rs          light/dark visuals
  tests/engine.rs          engine integration tests
  tests/support/mod.rs     local axum test server
```

Modified: `Cargo.toml` (workspace member), `Cargo.lock`, `README.md` (M2 status).

---

### Task 1: App crate and window shell

**Files:**
- Modify: `Cargo.toml`
- Create: `crates/lajufile-app/Cargo.toml`
- Create: `crates/lajufile-app/src/main.rs`
- Create: `crates/lajufile-app/src/lib.rs`
- Create: `crates/lajufile-app/src/app.rs`
- Create: `crates/lajufile-app/src/ui/mod.rs`

**Interfaces:**
- Produces: binary `lajufile`, app type `lajufile_app::app::LajuFileApp`.

- [ ] **Step 1: Create the milestone branch and commit this plan**

```powershell
git switch main
git pull
git switch -c milestone-2
git add docs/superpowers/plans/2026-11-14-lajufile-milestone-2-gui-tray.md
git commit -m "docs: add milestone 2 GUI and tray plan"
```

- [ ] **Step 2: Add the workspace member and dependencies**

Add `"crates/lajufile-app"` to `[workspace] members` and these to `[workspace.dependencies]`:

```toml
eframe = { version = "0.37", default-features = false, features = ["default_fonts", "glow", "clipboard", "links"] }
egui = "0.37"
egui_extras = "0.37"
tray-icon = "0.26"
```

Create `crates/lajufile-app/Cargo.toml`:

```toml
[package]
name = "lajufile-app"
version.workspace = true
edition.workspace = true
license.workspace = true
rust-version.workspace = true

[[bin]]
name = "lajufile"
path = "src/main.rs"

[dependencies]
lajufile-core = { path = "../lajufile-core" }
eframe.workspace = true
egui.workspace = true
egui_extras.workspace = true
tray-icon.workspace = true
reqwest.workspace = true
thiserror.workspace = true
tokio.workspace = true
url.workspace = true

[dev-dependencies]
axum = "0.8"
tempfile = "3"
tokio = { version = "1", features = ["macros", "net", "rt-multi-thread", "time"] }
tokio-stream = "0.1"
```

- [ ] **Step 3: Write the minimal app**

`src/main.rs` decodes `assets/icons/icon-32.png` with `eframe::icon_data::from_png_bytes`, sets a 1440×900 viewport with minimum 1024×640, installs the icon, and calls `eframe::run_native` with `LajuFileApp::new`.

`src/app.rs` starts as:

```rust
pub struct LajuFileApp;

impl LajuFileApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        cc.egui_ctx.set_visuals(egui::Visuals::light());
        Self
    }
}

impl eframe::App for LajuFileApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| ui.heading("LajuFile"));
    }
}
```

`src/lib.rs` exports `app` and `ui`. `src/ui/mod.rs` is empty except a module comment for now.

- [ ] **Step 4: Verify it builds and opens**

Run: `cargo check -p lajufile-app`
Expected: exit 0.

Run: `cargo run -p lajufile-app`
Expected: a 1440×900 native window titled `LajuFile` with the LajuFile icon. Close it manually.

- [ ] **Step 5: Commit**

```powershell
git add Cargo.toml Cargo.lock crates/lajufile-app
git commit -m "feat: scaffold LajuFile desktop app"
```

---

### Task 2: Engine thread, probe, and insert

**Files:**
- Create: `crates/lajufile-app/src/engine.rs`
- Modify: `crates/lajufile-app/src/lib.rs`
- Create: `crates/lajufile-app/tests/engine.rs`
- Create: `crates/lajufile-app/tests/support/mod.rs`

**Interfaces:**
- Consumes: `lajufile_core::{probe, http, naming, paths, settings, store}`.
- Produces:

```rust
pub struct EngineConfig {
    pub data_dir: PathBuf,
    pub db_path: PathBuf,
    pub settings: Settings,
}

pub struct EngineHandle { /* command sender, snapshot receiver, join handle */ }

impl EngineHandle {
    pub fn start(config: EngineConfig) -> Result<Self, EngineError>;
    pub fn send(&self, command: Command) -> Result<(), EngineError>;
    pub fn latest_snapshot(&self) -> Option<Snapshot>;
    pub fn shutdown(self);
}

pub enum Command {
    Add(AddRequest),
    Pause(i64),
    Resume(i64),
    Remove(i64),
    PauseAll,
    ResumeAll,
    UpdateSettings(Settings),
    Shutdown,
}

pub struct AddRequest {
    pub url: String,
    pub file_name: Option<String>,
    pub save_dir: Option<PathBuf>,
    pub start_now: bool,
}

pub struct Snapshot {
    pub downloads: Vec<Download>,
    pub settings: Settings,
    pub speeds: HashMap<i64, u64>,
    pub message: Option<String>,
}
```

- [ ] **Step 1: Write the test server and failing tests**

`tests/support/mod.rs` starts axum on `127.0.0.1:0` and returns a base URL plus a shutdown handle. Routes:
- `GET /file.bin` → 200, `Content-Length: 1048576`, `Content-Disposition: attachment; filename="file.bin"`, body 1 MiB of `b"LajuFile"` repeated.
- `GET /missing` → 404.

`tests/engine.rs` adds:

```rust
#[test]
fn add_probes_and_inserts() {
    let server = start_server();
    let dir = tempfile::tempdir().unwrap();
    let settings = Settings { download_dir: dir.path().join("downloads"), ..Settings::default() };
    let handle = EngineHandle::start(EngineConfig {
        data_dir: dir.path().to_path_buf(),
        db_path: dir.path().join("lajufile.db"),
        settings,
    }).unwrap();

    handle.send(Command::Add(AddRequest {
        url: format!("{}/file.bin", server.base),
        file_name: None,
        save_dir: None,
        start_now: false,
    })).unwrap();

    let snap = wait_for(&handle, |s| s.downloads.len() == 1, Duration::from_secs(5));
    assert_eq!(snap.downloads[0].file_name, "file.bin");
    assert_eq!(snap.downloads[0].status, DownloadStatus::Queued);

    handle.shutdown();
    let store = Store::open(&dir.path().join("lajufile.db")).unwrap();
    assert_eq!(store.list_downloads().unwrap().len(), 1);
}
```

Add `add_missing_url_keeps_store_empty`: add `${base}/missing`, wait until `snapshot.message` contains `"failed"`, assert `downloads.is_empty()`, then shutdown and assert the store is empty.

Add `add_duplicate_active_is_rejected`: add `/file.bin` with `start_now: false`, wait for one record, add the same URL again, wait for a message containing `"Already"`, and assert the snapshot still has exactly one download with the same id.

A `wait_for(handle, predicate, timeout)` helper polls `latest_snapshot()` every 25 ms.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p lajufile-app --test engine`
Expected: compile failure because `EngineHandle`, `Command`, and `AddRequest` do not exist.

- [ ] **Step 3: Implement the engine skeleton**

In `engine.rs`:
- Spawn a named thread; build a multi-thread Tokio runtime inside it.
- Create `std::sync::mpsc` channels for `Command`, `Event`, and `Snapshot`.
- `Store::open(&config.db_path)`; build the reqwest client with `build_client(Duration::from_secs(10), Duration::from_secs(30))`.
- On startup, change rows with status `Downloading` to `Paused`; if `settings.auto_resume` is true, set them to `Queued` instead. Leave queued rows queued.
- Loop: drain commands, drain events, publish a snapshot at most every 250 ms, sleep 25 ms. `Shutdown` breaks the loop.
- `latest_snapshot` drains the receiver and returns the last snapshot.

Add flow:
- Parse `AddRequest.url` with `Url::parse`; on failure publish `Add failed: ...`.
- Reject if `store.active_by_url(url.as_str())` is `Some`; publish `Already in LajuFile`.
- Spawn a probe with `ProbeRequest { url, options: RequestOptions { user_agent: Some(DEFAULT_USER_AGENT.into()), referrer: None, cookie: None }, timeout: Duration::from_secs(20) }`.
- On `ProbeResult`, resolve the name with `resolve_file_name(request.file_name.as_deref(), result.content_disposition.as_deref(), &result.final_url)`, resolve the directory from `request.save_dir` or `settings.download_dir`, create it, dedupe with `dedupe_path`, and use `temp_path_for`.
- Insert one `NewDownload` with status `Queued`, `connections: 1`, `ranged: result.ranged`, `size: result.size`, one `Segment { idx: 0, start: 0, end: result.size.unwrap_or(1).saturating_sub(1), done: 0 }`, `queue_order: store.next_queue_order()?`, and `now_millis()`.
- Publish `Added <file_name>` (or `Add failed: <probe error>`). `start_now` is accepted but only takes effect in Task 3.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p lajufile-app --test engine`
Expected: 3 tests pass.

Run: `cargo clippy -p lajufile-app --all-targets -- -D warnings`
Expected: exit 0.

- [ ] **Step 5: Commit**

```powershell
git add crates/lajufile-app/src/engine.rs crates/lajufile-app/src/lib.rs crates/lajufile-app/tests
git commit -m "feat: add headless download engine"
```

---

### Task 3: Download execution, queue, pause/resume/remove

**Files:**
- Modify: `crates/lajufile-app/src/engine.rs`
- Modify: `crates/lajufile-app/tests/engine.rs`
- Modify: `crates/lajufile-app/tests/support/mod.rs`

**Interfaces:**
- Consumes: Task 2 `EngineHandle`, `Command`, `Snapshot`.
- Produces: `start_now` now downloads; Pause/Resume/Remove/PauseAll/ResumeAll and `max_concurrent` work. No new public types.

- [ ] **Step 1: Add failing tests**

Add a `/slow.bin` route: 200, `Content-Length: 1048576`, and a 400 ms delay before the body. Add:

- `add_start_now_downloads_file`: settings `max_concurrent: 1`; add `/file.bin` with `start_now: true`; wait for status `Done`; assert the final file exists with 1 MiB, the `.lajufile` temp is gone, and after shutdown the store row is `Done`.
- `queue_never_exceeds_max_concurrent`: `max_concurrent: 1`; add two `/slow.bin` URLs with different `file_name` values and `start_now: true`; wait until the snapshot has at least one `Downloading`; assert exactly one `Downloading` and one `Queued`; wait until both are `Done`.
- `pause_wins_over_late_finish_and_resume_completes`: add `/slow.bin`; wait for `Downloading`; send `Pause(id)`; wait for `Paused`; sleep 600 ms; assert the row is still `Paused` (late-finish guard); send `Resume(id)`; wait for `Done`; assert the final file exists.
- `remove_deletes_record_and_temp`: add `/slow.bin`; wait for `Downloading`; send `Remove(id)`; wait for zero rows; sleep 600 ms; assert still zero rows; shutdown; assert the store is empty and the temp path no longer exists.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p lajufile-app --test engine`
Expected: the download tests time out or fail because no task is started.

- [ ] **Step 3: Implement downloads and the queue**

- `pump_queue`: while `active.len() < settings.max_concurrent.max(1)`, take the first `Queued` row by queue order and start it.
- `start_download(download)`:
  - Assign a new `generation: u64` from a counter; set `Downloading`; build `DownloadSpec` from the row (`expected_size: download.size`, UA `DEFAULT_USER_AGENT`, referrer from the row, no cookie).
  - Spawn `download_single(&client, spec, |done| { let _ = events.send(Event::Progress { id, generation, done }); })`.
  - Store `(generation, JoinHandle)` in `active`.
- `Event::Progress { id, generation, done }`: ignore if `generation` does not match the active entry; update in-memory progress and speed; persist with `update_progress(id, 0, done)` at most once per second per id.
- `Event::Finished { id, generation, result }`: if the generation does not match the active entry, or `cancelled.remove(&(id, generation))` is true, ignore it. Otherwise call `mark_done` on success or `update_status(Error, Some(&err.to_string()))` on failure, remove `active`, and `pump_queue`.
- `Pause`: if active, remove `(generation, handle)`, insert `(id, generation)` into `cancelled`, and call `abort()`; set status `Paused` for active or queued rows; no-op for `Done`.
- `Resume`: for `Paused` or `Error`, set `Queued` and `pump_queue`; the new start gets a new generation, so a late event from the old one is ignored. This restarts the transfer from zero; do not claim byte-range resume.
- `Remove`: abort and insert `(id, generation)` into `cancelled` if active; `delete_download`; remove the temp file best-effort; keep a completed final file.
- `PauseAll` / `ResumeAll`: apply the same rules to every active/queued or paused/error row.
- `UpdateSettings`: normalize `max_concurrent` to 1..=10, save with `settings.save(&config.data_dir)`; on error publish `Settings save failed: ...` and keep the old in-memory settings; on success update the engine settings and `pump_queue`.
- On engine startup, call `pump_queue` so rows left `Queued` by a previous session start automatically.
- Publish a snapshot after every event and at least every 250 ms. Speed is bytes-per-second from progress deltas; zero when idle.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p lajufile-app --test engine`
Expected: 7 tests pass.

- [ ] **Step 5: Commit**

```powershell
git add crates/lajufile-app/src/engine.rs crates/lajufile-app/tests
git commit -m "feat: run and queue downloads in the engine"
```

---

### Task 4: Main window

**Files:**
- Create: `crates/lajufile-app/src/ui/main_window.rs`
- Create: `crates/lajufile-app/src/ui/format.rs`
- Modify: `crates/lajufile-app/src/ui/mod.rs`
- Modify: `crates/lajufile-app/src/app.rs`

**Interfaces:**
- Consumes: `EngineHandle`, `Snapshot`, `Download`, `DownloadStatus`, `Category`.
- Produces:

```rust
pub enum Filter { All, Unfinished, Finished, Category(Category) }
impl Filter {
    pub fn matches(self, download: &Download) -> bool;
    pub fn label(self) -> &'static str;
}

pub enum Action {
    Select(i64),
    Pause(i64),
    Resume(i64),
    Remove(i64),
    Open(i64),
    OpenFolder(i64),
    CopyUrl(String),
    OpenAdd,
    OpenSettings,
    PauseAll,
    ResumeAll,
}

pub fn draw(app: &mut LajuFileApp, ctx: &egui::Context) -> Vec<Action>;
```

- [ ] **Step 1: Write failing format/filter tests**

In `src/ui/format.rs`:

```rust
#[test]
fn formats_sizes() {
    assert_eq!(format_size(0), "0 B");
    assert_eq!(format_size(1536), "1.5 KB");
    assert_eq!(format_size(4_700_000_000), "4.4 GB");
}

#[test]
fn formats_eta() {
    assert_eq!(format_eta(None), "—");
    assert_eq!(format_eta(Some(0)), "—");
    assert_eq!(format_eta(Some(125)), "2m 5s");
}
```

In `src/ui/mod.rs`, test that `Filter::Unfinished` matches queued/downloading/paused and not done, and that `Filter::Category(Category::Video)` matches only video.

Run: `cargo test -p lajufile-app --lib`
Expected: compile failure for missing functions.

- [ ] **Step 2: Implement the main window**

`LajuFileApp` gains: `engine`, `snapshot`, `selected`, `filter`, `show_add`, `show_settings`, `add_draft`, `settings_draft`, `toast`, `last_message`.

Layout:
- Top menu bar (28 px): `File` (Add URL, Settings, Exit), `View` (Light/Dark), `Help` (About). Exit sends `ViewportCommand::Close` only in Task 6; until then it closes normally.
- Toolbar (60 px): `Add URL` primary, `Pause`, `Resume`, `Remove`, right-aligned `Settings`. Buttons are disabled when the selected row cannot perform the action.
- Sidebar (232 px): `All`, `Unfinished`, `Finished`, then `Video`, `Music`, `Documents`, `Programs`, `Compressed`, `Other` with counts. Bottom card: `Browser capture` / `Not connected` / `Bridge arrives in M4` — do not fake a connected state.
- Central panel: heading with the filter label and `N items`; `egui_extras::TableBuilder` columns `Name 440 / Size 96 / Status 124 / Progress 224 / Speed 90 / ETA 80 / Added 106`, header 34 px, rows 62 px.
- Name cell: 36×36 rounded icon with the extension text, file name, and the URL host in muted text.
- Status cell: colored pill (`Downloading` blue, `Queued` amber, `Paused` gray, `Done` green, `Error` red).
- Progress cell: `egui::ProgressBar` 160 px wide plus percent; 100% for done.
- Speed/ETA: from `snapshot.speeds`; ETA = remaining / speed, `—` when unknown.
- Added: relative time (`just now`, `5m ago`, `2h ago`, `3d ago`) from `created_at`.
- Status bar (32 px): `N active · N queued · X MB/s` left; `M2 preview · single-stream` right.
- Row click selects; double-click opens a completed file; right-click shows Open, Open folder, Copy URL, Pause/Resume, Remove.
- Show a toast in the bottom-right for `snapshot.message` when it differs from `last_message`.
- `app.update` calls `ui::main_window::draw`, applies returned `Action`s, and requests a repaint every 250 ms.

`Action` handling in `app.rs` sends `Command`s; Open/Open folder use `explorer` (`explorer /select,<path>`), Copy URL uses `ctx.copy_text`.

- [ ] **Step 3: Verify tests and the window**

Run: `cargo test -p lajufile-app --lib`
Expected: format/filter tests pass.

Run: `cargo run -p lajufile-app`
Expected: main window with all sections, empty table. Add one URL manually through a temporary engine command only if needed; the Add dialog arrives in Task 5. Take a screenshot and compare against Figma frame `01 Main window · Light`; fix obvious spacing, clipping, and text-wrap defects.

- [ ] **Step 4: Commit**

```powershell
git add crates/lajufile-app/src/app.rs crates/lajufile-app/src/ui
git commit -m "feat: add main window with download table"
```

---

### Task 5: Add download and Settings dialogs

**Files:**
- Create: `crates/lajufile-app/src/ui/dialogs.rs`
- Modify: `crates/lajufile-app/src/ui/mod.rs`
- Modify: `crates/lajufile-app/src/ui/main_window.rs`
- Modify: `crates/lajufile-app/src/app.rs`

**Interfaces:**
- Consumes: `AddRequest`, `Settings`, `Theme`.
- Produces: `Action::OpenAdd`, `Action::OpenSettings`, `Action::SaveSettings(Settings)`, `Action::Add(AddRequest)`.

- [ ] **Step 1: Write failing draft/settings tests**

In `app.rs`:

```rust
#[test]
fn add_draft_trims_empty_values() {
    let draft = AddDraft { url: " https://example.com/a.bin ".into(), file_name: "  ".into(), folder: " ".into(), start_now: true };
    let request = draft.into_request();
    assert_eq!(request.url, "https://example.com/a.bin");
    assert_eq!(request.file_name, None);
    assert_eq!(request.save_dir, None);
}

#[test]
fn normalize_settings_clamps_limits() {
    let mut settings = Settings::default();
    settings.max_concurrent = 0;
    settings.max_connections = 99;
    normalize_settings(&mut settings);
    assert_eq!(settings.max_concurrent, 1);
    assert_eq!(settings.max_connections, 32);
}
```

Run: `cargo test -p lajufile-app --lib`
Expected: compile failure for `AddDraft`/`normalize_settings`.

- [ ] **Step 2: Implement the dialogs**

Add dialog (600 px wide):
- URL (required, focused), File name (optional, placeholder `From URL`), Save to folder, `Start downloading now` checkbox.
- Footer: `Cancel`, `Add to queue` (start_now false), `Add download` (start_now true).
- Disable Add when the URL is empty; keep the dialog open on validation failure. On submit send `Command::Add` and close.
- Add a note: `Connections: 1 (segmented downloads arrive in M3)`. Do not show an editable 8-connection stepper yet.

Settings dialog (720 px wide):
- Sections in one scroll: Downloads (folder, simultaneous downloads 1–10, connections disabled with `Segmented downloads arrive in M3`, speed limit disabled), Behavior (close to tray, auto-resume, theme Light/Dark), Browser capture (port and masked token read-only, Copy token, note `Bridge and Edge capture arrive in M4`).
- Buttons: `Cancel`, `Save`. Save calls `normalize_settings`, sends `Command::UpdateSettings`, updates local settings, and closes. A failed engine save is surfaced as a toast; the previous settings file and in-memory settings remain.
- `normalize_settings` clamps `max_concurrent` to 1..=10 and `max_connections` to 1..=32.

Wire toolbar/menu actions: Add opens the dialog, Settings opens it, Pause/Resume/Remove send commands for the selected row, context menu actions work, Copy URL copies, Open/Open folder launch Explorer.

- [ ] **Step 3: Verify tests and manual flows**

Run: `cargo test -p lajufile-app --lib`
Expected: all lib tests pass.

Run the app:
1. Add `https://speed.cloudflare.com/__down?bytes=1000000` with File name `test.bin` and Start now; it appears and completes.
2. Add the same URL again with File name `test-2.bin` while queued/paused; the toast says `Already in LajuFile` and only one row exists.
3. Add a known 404 URL; a failure toast appears and no row is created.
4. Change theme to Dark, save, reopen Settings; Dark is still selected.

- [ ] **Step 4: Commit**

```powershell
git add crates/lajufile-app/src
git commit -m "feat: add download and settings dialogs"
```

---

### Task 6: Tray, close-to-tray, theme, final verification

**Files:**
- Create: `crates/lajufile-app/src/tray.rs`
- Create: `crates/lajufile-app/src/ui/theme.rs`
- Modify: `crates/lajufile-app/src/lib.rs`
- Modify: `crates/lajufile-app/src/app.rs`
- Modify: `crates/lajufile-app/src/ui/main_window.rs`
- Modify: `README.md`

**Interfaces:**
- Consumes: `egui::IconData`, `MenuEvent`, `TrayIconEvent`.
- Produces:

```rust
pub struct Tray { /* keeps TrayIcon alive */ }
pub enum TrayEvent { Show, Add, PauseAll, ResumeAll, Exit }
pub fn create(icon: &egui::IconData) -> Option<Tray>;
pub fn should_hide_on_close(close_to_tray: bool, tray_available: bool) -> bool;
pub fn apply_theme(ctx: &egui::Context, theme: Theme);
```

- [ ] **Step 1: Write the failing close/tray test**

```rust
#[test]
fn close_hides_only_when_tray_exists() {
    assert!(should_hide_on_close(true, true));
    assert!(!should_hide_on_close(false, true));
    assert!(!should_hide_on_close(true, false));
}
```

Run: `cargo test -p lajufile-app --lib`
Expected: compile failure for `should_hide_on_close`.

- [ ] **Step 2: Implement tray and theme**

- `tray::create` builds a `TrayIcon` with `assets/icons/icon-32.png` decoded to RGBA, tooltip `LajuFile`, and menu items with IDs `show`, `add`, `pause-all`, `resume-all`, `exit`. On build failure return `None` and keep the app usable.
- In `app.update`, drain `MenuEvent::receiver()` and `TrayIconEvent::receiver()`:
  - Left click / `show` → `ViewportCommand::Visible(true)` + `Focus`.
  - `add` → show + open the Add dialog.
  - `pause-all` / `resume-all` → send commands.
  - `exit` → `allow_exit = true` + `ViewportCommand::Close`.
- Intercept `ctx.input(|i| i.viewport().close_requested())`: when `should_hide_on_close(settings.close_to_tray, tray_available)`, send `CancelClose` and `Visible(false)`; otherwise let it close.
- Update the tray tooltip with `LajuFile — N active` after each snapshot.
- `apply_theme` sets custom light/dark visuals: window `#F7F8FA`/`#0E1319`, panels white/`#141A21`, accent `#0B6BCB`/`#4C9AFF`, text `#18212B`/`#E7EDF5`, muted `#667085`/`#94A3B8`, borders `#E1E7EF`/`#26303B`. Apply on startup and when the theme changes.
- Keep native window decorations in M2; do not build a custom title bar.

- [ ] **Step 3: Verify tray manually**

Run: `cargo run -p lajufile-app`
1. Close with X; the window hides and the tray icon remains.
2. Click the tray icon; the window returns.
3. Tray `Add URL…` opens the Add dialog.
4. Tray `Pause all` pauses active downloads; `Resume all` restarts them.
5. Tray `Exit` terminates the app.
6. If the tray icon cannot be created (temporarily force `create` to return `None`), closing exits rather than hiding.

- [ ] **Step 4: Full verification**

```powershell
cargo fmt
cargo fmt --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo build --release -p lajufile-app
```

Expected: all tests pass, clippy/fmt clean, `target/release/lajufile.exe` launches.

- [ ] **Step 5: Update README and commit**

Update `README.md`: M2 status, `cargo run -p lajufile-app`, single-stream limitation, M3 next.

```powershell
git add crates/lajufile-app README.md Cargo.toml Cargo.lock
git commit -m "feat: add tray, close-to-tray, and themes"
```

---

## Out of scope / deferred

- Segmented downloads and real pause/resume (M3).
- Speed limit enforcement (M3).
- Bridge + Edge extension (M4).
- Autostart registry entry, single-instance mutex, custom title bar, installer, logging crate (hardening).
- `Pause` aborts the transfer and `Resume` restarts from byte zero in M2. The UI must not claim otherwise.
