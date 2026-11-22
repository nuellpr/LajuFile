# Changelog

## v2.0.0

### Stabilitas Mesin Unduhan & Concurrency
- **Perbaikan Resume Pasca Dynamic Splitting:** `validate_segments()` kini memvalidasi keunikan `idx` dan kesinambungan rentang `0..size` yang diurutkan berdasarkan `start`, sehingga resume setelah *work stealing* tidak lagi mengulang unduhan dari awal.
- **Sinkronisasi Work Stealing Tanpa Race Condition:** Mengganti pasangan atomik terpisah donor/thief dengan `Mutex<SegmentBounds { end, done, reserved }>` per segmen dan penyatuan jalur `flush_segment_buf`, menghilangkan risiko pemotongan *buffer* atau korupsi byte saat segmen dibelah di tengah pengunduhan.
- **Move File Aman Lintas Filesystem:** Menyatukan `Command::MoveFile` ke `move_file_no_overwrite` agar pemindahan berkas tidak pernah menimpa file tujuan secara diam-diam (termasuk lintas partisi).

### Keamanan & Proteksi Sistem
- **Enkripsi Kredensial `settings.json` at Rest:** `hf_token` dan `proxy_url` berkredensial kini dienkripsi otomatis pada disk dengan prefix `enc:v1:` (Windows DPAPI / `HMAC-SHA256` + `CTR` di Unix) lengkap dengan migrasi transparan dari konfigurasi lama.
- **Hardening Auto-Extract (*Fail-Closed* & Anti-Zip-Bomb):** Inspeksi `tar -tvf` dan `unzip -Z` kini *fail-closed*, memblokir seluruh mode entri selain file reguler/direktori (`-`/`d`), menambahkan batas jumlah entri (`50.000`), batas rasio dekompresi (`>100x` di atas `128 MiB`), pengecekan ruang disk sebelum ekstraksi, `LC_ALL=C`, serta menyembunyikan jendela konsol `tar` di Windows.

### Scheduler, Video, Platform & Ekstensi
- **Penegakan Scheduler & Aksi Selesai Antrean:** `pump_queue()` kini menghormati jendela jadwal aktif (`is_schedule_blocking()`), mengevaluasi jadwal langsung saat startup maupun perubahan pengaturan, dan menunda aksi **On Queue Complete** (*Exit/Sleep/Shutdown*) hingga seluruh verifikasi **SHA-256** dan **Auto-Extract** di latar belakang selesai (`in_flight_post_tasks`).
- **Batas Kecepatan Per-Item untuk Video:** `yt-dlp` kini menerapkan batas terketat antara batas global dan `speed_limit_kbps` per unduhan.
- **Platform Native Windows & Unix:** Mengganti `rundll32 powrprof.dll,SetSuspendState` dengan pemanggilan Win32 langsung `SetSuspendState(false, false, false)` (mencegah mode *hibernate* tak disengaja), menyembunyikan jendela konsol `shutdown`, dan mengganti subproses `date +%H:%M` di Unix dengan FFI libc `time` + `localtime_r`.
- **UI HuggingFace & Ekstensi Browser:** Otomatis mereset/memperbarui kolom `expected_sha256` saat mengganti file atau URL HuggingFace; memuat `common.js` sebelum `interceptor.js` (menghapus duplikasi helper), memfilter hanya klik primer tepercaya (`event.isTrusted && event.button === 0`), menangani `decodeURIComponent` secara aman serta balasan `sniff-page-media` pada iframe; menambahkan logo toko Edge `300×300` (`assets/icons/icon-300.png`) dan menyelaraskan panduan instalasi Firefox ke paket rilis `lajufile-extension-firefox.zip`.

## v1.0.9

### Fitur Baru
- **Dynamic Segment Splitting (Work Stealing):** Saat satu segmen unduhan selesai lebih cepat, worker secara otomatis membelah sisa rentang byte dari segmen lain yang masih berjalan (selama sisa `>= 1 MiB`) sehingga bandwidth tetap penuh hingga akhir unduhan tanpa bottleneck ekor (*tail latency*).
- **Scheduler & Aksi Selesai Antrean (*One-Shot*):** Menambahkan penjadwal jam aktif unduhan (*Start/Stop Hour*, termasuk jendela lintas tengah malam) serta aksi otomatis sekali jalan saat seluruh antrean selesai (**Keluar Aplikasi**, **Tidur / Sleep**, atau **Matikan Komputer / Shutdown** lintas Windows, macOS, dan Linux) lengkap dengan banner hitung mundur 10 detik dan tombol **Batalkan**.
- **Priority Queue & Batas Kecepatan Per-Item:** Mendukung **Pindahkan ke Atas Antrean** (*Move to Top*), **Paksa Mulai** (*Force Start* melewati batas konkurensi), dan batas kecepatan khusus per unduhan (`speed_limit_kbps`, migrasi skema SQLite `v3`) yang bekerja bersama pembatas kecepatan global.
- **Verifikasi SHA-256 Otomatis:** Kolom opsional **Expected SHA-256** pada dialog *Add Download* dan *Download File Info* yang otomatis mencocokkan hash berkas saat unduhan selesai dan menandai hasilnya di status & Properties.
- **AI Model Downloader (HuggingFace):** Menempel URL repositori HuggingFace (`https://huggingface.co/{owner}/{repo}`) di *Add Download* kini memuat daftar berkas model (termasuk bobot `.gguf`/`.safetensors`) lengkap dengan ukuran dan auto-isi checksum **SHA-256** (`lfs.oid`), serta dukungan **HuggingFace Access Token** bertopeng di Settings yang hanya dikirim ke host `*.huggingface.co`.
- **Multi-Site Media Sniffer:** Memperluas deteksi situs media di desktop dan ekstensi browser ke YouTube, Vimeo, Twitch, TikTok, X/Twitter, Dailymotion, SoundCloud, Bilibili, Reddit, Instagram, dan Facebook, ditambah deteksi stream `<video>`/`<audio>`/`.m3u8` langsung di popup ekstensi.
- **Ekstraksi Arsip Otomatis yang Aman (*Auto-Extract*):** Opsi **Auto-extract archives** di Settings untuk mengekstrak `.zip`, `.tar`, `.tar.gz`, dan `.tgz` secara otomatis ke folder bernama sama setelah selesai (dan lolos verifikasi SHA-256), dengan validasi pra-ekstraksi ketat terhadap *Zip-Slip* (*path traversal*) dan *symlink*.

### Keamanan, Stabilitas & Perbaikan
- **Streaming Langsung dari Probe `200 OK`:** `probe()` kini menyimpan *live stream* pada respons `200 OK` (dengan batas waktu segar 30 detik dan fallback GET baru bila kedaluwarsa), sehingga tautan *signed/one-time* dan file tanpa dukungan `Range` tidak lagi mengunduh dua kali atau gagal 403/410.
- **Keamanan & Validasi:** Penolakan absolut `file://` dan symlink escape pada arsip; SSRF *Web Page Grabber* menolak loopback/private/CGNAT di setiap hop; *self-updater* memverifikasi nama berkas aset secara ketat tanpa wildcard longgar; `deno` dan alat video memakai URL versi yang dipin dengan verifikasi **SHA-256** wajib; dan perbaikan race condition saat pause segmen di bawah beban CPU tinggi.
- **Ekstensi, CI & Struktur Kode:** Mengekstrak utilitas bersama ekstensi ke `extension/common.js` (termasuk dukungan packaging & pengujian `extension_sender.mjs`), memecah `app.rs` (`app/{mod,actions,helpers}`) dan `engine/mod.rs` (`engine/{mod,intake,actions,speed,ssrf,video,huggingface,extract}`) menjadi modul terfokus, serta memperbarui matriks runner Linux ARM64 (`ubuntu-24.04-arm`) dan macOS Intel (`macos-15-intel`) pada workflow rilis GitHub Actions.

## v1.0.8

### Optimasi Memori (RAM) & Efisiensi
- Memuat font sistem secara **on-demand sesuai aksara bahasa aktif** (`install_system_fonts_for_language`), tidak lagi memuat paksa puluhan MB font CJK/Korea/Indic (`msyh.ttc`, `malgun.ttf`, `Nirmala.ttf`, `Arial Unicode.ttf`) saat antarmuka memakai aksara Latin/Indonesia/Inggris, serta membebaskan font berat otomatis saat berganti bahasa.
- Menonaktifkan buffer OpenGL 3D yang tidak dipakai oleh UI 2D (`depth_buffer: 0`, `stencil_buffer: 0`, `multisampling: 0` di `eframe::NativeOptions`).
- Membatasi runtime `tokio` engine ke `worker_threads(2)`, `max_blocking_threads(4)`, dan `thread_stack_size(1 MiB)` (sebelumnya memakai jumlah inti CPU penuh).
- Membatasi cache SQLite (`PRAGMA cache_size = -512`, `journal_size_limit = 1 MiB`) dan pool koneksi idle HTTP (`pool_max_idle_per_host(4)`, `pool_idle_timeout(30s)`).
- Menambahkan `platform::trim_process_memory()` (Win32 `SetProcessWorkingSetSize`) yang dipanggil saat startup selesai, setelah unduhan selesai, saat berganti profil font, dan saat jendela disembunyikan ke System Tray (sekaligus melepas handle clipboard sementara).

### Keamanan & Dokumentasi
- Menambahkan kebijakan keamanan [SECURITY.md](SECURITY.md) (mengaktifkan tab **Security / Keamanan** di GitHub lengkap dengan tautan pelaporan kerentanan privat dan ringkasan area keamanan utama LajuFile).

## v1.0.7

### Keamanan & Integritas
- Memperketat proteksi SSRF pada Web Page Grabber menggunakan `RedirectPolicy::none()`, validasi DNS publik di setiap redirect (maks. 5 hop), dan IP pinning melalui `resolve_to_addrs`.
- Mengganti unduhan alat video berbasis subproses `curl` menjadi `reqwest` HTTPS-only (memblokir downgrade ke HTTP pada redirect), mendukung proxy aplikasi, membersihkan temp file secara RAII, dan mewajibkan verifikasi **SHA-256** di semua OS termasuk `ffmpeg` macOS (`ffmpeg.martin-riedl.de`).
- Membatasi izin direktori data di Unix (`0o700`), penulisan atomik `settings.json` berizin `0o600`, dan pembuatan `.cookie.key` secara atomik (`create_new`, `0o600`) tanpa menimpa kunci lama saat gagal baca/korup.
- Menjadikan auto-updater sadar arsitektur CPU (`x64` vs `arm64`), mendukung proxy pengaturan, dan menempatkan staging installer di direktori privat `data_dir/updates`.
- Memperbaiki uninstaller agar non-rekursif (tidak menghapus berkas pengguna lain di folder instalasi bersama) dan memperbaiki resolusi root bundle macOS (`infer_uninstall_dir`).

### Mesin Unduhan, Video & Platform
- Memperbaiki *infinite loop* saat deduplikasi nama file panjang (~180 karakter) atau direktori dalam dengan menyisipkan sufiks ` ({n})` sebelum pemotongan `stem`, serta membatasi percobaan `reserve_download_paths`.
- Menghindari retry-sleep pada error HTTP/disk permanen (mis. 404/403) pada unduhan tersegmentasi dan memastikan ukuran akhir unduhan *chunked* tersimpan ke database.
- Mengaktifkan routing **Category Folders** untuk unduhan default (termasuk ekstensi baru `.dmg`, `.deb`, `.rpm`, `.apk`, `.appimage`, `.tgz`), pengecekan duplikat di `insert_video`, serta `Command::Redownload` yang mempertahankan cookie terenkripsi.
- Meneruskan `--proxy`, `--limit-rate`, escaping `%%` pada template `-o`, dan pemisah `--` ke `yt-dlp`; menunggu thread `stdout`/`stderr` sebelum membaca pesan error, memverifikasi/mendeteksi ekstensi hasil akhir video, serta menjaga file `.part`/`.ytdl` saat Pause/Shutdown.
- Mengemas payload macOS ke sidecar `Contents/Resources/payload.bin` (`--payload-only`) agar tidak bentrok dengan `codesign`, melewati `package.ps1` saat packing, dan menambahkan single-instance `flock` serta anti-sleep (`caffeinate` / `systemd-inhibit`) untuk Unix.
- Memperbaiki duplikasi simbol Unix `acquire_unix_lock` (`E0428`) pada Linux/macOS serta menambahkan *graceful shutdown* untuk installer melalui endpoint bridge lokal terautentikasi `POST /shutdown` (menolak header `Origin` browser) sebelum *fallback* `taskkill`/`pkill`.
- Menambahkan cache *snapshot* in-memory pada `EngineState` dengan *overlay* `pending_segments` agar `publish()` tidak melakukan pemindaian SQLite penuh berulang kali pada setiap tick.
- Menstabilkan kecepatan unduhan dan grafik speed: mengganti perhitungan speed ke *sliding window* 4 detik (`SpeedTracker`) dengan *grace period* 3 detik dan *cooldown* decay 1 detik, menghindari penulisan SQLite `set_video_size` berulang pada setiap baris progres video, mengaktifkan `PRAGMA synchronous=NORMAL` pada WAL, menambahkan `PYTHONUNBUFFERED=1`, serta menyetel argumen `yt-dlp` (`-N 4`, `--buffer-size 64K`, `--progress-delta 0.25`).

### UI, Efisiensi, Ekstensi & Metadata
- Mengurangi penggunaan CPU saat idle (`recv_timeout(500ms)` di engine, repaint UI adaptif `1000ms`, reuse handle `Clipboard`, dan cache drive `roots()` di Folder Picker).
- Menyinkronkan nama file hasil probe `Content-Disposition` ke dialog *Download File Info* (bila belum diedit pengguna) dan menutup dialog stale bila request ekstensi timeout, serta merapikan badge bahasa *Built-in*/*RTL*, reset kualitas YouTube saat URL diubah, dan batas 512 KiB impor `.txt` Batch.
- Ekstensi browser kini membaca `remoteCapturedExtensions` dari `/rules` bila filter ekstensi lokal kosong, menangkap unduhan Firefox langsung tanpa delay 1,5 detik pada `onCreated`, merapikan menu YouTube agar responsif, serta memperbarui `README.md` dan `rust-toolchain.toml`.
- Memecah modul `engine`, `ui/dialogs`, dan `ui/main_window` menjadi sub-modul terfokus (`engine/{mod,ssrf,video}`, `ui/dialogs/{mod,settings,browser_lang,transfers,batch_update}`, `ui/main_window/{mod,chrome,table}`).

## v1.0.6

### Perbaikan
- Memastikan working directory aplikasi desktop tidak tertahan di folder sistem Windows yang terproteksi (`C:\Windows\System32` / `Program Files`) saat dijalankan lewat Windows Startup (`autostart`), shortcut shell, atau installer `--relaunch`.
- Menetapkan `.current_dir(...)` eksplisit yang dapat ditulis untuk proses anak `yt-dlp` (`spawn_video` ke folder simpan `save_path.parent()` / `%TEMP%`, dan `ytdlp_listing` ke `%TEMP%`) guna mencegah error `Permission denied: 'C:\Windows\system32\tmp*.tmp'`.
- Memperbaiki kegagalan `os error 32` saat auto-update / install menimpa biner yang sedang berjalan di Windows (`stop_running_lajufile` menunggu proses keluar penuh dengan fallback `/F` dan retry lock `ERROR_SHARING_VIOLATION`).

## v1.0.5

### Keamanan & Hardening
- Mengaktifkan MSVC **Control Flow Guard** (`/guard:cf`, `-C control-flow-guard=yes`), optimasi release `lto = "thin"`, `strip = "debuginfo"`, dan `/DEBUG:NONE` untuk menghilangkan path `.pdb` dari biner rilis.
- Self-updater kini memverifikasi integritas installer secara fail-closed melalui manifes `SHA256SUMS.txt` dari GitHub Releases serta validasi payload PE di Windows, plus dukungan Authenticode signing & verifikasi `signtool` pada pipeline rilis.
- Parser payload installer kini mendukung tabel sertifikat Authenticode (`IMAGE_DIRECTORY_ENTRY_SECURITY`) beserta alignment padding 0–7 byte dan membersihkan header sertifikat warisan agar `uninstall.exe` tetap valid.
- Pertahanan extension MV3: validasi ketat `sender.id`, halaman internal ekstensi, top-frame YouTube, URL HTTP/HTTPS, batas cache, dan sanitasi ukuran media; local HTTP bridge menambahkan middleware penolak DNS rebinding (`Host` loopback tunggal ketat) dan CORS khusus origin ekstensi.
- Verifikasi **SHA-256** otomatis saat mengunduh alat video (`yt-dlp` via `SHA2-256SUMS`, `deno` via `.sha256sum`, `ffmpeg` Windows via `gyan.dev` `.sha256` & Linux via `yt-dlp/FFmpeg-Builds` `checksums.sha256`).
- Enkripsi cookie di macOS/Linux ditingkatkan ke skema terautentikasi `HMAC-SHA256` + `SHA-256 CTR` (Windows tetap menggunakan DPAPI).

### Mesin Unduhan, Audio/Video, & UI
- Perbaikan finalisasi file lintas sistem berkas (**exFAT / FAT32 / NTFS**) menggunakan `MoveFileExW` tanpa overwrite di Windows dan fallback `create_new` copy yang aman.
- Cek kapasitas bebas disk (`GetDiskFreeSpaceExW` / `df`) sebelum alokasi segmen maupun unduhan file.
- Retry otomatis dengan jeda `[2s, 5s, 10s]` untuk kegagalan jaringan/HTTP sementara pada unduhan single-stream.
- Dukungan **HTTP / HTTPS / SOCKS proxy** di Settings + `User-Agent` dinamis mengikuti versi aplikasi (`LajuFile/1.0.5`).
- Perbaikan **Audio only (m4a)** agar menghasilkan berkas `.m4a` murni tanpa ekstensi ganda `.m4a.mp4` serta masuk ke kategori `Music`.
- **Windows Polish & Integrasi:** single-instance lock + aktivasi jendela existing via `/wake`, anti-sleep (`SetThreadExecutionState`) saat mengunduh, aksesibilitas `eframe/accesskit`, silent startup update checker dengan cooldown 24 jam + badge pembaruan di menu bar, dan shutdown halus sebelum force-kill saat update/uninstall.
- **Packaging & Metadata:** penyematan Windows Application Manifest (`asInvoker`, `PerMonitorV2` DPI, `longPathAware`, Common Controls v6), injeksi otomatis versi dari `Cargo.toml` ke `lajufile.rc`, pemeriksaan paritas versi di CI, paket zip ekstensi Chromium & Firefox otomatis di workflow rilis (dengan separator ZIP POSIX `/`, `strict_min_version: "140.0"`, dan `data_collection_permissions: ["none"]` untuk Firefox AMO), serta dokumen kebijakan privasi di [PRIVACY.md](PRIVACY.md).
