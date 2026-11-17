# LajuFile Hardening, Verified Packaging & Windows Polish Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Harden `LajuFile` releases, installer/self-updater integrity, extension/bridge trust boundaries, accessibility, and Windows single-instance / anti-sleep runtime behavior without bloating its `egui` + `native-tls` + `127.0.0.1` architecture.

**Architecture:** Extend existing workspace modules (`installer.rs`, `updater.rs`, `bridge.rs`, `engine.rs`, `app.rs`, `main.rs`) and add one small cross-platform OS shim (`platform.rs`) plus MSVC/release build config (`.cargo/config.toml`, `Cargo.toml`, `build.rs`, CI scripts) and MV3 message-sender guards (`extension/background.js`), keeping `EngineConfig` backwards-compatible for all existing test suites.

**Tech Stack:** Rust 2024 (`eframe`/`egui 0.37`, `axum 0.8`, `reqwest 0.13`, `tokio 1`, `windows-sys 0.61`), Vanilla JS (Chromium MV3), PowerShell & GitHub Actions.

**Spec:** [docs/superpowers/specs/2026-11-17-lajufile-hardening-polish-design.md](../specs/2026-11-17-lajufile-hardening-polish-design.md)

## Global Constraints

- Maintain `edition = "2024"` and `rust-version = "1.85"` in `Cargo.toml`.
- Keep all existing workspace tests green (`cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all -- --check`, `node --check extension/*.js`) on `windows-latest`, `ubuntu-latest`, and `macos-latest`.
- Preserve unsigned installer compatibility alongside PE Authenticode-signed installers; do not commit private Code Signing keys (`WINDOWS_PFX_BASE64` remains optional CI/local env secret).
- Never break existing `EngineConfig { data_dir, db_path, settings }` callers in tests, and never expose files outside `extension/` into the packed `extension/` directory inside installers.
- Use standard library or already-present workspace dependencies (`windows-sys`, `lajufile_core::checksum`); avoid new heavyweight crates.

## Review Focus

1. **PE Authenticode 8-byte alignment padding before `WIN_CERTIFICATE`**: Signing a setup `.exe` may insert `0..=7` zero bytes between `LAJUFILE_PK1` and `cert_offset`; `effective_payload_end` must trim only valid preceding NUL padding and reject forged/out-of-bounds `DataDirectory[4]` offsets cleanly.
2. **Chromium MV3 `options.html` tab sender (`open_in_tab: true`)**: Options tabs populate `sender.tab` alongside `sender.url = chrome-extension://<id>/...`; `background.js` must allow `config-updated` from extension UI tabs/popups while blocking content scripts and web frames.
3. **DNS-rebinding `Host` headers & web `localhost` CORS**: Bridge must reject non-loopback `Host` headers (`403 Forbidden` across all routes including `/health`) and reject `http://localhost` / `http://127.0.0.1` web origins in CORS while allowing extension schemes and legitimate `127.0.0.1` / `localhost` / `[::1]` requests.
4. **`SHA256SUMS.txt` verification & fail-closed self-updates**: `updater.rs` must handle `<hex>  file`, `<hex> *file`, and `./file`, clean `.part` on hash mismatch or missing `SHA256SUMS.txt`, and verify Windows `.exe` payloads via `installer::parse_payload` rather than trailing byte matching.
5. **Single-instance wake without corrupting `browser_paired` + Anti-sleep cleanup**: `POST /wake` must authenticate with `authorize` (not `authorize_and_pair`), single-instance wake must call `AllowSetForegroundWindow` before triggering `/wake`, and `SleepGuard` must restore `ES_CONTINUOUS` when downloads stop or on `Drop`.

---

### Task 1: PE Authenticode-Safe Payload Extraction (`installer.rs`)

**Files:**
- Modify: `crates/lajufile-app/src/installer.rs:131-178` (`parse_payload`, add `effective_payload_end`)
- Test: `crates/lajufile-app/src/installer.rs:665-754`

**Interfaces:**
- Produces: `pub fn parse_payload(exe_bytes: &[u8]) -> Result<(usize, Vec<PayloadEntry>), String>` supporting both raw unsigned blobs and PE32/PE32+ images carrying `IMAGE_DIRECTORY_ENTRY_SECURITY` (`DataDirectory[4]`) + `0..=7` alignment NULs.

- [ ] **Step 1: Write failing tests in `crates/lajufile-app/src/installer.rs`**

Add helper `synthetic_pe32_plus_with_security(stub_len: usize, pad_len: usize, cert_blob: &[u8]) -> Vec<u8>` (or inline test helper constructing valid `"MZ"`, `e_lfanew = 0x40`, `"PE\0\0"`, COFF `SizeOfOptionalHeader = 240`, Optional Header magic `0x20B`, `NumberOfRvaAndSizes = 16`, `DataDirectory[4]` `(cert_offset, cert_size)`). Write two tests:
1. `parse_payload_reads_authenticode_signed_pe_with_alignment_padding`: builds synthetic PE32+ stub + `pack_entries` + `5` zero alignment bytes + `fake_pkcs7` cert + valid `DataDirectory[4]`, asserts `parse_payload(&signed)` succeeds with exact `stub_end` and recovered entries, and mutating a payload byte still fails SHA-256.
2. `parse_payload_ignores_malformed_or_oob_security_directory_safely`: tests bogus `e_lfanew` / `cert_offset + cert_size > len` fails cleanly without panic.

- [ ] **Step 2: Run unit tests to verify failure**

Run: `cargo test -p lajufile-app installer::tests`
Expected: `parse_payload_reads_authenticode_signed_pe_with_alignment_padding` FAILS with `"installer payload magic not found"`.

- [ ] **Step 3: Implement `effective_payload_end(bytes: &[u8]) -> usize` and wire into `parse_payload`**

In `crates/lajufile-app/src/installer.rs`:
- Read `"MZ"` + `e_lfanew` (`0x3C..0x40`), `"PE\0\0"`, `magic` at `opt` (`0x10B` PE32 -> `rva_count_off = opt + 92`, `sec_dir_off = opt + 128`; `0x20B` PE32+ -> `rva_count_off = opt + 108`, `sec_dir_off = opt + 144`).
- If `number_of_rva_and_sizes > 4`, read `(cert_offset, cert_size)`. Validate `cert_offset > FOOTER_LEN`, `cert_size > 0`, and `(cert_offset as u64).checked_add(cert_size as u64) <= Some(bytes.len() as u64)`.
- Slice `before_cert = &bytes[..cert_offset as usize]`. If `before_cert.ends_with(MAGIC)`, return `before_cert.len()`. Otherwise scan `k in 1..=7` where `before_cert.len() >= FOOTER_LEN + k` and `before_cert[before_cert.len() - k..].iter().all(|&b| b == 0)` and `&before_cert[before_cert.len() - k - 12..before_cert.len() - k] == MAGIC`, returning `before_cert.len() - k`.
- Otherwise return `bytes.len()`.
- Use `let payload_end = effective_payload_end(exe_bytes);` in `parse_payload`.

- [ ] **Step 4: Run `installer::tests` and verify pass**

Run: `cargo test -p lajufile-app installer::tests`
Expected: PASS (all existing and 2 new installer tests pass).

- [ ] **Step 5: Commit Task 1**

```powershell
git add crates/lajufile-app/src/installer.rs
git commit -m "fix(installer): support Authenticode PE security table in payload parser"
```

---

### Task 2: Verified Self-Updater (`SHA256SUMS.txt`), Signing Hooks & Parallel Release CI

**Files:**
- Modify: `crates/lajufile-app/src/updater.rs:37-277,324-379`
- Modify: `crates/lajufile-app/src/app.rs:890-901`
- Modify: `installer/build-installer.ps1`
- Modify: `.github/workflows/release.yml`
- Test: `crates/lajufile-app/src/updater.rs:324-379` (`axum` / `TcpListener` local HTTP test server? Notice `crates/lajufile-app` depends on `axum`, `tokio`, `reqwest`, `lajufile-core`!)

**Interfaces:**
- Consumes: `crate::installer::parse_payload` from Task 1, `lajufile_core::checksum::sha256_file`.
- Produces:
  - `UpdateInfo.checksums_url: Option<String>` (populated when release has asset `SHA256SUMS.txt`, validated via `validate_update_download_url`).
  - `pub fn parse_sha256_manifest_entry(manifest: &str, asset_name: &str) -> Option<String>`
  - `pub fn verify_downloaded_installer(installer_path: &Path, expected_sha256: &str) -> Result<(), String>`
  - `pub fn spawn_download(asset_url: String, asset_name: String, checksums_url: Option<String>, tx: mpsc::Sender<UpdateEvent>)`

- [ ] **Step 1: Write failing tests in `crates/lajufile-app/src/updater.rs`**

Write unit tests in `updater::tests`:
1. `sha256_manifest_parser_handles_standard_binary_and_dot_slash_formats`: tests parsing `"AABB...  LajuFile-v1.0.5-windows-x64-setup.exe\nccdd... *./LajuFile-v1.0.5-linux-x64-setup.run\n"`, rejects non-64-hex or non-matching lines.
2. `verify_downloaded_installer_rejects_hash_mismatch_and_unsigned_invalid_payload`: writes packed payload using `installer::pack_entries` (plus synthetic Authenticode PE version), tests matching hash passes, mismatched SHA-256 errors `"SHA-256 verification failed"`, and corrupted trailer errors.
3. `download_installer_requires_checksums_url_and_verifies_via_http`: tests missing `checksums_url` (`None`) returns `Err` containing `"SHA256SUMS.txt"`. (Use loopback `std::net::TcpListener` or `verify_downloaded_installer` cleanly without needing external internet, plus helper `verify_installer_bytes` / `download_and_verify` unit test if loopback URL allowed — wait: `validate_update_download_url` only allows `github.com` hosts, so test `download_installer(..., None, &tx)` for fail-closed missing manifest AND `fetch_expected_sha256` / `parse_sha256_manifest_entry` + `verify_downloaded_installer` directly!).

- [ ] **Step 2: Run `updater::tests` to verify failure**

Run: `cargo test -p lajufile-app updater::tests`
Expected: FAIL because `checksums_url`, `parse_sha256_manifest_entry`, and `verify_downloaded_installer` do not exist yet.

- [ ] **Step 3: Implement verified self-update in `updater.rs` and update `app.rs` caller**

- Add `pub checksums_url: Option<String>` to `UpdateInfo` (note: also check `sha256_checksums_asset(assets: &[ReleaseAsset]) -> Option<&ReleaseAsset>` looking for exact filename `"SHA256SUMS.txt"`).
- Implement `parse_sha256_manifest_entry(manifest: &str, asset_name: &str) -> Option<String>`: split whitespace (`digest`, `name`), validate `digest.len() == 64 && digest.bytes().all(|b| b.is_ascii_hexdigit())`, trim leading `'*'` and `"./"` on `name`, compare exact filename (`== asset_name`), return lowercase hex.
- Implement `verify_downloaded_installer(path: &Path, expected_sha256: &str) -> Result<(), String>`: compute `sha256_file(path)`, compare case-insensitively with `expected_sha256`; when `cfg!(windows)`, read file and call `crate::installer::parse_payload(&bytes)` instead of `ends_with(b"LAJUFILE_PK1")`.
- Update `spawn_download(asset_url: String, asset_name: String, checksums_url: Option<String>, tx: mpsc::Sender<UpdateEvent>)` and `download_installer`:
  - Fail immediately if `checksums_url` is `None` (`"Release is missing SHA256SUMS.txt verification manifest — download manually from GitHub Releases"`).
  - Validate & fetch `checksums_url` via `build_update_client(Duration::from_secs(20))`, extract expected SHA-256 via `parse_sha256_manifest_entry(&manifest_text, asset_name)` (error cleanly if entry missing), download installer `.part`, call `verify_downloaded_installer(&part, &expected_sha256)` deleting `.part` on any error before renaming.
- Update `crates/lajufile-app/src/app.rs:899`: pass `info.checksums_url.clone()` from `if let UpdateUiState::Available(info) = &self.update_state`.

- [ ] **Step 4: Update `installer/build-installer.ps1` and `.github/workflows/release.yml`**

- In `installer/build-installer.ps1`:
  - Add optional `-PfxPath`, `-PfxPassword` params (defaulting to `$env:WINDOWS_PFX_PATH` / decoded `$env:WINDOWS_PFX_BASE64`, `$env:WINDOWS_PFX_PASSWORD`, `$env:WINDOWS_TIMESTAMP_URL ?? "http://timestamp.digicert.com"`).
  - Locate `signtool.exe` in Windows Kits when signing is configured; sign `target\release\lajufile.exe` **before** running `lajufile-setup.exe --pack` (keeping `lajufile-setup.exe` `--stub` unsigned!), sign `$outExe` **after** packing, and verify via `signtool verify /pa`. Clean up any temp PFX file in `finally`.
  - Compute SHA-256 (`Get-FileHash -Algorithm SHA256`) and write lowercase `SHA256SUMS.txt` in `dist\`.
- In `.github/workflows/release.yml`:
  - Allow `windows`, `linux`, `macos` jobs to run in parallel (remove `needs: windows` / `needs: linux`), each uploading its built installer(s) via `actions/upload-artifact@v4`.
  - On `windows`, decode `secrets.WINDOWS_PFX_BASE64` into temp `.pfx` only if non-empty, sign `target/release/lajufile.exe` before packing and sign `dist/LajuFile-${tag}-windows-x64-setup.exe` after packing (remove temporary `.pfx` afterwards with `if: always()`).
  - Add `publish` job (`needs: [windows, linux, macos]`, `runs-on: ubuntu-latest`) downloading all artifacts into `release-assets/`, generating sorted lowercase `SHA256SUMS.txt` with `sha256sum`, verifying via `sha256sum -c SHA256SUMS.txt`, and uploading all installers + `SHA256SUMS.txt` to `${{ github.ref_name }}`.

- [ ] **Step 5: Run `updater::tests` + check PowerShell syntax & workspace tests**

Run: `cargo test -p lajufile-app updater::tests; cargo check --workspace --all-targets`
Expected: PASS with 0 errors.

- [ ] **Step 6: Commit Task 2**

```powershell
git add crates/lajufile-app/src/updater.rs crates/lajufile-app/src/app.rs installer/build-installer.ps1 .github/workflows/release.yml
git commit -m "feat(updater,ci): verify updates via SHA256SUMS.txt and support Authenticode signing"
```

---

### Task 3: Compiler Hardening, Release Profile & `accesskit` Support

**Files:**
- Modify: `Cargo.toml:12-31`
- Create: `.cargo/config.toml`
- Modify: `crates/lajufile-app/build.rs:1-40`

**Interfaces:**
- Produces: MSVC Control Flow Guard (`/guard:cf`), stripped/sanitized release PDB metadata, optimized release profile (`lto = "thin"`, `codegen-units = 1`, `strip = "debuginfo"`), `eframe/accesskit`, and `windows-sys` power/threading/window features ready for Task 5.

- [ ] **Step 1: Verify baseline REA state before hardening changes (already recorded) & inspect exact Cargo dependency resolution**

Check whether `cargo check --workspace` passes right now before changes, or add features carefully.

- [ ] **Step 2: Update `Cargo.toml`, create `.cargo/config.toml`, and update `crates/lajufile-app/build.rs`**

1. Root `Cargo.toml`:
   - Add `"accesskit"` to `eframe` features: `features = ["accesskit", "default_fonts", "glow", "clipboard", "links", "x11", "wayland"]`.
   - Extend `windows-sys` features: `features = ["Win32_Foundation", "Win32_Security_Cryptography", "Win32_System_Memory", "Win32_System_Power", "Win32_System_Threading", "Win32_UI_WindowsAndMessaging"]`.
   - Add `[profile.release]` with `lto = "thin"`, `codegen-units = 1`, `strip = "debuginfo"`.
2. `.cargo/config.toml`:
   ```toml
   [target.'cfg(all(target_os = "windows", target_env = "msvc"))']
   rustflags = ["-C", "control-flow-guard=yes", "-C", "link-arg=/guard:cf"]
   ```
3. `crates/lajufile-app/build.rs`:
   - Inside Windows branch (`CARGO_CFG_TARGET_OS == "windows"`), when `std::env::var("PROFILE").as_deref() == Ok("release")` and `std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc")`, emit `println!("cargo:rustc-link-arg-bins=/DEBUG:NONE");` (test that `link.exe` accepts `/DEBUG:NONE` alongside `/guard:cf`; if MSVC rejects `/DEBUG:NONE` cleanly replace with `/PDBALTPATH:%_PDB%` and ledger ruling).

- [ ] **Step 3: Build release binary & run full workspace tests + REA binary verification**

Run:
```powershell
cargo test --workspace
cargo build --release -p lajufile-app --bin lajufile --bin lajufile-setup
```
Then run REA `inspect_binary_hardening` and `audit_release_exposure` on `target/release/lajufile.exe` and `target/release/lajufile-setup.exe`.
Expected:
- `guard_cf: enabled` AND `cfg_instrumented: enabled`
- `native.pdb_path` stripped (`not_observed` / zero path leaks if `/DEBUG:NONE` worked cleanly)
- Zero broken tests or `link.exe` errors.

- [ ] **Step 4: Commit Task 3**

```powershell
git add Cargo.toml Cargo.lock .cargo/config.toml crates/lajufile-app/build.rs
git commit -m "build(hardening): enable MSVC Control Flow Guard, release LTO, accesskit, and strip release PDB"
```

---

### Task 4: Extension Sender Validation & Localhost Bridge Hardening

**Files:**
- Modify: `extension/background.js:484-552` (plus bounded map cleanup on `handledIds` / `bypassUrls`)
- Create: `crates/lajufile-app/tests/extension_sender.mjs` (NOTE: outside `extension/` so installer does not package test scripts!)
- Modify: `crates/lajufile-app/src/bridge.rs:160-200`
- Modify: `crates/lajufile-app/tests/bridge.rs`
- Modify: `.github/workflows/ci.yml`

**Interfaces:**
- Consumes: `authorize` & `Command` enum in `engine.rs` (`Command::WakeWindow` or reserve `POST /wake` cleanly alongside Task 5 — note: add `Command::WakeWindow` stub in `engine.rs` if Task 4 commits before Task 5, or declare `POST /wake` cleanly with Task 4/5 coordination! **Wait — check dependency order!** Adding `Command::WakeWindow` match arm in `engine.rs` right here OR splitting `/wake` into Task 5 avoids broken intermediate commit! **Yes: keep `POST /wake` + `Command::WakeWindow` together in Task 5 so Task 4 compiles & passes 100% independently!**).

- [ ] **Step 1: Write failing tests for Bridge (`tests/bridge.rs`) and Extension (`tests/extension_sender.mjs`)**

1. In `crates/lajufile-app/tests/bridge.rs`, add tests:
   - `bridge_rejects_dns_rebinding_host_headers`: sends `GET /health` and `POST /downloads` (with valid token) overriding `.header("host", "evil.example:53841")` -> expects `StatusCode::FORBIDDEN`. Also tests valid `Host: localhost:53841` & `Host: [::1]:53841` -> passes.
   - `bridge_cors_allows_extensions_only_and_blocks_localhost_web_origins`: sends `OPTIONS /downloads` with `Origin: http://localhost:3000` and `Origin: http://127.0.0.1:8080` -> asserts `access-control-allow-origin` header is absent (while `chrome-extension://abc` still succeeds).
2. Create `crates/lajufile-app/tests/extension_sender.mjs` (using `node:test` + `node:assert/strict`) loading `extension/background.js` with minimal mocked `chrome` globals capturing the registered `onMessage` listener, asserting:
   - Rejects spoofed `sender.id !== chrome.runtime.id`.
   - Allows `config-updated` from `options.html` WITH `sender.tab` (`open_in_tab: true`) and from `popup.html` (`!sender.tab`), and REJECTS `config-updated` from `https://www.youtube.com/watch?v=abc` content script.
   - Allows `video-formats` / `video-download` from YouTube top frame (`frameId: 0`), REJECTS them from subframes (`frameId: 1`) and non-YouTube pages (`https://evil.example/`).
   - Rejects non-HTTP(S) target URLs (`file:///...`, `javascript:...`) on `intercept-link` & `bypass-url`.

- [ ] **Step 2: Run new tests to verify failure**

Run: `cargo test -p lajufile-app --test bridge; node --test crates/lajufile-app/tests/extension_sender.mjs`
Expected: Both new test suites FAIL.

- [ ] **Step 3: Implement `background.js` sender validation and `bridge.rs` Host + CORS hardening**

- In `extension/background.js`:
  - Bound `bypassUrls` and `handledIds` to max 512 entries (prune expired / oldest key).
  - Add pure helpers `isHttpUrl(raw)`, `isYouTubePageUrl(raw)`, `isOwnExtensionPage(sender)`, and full validation in `chrome.runtime.onMessage.addListener((message, sender, sendResponse) => ...)` matching the Spec verbatim. Return `false` on rejected/unhandled messages.
- In `crates/lajufile-app/src/bridge.rs`:
  - Restrict `is_allowed_origin` to `chrome-extension://`, `moz-extension://`, `extension://`, `safari-web-extension://`.
  - Implement `is_loopback_host_header(value: &HeaderValue) -> bool` handling `127.0.0.1[:port]`, `localhost[:port]`, `[::1][:port]` (port validated numeric `u16`).
  - Add `axum::middleware::from_fn(require_loopback_host)` layer on `router(...)` rejecting invalid/missing `Host` with `StatusCode::FORBIDDEN`.
- In `.github/workflows/ci.yml`: add `node --test crates/lajufile-app/tests/extension_sender.mjs` under `"Check extension scripts"`.

- [ ] **Step 4: Run tests + REA `analyze_javascript_application` on `extension/`**

Run: `node --check extension/background.js; node --test crates/lajufile-app/tests/extension_sender.mjs; cargo test -p lajufile-app --test bridge`
Expected: PASS, and REA `analyze_javascript_application` reports non-zero `sender_validation_observations`.

- [ ] **Step 5: Commit Task 4**

```powershell
git add extension/background.js crates/lajufile-app/tests/extension_sender.mjs crates/lajufile-app/src/bridge.rs crates/lajufile-app/tests/bridge.rs .github/workflows/ci.yml
git commit -m "fix(security): validate extension message senders and block bridge DNS rebinding"
```

---

### Task 5: Windows Single-Instance Wake, `/wake` Route & Anti-Sleep Polish

**Files:**
- Create: `crates/lajufile-app/src/platform.rs`
- Modify: `crates/lajufile-app/src/lib.rs` (expose `pub mod platform;`)
- Modify: `crates/lajufile-app/src/bridge.rs` (add `POST /wake` with `authorize` — NOT `authorize_and_pair`)
- Modify: `crates/lajufile-app/src/engine.rs:47-160,186-196,487-677,812-858,2077-2111` (`Command::WakeWindow`, `Snapshot.wake_seq`, `EngineHandle::set_wake_callback`, immediate publish)
- Modify: `crates/lajufile-app/src/app.rs` (`last_wake_seq`, `sleep_guard`, register wake callback, focus window via `platform`, release sleep on `Drop`)
- Modify: `crates/lajufile-app/src/main.rs` (`SingleInstanceGuard` + `signal_existing_instance` on duplicate launch)
- Test: `crates/lajufile-app/src/platform.rs` (`SleepGuard` state machine unit tests), `crates/lajufile-app/tests/bridge.rs` (`wake_requires_token_and_preserves_browser_paired_false`), `crates/lajufile-app/tests/engine.rs` (`wake_window_increments_wake_seq_and_invokes_callback`)

**Interfaces:**
- Consumes: `bridge::authorize` from `bridge.rs`, `Settings` from `lajufile-core`.
- Produces:
  - `platform::SingleInstanceGuard::acquire() -> Option<SingleInstanceGuard>`
  - `platform::signal_existing_instance(settings: &Settings) -> bool`
  - `platform::focus_window_by_title(title: &str)`
  - `platform::SleepGuard` (`new(on_change)`, `sync(&mut self, active: bool)`, clears on `Drop`)
  - `Command::WakeWindow`, `Snapshot.wake_seq: u64`, `EngineHandle::set_wake_callback(&self, cb: Arc<dyn Fn() + Send + Sync>)`

- [ ] **Step 1: Write failing tests in `tests/bridge.rs`, `tests/engine.rs`, and `platform.rs`**

1. `crates/lajufile-app/tests/bridge.rs`: `wake_requires_token_and_preserves_browser_paired_false`: verifies unauthenticated `POST /wake` returns `401 Unauthorized`, authenticated `POST /wake` returns `200 OK`, increments `snapshot.wake_seq`, and leaves `snapshot.settings.browser_paired == false`.
2. `crates/lajufile-app/tests/engine.rs`: `wake_window_invokes_callback_and_updates_wake_seq`: calls `handle.set_wake_callback(...)` with an `Arc<AtomicUsize>`, sends `Command::WakeWindow`, asserts callback fired and `wake_seq` incremented promptly (< 2s).
3. `crates/lajufile-app/src/platform.rs` `#[cfg(test)]`: `sleep_guard_deduplicates_transitions_and_resets_on_drop` verifying `SleepGuard` invokes its backend toggle only on `false -> true` and `true -> false`, and resets to `false` when dropped while `true`.

- [ ] **Step 2: Run new tests to verify they fail before implementation**

Run: `cargo test -p lajufile-app --test bridge wake_requires && cargo test -p lajufile-app --test engine wake_window`
Expected: FAIL to compile (`POST /wake`, `wake_seq`, `set_wake_callback`, `Command::WakeWindow` undefined).

- [ ] **Step 3: Implement `platform.rs` and wire `bridge.rs`, `engine.rs`, `app.rs`, and `main.rs`**

1. `crates/lajufile-app/src/platform.rs` (`pub mod platform;` in `lib.rs`):
   - `SingleInstanceGuard`: Windows `CreateMutexW(null(), 0, wide("Local\\LajuFileSingleInstance"))` checking `GetLastError() != ERROR_ALREADY_EXISTS`; closes handle on `Drop`. Non-Windows: lock-free probe via `Settings::load_or_create(&paths::data_dir())` health/wake check (wait: on non-Windows without mutex, `acquire()` attempts loopback `/wake` check only if `main.rs` detects an already-running instance via non-blocking `/health` + `/wake`, or returns `Some` otherwise — no extra lockfile crate needed!). Note: on Windows, `CreateMutexW` is kernel-scoped and atomic so a starting instance and existing instance never race.
   - `signal_existing_instance(settings: &Settings) -> bool`: on Windows calls `AllowSetForegroundWindow(ASFW_ANY)`, then uses blocking HTTP over `TcpStream::connect_timeout` (`127.0.0.1:bridge_port`, 800ms timeout) sending `POST /wake` with `x-lajufile-token: <token>` (`Host: 127.0.0.1:{port}`) — reuses std `TcpStream` cleanly without building a temporary Tokio runtime!
   - `focus_window_by_title(title: &str)`: on Windows calls `FindWindowW(null(), wide(title).as_ptr())`; if valid HWND, calls `ShowWindow(hwnd, SW_RESTORE)` when `IsIconic(hwnd) != 0 || IsWindowVisible(hwnd) == 0` and `SetForegroundWindow(hwnd)`.
   - `SleepGuard`: generic tracker with `set_prevent_sleep(bool)` calling `SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED)` / `SetThreadExecutionState(ES_CONTINUOUS)` on Windows.
2. `crates/lajufile-app/src/engine.rs`:
   - Add `pub wake_seq: u64` to `Snapshot` (`0` on error startup).
   - Add `wake_callback: Arc<RwLock<Option<Arc<dyn Fn() + Send + Sync>>>>` to `EngineHandle` and `EngineState`, with `EngineHandle::set_wake_callback(&self, cb: Arc<dyn Fn() + Send + Sync>)`.
   - Add `Command::WakeWindow`: increments `self.wake_seq`, sets `immediate_publish = true`, invokes wake callback. Also trigger wake callback + `immediate_publish = true` when inserting a `PendingCapture`.
   - In `engine_loop`, publish immediately when `state.immediate_publish` is true (`state.immediate_publish = false`).
3. `crates/lajufile-app/src/bridge.rs`:
   - Route `.route("/wake", post(wake))` calling `authorize(&headers, &state.token)?` and sending `Command::WakeWindow`, returning `(StatusCode::OK, Json(json!({ "ok": true })))`.
4. `crates/lajufile-app/src/app.rs`:
   - Register wake callback in `LajuFileApp::new` on `engine` calling `ctx.send_viewport_cmd(Minimized(false))`, `Visible(true)`, `Focus`, `request_repaint()`, and `platform::focus_window_by_title("LajuFile")`.
   - Track `last_wake_seq: u64` and `sleep_guard: SleepGuard` in `LajuFileApp`; in `poll_engine`, restore window if `snapshot.wake_seq > self.last_wake_seq`, and call `self.sleep_guard.sync(any_downloading)`.
   - Update `show_window` to call `crate::platform::focus_window_by_title("LajuFile")`.
5. `crates/lajufile-app/src/main.rs`:
   - Hold `_single_instance` via `platform::SingleInstanceGuard::acquire()`: if `None`, load `Settings::load_or_create(&paths::data_dir())` and call `platform::signal_existing_instance(&settings)` and return `Ok(())`.

- [ ] **Step 4: Run full test suite & CI checks**

Run:
```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
node --check extension/background.js
node --test crates/lajufile-app/tests/extension_sender.mjs
```
Expected: ALL pass with zero warnings and zero failures.

- [ ] **Step 5: Build final release & installer + verify with REA (`reverse-engineer-anything`)**

Run:
```powershell
powershell -ExecutionPolicy Bypass -File installer\build-installer.ps1
```
Then run REA verification against `target/release/lajufile.exe`, `dist/LajuFile-v1.0.4-Windows-x64-Setup.exe`, and `extension/` (`inspect_binary_hardening`, `audit_release_exposure`, `analyze_javascript_application`) and compare before/after metrics!

- [ ] **Step 6: Commit Task 5**

```powershell
git add crates/lajufile-app/src/platform.rs crates/lajufile-app/src/lib.rs crates/lajufile-app/src/bridge.rs crates/lajufile-app/src/engine.rs crates/lajufile-app/src/app.rs crates/lajufile-app/src/main.rs crates/lajufile-app/tests/bridge.rs crates/lajufile-app/tests/engine.rs
git commit -m "feat(windows): add single-instance wake, foreground focus, and download anti-sleep"
```
