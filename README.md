<p align="center">
  <img src="assets/lajufile.jpg" alt="LajuFile" width="760">
</p>

<h1 align="center">LajuFile</h1>

<p align="center">
  A fast, native download manager for Windows — built in Rust.
</p>

> **Status:** Milestone 1 (engine foundation) is complete. The desktop GUI, browser bridge, and Edge extension are in progress. There is no runnable desktop app yet.

## Why LajuFile

LajuFile is a personal project that aims to bring the essentials of a classic
download manager to Windows: segmented downloads, pause/resume, queues, speed
limits, and one-click capture from the browser. It is inspired by tools such as
Internet Download Manager, but the code and design are original.

## What works today

- Persistent settings in `%APPDATA%\LajuFile`
- SQLite download store (WAL, versioned schema, segments)
- HTTP metadata probe with `Range: bytes=0-0`
- Single-stream download to a `.lajufile` temp file, verified before the final
  file is exposed
- Safe finalization that never overwrites an existing file
- Windows-safe file names, deduplication, and reserved-device handling
- End-to-end test: probe → naming → store → download → mark done

## Roadmap

- [x] **M1 — Foundation:** workspace, settings, store, naming, probe, safe
      single-stream download, E2E tests
- [ ] **M2 — Engine:** segmented downloads, resume, queue, speed limit
- [ ] **M3 — Desktop app:** egui interface, tray, settings, autostart
- [ ] **M4 — Capture:** local bridge + Edge MV3 extension
- [ ] **M5 — Release:** hardening, docs, packaging

## Build

Requirements:

- Windows 10/11 x64
- Rust stable with the MSVC toolchain
- Visual Studio Build Tools with the C++ workload

```powershell
cargo test
cargo build
```

Milestone 1 is a library workspace (`lajufile-core`); the GUI binary arrives in
Milestone 3.

## Design

- Figma: [LajuFile — UI Design](https://www.figma.com/design/PASDzAEqJlF35dkv4L9eIx)
- Product design: [`docs/superpowers/specs/2026-11-14-lajufile-design.md`](docs/superpowers/specs/2026-11-14-lajufile-design.md)
- Milestone 1 plan: [`docs/superpowers/plans/2026-11-14-lajufile-milestone-1-foundation.md`](docs/superpowers/plans/2026-11-14-lajufile-milestone-1-foundation.md)

## License

MIT — see [LICENSE](LICENSE).
