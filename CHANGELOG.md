# Changelog

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
- **Packaging & Metadata:** penyematan Windows Application Manifest (`asInvoker`, `PerMonitorV2` DPI, `longPathAware`, Common Controls v6), injeksi otomatis versi dari `Cargo.toml` ke `lajufile.rc`, pemeriksaan paritas versi di CI, serta paket zip ekstensi Chromium & Firefox otomatis di workflow rilis.
