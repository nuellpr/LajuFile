<p align="center">
  <img src="assets/lajufile.jpg" alt="LajuFile" width="380">
</p>

<h1 align="center">LajuFile</h1>

<p align="center">
  <b>Manajer Unduhan Desktop Cepat & Native untuk Windows dan Linux — Dibangun dengan Rust</b><br>
  <i>A fast, native desktop download manager for Windows and Linux — built in Rust.</i>
</p>

<p align="center">
  <a href="https://github.com/nuellpr/LajuFile/actions/workflows/ci.yml"><img src="https://github.com/nuellpr/LajuFile/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="https://github.com/nuellpr/LajuFile/actions/workflows/release.yml"><img src="https://github.com/nuellpr/LajuFile/actions/workflows/release.yml/badge.svg" alt="Release"></a>
  <a href="https://github.com/nuellpr/LajuFile/releases/latest"><img src="https://img.shields.io/github/v/release/nuellpr/LajuFile?label=Release" alt="Latest Release"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/License-MIT-blue.svg" alt="License: MIT"></a>
  <img src="https://img.shields.io/badge/Platform-Windows%20x64%20%7C%20Linux%20x64-0078D6" alt="Platform">
</p>

<p align="center">
  <a href="#-bahasa-indonesia"><b>🇮🇩 Bahasa Indonesia</b></a> ·
  <a href="#-english"><b>🇬🇧 English</b></a> ·
  <a href="https://github.com/nuellpr/LajuFile/releases/latest"><b>⬇️ Unduh Rilis / Download Release</b></a>
</p>

---

# 🇮🇩 Bahasa Indonesia

## Tentang LajuFile

**LajuFile** adalah aplikasi manajer unduhan (*download manager*) desktop native untuk **Windows** dan **Linux** yang dibangun menggunakan **Rust** oleh **Nuel Wallep**. Terinspirasi dari kemudahan alur kerja *Internet Download Manager (IDM)* namun ditulis sepenuhnya dengan kode dan desain orisinal yang ringan, aman, dan modern.

## Fitur Utama

### 🚀 Mesin Unduhan & Antrean
- **Unduhan Tersegmen (Multi-Koneksi):** Memecah unduhan hingga **32 rentang byte paralel** sekaligus untuk memaksimalkan kecepatan.
- **True Resume & Pemulihan Otomatis:** Melanjutkan unduhan yang dijeda atau terputus menggunakan validator `If-Range` (atau verifikasi 64 KiB). Jika aplikasi tertutup mendadak, unduhan otomatis dilanjutkan dari posisi terakhir.
- **Manajemen Antrean & Batas Kecepatan:** Atur jumlah unduhan bersamaan (*simultaneous downloads*) dan batas kecepatan global dalam KB/s (`0` = tanpa batas).
- **Penyimpanan Aman:** Menulis ke berkas sementara `<nama>.lajufile`, memeriksa ukuran byte akhir, melindungi dari penimpaan berkas, serta menyimpan progres tiap segmen di database SQLite (mode WAL).

### 🖥️ Antarmuka & Ikon Bergaya IDM
- **Toolbar Lengkap:** Tombol cepat `Add URL`, `Batch`, `Resume`, `Stop`, `Stop All`, `Delete`, `Delete Completed`, dan `Options`, ditambah menu `File`, `Tasks`, `View`, dan `Help`.
- **Ikon Grafik Kustom (28 Ikon):** Ikon kategori (`All downloads`, `Unfinished`, `Finished`, `Video`, `Music`, `Documents`, `Programs`, `Compressed`, `Other`) di bilah samping, serta ikon tipe berkas otomatis (`MP4`, `MKV`, `MP3`, `PDF`, `DOCX`, `XLSX`, `PPTX`, `EXE`, `MSI`, `APK`, `ZIP`, `RAR`, `7Z`, `ISO`, `JPG`, `PNG`, `TXT`, `BIN`, dll.) pada daftar unduhan dan dialog.
- **Dialog Informasi, Progres & Selesai:**
  - **Download File Info:** Muncul saat menangkap unduhan dari browser, menampilkan ikon berkas, ukuran hasil *probe*, kategori, pengeditan nama/folder simpan, serta tombol `Start download` dan `Download later`.
  - **Download Progress:** Klik dua kali pada baris unduhan aktif/dijeda untuk melihat grafik progres dan tabel status per-koneksi/segmen (`N.`, `Downloaded`, `Progress`, `Info`).
  - **Download Complete:** Dialog penyelesaian dengan tombol cepat `Open`, `Open With`, dan `Open Folder`.
- **Tema Gelap & Terang:** Pilihan tema `Dark`, `Light`, atau mengikuti sistem (`System`).

### 🌐 Integrasi Browser (Google Chrome, Brave, Microsoft Edge)
- **Ekstensi LajuFile Capture (Manifest V3):** Menangkap unduhan langsung dari **Google Chrome**, **Brave**, **Microsoft Edge**, dan browser berbasis Chromium lainnya, lengkap dengan menu klik kanan **"Download with LajuFile"**.
- **Jembatan Lokal Aman:** Berjalan di `127.0.0.1:53841` dengan autentikasi token 64-hex.
- **Enkripsi Cookie:** Cookie sesi dari browser dienkripsi per-pengguna (menggunakan **Windows DPAPI** di Windows dan kunci khusus `0600` di Linux) dan tidak pernah disimpan sebagai teks biasa (*plaintext*).

### 🎬 Pengunduh Video YouTube
- **Tombol Melayang "Unduh video ini" di YouTube:** Menampilkan tombol berikon **LajuFile** di sudut pemutar video YouTube lengkap dengan daftar kualitas (`1440p HD 60 fps`, `1080p HD 60 fps`, `720p HD 60 fps`, `720p HD`, `480p`, `360p`, `240p`, `144p`, hingga `Audio only` M4A).
- **Audio & Video `.mp4` Kompatibel Penuh:** Mengelola `yt-dlp` dan `ffmpeg` secara otomatis serta memastikan penggabungan video H.264 + audio AAC agar setiap video `.mp4` yang diunduh selalu memiliki suara di pemutar bawaan Windows maupun Linux.

### ⚡ Otomasi & Produktivitas
- **Clipboard Monitor:** Menyalin (`Ctrl+C`) link unduhan atau link YouTube otomatis memunculkan dialog **Add Download**.
- **Batch Download / Text Import:** Mengunduh banyak link sekaligus dari daftar URL, pola berurutan dengan *wildcard* `*` (angka `1–10` dengan *zero-padding* maupun huruf `a–z`), atau impor langsung dari berkas `.txt`.
- **Berjalan di Latar Belakang (System Tray):** Menekan tombol **Close (`X`)** menyembunyikan jendela ke *System Tray* agar unduhan dan penangkap browser tetap berjalan di latar belakang hingga Anda memilih **Exit** pada ikon tray atau melalui Task Manager.
- **Auto-Start Windows:** Opsi **Launch LajuFile on Windows startup** di menu **Options** untuk menjalankan aplikasi otomatis saat Windows menyala.

---

## Unduh & Instalasi

Unduh paket rilis terbaru di halaman **[GitHub Releases](https://github.com/nuellpr/LajuFile/releases/latest)**:

| Sistem Operasi | Berkas Rilis | Cara Menjalankan |
| :--- | :--- | :--- |
| **Windows 10/11 (x64)** | `LajuFile-v0.1.0-windows-x64.zip` | Ekstrak `.zip`, lalu jalankan `lajufile.exe`. |
| **Linux (x64)** | `LajuFile-v0.1.0-linux-x64.tar.gz` | Ekstrak dengan `tar -xzf LajuFile-v0.1.0-linux-x64.tar.gz`, lalu jalankan `./lajufile`. |

Setiap paket rilis sudah menyertakan aplikasi utama (`lajufile.exe` / `lajufile`), folder ekstensi browser `extension/`, `README.md`, dan `LICENSE`.

---

## Cara Memasang Ekstensi Browser (Chrome, Brave, Edge)

1. Jalankan **LajuFile**, lalu klik tombol **Options** (atau **Manage** pada kartu *Browser Capture* di kiri bawah) dan salin **Port** serta **Token**.
2. Buka halaman ekstensi di browser Anda:
   - **Google Chrome:** `chrome://extensions`
   - **Brave:** `brave://extensions`
   - **Microsoft Edge:** `edge://extensions`
3. Aktifkan **Mode pengembang (Developer mode)**, klik **Muat yang belum dikemas (Load unpacked)**, lalu pilih folder `extension/`.
4. Buka pengaturan (*Options*) ekstensi **LajuFile Capture**, tempelkan **Port** dan **Token**, lalu klik **Save**.
5. Klik **Test connection** hingga status menampilkan **Connected**.

---

## Cara Mengunduh Video YouTube

- **Langsung dari Browser:** Buka video YouTube apa saja, klik tombol **"Unduh video ini"** berikon LajuFile di pojok kanan atas pemutar video, lalu pilih resolusi yang diinginkan.
- **Melalui Aplikasi LajuFile:** Klik **Add URL**, tempelkan link YouTube, klik **Load qualities** untuk memilih resolusi (atau biarkan *Best available*), lalu klik **Add**.

---

## Kompilasi dari Kode Sumber (Build)

### Prasyarat
- **Windows 10/11 x64:** Rust stable (toolchain MSVC) dan *Visual Studio Build Tools* (beban kerja C++).
- **Linux x64 (Ubuntu/Debian):** Rust stable dan paket dependensi sistem:
  ```bash
  sudo apt-get update
  sudo apt-get install -y libgtk-3-dev libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev libxkbcommon-dev libssl-dev libayatana-appindicator3-dev libxdo-dev libwayland-dev libgl1-mesa-dev pkg-config
  ```

### Perintah Kompilasi
```powershell
cargo test --workspace
cargo run -p lajufile-app
cargo build --release -p lajufile-app
```

---

# 🇬🇧 English

## About LajuFile

**LajuFile** is a fast, native desktop download manager for **Windows** and **Linux**, built in **Rust** by **Nuel Wallep**. Inspired by the classic *Internet Download Manager (IDM)* workflow, it combines segmented HTTP transfers, true resume, queues, speed limits, browser capture, and YouTube video downloads in a lightweight native application with original code and design.

## Key Features

- **Segmented Download Engine:** Up to **32 parallel byte-range connections** per file, `If-Range` true resume (with 64 KiB prefix fallback), global speed limits, and automatic crash recovery backed by SQLite in WAL mode.
- **IDM-Style Desktop UI & Custom Icons:** Full toolbar (`Add URL`, `Batch`, `Resume`, `Stop`, `Stop All`, `Delete`, `Delete Completed`, `Options`), 28 glossy category and file-type icons (`MP4`, `MKV`, `MP3`, `PDF`, `DOCX`, `XLSX`, `PPTX`, `EXE`, `MSI`, `APK`, `ZIP`, `RAR`, `7Z`, `ISO`, `JPG`, `PNG`, `TXT`, `BIN`, etc.), `Download File Info` capture dialog, per-segment `Download Progress` window, and `Download Complete` dialog.
- **Multi-Browser Capture (Chrome, Brave, Edge):** Manifest V3 extension (`LajuFile Capture`) communicating with a token-authenticated localhost bridge (`127.0.0.1:53841`) and encrypted per-user cookie storage (Windows DPAPI on Windows, `0600` user key on Linux).
- **YouTube Video Downloader:** Floating **"Unduh video ini"** button with the LajuFile icon and quality selector (`1440p HD 60 fps`, `1080p HD 60 fps`, `720p HD`, `Audio only`, etc.) on YouTube watch pages, powered by managed `yt-dlp` + `ffmpeg` with automatic H.264/AAC `.mp4` audio compatibility.
- **Automation & System Integration:** Clipboard Monitor (`Ctrl+C` auto-prompts for downloadable links), Batch Download / Text Import (URL lists, wildcard `*` numeric/alphabetic ranges, `.txt` import), Close-to-Tray background mode, and `Launch LajuFile on Windows startup`.

## Browser Extension Setup (Google Chrome, Brave, Microsoft Edge)

1. Start **LajuFile** and open **Options** (or click **Manage** under *Browser Capture*). Copy the **Port** and **Token**.
2. Open your browser's extensions page (`chrome://extensions`, `brave://extensions`, or `edge://extensions`).
3. Enable **Developer mode**, click **Load unpacked**, and select the `extension/` folder.
4. Open the **LajuFile Capture** extension options, paste the port and token, and click **Test connection** until it shows **Connected**.

## Build & Release

Pushing a version tag automatically builds and publishes portable archives for both **Windows** (`LajuFile-<tag>-windows-x64.zip`) and **Linux** (`LajuFile-<tag>-linux-x64.tar.gz`) via GitHub Actions:

```powershell
git tag v0.1.0
git push origin v0.1.0
```

## Design & Documentation

- Figma: <https://www.figma.com/design/PASDzAEqJlF35dkv4L9eIx>
- Spec: [`docs/superpowers/specs/2026-11-14-lajufile-design.md`](docs/superpowers/specs/2026-11-14-lajufile-design.md)
- Milestone Plans: [`docs/superpowers/plans/`](docs/superpowers/plans/)

## Lisensi / License

Didistribusikan di bawah **Lisensi MIT** (Bahasa Inggris & Bahasa Indonesia) — lihat berkas [LICENSE](LICENSE).  
Distributed under the **MIT License** (English & Indonesian) — see [LICENSE](LICENSE).
