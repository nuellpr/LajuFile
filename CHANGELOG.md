# Changelog

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
