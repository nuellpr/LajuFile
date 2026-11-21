# Security Policy / Keamanan

## Bahasa Indonesia

Harap laporkan masalah keamanan secara pribadi, bukan melalui isu publik: gunakan formulir **[Report a vulnerability](https://github.com/nuellpr/LajuFile/security/advisories/new)** di repositori ini di GitHub. Jelaskan apa yang Anda temukan, langkah reproduksinya, dampaknya, dan versi LajuFile yang Anda gunakan (**Settings → About** atau menu **Help** menampilkannya).

Anda akan mendapatkan tanggapan secepat mungkin, dan perbaikan akan dirilis sebelum detail teknis dipublikasikan.

Yang paling kritis dari sisi keamanan di LajuFile:

- **Bridge browser lokal dan saluran perintah.** Unduhan dan permintaan dari ekstensi browser dikirim melalui HTTP lokal yang hanya mendengar di `127.0.0.1` dengan token akses acak 64 karakter hex (`x-lajufile-token`, perbandingan *constant-time*), validasi header `Host` loopback tunggal untuk mencegah DNS rebinding, CORS khusus origin ekstensi, verifikasi `sender.id` pada ekstensi, serta penolakan header `Origin` browser pada endpoint `POST /shutdown` (`crates/lajufile-app/src/bridge.rs`, `extension/`).
- **Alat pendukung yang diunduh saat runtime.** `yt-dlp`, `ffmpeg`, dan `deno` diunduh melalui `reqwest` khusus HTTPS (menolak downgrade ke HTTP pada redirect) dan diverifikasi **SHA-256** secara *fail-closed* terhadap checksum resmi sebelum diekstrak atau dijalankan (`crates/lajufile-app/src/tools.rs`).
- **Auto-updater dan installer native.** Installer pembaruan diverifikasi terhadap manifes `SHA256SUMS.txt` dari GitHub Releases, memvalidasi payload PE/Authenticode, diunduh ke direktori privat `data_dir/updates`, serta menggunakan uninstaller non-rekursif agar tidak menyentuh berkas lain (`crates/lajufile-app/src/updater.rs`, `crates/lajufile-app/src/installer.rs`).
- **Berkas ke disk, proteksi SSRF, dan cookie terenkripsi.** Nama berkas yang diunduh disanitasi terhadap *path traversal* dan nama perangkat terlarang Windows sebelum menjadi jalur disk (`crates/lajufile-core/src/naming.rs`); Web Page Grabber memvalidasi DNS publik di setiap redirect dengan *IP pinning* (`crates/lajufile-app/src/engine/ssrf.rs`); dan cookie sesi disimpan terenkripsi dengan Windows DPAPI atau `HMAC-SHA256` + `CTR` berizin `0o600` di Unix (`crates/lajufile-core/src/cookie.rs`).

Hanya rilis terbaru (`v1.0.9+`) yang mendapatkan perbaikan keamanan.

---

## English

Please report security issues privately rather than in a public issue: use the **[Report a vulnerability](https://github.com/nuellpr/LajuFile/security/advisories/new)** form in this repository on GitHub. Include what you found, steps to reproduce it, potential impact, and the version you are running (**Settings → About** or the **Help** menu shows it).

You will receive a response as soon as possible, and a fix will be released before details are made public.

Key security-critical surfaces in LajuFile:

- **Local browser bridge and command channel.** Downloads from the browser extension are handed off over a local HTTP bridge bound strictly to `127.0.0.1`, guarded by a random 64-hex access token (`x-lajufile-token`, constant-time verification), strict loopback `Host` validation against DNS rebinding, extension-only CORS, strict extension `sender.id` checks, and browser `Origin` rejection on `POST /shutdown` (`crates/lajufile-app/src/bridge.rs`, `extension/`).
- **Runtime companion tools.** `yt-dlp`, `ffmpeg`, and `deno` are fetched over HTTPS-only `reqwest` (blocking HTTP redirect downgrades) and verified fail-closed against published **SHA-256** checksums before extraction or execution (`crates/lajufile-app/src/tools.rs`).
- **Self-updater and native installers.** Release installers are verified fail-closed against `SHA256SUMS.txt` from GitHub Releases, validated for PE/Authenticode payload integrity, staged in private `data_dir/updates`, and paired with a non-recursive uninstaller (`crates/lajufile-app/src/updater.rs`, `crates/lajufile-app/src/installer.rs`).
- **Disk writes, SSRF protection, and encrypted cookies.** Downloaded filenames are sanitized against path traversal and reserved Windows device names before becoming paths (`crates/lajufile-core/src/naming.rs`); Web Page Grabber enforces per-hop public DNS validation with IP pinning (`crates/lajufile-app/src/engine/ssrf.rs`); session cookies are encrypted at rest via Windows DPAPI or Unix `HMAC-SHA256` + `CTR` with `0o600` key files (`crates/lajufile-core/src/cookie.rs`).

Only the latest release (`v1.0.9+`) receives security fixes.
