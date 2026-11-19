<div align="center">

<img src="assets/icons/icon-128.png" alt="LajuFile Logo" width="88" />

# LajuFile

**Manajer unduhan multi-koneksi berkecepatan tinggi, ringan, aman, dan modern untuk Windows, macOS & Linux.**  
*Fast, lightweight, secure, multi-connection download manager built with Rust.*

[![Version](https://img.shields.io/badge/version-v1.0.7-3B82F6?style=flat-square)](#)
[![Rust](https://img.shields.io/badge/Rust-2024%20%2F%20MSRV%201.85-orange?style=flat-square)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/license-MIT-10B981?style=flat-square)](LICENSE)
[![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-6366F1?style=flat-square)](#instalasi--pembaruan-windows-macos--linux)

[🇮🇩 Bahasa Indonesia](#-bahasa-indonesia) · [🇬🇧 English](#-english) · [🔒 Security](#-keamanan--arsitektur--security-architecture) · [📋 Changelog](CHANGELOG.md) · [🛡️ Privacy](PRIVACY.md) · [📜 Lisensi](LICENSE)

</div>

---

## 🇮🇩 Bahasa Indonesia

**LajuFile** adalah manajer unduhan desktop berbasis **Rust** untuk **Windows, macOS, dan Linux** yang mempercepat unduhan HTTP/HTTPS hingga 32 segmen paralel secara otomatis, terintegrasi langsung dengan browser modern serta **`yt-dlp` + `ffmpeg`** untuk video dan audio, serta dilengkapi **Setup Installer native** dan **Auto-Updater** di setiap sistem operasi.

### ✨ Fitur Utama

| Kategori | Kemampuan |
| --- | --- |
| **Mesin Unduhan Cepat** | Hingga **32 koneksi paralel** per berkas (`Range` probing + prefix sanity check 64 KiB + resume otomatis), kontrol antrean serentak (`1–10`), pembatas kecepatan global cepat dari status bar, routing folder otomatis per kategori (**Category Folders**), serta verifikasi **SHA-256** di Properties. |
| **Visual Progres Real-Time** | Peta Segmen (**Segment Map**) + grafik kecepatan **Sparkline**, jendela progres mengambang yang bisa dipindahkan serta disembunyikan ke background, estimasi ETA akurat, tema **Dark & Light**, dan skala antarmuka `80%–150%`. |
| **Batch Smart Grabber (`Ctrl+B`)** | Antrekan banyak URL sekaligus, impor berkas daftar URL `.txt`, ekspansi pola angka wildcard (`file_*.zip`), dan **Fetch Links from Web Page** yang aman terhadap SSRF dan DNS rebinding. |
| **Integrasi Browser Lengkap** | Wizard Setup Ekstensi di menu `Browser` untuk **Chrome, Edge, Brave, Opera, Vivaldi, dan Firefox** (Windows, macOS, Linux) dengan kode **Quick-Pair**, penangkapan instan Firefox, filter domain/ekstensi tersinkron, tombol `Alt+Click` bypass ke browser, dan panel kualitas video mengambang di YouTube. |
| **Video & Audio (`yt-dlp` + `ffmpeg`)** | Unduh MP4 H.264 / Audio M4A otomatis lintas OS (termasuk dukungan proxy, batas kecepatan global, **Subtitles**, dan **Chapters**) dengan verifikasi **SHA-256** wajib pada instalasi tool pertama kali. |
| **Multi-Bahasa & RTL (`View -> Language...`)** | Katalog **86 bahasa** dengan deteksi bahasa OS otomatis, penanda akurat bahasa *Built-in* vs *TSV / EN*, dukungan terjemahan kustom `.tsv`, tata letak **RTL**, dan format angka lokal. |
| **Stabilitas & Platform Native** | Pembaruan in-app sadar arsitektur (`x64`/`arm64`) dari menu **Bantuan -> Periksa pembaruan...**, single-instance lock (`CreateMutexW` / `flock`), anti-sleep saat mengunduh (Windows, macOS `caffeinate`, Linux `systemd-inhibit`), drag-and-drop `.url` / `.txt` / `.ef2`, serta **System Tray**. |

### 📦 Instalasi & Pembaruan (Windows, macOS & Linux)

Unduh installer sesuai sistem operasi & arsitektur dari halaman [Releases](../../releases):

- **Windows:** Jalankan **`LajuFile-v1.0.7-windows-x64-setup.exe`**, setujui lisensi, lalu klik **Install** (memasang ke `%LOCALAPPDATA%\Programs\LajuFile`, membuat pintasan Desktop/Start Menu, dan mendaftarkan **Add/Remove Programs** + `uninstall.exe`).
- **macOS:** Buka **`LajuFile-v1.0.7-macos-arm64-setup.dmg`** (atau build `x64` sesuai Mac Anda) dan jalankan **`LajuFile Setup.app`**, yang memasang bundle **`LajuFile.app`** ke `~/Applications/LajuFile.app` lengkap dengan `extension/` dan `uninstall`.
- **Linux:** Jalankan **`chmod +x LajuFile-v1.0.7-linux-x64-setup.run && ./LajuFile-v1.0.7-linux-x64-setup.run`** (tersedia pula varian `arm64` di rilis ARM), memasang ke `~/.local/share/lajufile`, membuat launcher `.desktop`, dan symlink `~/.local/bin/lajufile`.

**Memperbarui versi terpasang:**
- **Langsung dari aplikasi (`v1.0.1+`):** Buka **Bantuan -> Periksa pembaruan...** (`Help -> Check for updates...`), lalu klik **Update now** — LajuFile mengunduh installer resmi sesuai arsitektur CPU dari GitHub Releases, memverifikasi `SHA256SUMS.txt`, memasangnya otomatis, dan memulai ulang aplikasi tanpa menghapus daftar unduhan atau pengaturan.
- **Secara manual:** Unduh installer terbaru di halaman [Releases](../../releases) lalu jalankan langsung.

> Catatan ekstensi browser: Folder `extension/` ikut terpasang otomatis bersama installer di setiap platform. Buka LajuFile -> **Browser -> Browser Extension Setup...** lalu salin kode **Quick-Pair**.

### 🛠️ Membangun dari Source & Membuat Installer

**Prasyarat:** Rust `>= 1.85` (Edition 2024) dan Node.js (untuk pemeriksaan skrip & tes ekstensi).

```powershell
# Clone repository
git clone https://github.com/nuellpr/LajuFile.git
cd LajuFile

# Jalankan pemeriksaan format, linter, tes Rust, & tes ekstensi
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
node --test crates/lajufile-app/tests/extension_sender.mjs

# Jalankan aplikasi desktop langsung
cargo run -p lajufile-app --bin lajufile

# Bangun Windows Setup Installer (.exe) -> dist/LajuFile-v1.0.7-Windows-x64-Setup.exe
powershell -ExecutionPolicy Bypass -File installer/build-installer.ps1
```

Di **macOS / Linux**, bangun installer native (`.dmg` atau `.run`) dengan:

```bash
chmod +x installer/build-unix-installer.sh
./installer/build-unix-installer.sh
```

### ⌨️ Pintasan Keyboard

| Pintasan | Fungsi |
| --- | --- |
| `Ctrl+N` | Tambah URL unduhan baru |
| `Ctrl+B` | Buka **Batch Smart Grabber** |
| `Ctrl+F` | Fokus kotak pencarian |
| `Ctrl+,` | Buka Pengaturan (**Settings**) |
| `Alt+Enter` | Buka detail **Properties** & verifikasi SHA-256 |
| `Space` | Jeda / Lanjutkan (*Pause / Resume*) item terpilih |
| `Delete` | Hapus item terpilih |
| `Alt+Click` *(Browser)* | Unduh langsung lewat browser tanpa ditangkap LajuFile |

---

## 🇬🇧 English

**LajuFile** is a fast, lightweight desktop download manager built in **Rust** (`egui`/`eframe` + `tokio` + SQLite WAL) for **Windows, macOS, and Linux**. It accelerates HTTP/HTTPS downloads with up to 32 parallel connections, integrates with Chromium & Firefox browsers via a Manifest V3 extension, automates `yt-dlp` + `ffmpeg` media downloads, and ships native executable setup installers with self-updating support.

### ✨ Highlights

- **Segmented HTTP Engine:** Up to 32 parallel range streams per file, automatic `If-Range` fallback, permanent-error retry short-circuiting, automatic **Category Folders** routing, global token-bucket speed limiter, chunked byte-size tracking, and built-in **SHA-256** verification.
- **Browser Extension & Setup Wizard:** Local HTTP bridge on `127.0.0.1` protected by a 64-hex secret token (`x-lajufile-token`) with constant-time verification and strict extension-only CORS allowlist (`chrome-extension://`, `moz-extension://`, `extension://`, `safari-web-extension://`). Supports **Chrome, Edge, Brave, Opera, Vivaldi, and Firefox** with one-paste **Quick-Pair**, immediate Firefox `onCreated` capture, synced extension filters, `Alt+Click` bypass, and a YouTube quality picker bar.
- **Batch Smart Grabber:** Expand numeric wildcard patterns (`file_*.zip`), import `.txt` link lists safely, or extract links from public web pages with per-hop SSRF DNS validation and IP pinning.
- **Video & Audio Integration:** Auto-installs `yt-dlp`, `deno`, and `ffmpeg` over HTTPS-only `reqwest` with mandatory **SHA-256** verification across Windows, macOS, and Linux; forwards app proxy and rate limits to `yt-dlp`, preserves pause state, supports H.264/AAC MP4 merging, M4A audio-only extraction, subtitles, and chapters.
- **86-Language Catalog & RTL:** OS display language auto-detection, accurate *Built-in* vs *TSV / EN* catalog indicators, runtime `.tsv` override support, locale number formatting, and RTL layout mirroring.
- **Cross-Platform Native Installers (`.exe`, `.dmg`, `.run`):** Self-contained setup wizard with SHA-256 integrity checks, macOS `.app` bundle sidecar payload compatible with strict `codesign`, non-recursive uninstaller protecting unrelated user files, architecture-aware in-app updater (`x64`/`arm64`), and Unix single-instance `flock` + sleep inhibition (`v1.0.7`).

---

## 🔒 Keamanan & Arsitektur / Security Architecture

LajuFile dirancang dengan prinsip *defense-in-depth* untuk penggunaan harian dan distribusi rilis yang aman:

1. **Enkripsi Cookie & Izin Berkas Lokal:** Cookie sesi dienkripsi menggunakan **Windows DPAPI** (`CryptProtectData` dengan `CRYPTPROTECT_UI_FORBIDDEN`) di Windows dan `HMAC-SHA256` + `SHA-256 CTR` di macOS/Linux. Di Unix, direktori data memakai mode `0o700`, sedangkan `settings.json` dan `.cookie.key` dibuat secara atomik dengan mode `0o600`.
2. **Bridge Autentikasi & CORS Ketat:** Bridge lokal hanya mendengar di `127.0.0.1`, mewajibkan header `x-lajufile-token` 64 karakter hex dengan perbandingan **constant-time**, memvalidasi header `Host` loopback tunggal untuk mencegah DNS rebinding, serta hanya mengizinkan CORS dari origin ekstensi browser.
3. **Proteksi SSRF pada Web Page Grabber:** Menggunakan `RedirectPolicy::none()` dengan loop redirect eksplisit (maks. 5 hop); setiap hop memvalidasi host & hasil resolusi DNS terhadap IP loopback, privat, CGNAT, link-local / cloud metadata, lalu melakukan IP pinning (`resolve_to_addrs`).
4. **Keamanan Berkas, Alat Video & Installer:** Nama berkas disanitasi terhadap *path traversal* dan nama perangkat terlarang Windows; payload installer diverifikasi **SHA-256** dengan uninstaller non-rekursif; self-updater mewajibkan manifes `SHA256SUMS.txt`; dan pengunduhan `yt-dlp`/`ffmpeg`/`deno` dilakukan melalui `reqwest` HTTPS-only dengan verifikasi **SHA-256** wajib di semua OS.
5. **Penyimpanan Lokal Aman:** SQLite berjalan dalam mode **WAL** dengan query *parameterized*. Berkas `.db`, `settings.json`, dan kunci lokal dikecualikan melalui `.gitignore`.

### 🗂️ Struktur Repositori

```text
LajuFile/
├── crates/
│   ├── lajufile-core/     # Mesin unduhan, segmen, HTTP probe, DPAPI/keyed cookie, SQLite store, SHA-256
│   └── lajufile-app/      # GUI eframe/egui, engine thread, bridge HTTP, tray, i18n, updater, & lajufile-setup
├── extension/             # Ekstensi browser Manifest V3 (background, content, interceptor, popup, options)
├── installer/             # Skrip build Setup Installer Windows (.exe), macOS (.dmg) & Linux (.run)
├── assets/                # Ikon aplikasi & aset kategori/jenis berkas
└── .github/workflows/     # Pipeline CI (Windows/macOS/Linux test) & Release (Setup Installers + SHA256SUMS)
```

---

## 🛡️ Code Signing & Privacy Policy

* **Code Signing & Eligibility:** Intended for free code signing provided by [SignPath.io](https://about.signpath.io/), certificate by [SignPath Foundation](https://signpath.org/) (once enrolled/approved; unsigned releases are noted in GitHub Releases until signing is active).
* **Team Roles:**
  * Committers, reviewers, and approvers: [nuellpr](https://github.com/nuellpr) (Repository Owner).
* **Privacy Policy:** See [PRIVACY.md](PRIVACY.md). LajuFile Desktop and the LajuFile Capture browser extension do not collect telemetry or transfer data to developer-operated servers; network connections occur only as initiated or configured by the user (local `127.0.0.1` extension bridge, target downloads, optional HTTPS-verified `yt-dlp`/`deno`/`ffmpeg` setup, and GitHub Releases update checks).

---

## 📜 Lisensi / License

Dirilis di bawah **MIT License** lengkap — lihat berkas [LICENSE](LICENSE).  
Hak Cipta © 2026 **Nuel Wallep**.
