<p align="center">
  <img src="assets/lajufile.jpg" alt="LajuFile" width="760">
</p>

<h1 align="center">LajuFile</h1>

<p align="center">
  A fast, native download manager for Windows — built in Rust.
</p>

> **Status:** Milestone 2 (desktop preview) is complete: a runnable Windows GUI
> with tray support, settings, and single-stream downloads. Segmented downloads,
> true resume, the browser bridge, and the Edge extension are next.

## Why LajuFile

LajuFile is a personal project that aims to bring the essentials of a classic
download manager to Windows: segmented downloads, pause/resume, queues, speed
limits, and one-click capture from the browser. It is inspired by tools such as
Internet Download Manager, but the code and design are original.

## What works today

- Native egui desktop window with download table, filters, and context menu
- Add URL with metadata probe, safe file naming, and duplicate active-URL check
- Single-stream downloads with a queue limited by `max_concurrent`
- Pause, restart (from zero in M2), remove, pause all, and resume all
- System tray: show, add URL, pause all, resume all, exit; close-to-tray
- Settings for save folder, concurrency, theme, and close-to-tray
- Light and dark themes
- Persistent settings in `%APPDATA%\LajuFile`
- SQLite download store (WAL, versioned schema, segments)
- HTTP metadata probe with `Range: bytes=0-0`
- Download to a `.lajufile` temp file, verified before the final file is exposed
- Safe finalization that never overwrites an existing file
- Windows-safe file names, deduplication, and reserved-device handling
- End-to-end tests: probe → naming → store → download → mark done

## Roadmap

- [x] **M1 — Foundation:** workspace, settings, store, naming, probe, safe
      single-stream download, E2E tests
- [x] **M2 — Desktop preview:** egui GUI, tray, settings, light/dark themes,
      queue, pause/restart, single-stream downloads
- [ ] **M3 — Engine:** segmented downloads, true resume, speed limit
- [ ] **M4 — Capture:** local bridge + Edge MV3 extension
- [ ] **M5 — Release:** hardening, docs, packaging

## Build

Requirements:

- Windows 10/11 x64
- Rust stable with the MSVC toolchain
- Visual Studio Build Tools with the C++ workload

```powershell
cargo test
cargo run -p lajufile-app
```

The debug build opens a console window for logs. Release builds use the Windows
GUI subsystem and do not show a console.

## Design

- Figma: [LajuFile — UI Design](https://www.figma.com/design/PASDzAEqJlF35dkv4L9eIx)
- Product design: [`docs/superpowers/specs/2026-11-14-lajufile-design.md`](docs/superpowers/specs/2026-11-14-lajufile-design.md)
- Milestone 1 plan: [`docs/superpowers/plans/2026-11-14-lajufile-milestone-1-foundation.md`](docs/superpowers/plans/2026-11-14-lajufile-milestone-1-foundation.md)
- Milestone 2 plan: [`docs/superpowers/plans/2026-11-14-lajufile-milestone-2-gui-tray.md`](docs/superpowers/plans/2026-11-14-lajufile-milestone-2-gui-tray.md)

## License

MIT — see [LICENSE](LICENSE).
