<p align="center">
  <img src="assets/lajufile.jpg" alt="LajuFile" width="760">
</p>

<h1 align="center">LajuFile</h1>
<p align="center">A fast, native download manager for Windows — built in Rust.</p>

> **Status:** Milestone 4 (browser capture) is complete. LajuFile now has a
> local bridge and an Edge MV3 extension, alongside segmented downloads,
> true resume, queues, and a global speed limit. Milestone 5 is release
> hardening and packaging.

## Why LajuFile

LajuFile is a personal project: a desktop download manager that keeps the
essentials of a classic download manager — segmented transfers, pause and
resume, queues, speed limits, and browser capture — in a small native
application. It is inspired by Internet Download Manager, but uses original
code and design.

## What works today

- Native egui window with a download table, sidebar filters, and a context menu.
- Add URL with metadata probe, file naming, and active duplicate detection.
- Segmented downloads with up to 32 parallel byte ranges.
- True resume using `If-Range`; without a validator, a 64 KiB prefix check.
- Queue with a configurable simultaneous-download limit.
- Global speed limit in KB/s (`0` means unlimited).
- Pause, resume, remove, pause all, and resume all.
- Tray icon: show, add URL, pause all, resume all, exit; close to tray.
- Settings for save folder, concurrency, connections, speed, theme, and close-to-tray.
- Light and dark themes.
- Persistent state in `%APPDATA%\LajuFile`; SQLite in WAL mode with per-segment progress.
- Probe uses `Range: bytes=0-0`; downloads use `Accept-Encoding: identity`.
- Downloads write to `<final>.lajufile`, are size-checked, and are finalized without overwriting an existing file.
- Windows-safe file names, duplicate-name handling, and reserved-device protection.
- Local bridge at `127.0.0.1:53841` with token authentication, CORS, and a 64 KiB body limit.
- `GET /health` and `POST /downloads` for browser integrations.
- Edge capture through `onDeterminingFilename`, with an `onCreated` fallback and a "Download with LajuFile" link menu.
- Browser cookies are stored with Windows DPAPI, never as plaintext.
- Capture settings in the app: status, port, token copy/regenerate, and Test connection.

## Browser capture

1. Start LajuFile and open **Settings → Browser capture**. Copy the port and token.
2. In Edge, open `edge://extensions`, enable **Developer mode**, and choose
   **Load unpacked**. Select the `extension/` folder in this repository.
3. Open the LajuFile Capture extension options and paste the port and token.
4. Click **Test connection**; the popup should show **Connected**.

The extension sends downloads to the local bridge:

- `201 Created` — LajuFile accepted the download and returns its id, filename, and size.
- `409 Conflict` — the URL is already active in LajuFile; the browser copy is cancelled.
- `422 Unprocessable Entity` — the probe failed; no LajuFile record is created.
- `401 Unauthorized` — the token does not match; the browser download continues.

The bridge only listens on localhost. The token is copied from Settings and can
be regenerated there; the old token stops working immediately. A port change
applies after restarting LajuFile.

### Capture limitations

- POST and form submissions are not captured.
- `blob:` URLs, DRM-protected media, and one-time URLs are not supported.
- Downloads started before LajuFile is running are not captured.
- Cookies are forwarded only for the original URL; cross-host redirect cookies are not supported.
- The popup has no "Open LajuFile" action because no protocol handler is registered yet.
- Edge may briefly show its download shelf before LajuFile cancels the browser copy.
- Pause persists progress at roughly one-second granularity.

## Roadmap

- [x] **M1 — Foundation:** store, probe, naming, single-stream download.
- [x] **M2 — Desktop app:** GUI, queue, tray, themes.
- [x] **M3 — Engine:** segmented downloads, true resume, global speed limit.
- [x] **M4 — Capture:** local bridge and Edge MV3 extension.
- [ ] **M5 — Release:** hardening, docs, packaging.

## Build

Requirements: Windows 10/11 x64, Rust stable with the MSVC toolchain, and
Visual Studio Build Tools with the C++ workload.

```powershell
cargo test
cargo run -p lajufile-app
cargo build --release -p lajufile-app
```

Debug builds keep a console for diagnostics. Release builds use the Windows
GUI subsystem and do not open a console window.

## Design

- Figma: <https://www.figma.com/design/PASDzAEqJlF35dkv4L9eIx>
- Spec: [`docs/superpowers/specs/2026-11-14-lajufile-design.md`](docs/superpowers/specs/2026-11-14-lajufile-design.md)
- Milestone 1 plan: [`docs/superpowers/plans/2026-11-14-lajufile-milestone-1-foundation.md`](docs/superpowers/plans/2026-11-14-lajufile-milestone-1-foundation.md)
- Milestone 2 plan: [`docs/superpowers/plans/2026-11-14-lajufile-milestone-2-gui-tray.md`](docs/superpowers/plans/2026-11-14-lajufile-milestone-2-gui-tray.md)
- Milestone 3 plan: [`docs/superpowers/plans/2026-11-15-lajufile-milestone-3-engine.md`](docs/superpowers/plans/2026-11-15-lajufile-milestone-3-engine.md)
- Milestone 4 plan: [`docs/superpowers/plans/2026-11-15-lajufile-milestone-4-bridge-extension.md`](docs/superpowers/plans/2026-11-15-lajufile-milestone-4-bridge-extension.md)

## License

MIT — see [LICENSE](LICENSE).
