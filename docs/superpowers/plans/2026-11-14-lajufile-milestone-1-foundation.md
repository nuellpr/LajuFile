# LajuFile Milestone 1 — Foundation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Membangun fondasi `lajufile-core` yang bisa membuka settings, menyimpan metadata unduhan di SQLite, memprobe URL HTTP/HTTPS, dan mengunduh satu file single-stream ke file temp lalu me-rename-nya dengan aman.

**Architecture:** Satu library Rust `lajufile-core` tanpa GUI. Modul dipisah per tanggung jawab: `paths` + `settings` untuk konfigurasi, `store` untuk SQLite, `naming` untuk nama file, `http` + `probe` untuk metadata, dan `download` untuk transfer single-stream. Orchestration end-to-end dibuktikan lewat integration test; actor/queue segmented dibangun di Milestone 2.

**Tech Stack:** Rust stable MSVC (edition 2024), Tokio, reqwest 0.13 (native-tls), rusqlite 0.42 (`bundled`), serde/serde_json, content-disposition 0.4, axum 0.8 (test server), tempfile.

**Spec:** `docs/superpowers/specs/2026-11-14-lajufile-design.md`

## Global Constraints

- Target hanya Windows 10/11 x64; toolchain `stable-x86_64-pc-windows-msvc`. Tidak ada kode GUI, bridge, extension, segmented download, queue, atau resume di milestone ini.
- Data dir `%APPDATA%\LajuFile`; DB `lajufile.db` mode WAL; temp file selalu `<nama-final>.lajufile`.
- Semua request mengirim `Accept-Encoding: identity`; redirect maksimal 10; probe memakai `Range: bytes=0-0`.
- Timeout probe ditentukan caller: 20 detik untuk Add manual (default), 3 detik untuk bridge (dipakai di M4).
- File final tidak boleh dibuat sebelum ukuran terverifikasi; jangan pernah menimpa file yang sudah ada.
- Token bridge 32 byte acak, hex 64 karakter; token kosong/rusak harus diperbaiki saat load.
- Nama file: sanitasi karakter Windows, maksimum 180 karakter, dedupe `" (n)"`, jaga total path ≤ 250 karakter.
- Semua error memakai `thiserror`; tidak ada `unwrap()` di kode produksi kecuali pada invariant yang tidak mungkin gagal dan diberi komentar.
- Perintah verifikasi: `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`.
- UI English; milestone ini belum punya string UI. Komentar kode boleh Indonesia/Inggris singkat; identifier Inggris.

## Review Focus

1. Server mengirim body terkompresi/`Content-Encoding` selain identity → probe dan download harus menolak, bukan menyimpan byte terkompresi sebagai file. Test di Task 5 dan 6.
2. `Content-Length`/probe size tidak sama dengan byte yang benar-benar ditulis → tidak boleh ada file final; temp dipertahankan. Test di Task 6.
3. `Content-Disposition` berisi `../`, backslash, atau karakter Windows invalid → nama harus tersanitasi dan tidak keluar dari folder tujuan. Test di Task 4.
4. File final sudah ada saat selesai → jangan timpa; dedupe sebelum mulai dan tolak race saat rename. Test di Task 4 dan 6.
5. `settings.json` berisi token kosong/rusak karena edit manual → token baru 32 byte dibuat dan disimpan, bukan diterima apa adanya. Test di Task 2.

---

### Task 1: Toolchain dan workspace

**Files:**
- Create: `Cargo.toml`, `rust-toolchain.toml`, `.gitignore`, `crates/lajufile-core/Cargo.toml`, `crates/lajufile-core/src/lib.rs`

**Interfaces:**
- Consumes: tidak ada.
- Produces: workspace `lajufile-core` yang lolos `cargo check`; modul ditambahkan oleh task berikutnya melalui `src/lib.rs`.

- [ ] **Step 1: Pasang/verifikasi toolchain**

Buka PowerShell **as Administrator** (UAC), lalu jalankan hanya jika `rustc`/`cl` belum ada:

```powershell
choco install visualstudio2022buildtools visualstudio2022-workload-vctools -y
Invoke-WebRequest https://win.rustup.rs/x86_64 -OutFile "$env:TEMP\rustup-init.exe"
& "$env:TEMP\rustup-init.exe" -y --default-toolchain stable-x86_64-pc-windows-msvc --profile default
rustup component add rustfmt clippy
```

Verifikasi di shell baru:

```powershell
rustc -V
cargo -V
```

Expected: versi stable dan target `x86_64-pc-windows-msvc`; `cargo check` nanti membuktikan `link.exe` ditemukan.

- [ ] **Step 2: Buat workspace dan manifest**

`Cargo.toml`:

```toml
[workspace]
members = ["crates/lajufile-core"]
resolver = "3"

[workspace.package]
version = "0.1.0"
edition = "2024"
license = "MIT"
rust-version = "1.85"

[workspace.dependencies]
content-disposition = "0.4"
directories = "6"
hex = "0.4"
percent-encoding = "2"
rand = "0.9"
reqwest = { version = "0.13", default-features = false, features = ["native-tls"] }
rusqlite = { version = "0.42", features = ["bundled"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "2"
tokio = { version = "1", features = ["fs", "io-util", "macros", "rt-multi-thread", "sync", "time"] }
url = "2"

```

`crates/lajufile-core/Cargo.toml`:

```toml
[package]
name = "lajufile-core"
version.workspace = true
edition.workspace = true
license.workspace = true
rust-version.workspace = true

[dependencies]
content-disposition.workspace = true
directories.workspace = true
hex.workspace = true
percent-encoding.workspace = true
rand.workspace = true
reqwest.workspace = true
rusqlite.workspace = true
serde.workspace = true
serde_json.workspace = true
thiserror.workspace = true
tokio.workspace = true
url.workspace = true

[dev-dependencies]
axum = "0.8"
tempfile = "3"
tokio = { version = "1", features = ["macros", "net", "rt-multi-thread"] }
```

`crates/lajufile-core/src/lib.rs`:

```rust
//! Core download engine and persistence for LajuFile.
```

`rust-toolchain.toml`:

```toml
[toolchain]
channel = "stable"
components = ["rustfmt", "clippy"]
targets = ["x86_64-pc-windows-msvc"]
```

`.gitignore`:

```text
/target
*.lajufile
```

- [ ] **Step 3: Inisialisasi git dan buktikan build**

```powershell
git init
cargo check
cargo fmt --check
cargo clippy --all-targets -- -D warnings
```

Expected: `cargo check` sukses (mengunduh crate dan mengompilasi `rusqlite` bundled), fmt/clippy tanpa error.

- [ ] **Step 4: Commit**

```powershell
git add Cargo.toml Cargo.lock rust-toolchain.toml .gitignore crates/lajufile-core/Cargo.toml crates/lajufile-core/src/lib.rs
git commit -m "chore: bootstrap LajuFile Rust workspace"
```

---

### Task 2: Paths dan settings

**Files:**
- Create: `crates/lajufile-core/src/paths.rs`, `crates/lajufile-core/src/settings.rs`, `crates/lajufile-core/tests/settings.rs`
- Modify: `crates/lajufile-core/src/lib.rs`

**Interfaces:**
- Consumes: workspace dependencies dari Task 1.
- Produces:
  - `paths::{data_dir, default_download_dir, ensure_dir, settings_path, db_path}`
  - `settings::{Settings, Theme, SettingsError, generate_token}`
  - `Settings::{load_or_create(dir: &Path) -> Result<Self, SettingsError>, save(&self, dir: &Path) -> Result<(), SettingsError>}`

- [ ] **Step 1: Tulis failing tests**

`tests/settings.rs` memuat minimal test berikut (nama dan assertion harus sama):

```rust
use lajufile_core::settings::{generate_token, Settings, Theme};

#[test]
fn defaults_match_spec() {
    let s = Settings::default();
    assert_eq!(s.max_connections, 8);
    assert_eq!(s.max_concurrent, 3);
    assert_eq!(s.global_speed_limit_kbps, 0);
    assert_eq!(s.close_to_tray, true);
    assert_eq!(s.autostart, false);
    assert_eq!(s.auto_resume, false);
    assert_eq!(s.theme, Theme::Light);
    assert_eq!(s.bridge_port, 53841);
}

#[test]
fn first_run_creates_64_hex_token_and_file() {
    let dir = tempfile::tempdir().unwrap();
    let s = Settings::load_or_create(dir.path()).unwrap();
    assert_eq!(s.bridge_token.len(), 64);
    assert!(s.bridge_token.chars().all(|c| c.is_ascii_hexdigit()));
    assert!(dir.path().join("settings.json").exists());
}

#[test]
fn second_load_keeps_token() {
    let dir = tempfile::tempdir().unwrap();
    let a = Settings::load_or_create(dir.path()).unwrap();
    let b = Settings::load_or_create(dir.path()).unwrap();
    assert_eq!(a.bridge_token, b.bridge_token);
}

#[test]
fn empty_token_is_repaired() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Settings::default();
    s.bridge_token.clear();
    s.save(dir.path()).unwrap();
    let loaded = Settings::load_or_create(dir.path()).unwrap();
    assert_eq!(loaded.bridge_token.len(), 64);
    assert!(loaded.bridge_token.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn invalid_json_is_an_error_and_file_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    std::fs::write(&path, "{ not json").unwrap();
    assert!(Settings::load_or_create(dir.path()).is_err());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "{ not json");
}

#[test]
fn save_is_atomic_and_roundtrips() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = Settings::default();
    s.max_connections = 16;
    s.bridge_token = generate_token();
    s.save(dir.path()).unwrap();
    assert!(!dir.path().join("settings.json.tmp").exists());
    let loaded = Settings::load_or_create(dir.path()).unwrap();
    assert_eq!(loaded.max_connections, 16);
    assert_eq!(loaded.bridge_token, s.bridge_token);
}

#[test]
fn generated_tokens_differ_and_are_hex() {
    let a = generate_token();
    let b = generate_token();
    assert_ne!(a, b);
    assert_eq!(a.len(), 64);
    assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
}
```

- [ ] **Step 2: Jalankan test untuk memastikan gagal**

```powershell
cargo test --test settings
```

Expected: gagal karena modul `settings` belum ada.

- [ ] **Step 3: Implementasi `paths` dan `settings`**

`paths.rs`:

```rust
pub fn data_dir() -> PathBuf;
pub fn default_download_dir() -> PathBuf;
pub fn ensure_dir(path: &Path) -> std::io::Result<()>;
pub fn settings_path(data_dir: &Path) -> PathBuf;
pub fn db_path(data_dir: &Path) -> PathBuf;
```

- `data_dir`: `directories::BaseDirs::new().map(|b| b.data_dir().join("LajuFile"))`; fallback `std::env::current_dir().unwrap_or_default().join(".lajufile")` bila `None`. Di Windows ini menghasilkan `%APPDATA%\\LajuFile`.
- `default_download_dir`: `directories::UserDirs::new()` → `download_dir()/LajuFile`; fallback `data_dir().join("downloads")`.
- `ensure_dir`: `std::fs::create_dir_all`.
- `settings_path`: `dir.join("settings.json")`; `db_path`: `dir.join("lajufile.db")`.

`settings.rs`:

```rust
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Theme { Light, Dark }

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Settings {
    pub download_dir: PathBuf,
    pub max_connections: u32,
    pub max_concurrent: u32,
    pub global_speed_limit_kbps: u64,
    pub close_to_tray: bool,
    pub autostart: bool,
    pub auto_resume: bool,
    pub theme: Theme,
    pub bridge_port: u16,
    pub bridge_token: String,
}

#[derive(Debug, thiserror::Error)]
pub enum SettingsError {
    #[error("settings I/O at {path}: {source}")]
    Io { path: PathBuf, source: std::io::Error },
    #[error("invalid settings JSON at {path}: {source}")]
    Json { path: PathBuf, source: serde_json::Error },
}

pub fn generate_token() -> String; // hex::encode(rand::random::<[u8; 32]>())
```

- `impl Default`: semua nilai spec; `download_dir: paths::default_download_dir()`, token kosong.
- `load_or_create`: `ensure_dir`; jika file tidak ada → buat default + token, `save`, return. Jika ada → baca + `serde_json::from_str`; bila token bukan 64 hex valid → generate + `save`; return.
- `save`: serialize pretty; tulis ke `settings.json.tmp` dengan `File::create` + `write_all` + `sync_all`; drop; `std::fs::rename` ke `settings.json`.
- Tambahkan `pub mod paths; pub mod settings;` ke `lib.rs`.

- [ ] **Step 4: Jalankan test sampai lulus**

```powershell
cargo test --test settings
cargo clippy --all-targets -- -D warnings
```

Expected: semua test settings PASS, clippy bersih.

- [ ] **Step 5: Commit**

```powershell
git add crates/lajufile-core/src/lib.rs crates/lajufile-core/src/paths.rs crates/lajufile-core/src/settings.rs crates/lajufile-core/tests/settings.rs
git commit -m "feat: add paths and persistent settings"
```

---

### Task 3: SQLite store dan model unduhan

**Files:**
- Create: `crates/lajufile-core/src/store/mod.rs`, `crates/lajufile-core/src/store/model.rs`, `crates/lajufile-core/src/store/schema_v1.sql`, `crates/lajufile-core/tests/store.rs`
- Modify: `crates/lajufile-core/src/lib.rs`

**Interfaces:**
- Consumes: `paths::db_path` (test), `Settings` tidak dipakai langsung.
- Produces:
  - `store::Store` dengan `open`, `open_in_memory`, `insert_download`, `get_download`, `list_downloads`, `active_by_url`, `next_queue_order`, `update_status`, `update_progress`, `mark_done`, `delete_download`, `segments`.
  - `store::{Download, NewDownload, Segment, DownloadStatus, Category, StoreError, now_millis}`.

- [ ] **Step 1: Tulis failing tests**

`tests/store.rs` memuat:

```rust
fn sample(url: &str, size: u64) -> NewDownload { /* isi semua field; status Queued; queue_order 1; created_at 1_700_000_000_000 */ }

#[test]
fn open_creates_schema_v1_and_reopen_keeps_data() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("lajufile.db");
    let mut store = Store::open(&path).unwrap();
    let id = store.insert_download(&sample("https://example.com/a.iso", 1000)).unwrap();
    drop(store);
    let store = Store::open(&path).unwrap();
    assert_eq!(store.get_download(id).unwrap().id, id);
}

#[test]
fn insert_and_get_roundtrip_all_fields() {
    // assert url, final_url, file_name, save_path, temp_path, size,
    // status Queued, category Other, connections 1, ranged true,
    // etag, last_modified, downloaded 0
}

#[test]
fn insert_persists_segments_in_order() {
    // dua segmen; segments(id) mengembalikan idx 0 lalu 1
}

#[test]
fn active_by_url_only_matches_queued_downloading_paused() {
    // queued/paused/downloading -> Some; done/error -> None; URL exact match
}

#[test]
fn update_progress_updates_download_and_segment() {
    // downloaded 500; segment 0 done 500
}

#[test]
fn mark_done_sets_status_completed_at_and_downloaded() {
    // completed_at Some(123); downloaded == size
}

#[test]
fn delete_cascades_segments() {
    // delete; get -> NotFound; segments -> empty
}

#[test]
fn category_mapping_is_case_insensitive() {
    assert_eq!(Category::from_file_name("movie.MP4"), Category::Video);
    assert_eq!(Category::from_file_name("song.mp3"), Category::Music);
    assert_eq!(Category::from_file_name("report.pdf"), Category::Documents);
    assert_eq!(Category::from_file_name("setup.EXE"), Category::Programs);
    assert_eq!(Category::from_file_name("archive.zip"), Category::Compressed);
    assert_eq!(Category::from_file_name("ubuntu.iso"), Category::Other);
    assert_eq!(Category::from_file_name("README"), Category::Other);
}
```

- [ ] **Step 2: Jalankan test untuk memastikan gagal**

```powershell
cargo test --test store
```

Expected: gagal karena modul `store` belum ada.

- [ ] **Step 3: Implementasi model, schema, dan Store**

`store/schema_v1.sql` harus sama dengan skema di spec §9: tabel `downloads`, `segments` dengan `ON DELETE CASCADE`, dan `idx_downloads_status`. Tidak ada `BEGIN`/`COMMIT` di file; migrasi memakai transaksi Rust.

`store/model.rs`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DownloadStatus { Queued, Downloading, Paused, Done, Error }

impl DownloadStatus {
    pub fn as_str(self) -> &'static str;
    pub fn from_db(value: &str) -> Result<Self, StoreError>;
    pub fn is_active(self) -> bool; // Queued | Downloading | Paused
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category { Video, Music, Documents, Programs, Compressed, Other }

impl Category {
    pub fn as_str(self) -> &'static str;
    pub fn from_db(value: &str) -> Result<Self, StoreError>;
    pub fn from_file_name(name: &str) -> Self;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment { pub idx: u32, pub start: u64, pub end: u64, pub done: u64 }

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Download {
    pub id: i64, pub url: String, pub final_url: Option<String>,
    pub file_name: String, pub save_path: PathBuf, pub temp_path: PathBuf,
    pub size: Option<u64>, pub downloaded: u64, pub status: DownloadStatus,
    pub category: Category, pub connections: u32, pub ranged: bool,
    pub etag: Option<String>, pub last_modified: Option<String>,
    pub cookie_enc: Option<Vec<u8>>, pub referrer: Option<String>,
    pub user_agent: Option<String>, pub error: Option<String>,
    pub queue_order: i64, pub created_at: i64, pub completed_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewDownload {
    pub url: String, pub final_url: Option<String>,
    pub file_name: String, pub save_path: PathBuf, pub temp_path: PathBuf,
    pub size: Option<u64>, pub status: DownloadStatus, pub category: Category,
    pub connections: u32, pub ranged: bool, pub etag: Option<String>,
    pub last_modified: Option<String>, pub cookie_enc: Option<Vec<u8>>,
    pub referrer: Option<String>, pub user_agent: Option<String>,
    pub queue_order: i64, pub created_at: i64, pub segments: Vec<Segment>,
}

pub fn now_millis() -> i64;
```

`Category::from_file_name`: extension lowercase; video `mp4,mkv,avi,mov,webm,wmv,flv,m4v,mpg,mpeg,ts`; music `mp3,flac,wav,aac,ogg,m4a,wma,opus`; documents `pdf,doc,docx,xls,xlsx,ppt,pptx,txt,rtf,csv,epub,md`; programs `exe,msi,msix,appx,dll,bat,cmd,ps1`; compressed `zip,rar,7z,tar,gz,bz2,xz,zst`; selain itu `Other` (termasuk `iso`). Case-insensitive.

`store/mod.rs`:

```rust
pub mod model;
pub use model::*;

pub struct Store { conn: rusqlite::Connection }

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("sqlite: {0}")] Sqlite(#[from] rusqlite::Error),
    #[error("I/O: {0}")] Io(#[from] std::io::Error),
    #[error("download {0} not found")] NotFound(i64),
    #[error("unsupported schema version {0}")] UnsupportedSchema(i32),
    #[error("invalid status in DB: {0}")] InvalidStatus(String),
    #[error("invalid category in DB: {0}")] InvalidCategory(String),
}
```

- `open`: buat parent, `Connection::open`, `PRAGMA journal_mode=WAL` (query_row, bukan pragma_update), `PRAGMA foreign_keys=ON`, `busy_timeout(5s)`, migrasi.
- `open_in_memory`: schema + foreign keys; WAL tidak diwajibkan.
- Migrasi: baca `PRAGMA user_version`; 0 → transaksi `execute_batch(schema_v1.sql)`, set `user_version=1`, commit; 1 → no-op; lainnya → `UnsupportedSchema`.
- `insert_download`: transaksi; insert semua kolom; `last_insert_rowid`; insert segmen; commit. Path disimpan dengan `to_string_lossy()`; `u64`→`i64`.
- `get_download`: query by id, map status/category via `from_db`, konversi `Option<i64>` ke `Option<u64>`.
- `list_downloads`: `ORDER BY queue_order, id`.
- `active_by_url`: `WHERE url = ? AND status IN ('queued','downloading','paused') ORDER BY id DESC LIMIT 1`.
- `next_queue_order`: `SELECT COALESCE(MAX(queue_order), 0) + 1 FROM downloads`.
- `update_status`: update status + error; 0 rows → NotFound.
- `update_progress`: transaksi; update `downloads.downloaded`; update `segments.done` untuk `(id, idx)`; 0 rows → NotFound.
- `mark_done`: status `done`, `completed_at`, `downloaded = COALESCE(size, downloaded)`, `error = NULL`.
- `delete_download`: delete by id; 0 rows → NotFound; cascade menghapus segmen.
- `segments`: `ORDER BY idx`.
- Tambahkan `pub mod store;` ke `lib.rs`.

- [ ] **Step 4: Jalankan test sampai lulus**

```powershell
cargo test --test store
cargo clippy --all-targets -- -D warnings
```

Expected: semua test store PASS.

- [ ] **Step 5: Commit**

```powershell
git add crates/lajufile-core/src/lib.rs crates/lajufile-core/src/store crates/lajufile-core/tests/store.rs
git commit -m "feat: add SQLite download store"
```

---

### Task 4: Nama file, kategori, dan dedupe

**Files:**
- Create: `crates/lajufile-core/src/naming.rs`, `crates/lajufile-core/tests/naming.rs`
- Modify: `crates/lajufile-core/src/lib.rs`

**Interfaces:**
- Consumes: `store::Category` dari Task 3.
- Produces:
  - `naming::{MAX_FILE_NAME_CHARS, sanitize_file_name, truncate_file_name, resolve_file_name, dedupe_path, temp_path_for}`
  - `sanitize_file_name(&str) -> String`
  - `resolve_file_name(explicit: Option<&str>, content_disposition: Option<&str>, url: &Url) -> String`
  - `dedupe_path(dir: &Path, file_name: &str) -> PathBuf`
  - `temp_path_for(final_path: &Path) -> PathBuf` (append `.lajufile`)

- [ ] **Step 1: Tulis failing tests**

Test wajib:

```rust
#[test]
fn sanitizes_windows_invalid_characters() {
    assert_eq!(sanitize_file_name(r#"a<b>c:d"e/f\g|h?i*"#), "a_b_c_d_e_f_g_h_i_");
}

#[test]
fn trims_trailing_dots_and_spaces_and_empty_becomes_download() {
    assert_eq!(sanitize_file_name("report.pdf... "), "report.pdf");
    assert_eq!(sanitize_file_name("   "), "download");
}

#[test]
fn truncates_to_180_chars_preserving_extension() {
    let name = format!("{}.pdf", "a".repeat(300));
    let out = truncate_file_name(&name, 180);
    assert!(out.chars().count() <= 180);
    assert!(out.ends_with(".pdf"));
}

#[test]
fn explicit_name_wins_over_content_disposition() {
    let url = Url::parse("https://example.com/from-url.bin").unwrap();
    let cd = Some("attachment; filename*=UTF-8''from-cd%20file.iso");
    assert_eq!(resolve_file_name(Some("explicit.zip"), cd, &url), "explicit.zip");
}

#[test]
fn utf8_content_disposition_wins_over_url() {
    let url = Url::parse("https://example.com/from-url.bin").unwrap();
    let cd = Some("attachment; filename*=UTF-8''laporan%20annual.pdf");
    assert_eq!(resolve_file_name(None, cd, &url), "laporan annual.pdf");
}

#[test]
fn url_path_is_percent_decoded() {
    let url = Url::parse("https://example.com/my%20file.iso").unwrap();
    assert_eq!(resolve_file_name(None, None, &url), "my file.iso");
}

#[test]
fn content_disposition_path_traversal_is_sanitized() {
    let url = Url::parse("https://example.com/a").unwrap();
    let cd = Some(r#"attachment; filename="..\..\evil.exe""#);
    let out = resolve_file_name(None, cd, &url);
    assert_eq!(out, ".._.._evil.exe");
    assert!(!out.contains('/'));
    assert!(!out.contains('\\'));
}

#[test]
fn missing_name_falls_back_to_download() {
    let url = Url::parse("https://example.com/").unwrap();
    assert_eq!(resolve_file_name(None, None, &url), "download");
}

#[test]
fn dedupes_existing_file_and_temp() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("file.zip"), b"x").unwrap();
    assert_eq!(dedupe_path(dir.path(), "file.zip").file_name().unwrap(), "file (1).zip");
    std::fs::write(dir.path().join("file (1).zip.lajufile"), b"x").unwrap();
    assert_eq!(dedupe_path(dir.path(), "file.zip").file_name().unwrap(), "file (2).zip");
}
```

Mapping `Category` sudah diimplementasikan dan diuji di Task 3; task ini tidak mengubahnya.

- [ ] **Step 2: Jalankan test untuk memastikan gagal**

```powershell
cargo test --test naming
```

Expected: gagal karena modul `naming` belum ada.

- [ ] **Step 3: Implementasi**

- `sanitize_file_name`: ganti `< > : " / \ | ? *` dan karakter kontrol dengan `_`; trim spasi/titik di akhir; trim spasi di awal; kosong → `"download"`; panggil truncate 180.
- `truncate_file_name`: hitung char; jika ≤ max return; ambil ekstensi terakhir (titik setelah karakter pertama, ekstensi ≤ 20 char); sisakan ruang ekstensi; potong stem; trim trailing spasi/titik; jika kosong → `"download" + ext`; total ≤ max.
- `resolve_file_name`: prioritas explicit non-empty → `content_disposition` via `content_disposition::parse_content_disposition(cd).filename_full()` → segmen terakhir path URL yang di-percent-decode (`percent_encoding::percent_decode_str(...).decode_utf8_lossy()`) → `"download"`; sanitasi hasil.
- `dedupe_path`: hitung max = `min(180, 250 - dir_string_len - 1)`; truncate name ke max; cek final dan `temp_path_for(candidate)`; jika salah satu ada, coba `"stem (n).ext"` untuk n=1..=9999; return path pertama yang bebas. Jangan pernah menimpa.
- `temp_path_for`: `PathBuf::from(format!("{}.lajufile", final_path.display()))` atau `OsString` append yang aman.
- Tambahkan `pub mod naming;` ke `lib.rs`.

- [ ] **Step 4: Jalankan test sampai lulus**

```powershell
cargo test --test naming
cargo clippy --all-targets -- -D warnings
```

Expected: semua test naming PASS.

- [ ] **Step 5: Commit**

```powershell
git add crates/lajufile-core/src/lib.rs crates/lajufile-core/src/naming.rs crates/lajufile-core/tests/naming.rs
git commit -m "feat: sanitize and dedupe download file names"
```

---

### Task 5: HTTP client dan probe metadata

**Files:**
- Create: `crates/lajufile-core/src/http.rs`, `crates/lajufile-core/src/probe.rs`, `crates/lajufile-core/tests/probe.rs`, `crates/lajufile-core/tests/support/mod.rs`
- Modify: `crates/lajufile-core/src/lib.rs`

**Interfaces:**
- Consumes: `url::Url`, reqwest.
- Produces:
  - `http::{DEFAULT_USER_AGENT, RequestOptions, build_client, apply_headers, ContentRange, parse_content_range}`
  - `probe::{ProbeRequest, ProbeResult, ProbeError, probe}`
  - `probe(client: &reqwest::Client, req: &ProbeRequest) -> Result<ProbeResult, ProbeError>`

- [ ] **Step 1: Buat test server helper**

`tests/support/mod.rs` menyediakan `pub async fn start_server() -> String` (base URL) dengan payload 1 MiB deterministik dan route:

- `GET /range`: jika `Range: bytes=0-0` → `206`, `Content-Range: bytes 0-0/1048576`, body 1 byte, header `ETag: "v1"`, `Last-Modified`, `Content-Disposition: attachment; filename*=UTF-8''ubuntu%20test.iso`; tanpa Range → `200` body penuh. Handler mengembalikan `400` jika `Accept-Encoding` bukan `identity`.
- `GET /plain`: `200` body penuh, `Content-Length`.
- `GET /empty`: `200` body kosong, `Content-Length: 0`.
- `GET /missing`: `404`.
- `GET /redirect`: `307` ke `/range`.
- `GET /encoded`: `200` + `Content-Encoding: gzip` + body kecil.
- `GET /slow`: tidur 200 ms lalu `200`.
- `GET /range-nolen`: `206` dengan `Content-Range: bytes 0-0/*`.

Gunakan `axum::Router`, `tokio::net::TcpListener` port 0, `axum::serve`, dan spawn task.

- [ ] **Step 2: Tulis failing tests**

`tests/probe.rs`:

```rust
#[tokio::test]
async fn probe_206_returns_size_and_metadata() {
    let base = support::start_server().await;
    let client = build_client(Duration::from_secs(5), Duration::from_secs(60)).unwrap();
    let req = ProbeRequest { url: Url::parse(&format!("{base}/range")).unwrap(), options: RequestOptions::default(), timeout: Duration::from_secs(2) };
    let r = probe(&client, &req).await.unwrap();
    assert_eq!(r.size, Some(1_048_576));
    assert!(r.ranged);
    assert_eq!(r.etag.as_deref(), Some("\"v1\""));
    assert!(r.content_disposition.unwrap().contains("ubuntu"));
    assert!(r.final_url.as_str().ends_with("/range"));
}

#[tokio::test]
async fn probe_200_without_range_sets_ranged_false_and_size() { /* /plain -> size 1 MiB, !ranged */ }

#[tokio::test]
async fn probe_404_maps_to_status_error() { /* matches!(err, ProbeError::Status(404)) */ }

#[tokio::test]
async fn probe_follows_redirect_and_returns_final_url() { /* /redirect -> ends /range */ }

#[tokio::test]
async fn probe_416_with_zero_total_is_empty_file() { /* /empty-probe -> size Some(0), ranged */ }

#[tokio::test]
async fn probe_rejects_encoded_response() { /* /encoded -> UnsupportedEncoding("gzip") */ }

#[tokio::test]
async fn probe_times_out() { /* /slow, timeout 50ms -> Request timeout */ }

#[tokio::test]
async fn probe_handles_unknown_total() { /* /range-nolen -> size None, ranged true */ }
```

Tambahan unit test di `http.rs`: `apply_headers` menghasilkan `Accept-Encoding: identity`, `User-Agent` default/override, `Referer`, `Cookie`, `Accept: */*`; `parse_content_range("bytes 0-0/1000")` → `ContentRange { start:0, end:0, total:Some(1000) }`; `"bytes */0"` → `total:Some(0)`; invalid → `None`.

Test 416 membutuhkan route tambahan `/empty-probe` yang mengembalikan `416` + `Content-Range: bytes */0`.

- [ ] **Step 3: Jalankan test untuk memastikan gagal**

```powershell
cargo test --test probe
```

Expected: gagal karena `http`/`probe` belum ada.

- [ ] **Step 4: Implementasi**

`http.rs`:

```rust
pub const DEFAULT_USER_AGENT: &str = "LajuFile/0.1";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RequestOptions {
    pub user_agent: Option<String>,
    pub referrer: Option<String>,
    pub cookie: Option<String>,
}

pub fn build_client(connect_timeout: Duration, read_timeout: Duration) -> Result<reqwest::Client, reqwest::Error>;

pub fn apply_headers(builder: reqwest::RequestBuilder, opts: &RequestOptions) -> reqwest::RequestBuilder;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContentRange { pub start: u64, pub end: u64, pub total: Option<u64> }

pub fn parse_content_range(value: &str) -> Option<ContentRange>;
```

- `build_client`: redirect `Policy::limited(10)`, connect timeout, read timeout.
- `apply_headers`: UA override atau default; Referer, Cookie, `Accept: */*`, `Accept-Encoding: identity`.
- `parse_content_range`: format `bytes <start>-<end>/<total|*>`; angka u64; return None jika format salah.

`probe.rs`:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeResult {
    pub final_url: Url,
    pub size: Option<u64>,
    pub ranged: bool,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub content_type: Option<String>,
    pub content_disposition: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ProbeRequest {
    pub url: Url,
    pub options: RequestOptions,
    pub timeout: Duration,
}

#[derive(Debug, thiserror::Error)]
pub enum ProbeError {
    #[error("HTTP {0}")] Status(u16),
    #[error("request failed: {0}")] Request(#[from] reqwest::Error),
    #[error("invalid Content-Range: {0}")] ContentRange(String),
    #[error("unsupported URL scheme: {0}")] UnsupportedScheme(String),
    #[error("unsupported Content-Encoding: {0}")] UnsupportedEncoding(String),
}

pub async fn probe(client: &reqwest::Client, req: &ProbeRequest) -> Result<ProbeResult, ProbeError>;
```

Alur:

- Tolak skema selain `http`/`https` → `UnsupportedScheme`.
- GET + `.timeout(req.timeout)` + `Range: bytes=0-0` + `apply_headers`; kirim.
- `final_url = response.url().clone()`.
- Baca header `ETag`, `Last-Modified`, `Content-Type`, `Content-Disposition` (hanya jika valid UTF-8).
- Jika ada `Content-Encoding` dan bukan `identity` → `UnsupportedEncoding`.
- `206`: parse `Content-Range`, wajib `start == 0`; `size = total`; `ranged = true`.
- `200`: `ranged = false`; `size = Content-Length` (parse header; `None` jika tidak ada).
- `416`: parse `Content-Range`; hanya terima jika total `Some(0)` → `size Some(0)`, `ranged true`; selain itu `Status(416)`.
- Status lain → `Status(code)`.
- Jangan membaca body.

- [ ] **Step 5: Jalankan test sampai lulus**

```powershell
cargo test --test probe
cargo test --lib
cargo clippy --all-targets -- -D warnings
```

Expected: semua PASS.

- [ ] **Step 6: Commit**

```powershell
git add crates/lajufile-core/src/lib.rs crates/lajufile-core/src/http.rs crates/lajufile-core/src/probe.rs crates/lajufile-core/tests/probe.rs crates/lajufile-core/tests/support/mod.rs
git commit -m "feat: probe HTTP metadata with identity encoding"
```

---

### Task 6: Single-stream download ke temp lalu rename

**Files:**
- Create: `crates/lajufile-core/src/download.rs`, `crates/lajufile-core/tests/download.rs`
- Modify: `crates/lajufile-core/src/lib.rs`

**Interfaces:**
- Consumes: `http::{RequestOptions, build_client, apply_headers, parse_content_range}`, `naming::temp_path_for`.
- Produces:
  - `download::{DownloadSpec, DownloadOutcome, DownloadError, download_single}`
  - `download_single<F>(client: &reqwest::Client, spec: &DownloadSpec, on_progress: F) -> Result<DownloadOutcome, DownloadError> where F: FnMut(u64) + Send`
  - Progress bernilai kumulatif byte yang sudah ditulis.

- [ ] **Step 1: Tulis failing tests**

`tests/download.rs` memakai `support::start_server()` dan `tempfile`:

```rust
#[tokio::test]
async fn downloads_file_and_removes_temp() {
    let base = support::start_server().await;
    let dir = tempfile::tempdir().unwrap();
    let final_path = dir.path().join("ubuntu.iso");
    let temp_path = temp_path_for(&final_path);
    let spec = DownloadSpec {
        url: Url::parse(&format!("{base}/plain")).unwrap(),
        temp_path: temp_path.clone(),
        final_path: final_path.clone(),
        expected_size: Some(1_048_576),
        options: RequestOptions::default(),
    };
    let last = Arc::new(AtomicU64::new(0));
    let last2 = last.clone();
    let out = download_single(&client, &spec, move |n| { last2.store(n, Ordering::SeqCst); }).await.unwrap();
    assert_eq!(out.bytes, 1_048_576);
    assert_eq!(std::fs::read(&final_path).unwrap(), support::payload());
    assert!(!temp_path.exists());
    assert_eq!(last.load(Ordering::SeqCst), 1_048_576);
}

#[tokio::test]
async fn refuses_existing_destination() { /* create final; expect DestinationExists; final unchanged */ }

#[tokio::test]
async fn size_mismatch_leaves_temp_and_no_final() {
    // expected 1_048_577 vs server 1_048_576; expect SizeMismatch; temp exists; final absent
}

#[tokio::test]
async fn zero_byte_download_creates_empty_file() { /* /empty, expected Some(0) */ }

#[tokio::test]
async fn rejects_encoded_response() { /* /encoded -> UnsupportedEncoding; no final */ }

#[tokio::test]
async fn missing_url_maps_to_status_error() { /* /missing -> Status(404) */ }

#[tokio::test]
async fn retry_truncates_stale_temp() {
    // pre-write garbage to temp; download /plain; final correct; temp gone
}
```

- [ ] **Step 2: Jalankan test untuk memastikan gagal**

```powershell
cargo test --test download
```

Expected: gagal karena `download` belum ada.

- [ ] **Step 3: Implementasi**

```rust
#[derive(Debug, Clone)]
pub struct DownloadSpec {
    pub url: Url,
    pub temp_path: PathBuf,
    pub final_path: PathBuf,
    pub expected_size: Option<u64>,
    pub options: RequestOptions,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadOutcome { pub final_path: PathBuf, pub bytes: u64 }

#[derive(Debug, thiserror::Error)]
pub enum DownloadError {
    #[error("HTTP {0}")] Status(u16),
    #[error("request failed: {0}")] Request(#[from] reqwest::Error),
    #[error("I/O: {0}")] Io(#[from] std::io::Error),
    #[error("size mismatch: expected {expected}, got {actual}")] SizeMismatch { expected: u64, actual: u64 },
    #[error("unexpected Content-Range: {0}")] ContentRange(String),
    #[error("destination already exists: {0}")] DestinationExists(PathBuf),
    #[error("unsupported Content-Encoding: {0}")] UnsupportedEncoding(String),
    #[error("unsupported URL scheme: {0}")] UnsupportedScheme(String),
}
```

Alur `download_single`:

1. Tolak skema selain http/https.
2. Jika `final_path.exists()` → `DestinationExists`.
3. Buat parent `temp_path`; `tokio::fs::File::create(&temp_path)` (truncate temp lama).
4. GET + `apply_headers` + `Accept-Encoding: identity`; tanpa Range.
5. Tolak status selain `200`/`206`; untuk `206`, validasi `Content-Range.start == 0`; tentukan `expected`:
   - mulai dari `spec.expected_size`;
   - untuk `206`, `total` dari Content-Range;
   - untuk `200`, `response.content_length()`;
   - jika dua nilai diketahui dan berbeda → `SizeMismatch`.
6. Tolak `Content-Encoding` selain identity.
7. Loop `response.chunk().await?`; tulis; tambah `downloaded`; panggil `on_progress(downloaded)`; jika melebihi `expected` → `SizeMismatch`.
8. `flush` + `sync_all` + drop file.
9. Jika `expected` Some dan `downloaded != expected` → `SizeMismatch` (temp tetap).
10. Cek final belum ada; `std::fs::rename(temp, final)`; jika gagal karena final muncul → error, jangan timpa.
11. Return `DownloadOutcome`.

- [ ] **Step 4: Jalankan test sampai lulus**

```powershell
cargo test --test download
cargo clippy --all-targets -- -D warnings
```

Expected: semua test download PASS.

- [ ] **Step 5: Commit**

```powershell
git add crates/lajufile-core/src/lib.rs crates/lajufile-core/src/download.rs crates/lajufile-core/tests/download.rs
git commit -m "feat: add single-stream download with safe rename"
```

---

### Task 7: End-to-end Milestone 1 dan verifikasi penuh

**Files:**
- Create: `crates/lajufile-core/tests/single_download.rs`

**Interfaces:**
- Consumes: semua API Task 2–6.
- Produces: bukti bahwa probe → naming → store → download → mark done bekerja sebagai satu alur.

- [ ] **Step 1: Tulis acceptance test**

`tests/single_download.rs`:

```rust
#[tokio::test]
async fn probe_insert_download_and_finish() {
    let base = support::start_server().await;
    let dir = tempfile::tempdir().unwrap();
    let download_dir = dir.path().join("downloads");
    std::fs::create_dir_all(&download_dir).unwrap();

    let mut store = Store::open(&dir.path().join("lajufile.db")).unwrap();
    let client = build_client(Duration::from_secs(5), Duration::from_secs(60)).unwrap();
    let probe_req = ProbeRequest {
        url: Url::parse(&format!("{base}/range")).unwrap(),
        options: RequestOptions::default(),
        timeout: Duration::from_secs(2),
    };
    let meta = probe(&client, &probe_req).await.unwrap();
    assert_eq!(meta.size, Some(1_048_576));

    let name = resolve_file_name(None, meta.content_disposition.as_deref(), &meta.final_url);
    assert_eq!(name, "ubuntu test.iso");
    let final_path = dedupe_path(&download_dir, &name);
    let temp_path = temp_path_for(&final_path);
    let segment = Segment { idx: 0, start: 0, end: meta.size.unwrap() - 1, done: 0 };
    let new = NewDownload {
        url: probe_req.url.to_string(),
        final_url: Some(meta.final_url.to_string()),
        file_name: name.clone(),
        save_path: final_path.clone(),
        temp_path: temp_path.clone(),
        size: meta.size,
        status: DownloadStatus::Queued,
        category: Category::from_file_name(&name),
        connections: 1,
        ranged: meta.ranged,
        etag: meta.etag.clone(),
        last_modified: meta.last_modified.clone(),
        cookie_enc: None,
        referrer: None,
        user_agent: Some(DEFAULT_USER_AGENT.to_string()),
        queue_order: 1,
        created_at: now_millis(),
        segments: vec![segment],
    };
    let id = store.insert_download(&new).unwrap();
    store.update_status(id, DownloadStatus::Downloading, None).unwrap();

    let spec = DownloadSpec {
        url: meta.final_url.clone(),
        temp_path,
        final_path: final_path.clone(),
        expected_size: meta.size,
        options: RequestOptions::default(),
    };
    let outcome = download_single(&client, &spec, |_| {}).await.unwrap();
    store.update_progress(id, outcome.bytes, 0, outcome.bytes).unwrap();
    store.mark_done(id, now_millis()).unwrap();

    let row = store.get_download(id).unwrap();
    assert_eq!(row.status, DownloadStatus::Done);
    assert_eq!(row.downloaded, 1_048_576);
    assert!(row.completed_at.is_some());
    assert_eq!(std::fs::read(&final_path).unwrap(), support::payload());
    assert!(!spec.temp_path.exists());
    assert_eq!(store.active_by_url(&row.url).unwrap(), None);
}
```

- [ ] **Step 2: Jalankan dan perbaiki kegagalan integrasi**

```powershell
cargo test --test single_download -- --nocapture
```

Expected: PASS. Jika gagal, perbaiki pemanggilan API, bukan mengubah perilaku yang sudah diuji task lain.

- [ ] **Step 3: Verifikasi penuh**

```powershell
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

Expected: fmt bersih, clippy tanpa warning, semua test PASS (settings, store, naming, probe, download, single_download).

- [ ] **Step 4: Commit**

```powershell
git add crates/lajufile-core/tests/single_download.rs
git commit -m "test: add milestone 1 end-to-end download"
```

---

## Catatan cakupan

- Milestone 1 selesai jika semua test di atas lulus dan `cargo clippy --all-targets -- -D warnings` bersih di Windows.
- Belum termasuk: GUI, tray, actor/queue, segmented, pause/resume, speed limit, bridge, extension, DPAPI, logging file. Itu Milestone 2–5.
- Setelah M1 lulus, plan Milestone 2 dibuat terpisah dengan spec yang sama dan API `Store`/`ProbeResult`/`DownloadSpec` sebagai titik sambung.
