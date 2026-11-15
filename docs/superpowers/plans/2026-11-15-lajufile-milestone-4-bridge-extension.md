# LajuFile Milestone 4 — Bridge + Edge Capture Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Capture downloads from Microsoft Edge into LajuFile through a token-protected localhost bridge, while keeping the browser download as the fallback on every failure.

**Architecture:** A small axum server inside the app bind to `127.0.0.1` and sends add requests to the existing engine over a command channel. The engine owns probing, duplicate checks, SQLite, and the download; the bridge only translates HTTP to `Command::BridgeAdd` and maps the outcome back to HTTP. An MV3 Edge extension posts browser downloads to the bridge and cancels the browser copy only after LajuFile accepts it.

**Tech Stack:** Rust 2024, axum 0.8, tower-http 0.7 (CORS), tokio 1, reqwest 0.13, serde, rusqlite 0.42, windows-sys 0.61 (DPAPI), eframe/egui 0.37. Edge MV3 extension in plain JavaScript.

**Spec:** `docs/superpowers/specs/2026-11-14-lajufile-design.md` — sections 11, 12, 13, 14, 15. GUI reference: https://www.figma.com/design/PASDzAEqJlF35dkv4L9eIx

## Global Constraints

- Windows 10/11 x64 only; bridge binds only `127.0.0.1`, never `0.0.0.0`.
- Bridge token is 32 random bytes as 64 hex chars, sent in `X-LajuFile-Token`; compare in constant time; never log it.
- Request body limit is 64 KiB; CORS and `OPTIONS` are allowed for the Edge extension.
- `GET /health` is unauthenticated and returns only the version. Every other endpoint requires the token.
- Bridge add order: validate URL → active duplicate → probe with a **3 second** timeout → insert record and segments in one transaction → reply. Probe failure means 422 and **zero records**; manual Add keeps its 20 second probe.
- A bind failure is reported in the GUI but never stops the app or its downloads.
- Cookies are persisted only as DPAPI-protected bytes; plaintext cookies are never written to SQLite or logs.
- No bridge/extension work outside this plan; M4 does not add a custom URL protocol, installer, autostart, or single-instance mutex.
- UI language is English. Existing light/dark themes remain; browser-capture controls follow the current design.
- Verification for every Rust task: focused test, then `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --check`. Full workspace before handoff.

## Review Focus

These are the failure modes most likely to lose a download, corrupt a file, or expose a token; each has a test or a named manual check:

1. **Probe failure or timeout must create no ghost record** — a dead or slow URL leaves the store empty and returns 422. Covered in Task 2 (`bridge_probe_failure_returns_422_without_record`).
2. **A wrong or missing token must not reach the engine** — 401 before URL parsing, duplicate checks, or any store write. Covered in Task 1 (`bridge_rejects_missing_and_wrong_tokens`) and Task 4 (token rotation test).
3. **Cookie plaintext must never be stored** — only DPAPI bytes in `cookie_enc`; a decrypt failure marks the row Error instead of downloading without credentials. Covered in Task 3 (`cookie_roundtrip`, `bridge_context_is_persisted_encrypted`, `corrupt_cookie_marks_download_error`).
4. **A busy bridge port must not kill the app** — the engine keeps running, reports `BridgeState::Error`, and the GUI shows the message. Covered in Task 1 (`bridge_bind_failure_is_reported`).
5. **The extension must not cancel the browser download unless LajuFile accepted it** — only 201/409 cancel and erase; 401 and network errors call `suggest()` and leave the browser download alone. Manual checklist in Task 5.

---

## File Structure

```
crates/lajufile-core/
  src/cookie.rs              DPAPI protect/unprotect
  src/lib.rs                 export cookie module
  Cargo.toml                 windows-sys dependency
  tests/                      existing tests unchanged

crates/lajufile-app/
  src/bridge.rs              axum server, auth, CORS, health, POST /downloads
  src/engine.rs              bridge lifecycle/status, BridgeAdd, cookie persistence
  src/app.rs                 bridge test/regenerate actions, status polling
  src/ui/dialogs.rs          browser capture controls
  src/ui/main_window.rs      sidebar capture card
  tests/bridge.rs            bridge HTTP and engine integration
  tests/engine.rs            cookie/context persistence tests
  tests/support/mod.rs       app test server routes
  Cargo.toml                 axum/tower-http/serde runtime deps

extension/
  manifest.json
  background.js
  options.html
  options.js
  options.css
  popup.html
  popup.js
  popup.css
  icons/icon-16.png, icon-32.png, icon-48.png, icon-128.png

README.md                    M4 status, pairing, sideload, limitations
```

---

### Task 1: Bridge server foundation

**Files:**
- Create: `crates/lajufile-app/src/bridge.rs`
- Modify: `crates/lajufile-app/src/lib.rs`
- Modify: `crates/lajufile-app/src/engine.rs`
- Modify: `crates/lajufile-app/Cargo.toml`
- Modify: `Cargo.toml`
- Test: `crates/lajufile-app/tests/bridge.rs`

**Interfaces:**
- Produces:
  - `pub const BRIDGE_HEADER: &str = "x-lajufile-token";`
  - `pub const MAX_BODY_BYTES: usize = 64 * 1024;`
  - `#[derive(Debug, Clone, Copy, PartialEq, Eq)] pub enum BridgeState { Starting, Bound, Error }`
  - `#[derive(Debug, Clone, PartialEq, Eq)] pub struct BridgeStatus { pub state: BridgeState, pub port: u16, pub message: Option<String> }`
  - `pub struct BridgeServer { pub status: BridgeStatus, shutdown: Option<tokio::sync::oneshot::Sender<()>> }`
  - `impl BridgeServer { pub fn shutdown(mut self) { ... } }`
  - `pub async fn start(port: u16, token: Arc<RwLock<String>>, commands: Sender<Command>) -> BridgeServer`
  - `pub fn constant_time_eq(left: &[u8], right: &[u8]) -> bool`
  - `Snapshot.bridge: BridgeStatus`
- Engine changes:
  - `EngineHandle::start` clones the command sender and passes it to `engine_loop`.
  - `engine_loop` starts the bridge before entering the command loop and stores the returned server/status in `EngineState`.
  - `publish` includes the current bridge status; `shutdown` sends the bridge shutdown signal.
  - If start fails, `BridgeStatus { state: Error, port: configured, message: Some(reason) }`; the engine still enters its loop.

- [ ] **Step 1: Add dependencies**

Workspace `Cargo.toml`:

```toml
tower-http = { version = "0.7", features = ["cors"] }
```

App `Cargo.toml` `[dependencies]`: add `axum = "0.8"`, `serde.workspace = true`, `serde_json.workspace = true`, `tower-http.workspace = true`, and `tokio = { workspace = true, features = ["net"] }`. Remove the duplicate axum dev-dependency if Cargo complains.

- [ ] **Step 2: Write the failing bridge foundation tests**

`crates/lajufile-app/tests/bridge.rs` starts an engine with `bridge_port: 0`, waits for `snapshot.bridge.state == Bound`, and uses `reqwest`:

```rust
#[test]
fn health_is_public_and_returns_version() {
    // GET http://127.0.0.1:{port}/health -> 200, JSON version == "0.1.0"
}

#[test]
fn bridge_rejects_missing_and_wrong_tokens() {
    // POST /downloads with {} and no token -> 401
    // POST /downloads with X-LajuFile-Token: wrong -> 401
}

#[test]
fn options_allows_extension_cors_headers() {
    // OPTIONS /downloads with Origin: chrome-extension://abc and
    // Access-Control-Request-Headers: content-type,x-lajufile-token
    // -> 200/204, allow-origin exists, allow-headers contains both names
}

#[test]
fn oversized_body_is_rejected() {
    // POST /downloads with valid token and 65 KiB body -> 413
}

#[test]
fn bridge_bind_failure_is_reported() {
    // bind a std TcpListener on 127.0.0.1:0, use its port in settings,
    // wait for BridgeState::Error and a non-empty message.
}
```

- [ ] **Step 3: Run the tests and verify they fail**

Run: `cargo test -p lajufile-app --test bridge`

Expected: FAIL to compile because `lajufile_app::bridge` and `Snapshot.bridge` do not exist.

- [ ] **Step 4: Implement the server and engine lifecycle**

`bridge.rs` builds:

```rust
Router::new()
    .route("/health", get(health))
    .route("/downloads", post(downloads_placeholder))
    .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
    .layer(
        CorsLayer::new()
            .allow_origin(Any)
            .allow_methods([Method::GET, Method::POST])
            .allow_headers([header::CONTENT_TYPE, HeaderName::from_static(BRIDGE_HEADER)]),
    )
    .with_state(state)
```

`health` returns `Json(serde_json::json!({ "version": env!("CARGO_PKG_VERSION") }))`. `downloads_placeholder` requires a valid token, extracts `Bytes` (so `DefaultBodyLimit` applies), and returns `StatusCode::NOT_IMPLEMENTED`; Task 2 replaces it. Auth reads the header, compares with `constant_time_eq`, and returns 401 JSON on mismatch. Bind `(Ipv4Addr::LOCALHOST, port)`; return the actual port from `local_addr`. Spawn `axum::serve` with a graceful-shutdown receiver. If bind fails, return `BridgeServer` with `BridgeState::Error` and the error text.

- [ ] **Step 5: Run the tests and verify they pass**

Run: `cargo test -p lajufile-app --test bridge`

Expected: PASS for all five tests.

- [ ] **Step 6: Verify and commit**

Run: `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --check`.

```bash
git add Cargo.toml Cargo.lock crates/lajufile-app/Cargo.toml crates/lajufile-app/src/bridge.rs crates/lajufile-app/src/lib.rs crates/lajufile-app/src/engine.rs crates/lajufile-app/tests/bridge.rs
git commit -m "feat: add token-protected local bridge"
```

---

### Task 2: Bridge add flow and engine outcome mapping

**Files:**
- Modify: `crates/lajufile-app/src/bridge.rs`
- Modify: `crates/lajufile-app/src/engine.rs`
- Modify: `crates/lajufile-app/tests/bridge.rs`
- Modify: `crates/lajufile-app/tests/support/mod.rs`

**Interfaces:**
- Produces:
  - `#[derive(Debug, Clone, Serialize, Deserialize)] pub struct BridgeAddRequest { pub url: String, pub filename: Option<String> }`
  - `#[derive(Debug, Clone)] pub enum BridgeAddOutcome { Created { id: i64, filename: String, size: Option<u64> }, Conflict { id: i64 }, Invalid(String), ProbeFailed(String), Internal(String) }`
  - `Command::BridgeAdd { request: BridgeAddRequest, reply: tokio::sync::oneshot::Sender<BridgeAddOutcome> }`
  - `Event::BridgeProbe { request: AddRequest, reply: tokio::sync::oneshot::Sender<BridgeAddOutcome>, result: Box<Result<ProbeResult, ProbeError>> }`
  - Internal `insert_from_probe(&mut self, request: AddRequest, result: ProbeResult) -> Result<Inserted, InsertError>`, where `Inserted { id, file_name, size }` and `InsertError::{Duplicate(Download), Failed(String)}`. `handle_probe` uses it for manual Add.
- HTTP mapping:
  - 201 `{ "id": id, "filename": filename, "size": size_or_null }`
  - 409 `{ "id": existing_id }`
  - 422 `{ "error": probe_error }`
  - 400 `{ "error": "invalid URL" }`
  - 401 for token failure
  - 500 for engine/store/name failures
  - Handler waits at most 5 seconds on the oneshot; timeout is 500.

- [ ] **Step 1: Add failing bridge add tests**

Extend `tests/support/mod.rs` with `/slow-probe.bin` that sleeps 4 seconds before returning the 1 MiB file. Add to `tests/bridge.rs`:

```rust
#[test]
fn bridge_add_creates_download_and_returns_201() {
    // POST /downloads { url: "{base}/file.bin", filename: null }
    // -> 201, id > 0, filename == "file.bin", size == Some(1_048_576)
    // wait snapshot Done and final file exists.
}

#[test]
fn bridge_duplicate_returns_409() {
    // POST the same URL twice; first 201, second 409 with the first id;
    // exactly one row in the store.
}

#[test]
fn bridge_probe_failure_returns_422_without_record() {
    // POST /missing -> 422, store has zero rows after shutdown.
}

#[test]
fn bridge_rejects_invalid_url_with_400() {
    // POST { url: "not a url" } -> 400, no row.
}

#[test]
fn bridge_probe_timeout_returns_422_without_record() {
    // POST /slow-probe.bin -> 422 around 3 seconds, no row.
}
```

- [ ] **Step 2: Run the tests and verify they fail**

Run: `cargo test -p lajufile-app --test bridge`

Expected: FAIL because `BridgeAddRequest`, `BridgeAddOutcome`, and `POST /downloads` behavior do not exist.

- [ ] **Step 3: Implement the bridge-to-engine path**

Add the command and event above. Derive `Deserialize` with `#[serde(default)]` on `filename` (and the optional context fields added in Task 3) so a body with only `url` is valid. `handle_bridge_add` parses the URL, checks `active_by_url(url.to_string())`, sends `Conflict` on a hit, and otherwise spawns `probe` with `timeout: Duration::from_secs(3)` and the default UA. The probe event calls `insert_from_probe`; map `Duplicate` to `Conflict`, `Failed` to `Internal`, probe error to `ProbeFailed`.

Refactor the current `handle_probe` insert block into `insert_from_probe` without changing manual Add behavior or its 20 second probe. It must keep the duplicate re-check, directory creation, name resolution, temp reservation, segment planning, transaction insert, temp cleanup on insert failure, and `pump_queue`.

- [ ] **Step 4: Run the tests and verify they pass**

Run: `cargo test -p lajufile-app --test bridge`

Expected: PASS for the five new tests and the Task 1 tests.

- [ ] **Step 5: Verify and commit**

Run: `cargo test -p lajufile-app --test engine`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --check`.

```bash
git add crates/lajufile-app/src/bridge.rs crates/lajufile-app/src/engine.rs crates/lajufile-app/tests/bridge.rs crates/lajufile-app/tests/support/mod.rs
git commit -m "feat: add bridge download endpoint"
```

---

### Task 3: Cookie, referrer, and user-agent persistence

**Files:**
- Create: `crates/lajufile-core/src/cookie.rs`
- Modify: `crates/lajufile-core/src/lib.rs`
- Modify: `crates/lajufile-core/Cargo.toml`
- Modify: `Cargo.toml`
- Modify: `crates/lajufile-app/src/bridge.rs`
- Modify: `crates/lajufile-app/src/engine.rs`
- Modify: `crates/lajufile-app/src/ui/dialogs.rs`
- Modify: `crates/lajufile-app/tests/engine.rs`
- Modify: `crates/lajufile-app/tests/bridge.rs`
- Modify: `crates/lajufile-app/tests/support/mod.rs`

**Interfaces:**
- Produces:
  - `pub fn protect_cookie(value: &str) -> Result<Vec<u8>, CookieError>`
  - `pub fn unprotect_cookie(blob: &[u8]) -> Result<String, CookieError>`
  - `CookieError::{Dpapi(String), Utf8}`
  - `BridgeAddRequest { url, filename, referrer: Option<String>, user_agent: Option<String>, cookie: Option<String> }`
  - `AddRequest { ..., referrer: Option<String>, user_agent: Option<String>, cookie: Option<String> }`
- Engine behavior:
  - Probe options use the request's UA, referrer, and cookie; missing UA falls back to `DEFAULT_USER_AGENT`.
  - `NewDownload.cookie_enc = request.cookie.map(protect_cookie).transpose()`, and the row keeps referrer/UA.
  - Segmented and single download specs receive the same UA/referrer; the cookie is decrypted per start with `unprotect_cookie`. A decrypt error marks the row Error and starts nothing.

- [ ] **Step 1: Add DPAPI and failing persistence tests**

Workspace `Cargo.toml`:

```toml
windows-sys = { version = "0.61", features = ["Win32_Foundation", "Win32_Security_Cryptography", "Win32_System_Memory"] }
```

Core `Cargo.toml` adds `windows-sys.workspace = true`.

Core unit tests in `cookie.rs`:

```rust
#[test]
fn cookie_roundtrip() {
    let blob = protect_cookie("session=abc; theme=dark").unwrap();
    assert_ne!(blob, b"session=abc; theme=dark");
    assert_eq!(unprotect_cookie(&blob).unwrap(), "session=abc; theme=dark");
}

#[test]
fn tampered_cookie_fails() {
    let mut blob = protect_cookie("session=abc").unwrap();
    blob[0] ^= 0xff;
    assert!(unprotect_cookie(&blob).is_err());
}
```

App test server adds `/auth.bin` requiring `Cookie: session=abc`, `Referer: https://example.test/page`, and `User-Agent: LajuFileTest/1.0`; it returns 1 MiB only when all match. Add `bridge_context_is_persisted_encrypted` in `tests/bridge.rs`: POST with those headers, wait Done, shutdown, open the store, assert the row's `cookie_enc` is not the plaintext, `unprotect_cookie` returns it, and referrer/UA match. Add `corrupt_cookie_marks_download_error` in `tests/engine.rs`: insert a Queued `NewDownload` with `cookie_enc: Some(vec![1, 2, 3])` directly into a store, start the engine, and assert the row becomes Error with a cookie message.

- [ ] **Step 2: Run the tests and verify they fail**

Run: `cargo test -p lajufile-core --lib cookie` and `cargo test -p lajufile-app --test bridge`

Expected: FAIL to compile because the cookie module and the new fields do not exist.

- [ ] **Step 3: Implement DPAPI and wire context through**

Use `CryptProtectData` / `CryptUnprotectData` with `CRYPTPROTECT_UI_FORBIDDEN`; copy the input into a `CRYPTOAPI_BLOB`, free the output with `LocalFree`. Treat an empty/whitespace cookie as absent before `protect_cookie` so a cookie-less capture is not rejected. Reject empty input to `protect_cookie` itself and non-UTF-8 decrypted output. No cookie text in errors or logs.

Extend `BridgeAddRequest`, `AddRequest`, and every `AddRequest` literal in app tests with the three optional fields (`None` for manual Add). Add `AddDraft::into_request` fields set to `None`. Probe, insert, segmented, and single paths use the context as described above.

- [ ] **Step 4: Run the tests and verify they pass**

Run: `cargo test -p lajufile-core --lib cookie`, `cargo test -p lajufile-app --test bridge`, `cargo test -p lajufile-app --test engine`.

Expected: PASS, including the existing 115 tests.

- [ ] **Step 5: Verify and commit**

Run: `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --check`.

```bash
git add Cargo.toml Cargo.lock crates/lajufile-core/Cargo.toml crates/lajufile-core/src/cookie.rs crates/lajufile-core/src/lib.rs crates/lajufile-app/src
git add crates/lajufile-app/tests
git commit -m "feat: persist bridge cookies with DPAPI"
```

---

### Task 4: Browser capture settings and bridge status

**Files:**
- Modify: `crates/lajufile-app/src/bridge.rs`
- Modify: `crates/lajufile-app/src/engine.rs`
- Modify: `crates/lajufile-app/src/app.rs`
- Modify: `crates/lajufile-app/src/ui/dialogs.rs`
- Modify: `crates/lajufile-app/src/ui/main_window.rs`
- Modify: `crates/lajufile-app/src/ui/mod.rs`
- Test: `crates/lajufile-app/src/app.rs` unit tests
- Test: `crates/lajufile-app/tests/bridge.rs`

**Interfaces:**
- Produces:
  - `pub fn health_check(port: u16, timeout: Duration) -> Result<(), String>` (sync TCP `GET /health`, 200 required).
  - `Action::TestBridge(u16)` and `Action::RegenerateToken`.
  - `LajuFileApp.bridge_test: Option<Receiver<Result<(), String>>>` polled in `logic`.
  - Sidebar capture card driven by `snapshot.bridge`: Bound → `Connected`, actual port; Error → `Bridge unavailable` + message; Starting/Disabled → `Starting bridge`.
- Settings browser-capture controls:
  - Port `DragValue` 1024..=65535 with note `Applies after restart`.
  - Masked token, Copy token, Regenerate token (sets a fresh 64-hex token; Save applies it to the running bridge immediately).
  - Test connection button (background thread; result toast).
  - Pairing hint: `Paste the port and token into the LajuFile Edge extension options.`

- [ ] **Step 1: Write failing UI/health/token tests**

In `app.rs` tests:

```rust
#[test]
fn normalize_settings_clamps_bridge_port() {
    let mut settings = Settings { bridge_port: 0, ..Settings::default() };
    normalize_settings(&mut settings);
    assert_eq!(settings.bridge_port, 1024);
    settings.bridge_port = 70_000;
    normalize_settings(&mut settings);
    assert_eq!(settings.bridge_port, 65_535);
}
```

In `bridge.rs` tests, bind a `std::net::TcpListener` on `127.0.0.1:0` in a thread and answer `HTTP/1.1 200 OK`; assert `health_check(port, 1s)` is `Ok(())` and a closed port is `Err`.

In `tests/bridge.rs`, start with token A, POST `/downloads` with `{}` and token A (valid token reaches the handler and returns 400), then `Command::UpdateSettings` with token B, wait for the snapshot token to change, and assert token A now gets 401 while token B still reaches the handler (400).

- [ ] **Step 2: Run the tests and verify they fail**

Run: `cargo test -p lajufile-app --lib` and `cargo test -p lajufile-app --test bridge`

Expected: FAIL because port clamping, `health_check`, and the actions do not exist.

- [ ] **Step 3: Implement the controls and token lifecycle**

`normalize_settings` clamps the port to 1024..=65535 and keeps `bridge_token` valid. `update_settings` writes the token into the bridge's shared `RwLock` immediately after a successful save; if the port changed, leave the bound server alone and expose the note via the settings UI. `health_check` uses connect/read timeouts and checks the status line starts with `HTTP/1.1 200`.

In `app.rs`, `Action::TestBridge` spawns a thread with `health_check`, stores its receiver, and `logic` turns the result into a toast. `Action::RegenerateToken` sets `settings_draft.bridge_token = generate_token()`; the Settings dialog shows `Save to apply; existing extension pairings will stop working.` The main-window capture card reads `snapshot.bridge`; `Manage` opens Settings.

- [ ] **Step 4: Run the tests and verify they pass**

Run: `cargo test -p lajufile-app --lib`, `cargo test -p lajufile-app --test bridge`, `cargo test -p lajufile-app --test engine`.

Expected: PASS. Launch the app and screenshot Settings and the sidebar card for visual verification.

- [ ] **Step 5: Verify and commit**

Run: `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --check`.

```bash
git add crates/lajufile-app/src
git add crates/lajufile-app/tests/bridge.rs
git commit -m "feat: show bridge status and capture settings"
```

---

### Task 5: Edge MV3 extension

**Files:**
- Create: `extension/manifest.json`
- Create: `extension/background.js`
- Create: `extension/options.html`, `options.js`, `options.css`
- Create: `extension/popup.html`, `popup.js`, `popup.css`
- Create: `extension/icons/icon-16.png`, `icon-32.png`, `icon-48.png`, `icon-128.png`

**Interfaces:**
- Bridge contract: `GET /health`; `POST /downloads` with `X-LajuFile-Token`, body `{ url, filename, referrer, user_agent, cookie }`; 201/409 accepted; 401 unpaired; 400/422/500 failure.
- Storage (`chrome.storage.local`): `{ port: 53841, token: "", autoCapture: true }`.
- Manual pairing; no auto-discovery.

- [ ] **Step 1: Create the manifest and options page**

`manifest.json` uses MV3, permissions `downloads`, `cookies`, `contextMenus`, `storage`, `notifications`; host permissions `<all_urls>` and `http://127.0.0.1/*`; background service worker `background.js`; action popup; options page; 16/32/48/128 icons copied from `assets/icons/`.

`options.html`/`options.js` provide: bridge port, token (password field with paste), auto-capture checkbox, `Test connection`, `Save`, and a pairing hint. Test calls `/health` and shows `Connected` or the error; Save persists to `chrome.storage.local` and notifies the background worker.

- [ ] **Step 2: Implement the background capture flow**

`background.js` must provide:

```js
async function getConfig()
async function health(port)              // 2 s timeout, cached 5 s
async function postDownload(payload)      // 5 s timeout, token header
async function cookieHeader(url)          // chrome.cookies.getAll({url}), "name=value; ..."; omitted when empty
async function capture(item, allowBrowserFallback)
```

Register `chrome.downloads.onDeterminingFilename` and `chrome.downloads.onCreated` at top level. Build the POST payload with `cookie` omitted when `cookieHeader` returns an empty string. `onDeterminingFilename` holds `suggest` until the POST resolves, must call `suggest()` exactly once in every branch, and returns `true` while pending. On 201/409: `chrome.downloads.cancel(id)`, `chrome.downloads.erase({ id })`, then `suggest()`. On 401: create a notification `Pair the LajuFile extension in its options` and call `suggest()`; on network/400/422/500: call `suggest()` and leave the browser download alone. `onCreated` is the fallback: after 1500 ms, if the id was not handled by `onDeterminingFilename`, run the same POST and cancel only on 201/409. Add the id to a `handledIds` set before either path completes.

Create the context menu on install with id `lajufile-download`, title `Download with LajuFile`, contexts `["link"]`. On click, POST `info.linkUrl` with the tab URL as referrer and cookies; notify success/failure; never cancel a browser download because none exists.

- [ ] **Step 3: Implement the popup**

`popup.html`/`popup.js` show bridge status (`Connected` / `Not connected`), the configured port, an auto-capture toggle, `Test connection`, `Extension options`, and a footer with `LajuFile 0.1.0`. Do **not** add an `Open LajuFile` button: there is no registered `lajufile://` protocol in M4, and a dead button is worse than a documented deviation. The tray remains the way to open the app.

- [ ] **Step 4: Static checks and manual smoke**

Run:

```powershell
node --check extension/background.js
node --check extension/options.js
node --check extension/popup.js
node -e "JSON.parse(require('fs').readFileSync('extension/manifest.json','utf8')); console.log('manifest ok')"
```

Manual Edge: `edge://extensions` → Developer mode → Load unpacked → `extension/`; open options, paste port/token from LajuFile Settings, Test connection; download a normal file and confirm LajuFile gets it and the browser shelf copy is gone; trigger the same URL twice and confirm 409 leaves one row; set a wrong token and confirm the notification + browser download continues; set `Ask where to save` and confirm no Save As prompt appears for captured downloads.

- [ ] **Step 5: Commit**

```bash
git add extension
git commit -m "feat: add Edge capture extension"
```

---

### Task 6: End-to-end verification, documentation, release

**Files:**
- Modify: `README.md`
- Test: `crates/lajufile-app/tests/bridge.rs` (add a full POST-to-file test if Task 2 did not)
- Verify: `extension/`

**Interfaces:**
- Consumes all previous tasks. No new public API.

- [ ] **Step 1: Add the final end-to-end assertion**

If not already covered, add `bridge_download_writes_complete_file`: start bridge on port 0, POST `/file.bin` with a valid token, wait Done, shutdown, assert the final file is 1 MiB and the temp file is gone.

- [ ] **Step 2: Update README**

README must state: M4 complete; bridge at `127.0.0.1:53841`; token from Settings; Edge sideload steps; `GET /health`, `POST /downloads`; 201/409/422/401 behavior; cookie DPAPI; limitations (POST/form, blob, DRM, one-time URLs, downloads before app start, cross-host cookies, no `Open LajuFile` in popup); M5 next (hardening/release). Add the M4 plan link.

- [ ] **Step 3: Full verification**

```powershell
cargo fmt
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --check
cargo build --release -p lajufile-app
```

Expected: all workspace tests pass; clippy/fmt clean; release binary builds.

- [ ] **Step 4: Manual checklist**

- App starts with bridge on 53841; sidebar says `Connected`.
- Wrong token returns 401 and the app toast/extension notification explains pairing.
- A dead URL returns 422 and leaves the list unchanged.
- Duplicate active URL returns 409 and leaves one row.
- Edge capture creates a row with the expected filename and final size.
- Cookie-protected test URL downloads only with the forwarded cookie.
- Stop the app; extension shows `Not connected`; browser download still works.
- Settings token regeneration makes the old extension token fail until re-paired.
- Close to tray and reopen from tray after bridge work.

- [ ] **Step 5: Commit**

```bash
git add README.md crates/lajufile-app/tests/bridge.rs
git commit -m "docs: document bridge and Edge capture"
```

---

## Self-Review

**Spec coverage:** bridge bind/token/CORS/body limit/health, synchronous 3s probe, 201/409/422/400/401/500, manual pairing, token regeneration, DPAPI cookies, referrer/UA, Edge determining-filename capture, fallback, context menu, popup, options, GUI status, limitations, and release verification each map to a task above.

**Type consistency:** `BridgeAddRequest` is the extension/HTTP shape; `AddRequest` is the engine shape. `BridgeAddOutcome` is consumed only by `bridge.rs`. `BridgeStatus` is produced by `bridge.rs` and consumed by `engine.rs`, `Snapshot`, and the UI. `Segment` end remains inclusive; cookie bytes are `Option<Vec<u8>>` in the existing store model.

**Review Focus:** each of the five failure modes has a named test or manual check. The manual extension check is required because a headless Edge capture test would not prove the browser shelf behavior.

**Proportion:** code blocks are limited to signatures, test names, and exact protocol values. Bodies are left to the implementer except where the HTTP contract or failure order is the decision.
