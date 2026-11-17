<div align="center">

<img src="assets/icons/icon-128.png" alt="LajuFile Logo" width="88" />

# LajuFile

**Manajer unduhan multi-koneksi berkecepatan tinggi, ringan, aman, dan modern untuk Windows, macOS & Linux.**  
*Fast, lightweight, secure, multi-connection download manager built with Rust.*

[![Version](https://img.shields.io/badge/version-v1.0.5-3B82F6?style=flat-square)](#)
[![Rust](https://img.shields.io/badge/Rust-2024%20%2F%20MSRV%201.85-orange?style=flat-square)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/license-MIT-10B981?style=flat-square)](LICENSE)
[![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-6366F1?style=flat-square)](#instalasi--installation)

[🇮🇩 Bahasa Indonesia](#-bahasa-indonesia) · [🇬🇧 English](#-english) · [🔒 Security](#-keamanan--security-architecture) · [🛡️ Privacy](PRIVACY.md) · [📜 Lisensi](LICENSE)

</div>

---

## 🇮🇩 Bahasa Indonesia

**LajuFile** adalah manajer unduhan desktop berbasis **Rust** untuk **Windows, macOS, dan Linux** yang membagi berkas HTTP/HTTPS menjadi beberapa segmen paralel secara otomatis, terintegrasi langsung dengan browser modern serta **yt-dlp / ffmpeg** untuk video hingga 4K, dan hadir dengan **Setup Installer executable** di setiap platform.

### ✨ Fitur Utama

| Kategori | Kemampuan |
| --- | --- |
| **Mesin Unduhan Cepat** | Hingga **32 koneksi paralel** per berkas (`Range` probing + prefix sanity check 64 KiB + resume otomatis), kontrol antrean serentak (`1–10`), pembatas kecepatan global cepat dari status bar, serta verifikasi **SHA-256** di jendela Properties. |
| **Visual Progres Real-Time** | Peta Segmen (**Segment Map**) + grafik kecepatan **Sparkline**, jendela progres yang **bebas diseret/dipindahkan ke mana saja** serta tombol **Hide / Sembunyikan** ke background, estimasi ETA akurat, tema **Dark & Light**, dan skala antarmuka `80%–150%`. |
| **Batch Smart Grabber (`Ctrl+B`)** | Antrekan banyak URL sekaligus, ekspansi pola angka otomatis (contoh `file-[01..20].zip`), dan **Fetch Links from Web Page** untuk mengekstrak tautan dari halaman web secara aman. |
| **Integrasi Browser Lengkap** | Wizard Setup Ekstensi di menu `Browser` dengan deteksi otomatis **Chrome, Edge, Brave, Opera, Vivaldi, dan Firefox** di Windows, macOS, dan Linux, kode **Quick-Pair**, toggle per-situs, tombol `Alt+Click` bypass ke browser, penangkapan tautan PDF/media, dan tombol mengambang **"Download with LajuFile"** di YouTube. |
| **Video & Audio (`yt-dlp` + `ffmpeg`)** | Unduh MP4 H.264 / Audio M4A terbaik otomatis untuk Windows, macOS, dan Linux dengan fragmen paralel (`-N 8` + chunking) dan dukungan **Subtitles** & **Chapters** di Settings. |
| **Multi-Bahasa & RTL (`View -> Language...`)** | Mendukung katalog **86 bahasa Windows**, deteksi bahasa OS otomatis (Windows, macOS, Linux), dukungan override terjemahan `.tsv`, tata letak **RTL** (Arab, Ibrani, Persia, Urdu), dan format angka lokal. |
| **Aturan & Pembaruan Otomatis** | Pencarian instan (`Ctrl+F`), sortir tabel, kebijakan duplikat (*Skip*, *Resume*, *Rename (#)*), **Periksa pembaruan** langsung dari menu **Bantuan (Help)** via GitHub Releases, drag-and-drop berkas `.url` / `.txt` / `.ef2`, serta **System Tray**. |

### 📦 Instalasi & Pembaruan (Windows, macOS & Linux)

Unduh installer sesuai sistem operasi dari halaman [Releases](../../releases):

- **Windows:** Jalankan **`LajuFile-v1.0.5-windows-x64-setup.exe`**, setujui lisensi, dan klik **Install** (memasang ke `%LOCALAPPDATA%\Programs\LajuFile`, membuat pintasan Desktop/Start Menu, dan mendaftarkan **Add/Remove Programs** + `uninstall.exe`).
- **macOS:** Buka **`LajuFile-v1.0.5-macos-arm64-setup.dmg`** dan jalankan **`LajuFile Setup.app`** (atau jalankan executable **`LajuFile-v1.0.5-macos-arm64-setup`** via terminal `chmod +x`), yang memasang bundle **`LajuFile.app`** ke `~/Applications/LajuFile.app` lengkap dengan `extension/` dan `uninstall`.
- **Linux:** Jalankan **`chmod +x LajuFile-v1.0.5-linux-x64-setup.run && ./LajuFile-v1.0.5-linux-x64-setup.run`**, memasang ke `~/.local/share/lajufile`, membuat launcher `.desktop` di menu aplikasi, dan symlink `~/.local/bin/lajufile`.

**Memperbarui versi terpasang:**
- **Langsung dari aplikasi (`v1.0.1+`):** Buka **Bantuan -> Periksa pembaruan...** (`Help -> Check for updates...`), lalu klik **Update now** — LajuFile mengunduh installer resmi dari GitHub Releases, memasangnya otomatis, dan memulai ulang aplikasi (antrean & pengaturan tetap aman di `%APPDATA%\LajuFile`).
- **Secara manual (`v1.0.0` atau sebelumnya):** Unduh installer `.exe` terbaru di halaman [Releases](../../releases) lalu jalankan langsung (menimpa program secara bersih tanpa menghapus daftar unduhan atau pengaturan Anda).

Untuk menghubungkan browser, buka LajuFile -> **Browser -> Browser Extension Setup...** lalu salin kode **Quick-Pair**.

> Catatan ekstensi browser: Folder `extension/` ikut terpasang otomatis bersama installer di setiap platform, siap dipasangkan menggunakan kode **Quick-Pair** satu klik.

### 🛠️ Membangun dari Source & Membuat Installer

**Prasyarat:** Rust `>= 1.85` (Edition 2024).

```powershell
# Clone repository
git clone https://github.com/nuellpr/LajuFile.git
cd LajuFile

# Jalankan seluruh pengujian & linter
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace

# Jalankan aplikasi desktop langsung
cargo run -p lajufile-app --bin lajufile

# Bangun Windows Setup Installer (.exe) -> dist/LajuFile-v1.0.5-Windows-x64-Setup.exe
powershell -ExecutionPolicy Bypass -File installer/build-installer.ps1
```

Di **macOS / Linux**, bangun installer native dengan:

```bash
chmod +x installer/build-unix-installer.sh
./installer/build-unix-installer.sh
```

### ⌨️ Pintasan Keyboard

- `Ctrl+N` — Tambah URL unduhan baru
- `Ctrl+B` — Buka **Batch Smart Grabber**
- `Ctrl+F` — Fokus kotak pencarian
- `Ctrl+,` — Buka Pengaturan (**Settings**)
- `Alt+Enter` — Buka detail **Properties** & verifikasi SHA-256
- `Space` — Jeda / Lanjutkan (*Pause / Resume*) item terpilih
- `Delete` — Hapus item terpilih
- `Alt+Click` (di Browser) — Biarkan browser mengunduh langsung tanpa ditangkap LajuFile

---

## 🇬🇧 English

**LajuFile** is a modern, lightweight desktop download manager built in **Rust** (`egui`/`eframe` + `tokio` + SQLite WAL) for **Windows, macOS, and Linux**. It accelerates HTTP/HTTPS downloads with parallel segmented connections, captures browser downloads & YouTube streams via a Manifest V3 extension, and ships with native executable Setup Installers on every desktop OS.

### ✨ Highlights

- **Segmented HTTP Engine:** Up to 32 parallel range streams per file, automatic `If-Range` fallback to single stream when needed, global token-bucket speed limiter, and built-in **SHA-256** verification.
- **Browser Extension & Setup Wizard:** Local HTTP bridge on `127.0.0.1` protected by a 64-hex secret token (`x-lajufile-token`) with constant-time verification and strict extension/localhost CORS allowlist. Supports **Chrome, Edge, Brave, Opera, Vivaldi, and Firefox** on Windows, macOS & Linux with one-paste **Quick-Pair**, `Alt+Click` bypass, inline PDF/media interception, and a YouTube quality picker bar.
- **Batch Smart Grabber:** Expand numeric ranges (`[01..20]`) or extract downloadable links directly from public web pages.
- **Video & Audio Integration:** Auto-installs `yt-dlp` and `ffmpeg` over HTTPS on first video request across Windows, macOS, and Linux; supports H.264/AAC MP4 merging, M4A audio-only extraction, embedded subtitles, and chapters.
- **86-Language Catalog & RTL:** OS display language auto-detection, built-in translations, runtime `.tsv` override support, locale number formatting, and automatic RTL layout mirroring.
- **Cross-Platform Native Installers (`.exe`, `.dmg`, `.run`):** Self-contained setup wizard with SHA-256 payload integrity verification, desktop/menu launchers (Windows `.lnk` + Add/Remove Programs, macOS `.app` bundle + `.dmg`, Linux XDG `.desktop` + `~/.local/bin`), in-app **Help -> Check for updates...** self-updater, and built-in uninstaller (`v1.0.5`).

---

## 🔒 Keamanan & Arsitektur / Security Architecture

LajuFile dirancang dengan prinsip *defense-in-depth* untuk penggunaan harian dan rilis open-source yang aman:

1. **Enkripsi Cookie Lokal:** Cookie sesi dari browser dienkripsi di Windows menggunakan **Windows DPAPI** (`CryptProtectData` dengan `CRYPTPROTECT_UI_FORBIDDEN`) dan key-protected storage di Linux sebelum disimpan ke SQLite.
2. **Bridge Autentikasi & CORS Ketat:** Bridge lokal hanya mendengar di `127.0.0.1`, mewajibkan header `x-lajufile-token` 64 karakter hex dengan perbandingan **constant-time**, memvalidasi skema `http`/`https`, dan hanya mengizinkan CORS dari origin ekstensi browser (`chrome-extension://`, `moz-extension://`, `extension://`, `safari-web-extension://`) serta localhost.
3. **Proteksi SSRF pada Web Page Grabber:** Fitur ekstraksi tautan halaman web memvalidasi DNS dan menolak alamat IP loopback (`127.0.0.0/8`, `::1`), privat (`10.0.0.0/8`, `172.16.0.0/12`, `192.168.0.0/16`), CGNAT (`100.64.0.0/10`), link-local / cloud metadata (`169.254.169.254`), serta memblokir redirect ke host internal.
4. **Keamanan Berkas & Installer:** Nama berkas disanitasi terhadap path traversal dan nama perangkat terlarang Windows; payload installer diverifikasi dengan **SHA-256** dan menolak path relatif tidak aman (`..`, path absolut, maupun drive prefix). Alat video diunduh dengan `curl --proto =https`.
5. **Penyimpanan Lokal Aman:** SQLite berjalan dalam mode **WAL** dengan seluruh query menggunakan *parameterized bindings*. Berkas `.db`, `settings.json`, dan kunci lokal dikecualikan melalui `.gitignore`.

### 🗂️ Struktur Repositori

```
LajuFile/
├── crates/
│   ├── lajufile-core/     # Mesin unduhan, segmen, HTTP probe, DPAPI cookie, SQLite store, SHA-256
│   └── lajufile-app/      # GUI eframe/egui, engine thread, bridge HTTP, tray, i18n, & lajufile-setup
├── extension/             # Ekstensi browser Manifest V3 (background, content, interceptor, popup, options)
├── installer/             # Skrip build Setup Installer Windows (.exe), macOS (.dmg) & Linux (.run)
├── assets/                # Ikon aplikasi & aset kategori/jenis berkas
└── .github/workflows/     # Pipeline CI (Windows/macOS/Linux test) & Release (Setup Installers)
```

---

## 🛡️ Code signing policy

* **Code Signing & Eligibility:** Intended for free code signing provided by [SignPath.io](https://about.signpath.io/), certificate by [SignPath Foundation](https://signpath.org/) (once enrolled/approved; unsigned releases are noted in GitHub Releases until signing is active).
* **Team Roles:**
  * Committers, reviewers, and approvers: [nuellpr](https://github.com/nuellpr) (Repository Owner).
* **Privacy Policy:** See [PRIVACY.md](PRIVACY.md). This program and the LajuFile Capture browser extension will not collect telemetry or transfer information to developer-operated servers; network connections happen only as initiated or configured by the user (local `127.0.0.1` extension bridge, target downloads, optional first-run HTTPS downloads of open-source `yt-dlp`/`deno`/`ffmpeg`, and release update checks).

---

## 📜 Lisensi / License

Dirilis di bawah **MIT License** lengkap dengan bagian **TERMS AND CONDITIONS** dan **END TERMS AND CONDITIONS** — lihat berkas [LICENSE](LICENSE).  
Hak Cipta © 2026 **Nuel Wallep**.
