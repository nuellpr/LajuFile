# LajuFile — Desain v1

- Tanggal: 2026-11-14
- Status: draft untuk approval
- Pemilik: Nuel
- Target: Windows 10/11 x64, dipakai pribadi, disiapkan agar layak open-source

## 1. Ringkasan

LajuFile adalah download manager desktop Windows berbasis Rust dengan GUI egui, yang meniru pengalaman Internet Download Manager (IDM): unduhan HTTP/HTTPS multi-koneksi, resume, antrean, batas kecepatan, kategori, system tray, dan penangkapan unduhan dari browser Edge lewat extension + bridge lokal.

v1 harus sudah bisa menangkap unduhan dari browser. Fitur berat IDM yang lain (penangkap video, site grabber, scheduler) bukan bagian v1.

## 2. Intent dan alasan

- Pemakaian harian memakai Edge; IDM menunjukkan unduhan besar jauh lebih cepat dengan banyak koneksi dan bisa dilanjutkan.
- Tujuan: alat sendiri yang ringan, tanpa langganan, bisa dimodifikasi, dan berpotensi dipublikasikan di GitHub.
- Definisi sukses: LajuFile dipakai sehari-hari; unduhan dari Edge masuk otomatis; pause/resume dan restart aplikasi tidak merusak file; kecepatan mendekati kapasitas jaringan.

## 3. Tujuan v1

1. Tambah unduhan lewat URL (dialog), antrean, mulai, jeda, lanjutkan, hapus.
2. Unduhan HTTP/HTTPS multi-segment (default 8 koneksi, maksimum 32), fallback ke single-stream.
3. Resume setelah pause dan setelah aplikasi ditutup/crash, dengan validasi ETag/Last-Modified/ukuran.
4. Batas kecepatan global; tampilan kecepatan dan ETA.
5. GUI mirip IDM: toolbar, sidebar kategori, tabel unduhan, status bar, tray.
6. Extension Edge (Chromium MV3): tangkap unduhan otomatis + context menu "Download with LajuFile", dengan cookie/referer/user-agent.
7. Bridge lokal `127.0.0.1` dengan token.
8. Pengaturan persisten, minimize to tray, autostart opsional.

## 4. Non-goals v1

- Penangkap video/streaming (YouTube dan sejenisnya), site grabber, scheduler, FTP/SFTP, torrent.
- Firefox (menyusul setelah Chromium), macOS, Linux.
- Auto-update, installer, code signing, publikasi ke Edge Add-ons.
- Notifikasi toast native (v1.1). Status tray + status bar cukup untuk v1.
- Clipboard monitor otomatis (v1.1). v1 tetap mendukung tempel URL manual.
- Multi-bahasa. UI v1 berbahasa Inggris; string dikumpulkan agar mudah dilokalkan.

## 5. Anti-goals

- Kehilangan unduhan pengguna saat capture gagal. Extension wajib fail-safe: biarkan unduhan browser berjalan.
- File final yang korup/setengah jadi. File final hanya muncul setelah selesai; resume divalidasi.
- Berjalan sebagai admin/service. Semua integrasi memakai HKCU dan proses user biasa.
- Menyimpan cookie sesi dalam bentuk plaintext.
- Menambah fitur "untuk nanti" yang tidak ada di tujuan v1.

## 6. Konstrain

- Windows 10/11 x64, Rust stable toolchain MSVC. Saat ini Rust belum terpasang di mesin; perlu rustup + Visual Studio Build Tools.
- Browser target pertama: Edge (Chromium). Extension dipasang sideload lewat `edge://extensions` (Developer mode). Ini terdokumentasi di README.
- GUI memakai `eframe`/`egui` 0.37 dan `egui_extras` 0.37 (versi saat desain). Builder memakai versi terbaru yang kompatibel.
- `tray-icon` 0.26 punya contoh resmi integrasi eframe; ikon tray dibuat di dalam closure `run_native`.
- Bridge hanya listen di loopback dan hanya menerima request bertoken.
- Open-source nanti: tidak memakai aset berlisensi ketat; lisensi final diputuskan saat publikasi (default MIT).
- Lingkungan pengembangan saat ini: Windows 10 Pro 22H2, Git dan Chocolatey tersedia; Rust/rustup, Visual Studio Build Tools, dan winget belum terpasang. Instalasi toolchain (butuh izin admin/UAC) dilakukan setelah desain disetujui.

## 7. Pendekatan yang dipertimbangkan

### GUI

- **A. Tauri 2 (Rust + HTML/CSS/TS).** UI paling mudah dipoles, tapi dua bahasa, butuh Node, dan menambah lapisan IPC. Tidak dipilih.
- **B. egui/eframe (100% Rust).** Satu toolchain, binary kecil, iterasi cepat, tray-icon punya contoh resmi dengan eframe. Polish UI lebih terbatas tapi cukup untuk toolbar + sidebar + tabel + progress bar. **Dipilih.**
- **C. Win32 native via windows-rs.** Paling mirip IDM secara visual, tapi biaya implementasi jauh lebih besar. Tidak dipilih.

### Jembatan browser

- **Native messaging** (registry + manifest per browser): bisa auto-launch, terasa paling "IDM asli", tapi setup dan debugging lebih rumit. Tidak dipilih v1.
- **Localhost HTTP bridge + token**: app harus sedang berjalan di tray; mudah di-debug; satu jalur untuk semua Chromium. **Dipilih.**

### Keputusan pengguna

- 2026-11-14: memilih **B (egui)**, browser harian **Edge**, dan **v1 wajib sudah bisa menangkap unduhan dari browser**.

## 8. Arsitektur

```
lajufile/
  Cargo.toml              # workspace
  crates/
    lajufile-core/        # engine unduh, store SQLite, settings, bridge
    crates/lajufile-app/  # eframe GUI + tray, wiring ke core
  extension/              # Edge MV3 (JS, tanpa build step)
  docs/
```

Satu proses aplikasi, tiga bagian:

- **Main thread**: eframe/egui + tray-icon.
- **Engine thread**: Tokio runtime. `DownloadManager` adalah actor yang memiliki SQLite, settings, antrean, dan task unduhan. Satu-satunya pemilik state unduhan.
- **Bridge task**: server axum di `127.0.0.1:port` di dalam runtime engine; meneruskan command ke actor.

Komunikasi UI ↔ engine:

- UI → engine: channel `Command` (`AddDownload`, `Pause`, `Resume`, `Remove`, `PauseAll`, `ResumeAll`, `UpdateSettings`, `Shutdown`).
- Engine → UI: snapshot penuh tiap 250 ms (`Vec<DownloadView>` + statistik) dan event diskrit (selesai, gagal, settings berubah).
- UI tidak pernah menyentuh SQLite.

Alasan actor: `rusqlite::Connection` tidak Sync, menghindari lock, GUI tetap responsif, dan engine bisa dites tanpa GUI.

Dependency utama (builder boleh menyesuaikan versi):

`eframe`, `egui_extras` (Table), `tray-icon`, `tokio`, `reqwest`, `serde`/`serde_json`, `rusqlite` (feature `bundled`), `axum`, `rand`, `base64`, `windows` (DPAPI + MessageBox), `opener`, `winreg`, `single-instance`, `chrono`, `image` (ikon tray).

## 9. Model data dan penyimpanan

Lokasi data via crate `directories`, di `%APPDATA%\LajuFile\`:

- `lajufile.db` — SQLite, mode WAL.
- `settings.json` — pengaturan.
- `logs/lajufile.log` — log; rotasi 5 MB, simpan satu arsip.

### Skema SQLite (v1)

```sql
CREATE TABLE downloads (
  id            INTEGER PRIMARY KEY AUTOINCREMENT,
  url           TEXT NOT NULL,
  final_url     TEXT,
  file_name     TEXT NOT NULL,
  save_path     TEXT NOT NULL,      -- path final
  temp_path     TEXT NOT NULL,      -- path .lajufile selama unduh
  size          INTEGER,            -- NULL jika tidak diketahui
  downloaded    INTEGER NOT NULL DEFAULT 0,
  status        TEXT NOT NULL,      -- queued|downloading|paused|done|error
  category      TEXT NOT NULL,      -- video|music|documents|programs|compressed|other
  connections   INTEGER NOT NULL,
  ranged        INTEGER NOT NULL DEFAULT 0,
  etag          TEXT,
  last_modified TEXT,
  cookie_enc    BLOB,               -- DPAPI, NULL jika tidak ada/gagal enkripsi
  referrer      TEXT,
  user_agent    TEXT,
  error         TEXT,
  queue_order   INTEGER NOT NULL,
  created_at    INTEGER NOT NULL,   -- unix millis
  completed_at  INTEGER
);

CREATE TABLE segments (
  download_id INTEGER NOT NULL REFERENCES downloads(id) ON DELETE CASCADE,
  idx         INTEGER NOT NULL,
  start       INTEGER NOT NULL,
  end         INTEGER NOT NULL,     -- inclusive
  done        INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (download_id, idx)
);

CREATE INDEX idx_downloads_status ON downloads(status, queue_order);
```

`PRAGMA user_version` dipakai untuk migrasi. v1 hanya versi 1.

### Settings (JSON)

```json
{
  "download_dir": "C:\\Users\\<user>\\Downloads\\LajuFile",
  "max_connections": 8,
  "max_concurrent": 3,
  "global_speed_limit_kbps": 0,
  "close_to_tray": true,
  "autostart": false,
  "auto_resume": false,
  "theme": "light",
  "bridge_port": 53841,
  "bridge_token": "<32 byte hex>"
}
```

- Token dibuat saat first run.
- Perubahan settings langsung berlaku, kecuali `bridge_port` (perlu restart; UI menampilkan catatan).
- Cookie disimpan terenkripsi DPAPI (scope CurrentUser). Jika DPAPI gagal, cookie tidak dipersist; unduhan tetap jalan pada sesi itu.

### File unduhan

- Selama unduh: file temp `<nama-final>.lajufile` di folder tujuan.
- Setelah selesai dan tervalidasi: rename ke nama final.
- Folder kategori tidak dipakai; kategori hanya filter visual. Semua file masuk `download_dir`.

## 10. Mesin unduh

### Probe metadata

- `GET` dengan `Range: bytes=0-0`, redirect diikuti (maks 10), header `Accept-Encoding: identity`.
- Timeout: 20 detik untuk Add manual/resume; 3 detik untuk probe capture dari bridge (harus selesai sebelum extension menyerah).
- `206 Partial Content` + `Content-Range` → ranged, ukuran total diketahui.
- `200 OK` → tidak ranged, ukuran dari `Content-Length` bila ada.
- `416` → file kosong.
- Simpan: `final_url`, `size`, `etag`, `last_modified`, `content_type`, saran nama dari `Content-Disposition`.

### Segmentasi

- Jumlah koneksi = `clamp(size / 1 MiB, 1, max_connections)` jika ranged dan ukuran diketahui; selain itu 1.
- Rentang dibagi rata; segmen terakhir menerima sisa.
- File temp di-`set_len(size)` bila ukuran diketahui.
- Setiap segmen membuka handle file sendiri, `seek` ke `start + done`, lalu menulis berurutan.
- Validasi respons segmen: status `206` dan `Content-Range` cocok. Jika server mengabaikan Range, fallback ke single-stream dari awal dan beri tahu pengguna.

### Siklus status

`queued → downloading → (paused | done | error)`

- Scheduler tick 500 ms: mulai `queued` selama jumlah aktif < `max_concurrent`, FIFO berdasarkan `queue_order`.
- **Pause**: cancel token; chunk yang sedang ditulis diselesaikan; counter atomik mencerminkan byte yang benar-benar tertulis; status `paused`; progress disimpan ke DB.
- **Resume**: probe ulang. Jika ada `ETag`/`Last-Modified`, kirim `If-Range` dan cocokkan; mismatch → restart dari 0. Jika tidak ada validator, minta `Range: bytes=0-65535`, bandingkan 64 KiB pertama dengan awal file temp; match → lanjut, mismatch/tidak didukung → restart. Restart ditandai di UI.
- Batasan: tanpa validator, perubahan setelah 64 KiB dengan ukuran sama tidak terdeteksi; risiko didokumentasikan.
- **Error**: tiap segmen retry 3x dengan backoff 2/5/10 detik; setelah itu status `error` dengan pesan.
- **Selesai**: flush, verifikasi ukuran, rename temp → final (dedupe bila bentrok), status `done`, catat `completed_at`.
- **Hapus**: konfirmasi; hapus record + temp; opsi hapus file final.
- **Non-ranged**: single stream, tidak bisa resume; pause berarti restart saat dilanjutkan (ditandai di UI).

### Kecepatan dan batas

- Counter byte atomik per unduhan; sampel 1 detik untuk kecepatan; ETA = sisa / kecepatan.
- Batas global memakai token bucket (KB/s; 0 = unlimited) yang dibagi semua koneksi; perubahan settings langsung berlaku.

### Nama file

Prioritas: `filename` dari extension → `Content-Disposition` (`filename*=UTF-8''` dulu, lalu `filename=`) → path URL → `download`.

- Sanitasi karakter Windows `<>:"/\|?*`, karakter kontrol, titik/spasi di akhir.
- Batas 180 karakter; dedupe `" (n)"` jika sudah ada.
- Jika panjang path > 250 karakter, potong nama file.

### Duplikat

URL sama (exact match) yang masih aktif (`queued`/`downloading`/`paused`) → bridge mengembalikan `409` dengan id yang ada; UI memfokuskan baris tersebut. Status `error`/`done` bukan duplikat.

### Redirect dan header

- Redirect diikuti maksimal 10; `final_url` disimpan.
- Header per request: `User-Agent` (dari extension, atau `LajuFile/0.1`), `Referer`, `Cookie`, `Accept: */*`, `Accept-Encoding: identity` (wajib agar Range byte-akurat).
- Terverifikasi di source reqwest: header `Cookie`/`Authorization` dibuang pada redirect lintas host, tetapi dipertahankan untuk host yang sama. v1 menerima perilaku ini karena lebih aman untuk kredensial. Jika ada URL nyata yang gagal karena butuh cookie di host CDN, baru tambahkan redirect manual dengan kebijakan eksplisit.

## 11. Bridge lokal

- Listen `127.0.0.1:53841` (dapat diubah; berlaku setelah restart).
- Token 32 byte acak (hex) dibuat saat first run, dikirim lewat header `X-LajuFile-Token`.
- Perbandingan token constant-time; body limit 64 KB; tidak pernah menulis token ke log.
- CORS: tangani `OPTIONS`, izinkan origin extension. Karena extension memakai host permission, CORS bukan pengaman utama; token yang mengamankan.
- Endpoint:
  - `GET /health` → `{ "version": "0.1.0" }`; **tanpa token** agar extension bisa membedakan app mati vs token salah.
  - `POST /downloads` body `{ url, filename?, referrer?, user_agent?, cookie? }`
    - Urutan: validasi URL → cek duplikat aktif → probe metadata sinkron (timeout 3 dtk, cookie + redirect ikut) → insert record + segments dalam satu transaksi → `201`.
    - `201 { id, filename, size }` berarti metadata sudah terverifikasi dan task sudah masuk queue. Tidak ada record kalau probe gagal.
    - `409 { id }` duplikat aktif; extension memperlakukan ini sebagai "sudah ditangani".
    - `422 { error }` probe gagal (timeout, 4xx/5xx selain 416, URL tidak bisa diunduh). Tidak ada record; browser dibiarkan melanjutkan.
    - `401` token salah, `400` URL tidak valid/bukan http(s), `500` kegagalan internal.
- Timeout POST di extension 5 detik (probe 3 dtk + margin). Manual Add memakai probe 20 dtk dan tidak lewat bridge.
- Token pairing manual: salin port + token dari Settings ke options extension; regenerate token membuat token lama langsung invalid dan user harus paste ulang. Tidak ada auto-pair di v1.
- `401` saat capture: extension memberi notifikasi dan membiarkan unduhan browser; tidak cancel.
- v1 tidak menyediakan endpoint baca daftar unduhan untuk extension.
- Jika bind gagal: banner di GUI + log; aplikasi tetap jalan tanpa capture.
- Instance kedua: named mutex (`single-instance`); instance kedua menampilkan MessageBox "LajuFile already running" lalu keluar.

## 12. Extension Edge (MV3)

File: `manifest.json`, `background.js` (service worker), `options.html/js/css`, `popup.html/js`, `icons/`.

Permission: `downloads`, `cookies`, `contextMenus`, `storage`, `notifications`; host permission `<all_urls>` dan `http://127.0.0.1/*`.

Alur auto-capture (utama: `chrome.downloads.onDeterminingFilename`, fallback: `chrome.downloads.onCreated`):

1. Service worker memelihara flag "bridge hidup" dari `GET /health` tiap 5 detik dan saat startup.
2. Saat filename determination: abaikan skema selain http/https (termasuk `blob:`/`data:`). Jika auto-capture mati atau bridge mati → panggil `suggest()` dan biarkan browser.
3. Jika bridge hidup: tahan `suggest()` (return `true`), lalu `POST /downloads` (timeout 5 dtk) sambil mengumpulkan cookie via `chrome.cookies.getAll({ url })`, referer dari `item.referrer`, UA dari `navigator.userAgent`, filename dari basename `item.filename`.
4. `201` atau `409`: panggil `chrome.downloads.cancel(item.id)` lalu `erase`, baru `suggest()`; tujuannya mencegah dialog Save As dan memastikan hanya LajuFile yang mengunduh. `409` juga memicu notifikasi "Already in LajuFile".
5. Gagal (timeout/4xx/5xx selain 201/409): panggil `suggest()` dan biarkan browser melanjutkan. Tidak ada re-issue karena unduhan belum pernah dibatalkan. `401` memicu notifikasi token salah.
6. `onCreated` menjadi jaring pengaman untuk unduhan yang tidak melewati filename determination. Alurnya sama, tetapi `suggest()` sudah terjadi; cancel dilakukan setelah `201`/`409`; dialog Save As mungkin sempat muncul. Id yang sudah ditangani disimpan di `handled` set agar tidak diproses dua kali.

Context menu pada link: "Download with LajuFile" memakai alur POST yang sama; jika app mati/gagal tampilkan `chrome.notifications`, tanpa cancel.

Popup: status bridge + toggle auto-capture + "Test connection" + "Open LajuFile". Tombol "Send this page URL" dihapus dari v1 (defer v1.1). Options: port, token, toggle auto-capture, tombol "Test connection"; setelah regenerate token, user paste ulang.

Batasan yang didokumentasikan:

- Unduhan hasil POST/form, `blob:`, dan stream ber-DRM tidak tertangkap.
- Unduhan yang dimulai sebelum aplikasi berjalan tidak tertangkap.
- Shelf unduhan browser bisa sempat berkedip sebelum dibatalkan; filename determination ditahan maksimal 5 detik saat probe.
- Cookie lintas host setelah redirect tidak didukung v1; extension hanya mengirim cookie untuk URL awal. URL yang butuh cookie di host CDN/SSO akan gagal di LajuFile dan dibiarkan ke browser.
- URL one-time/signed yang berubah saat probe bisa lolos probe lalu gagal di unduhan sebenarnya; ini batasan yang sama dengan download manager lain dan dicatat di UI/log.

## 13. GUI

```
┌──────────────────────────────────────────────────────────────────────┐
│ File   Tasks   Help                                                  │
├──────────────────────────────────────────────────────────────────────┤
│ [+ Add URL] [Pause] [Resume] [Remove]               [Settings]       │
├──────────────┬───────────────────────────────────────────────────────┤
│ All (12)     │ Name           Size     Status    Progress    Speed   │
│ Unfinished 3 │ ubuntu.iso     4.7 GB   Download  ▓▓▓▓░░ 62%   8.2MB/s │
│ Finished 9   │ report.pdf     2.1 MB   Done      ▓▓▓▓▓▓ 100%  —      │
│ Video        │ ...                                                   │
│ Music        │                                                       │
│ Documents    │                                                       │
│ Programs     │                                                       │
│ Compressed   │                                                       │
│ Other        │                                                       │
├──────────────┴───────────────────────────────────────────────────────┤
│ 2 active · 1 queued · Total 8.2 MB/s                                 │
└──────────────────────────────────────────────────────────────────────┘
```

Mockup Figma: https://www.figma.com/design/PASDzAEqJlF35dkv4L9eIx — frame `01 Main window · Light`, `02 Add download · Context`, `03 Main window · Dark`, `04 Settings`, `05 Edge extension · Popup`, `06 Tray menu`. Palet dan spacing mengikuti frame; implementasi egui tidak perlu pixel-perfect.

- Tema terang default ala IDM (toolbar abu-abu, seleksi biru, progress hijau); toggle gelap.
- Toolbar: Add URL, Pause, Resume, Remove, Settings.
- Sidebar: All, Unfinished, Finished + kategori (Video, Music, Documents, Programs, Compressed, Other) dengan jumlah.
- Tabel (egui_extras): Name, Size, Status, Progress, Speed, ETA, Added. Klik kanan: Open, Open folder, Copy URL, Pause/Resume, Remove.
- Status bar: total kecepatan, jumlah aktif/antre.
- Add dialog: URL (wajib), filename opsional, folder, jumlah koneksi, "Start now" / "Add to queue".
- Settings: folder, koneksi (1–32), unduhan bersamaan (1–10), batas kecepatan, tema, close-to-tray, autostart, auto-resume, port bridge, token (copy/regenerate + peringatan paste ulang), tombol Test connection, petunjuk pairing extension.
- Tray: Show, Pause all, Resume all, Add URL, Exit; klik kiri = show; tooltip menampilkan status.
- Tombol X → hide ke tray bila setting aktif; Exit hanya dari menu tray.
- Kategori ditentukan dari ekstensi file (daftar tetap di kode).

## 14. Persistensi dan recovery

- Snapshot progress ke DB tiap 1 detik (satu transaksi batch); pause/selesai/error langsung ditulis.
- Setelah crash, record berstatus `downloading` diubah menjadi `paused` saat startup. `auto_resume=false` default; jika true, unduhan dilanjutkan otomatis.
- File temp dipertahankan; file final tidak pernah setengah jadi.
- Log mencatat error unduhan, bridge, dan startup.

## 15. Testing dan verifikasi

Unit test:

- Pembagian rentang: file kecil, ukuran 0, ukuran tidak diketahui, jumlah koneksi 1 dan 32.
- Sanitasi dan dedupe nama file, parsing `Content-Disposition`.
- Token bridge (valid/invalid), default settings, migrasi skema.

Integration test dengan server lokal (axum test server atau `wiremock`):

- Unduhan 10 MB dengan Range → isi file identik dengan sumber.
- Server tanpa Range → single-stream benar.
- Pause → resume → file benar.
- ETag berubah → restart dari 0, bukan korup.
- Tanpa ETag/Last-Modified: prefix 64 KiB cocok → resume; prefix berubah → restart.
- 404 → status error.
- Retry pada kegagalan sementara.
- Bridge: 201 hanya setelah probe sukses; probe gagal → 422 tanpa record; duplikat aktif → 409; token salah → 401; `/health` tanpa token.

Manual checklist:

- Capture Edge: klik unduhan biasa, context menu link, saat aplikasi mati, dan setelan "Ask where to save each file" aktif (tidak boleh ada dialog Save As).
- Capture Edge: URL duplikat (409) dan token salah (401) → browser tidak kehilangan unduhan; notifikasi muncul.
- Tray: hide/show, pause all, exit, autostart.
- Restart aplikasi saat unduhan berjalan → resume benar.
- Bandingkan hash file hasil unduhan dengan sumber.
- UI: filter kategori, add dialog, settings, error state.

Perintah verifikasi: `cargo test`, `cargo clippy -- -D warnings`, `cargo fmt --check`.

## 16. Milestone

1. **Fondasi**: workspace, settings, store SQLite, probe, unduhan single-stream, temp + rename.
2. **Engine**: segmented, resume, pause, retry, queue, speed limit.
3. **GUI**: toolbar, sidebar, tabel, add/settings dialog, tray, status bar.
4. **Capture**: bridge + token, extension Edge, pairing, auto-capture dan context menu.
5. **Hardening**: recovery, integration tests, README, release build; CI opsional.

Definition of done v1: seluruh tujuan (1–8) berjalan di mesin pengguna, checklist manual lulus, test engine lulus.

## 17. Risiko dan mitigasi

| Risiko | Mitigasi |
| --- | --- |
| Integrasi tray + eframe bermasalah | Ikuti contoh resmi `tray-icon/examples/egui.rs`; hide/show via viewport command; fallback Win32 `ShowWindow` |
| Race saat capture Edge | Flag health cache 5 detik; tahan `suggest()` lalu POST; cancel hanya setelah `201`/`409`; gagal → browser lanjut; handled-id set |
| File korup setelah resume | Validasi ukuran/ETag/Last-Modified; tanpa validator pakai prefix check 64 KiB; mismatch → restart; file temp |
| Cookie sensitif | DPAPI; token tidak dilog; bridge loopback + token |
| Perilaku redirect lintas host | Cookie dibuang oleh reqwest (terverifikasi); diterima untuk keamanan; redirect manual hanya jika ada kasus nyata |
| Download butuh cookie lintas host | Tidak didukung v1; dokumentasikan; browser jadi fallback |
| URL one-time berubah saat probe | Batasan didokumentasikan; record error terlihat, user bisa retry/copy URL |
| Rust toolchain belum terpasang | Install rustup + MSVC Build Tools sebelum milestone 1 |
| Sideload extension menyulitkan | README langkah rinci; publikasi store menyusul |
| Path panjang Windows | Sanitasi + batas 180 karakter + cek 250 |
| Scheduler/notifikasi memperluas scope | Tetap non-goal v1 |

## 18. Yang diserahkan ke builder

- Versi dan fitur crate yang tepat; struktur modul; axum vs server minimal lain.
- Detail widget egui, cara membuat ikon tray/app, crate logging.
- Skema indeks DB detail dan isi migrasi.
- Implementasi test server dan fixture.
- Instruksi build Windows dan isi README.
- Semua ini tidak mengubah perilaku yang disepakati.
