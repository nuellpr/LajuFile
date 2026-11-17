# LajuFile Hardening, Verified Packaging & Windows Polish — Design

Status: draft for approval  
Date: 2026-11-17  
Scope: Pilihan 1 (Paket Inti Fokus & Minim Bloat)

## Intent

Upgrade `LajuFile` based on REA binary/extension comparison findings against similar Rust download managers while preserving its core advantages: a small pure-Rust `egui` + `native-tls` binary (~14 MB baseline) and a zero-registry `127.0.0.1` HTTP bridge without separate native-messaging host processes.

## Goals

1. **Authenticode-safe Installer & Verified Self-Updater**: Fix the PE trailer assumption so Authenticode-signed Windows setup executables extract and self-update reliably, and verify release installer downloads against published `SHA256SUMS.txt`.
2. **Compiler/Linker Hardening & Accessibility**: Enable Windows Control Flow Guard (`/guard:cf`), strip or sanitize release `.pdb` exposure, add explicit release optimization settings, and enable `accesskit` UI Automation support in `eframe`.
3. **Extension & Bridge Defense-in-Depth**: Validate Chrome runtime message senders (`sender.id`, `sender.url`, `frameId`) in the MV3 background worker, block DNS rebinding on the local HTTP bridge via `Host` verification, tighten CORS origins to extension schemes, and expose an authenticated `/wake` endpoint.
4. **Windows Native Runtime Polish**: Prevent duplicate instances on launch by waking the running tray/window to the foreground, keep Windows awake while active downloads are running, and reliably bring captured-download prompts to the foreground.

## Non-Goals

- Switching away from `egui`/`eframe` or `native-tls`, or adopting Chrome Native Messaging (`*-nmhost.exe`).
- Post-download system automation UI (Sleep/Shutdown/Hibernate on queue completion), Credential Manager migration for `bridge_token`, or `cargo-auditable` SBOM embedding.
- Generating or storing private Code Signing certificates in the repository (CI will sign only when an external secret is supplied).
- Refactoring existing UI dialogs (`dialogs.rs`, `main_window.rs`) beyond the `accesskit` feature enablement and window foreground restoration.

## Anti-Goals

- Breaking backward compatibility for existing `EngineConfig` callers in `tests/engine.rs` and `tests/bridge.rs`, or breaking unsigned self-extracting installers (`cargo test` payloads).
- Blocking browser downloads or failing closed if `wake` fails when a tray instance is unreachable.
- Adding heavy dependencies where stdlib or already-installed workspace crates (`windows-sys`, `lajufile-core::checksum`) suffice.

## Constraints

- Target toolchain: Rust `edition = "2024"`, MSRV `1.85` (`Cargo.toml`), cross-platform CI (`windows-latest`, `ubuntu-latest`, `macos-latest`) in `.github/workflows/ci.yml` and `release.yml`.
- All existing workspace tests, `cargo fmt --check`, `cargo clippy -- -D warnings`, and `node --check extension/*.js` must pass.
- No private keys committed; signing hooks must degrade cleanly when secrets are absent.

## Approaches Considered

- **Approach 1 — Focused Hardening, Verified Packaging & Core OS Polish (Chosen)**: Closes concrete security/packaging gaps discovered by REA + codebase inspection (Authenticode trailer conflict, unverified self-update, missing `/guard:cf`, missing `sender` validation, DNS-rebinding risk, missing single-instance/anti-sleep) with minimal diff and zero UI clutter.
- **Approach 2 — Full Feature Expansion (+ Post-Download Actions, Credential Manager, SBOM)**: Adds settings UI and extra platform credential/power integrations. Rejected for this milestone to keep changes focused, verifiable, and bloat-free.

## Detailed Design

### 1. Authenticode-Safe Packaging, Hardening & Verified Updater

- **PE Certificate-Aware Payload Extraction (`crates/lajufile-app/src/installer.rs`)**:
  - `parse_payload(exe_bytes: &[u8])` currently reads the 84-byte `LAJUFILE_PK1` footer at `exe_bytes.len() - FOOTER_LEN`. Authenticode appends a `WIN_CERTIFICATE` attribute table (plus up to 7 `0x00` 8-byte alignment padding bytes) *after* the trailer and records `(cert_offset, cert_size)` in Optional Header `DataDirectory[4]` (`IMAGE_DIRECTORY_ENTRY_SECURITY`).
  - Add a pure-Rust PE header helper `effective_payload_end(bytes: &[u8]) -> usize`:
    - Check `"MZ"`, `e_lfanew` (`u32` LE at offset `0x3C`), and `"PE\0\0"`.
    - Read Optional Header magic (`0x10B` PE32 -> `NumberOfRvaAndSizes` at `opt + 92`, `DataDirectory[4]` at `opt + 128`; `0x20B` PE32+ -> `NumberOfRvaAndSizes` at `opt + 108`, `DataDirectory[4]` at `opt + 144`).
    - When `NumberOfRvaAndSizes > 4` and `DataDirectory[4]` gives nonzero `cert_offset` (raw file offset) + `cert_size` with `cert_offset + cert_size <= bytes.len()` and `cert_offset >= FOOTER_LEN`, inspect `&bytes[..cert_offset]`.
    - If `&bytes[cert_offset - 12..cert_offset] == MAGIC`, return `cert_offset`. Otherwise strip `1..=7` trailing `0x00` padding bytes immediately preceding `cert_offset` if `&bytes[cert_offset - k - 12..cert_offset - k] == MAGIC` and all skipped `k` bytes equal `0x00`.
    - For unsigned PEs or non-PE test buffers (`b"MZ_FAKE_STUB_HEADER"`), fall back to `bytes.len()`.
  - `parse_payload` parses the payload ending at `effective_payload_end(exe_bytes)`, still extracting `stub = &exe_bytes[..stub_end]` and enforcing SHA-256 over the packed body.
- **Build & Signing Flow (`installer/build-installer.ps1`, `.github/workflows/release.yml`, `installer/build-unix-installer.sh`)**:
  - In Windows packaging (`build-installer.ps1` and `release.yml`):
    - Sign `target/release/lajufile.exe` **before** `--pack` if signing is configured; leave `--stub` (`lajufile-setup.exe`) unsigned during `--pack` so the extracted `uninstall.exe` never carries an invalidated certificate directory offset; sign the final `dist/LajuFile-*-windows-x64-setup.exe` **after** `--pack` when `WINDOWS_PFX_BASE64` (plus `WINDOWS_PFX_PASSWORD`, optional `WINDOWS_TIMESTAMP_URL`) and `signtool.exe` exist, and verify with `signtool verify /pa`.
  - Split `release.yml` into parallel platform build jobs (`windows`, `linux`, `macos`) uploading workflow artifacts, followed by one `publish` job (`needs: [windows, linux, macos]`) that computes sorted lowercase `SHA256SUMS.txt` across all release installers and uploads all installers + `SHA256SUMS.txt` atomically via `gh release upload --clobber` / `gh release create`.
- **Verified Self-Update (`crates/lajufile-app/src/updater.rs`)**:
  - Extend `GitHubRelease` / `UpdateInfo` so `UpdateInfo` stores `checksums_url: Option<String>` pointing to the `SHA256SUMS.txt` asset in the release (validated via `validate_update_download_url`). `Action::StartSelfUpdate` keeps its existing fields while `app.rs` passes `info.checksums_url.clone()` from `UpdateUiState::Available(info)`.
  - Fail closed if no `SHA256SUMS.txt` asset exists or if `SHA256SUMS.txt` lacks an entry matching `asset_name`. Parse standard `<64-hex>  <filename>` / `<64-hex> *<filename>` lines cleanly.
  - During `download_installer`, fetch `SHA256SUMS.txt`, verify `lajufile_core::checksum::sha256_file(&part)` matches the expected digest before finalizing, and on Windows verify `crate::installer::parse_payload(&bytes).is_ok()` instead of `ends_with(b"LAJUFILE_PK1")`.

### 2. Compiler Hardening, Binary Footprint & Accessibility

- **Workspace Release Profile (`Cargo.toml`)**:
  - Add `[profile.release]` (`lto = "thin"`, `codegen-units = 1`, `strip = "debuginfo"`).
  - Add `"accesskit"` to `eframe` features in `[workspace.dependencies]`, and add Windows features `"Win32_System_Power"`, `"Win32_System_Threading"`, `"Win32_UI_WindowsAndMessaging"` to `windows-sys`.
- **Windows MSVC Config (`.cargo/config.toml` & `crates/lajufile-app/build.rs`)**:
  - In `.cargo/config.toml` under `[target.'cfg(all(target_os = "windows", target_env = "msvc"))']`, set `rustflags = ["-C", "control-flow-guard=yes", "-C", "link-arg=/guard:cf"]` so both Control Flow Guard instrumentation and the PE `IMAGE_DLLCHARACTERISTICS_GUARD_CF` (`0x4000`) flag are enabled.
  - In `crates/lajufile-app/build.rs`, when `CARGO_CFG_TARGET_OS == "windows"`, `CARGO_CFG_TARGET_ENV == "msvc"`, and `PROFILE == "release"`, emit `cargo:rustc-link-arg-bins=/DEBUG:NONE` (if MSVC `link.exe` accepts `/DEBUG:NONE` cleanly without breaking build, else `/PDBALTPATH:%_PDB%`; verified during build + REA audit) so local PDB paths/names are not leaked in release executables while preserving debug info during dev/test builds.

### 3. Extension & Bridge Defense-in-Depth

- **Extension Sender Validation (`extension/background.js`)**:
  - In `chrome.runtime.onMessage.addListener((message, sender, sendResponse) => ...)`:
    - Require `message && typeof message.type === "string"` and `sender && sender.id === chrome.runtime.id`.
    - Classify `isOwnPage`: `String(sender.url || "").startsWith(chrome.runtime.getURL("")) && (sender.frameId === undefined || sender.frameId === 0)` (designer call: do **not** require `!sender.tab`, because `extension/manifest.json` uses `"open_in_tab": true` for `options.html`, where Chromium sets `sender.tab` on extension tab messages; web pages cannot spoof `sender.url` to `chrome-extension://<id>/...`, and `fromPage = Boolean(sender.tab) && !isOwnPage`).
    - Validate `isHttpUrl(url)`: parses with `new URL` and requires `protocol === "http:" || protocol === "https:"` plus non-empty hostname.
    - Validate `isYouTubeTab(sender)`: `fromPage && (sender.frameId === undefined || sender.frameId === 0) && isYouTubePageUrl(sender.url || (sender.tab && sender.tab.url))` (`youtube.com`, `www.youtube.com`, `m.youtube.com`).
    - Enforce route permissions:
      - `"config-updated"`: requires `isOwnPage`.
      - `"bypass-url"`: requires `(fromPage || isOwnPage)` and `isHttpUrl(message.url)`.
      - `"intercept-link"`: requires `fromPage`, `isHttpUrl(sender.url || (sender.tab && sender.tab.url))`, and `isHttpUrl(message.url)`.
      - `"video-formats"` & `"video-download"`: require `(isOwnPage || isYouTubeTab(sender))` and `isYouTubeVideoUrl(message.url)`.
  - Cap `handledIds` and `bypassUrls` maps (evict expired/oldest beyond 512 entries) to bound memory in long sessions.
- **Bridge Hardening & `/wake` Route (`crates/lajufile-app/src/bridge.rs`)**:
  - Add an Axum middleware on the entire router validating the HTTP `Host` header is a loopback host (`127.0.0.1`, `localhost`, or `[::1]`, with optional numeric port), returning `403 Forbidden` otherwise to mitigate DNS rebinding.
  - Restrict `is_allowed_origin` CORS origins to extension schemes (`chrome-extension://`, `moz-extension://`, `extension://`, `safari-web-extension://`), removing broad `http://localhost` / `http://127.0.0.1` web origins.
  - Add `POST /wake` calling `authorize(&headers, &state.token)?` (not `authorize_and_pair`, so waking from desktop launch does not falsely mark `browser_paired = true`) and sending `Command::WakeWindow`.

### 4. Windows Native Runtime Polish

- **Platform Helpers Module (`crates/lajufile-app/src/platform.rs`)**:
  - Small focused cross-platform helper module (`#[cfg(windows)]` implementations + safe no-ops on Unix, except `/wake` helper which uses settings + loopback HTTP across platforms):
    - `SingleInstanceGuard`: On Windows, `CreateMutexW(null, 0, "Local\\LajuFileSingleInstance")` checking `ERROR_ALREADY_EXISTS`; closes handle on `Drop`. On non-Windows, falls back to file lock (`flock` / `O_EXLOCK` or checking running `/wake` if configured — check `libc` absence and keep non-Windows simple via `/wake` health/wake probe without new crates: probe configured `bridge_port` `/health` + `POST /wake` if `settings.json` exists, or Unix socket/lockfile if trivial).
    - `signal_existing_instance(settings: &Settings) -> bool`: On Windows calls `AllowSetForegroundWindow(ASFW_ANY)`, then sends `POST http://127.0.0.1:{bridge_port}/wake` with `x-lajufile-token` and a short timeout (e.g. `800 ms`). Only used by the binary `main.rs`, never inside `tests/`.
    - `focus_window_by_title(title: &str)`: On Windows uses `FindWindowW` matching `"LajuFile"`, `ShowWindow(hwnd, SW_RESTORE)` if minimized/hidden, and `SetForegroundWindow(hwnd)`. Called inside `LajuFileApp::show_window`.
    - `set_prevent_sleep(active: bool)`: On Windows calls `SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED)` when `active` is `true`, and `SetThreadExecutionState(ES_CONTINUOUS)` when `false`.
- **Immediate Window Wake & Anti-Sleep Tracking (`main.rs`, `engine.rs`, `app.rs`)**:
  - Keep `EngineConfig` struct untouched so all `tests/engine.rs` and `tests/bridge.rs` call sites remain unchanged.
  - Add `EngineHandle::set_wake_callback(&self, cb: Arc<dyn Fn() + Send + Sync>)` (or `Command::SetWakeCallback`), called once in `LajuFileApp::new`, plus `Snapshot.wake_seq: u64`.
  - When `Command::WakeWindow` arrives or a new `pending_capture` prompt opens in `engine.rs`, increment `wake_seq`, publish immediately, and invoke the registered wake callback (which calls `ctx.send_viewport_cmd(Minimized(false))`, `Visible(true)`, `Focus`, and `ctx.request_repaint()`).
  - In `LajuFileApp::poll_engine`, check `snapshot.wake_seq > self.last_wake_seq` -> call `self.show_window(ctx)` (`focus_window_by_title`), and check active `DownloadStatus::Downloading` presence -> update `self.sleep_inhibited` via a deduplicated state tracker; clear sleep inhibition in `Drop for LajuFileApp`.

## What Is Left to the Builder

- Verify `Action::StartSelfUpdate` and `ui/dialogs.rs` wiring against `UpdateInfo.checksums_url`.
- Verify `/DEBUG:NONE` vs `/PDBALTPATH:%_PDB%` compatibility with `/guard:cf` under local MSVC `cargo build --release` and re-audit resulting binaries/extension with REA (`inspect_binary_hardening`, `audit_release_exposure`, `analyze_javascript_application`) to confirm measurable improvement.
- Ensure CI check tests run locally (`cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `node --check extension/*.js`).
