# LajuFile M5 - Save Location Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let the user pick a save folder per download from the Add dialog, and optionally be asked for it on every manual add.

**Architecture:** A small in-app egui folder picker modal (no new crate), a new `ask_where_to_save` setting, and an Add-dialog Browse button. `AddRequest.save_dir` already carries the folder into the engine, so no engine change is needed.

**Tech Stack:** Rust 2024, egui/eframe 0.37, existing `lajufile-core` settings + `lajufile-app` UI.

**Spec:** `docs/superpowers/specs/2026-11-15-lajufile-m5-m6-design.md` (M5 section).

## Global Constraints

- Windows 10/11 x64; English UI copy.
- No new dependencies: the picker is plain egui (`std::fs` for directory listing).
- Browser captures keep using `settings.download_dir`; never prompt on the bridge path.
- `Settings` must stay backward compatible with existing `%APPDATA%\LajuFile\settings.json` files (`#[serde(default)]` on new fields).
- Tests must pass with `cargo test --workspace`, and clippy with `-D warnings`.

## Review Focus

1. A settings file written before M5 must load with `ask_where_to_save = false` (missing field, not an error).
2. Cancelling the folder prompt must not queue a download.
3. A folder the user picks must survive into the queued row's `save_path` (`save_dir` from `AddRequest`).
4. The picker must not panic on an empty, missing, or permission-denied folder.
5. Existing Add behavior must be unchanged when `ask_where_to_save` is off.

---

### Task 1: `ask_where_to_save` setting

**Files:**
- Modify: `crates/lajufile-core/src/settings.rs`
- Modify: `crates/lajufile-app/src/ui/dialogs.rs`
- Test: `crates/lajufile-core/src/settings.rs` (existing `#[cfg(test)]` module)

**Interfaces:**
- Produces: `Settings.ask_where_to_save: bool` (default `false`, serde default).

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn ask_where_to_save_defaults_false_and_roundtrips() {
    let settings = Settings::default();
    assert!(!settings.ask_where_to_save);
    let dir = tempfile::tempdir().unwrap();
    let mut changed = Settings::default();
    changed.ask_where_to_save = true;
    changed.save(dir.path()).unwrap();
    assert!(Settings::load_or_create(dir.path()).unwrap().ask_where_to_save);
}
```

- [ ] **Step 2: Run it and watch it fail**

Run: `cargo test -p lajufile-core ask_where_to_save_defaults_false_and_roundtrips`
Expected: compile error, `no field ask_where_to_save`.

- [ ] **Step 3: Add the field**

In `Settings`, next to `auto_resume`:

```rust
#[serde(default)]
pub ask_where_to_save: bool,
```

- [ ] **Step 4: Run the test**

Run: `cargo test -p lajufile-core ask_where_to_save_defaults_false_and_roundtrips`
Expected: PASS.

- [ ] **Step 5: Add the Settings checkbox**

In `ui/dialogs.rs`, under the Behavior section, after "Auto-resume on startup":

```rust
ui.checkbox(
    &mut settings.ask_where_to_save,
    "Ask where to save on every manual download",
);
```

- [ ] **Step 6: Run the app tests**

Run: `cargo test -p lajufile-app --lib`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add crates/lajufile-core/src/settings.rs crates/lajufile-app/src/ui/dialogs.rs
git commit -m "feat: add ask where to save setting"
```

### Task 2: Folder picker modal

**Files:**
- Create: `crates/lajufile-app/src/ui/folder_picker.rs`
- Modify: `crates/lajufile-app/src/ui/mod.rs`
- Test: unit tests in `folder_picker.rs`

**Interfaces:**
- Produces:
  - `pub struct FolderPicker { current: PathBuf, entries: Vec<PathBuf>, error: Option<String> }`
  - `impl FolderPicker { pub fn new(start: PathBuf) -> Self; pub fn show(&mut self, ctx: &egui::Context) -> Option<FolderChoice> }`
  - `pub enum FolderChoice { Select(PathBuf), Cancel }`
  - internal `fn visible_dirs(path: &Path) -> Result<Vec<PathBuf>, String>` returns sorted sub-directories, skipping hidden/system entries.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn visible_dirs_lists_only_directories_sorted() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("beta")).unwrap();
    std::fs::create_dir(dir.path().join("alpha")).unwrap();
    std::fs::write(dir.path().join("file.txt"), b"x").unwrap();
    let dirs = visible_dirs(dir.path()).unwrap();
    let names: Vec<String> = dirs
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, vec!["alpha".to_string(), "beta".to_string()]);
}

#[test]
fn visible_dirs_reports_missing_path() {
    assert!(visible_dirs(std::path::Path::new("Z:\\definitely-missing")).is_err());
}
```

- [ ] **Step 2: Run them and watch them fail**

Run: `cargo test -p lajufile-app --lib folder_picker`
Expected: compile error, module/function missing.

- [ ] **Step 3: Implement the module**

Window titled `Save to folder` with:
- a read-only current path label, `..` (parent), drive buttons for existing `C:\`-style roots, and a scrollable list of `visible_dirs(&self.current)`; clicking a sub-directory descends into it;
- `New folder` (creates `current/New folder`, then refreshes), `Select` (returns `FolderChoice::Select(current)`), `Cancel`;
- errors from `visible_dirs` shown inline, never unwrapped.

Register in `ui/mod.rs` as `pub mod folder_picker;`.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p lajufile-app --lib folder_picker`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/lajufile-app/src/ui/folder_picker.rs crates/lajufile-app/src/ui/mod.rs
git commit -m "feat: add in-app folder picker"
```

### Task 3: Browse button and ask-on-add flow

**Files:**
- Modify: `crates/lajufile-app/src/app.rs`
- Modify: `crates/lajufile-app/src/ui/dialogs.rs`
- Modify: `crates/lajufile-app/src/ui/mod.rs` (new actions)
- Test: `crates/lajufile-app/src/app.rs` unit tests

**Interfaces:**
- Consumes: `FolderPicker`, `FolderChoice`, `Settings.ask_where_to_save`.
- Produces:
  - `Action::OpenFolderPicker` (Add dialog Browse button)
  - `LajuFileApp.folder_picker: Option<FolderPicker>`
  - `LajuFileApp.pending_add: Option<AddRequest>`
  - `fn should_prompt_for_folder(settings: &Settings, manual: bool) -> bool` returning `manual && settings.ask_where_to_save`.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn folder_prompt_only_for_manual_adds_when_enabled() {
    let mut settings = Settings::default();
    assert!(!should_prompt_for_folder(&settings, true));
    settings.ask_where_to_save = true;
    assert!(should_prompt_for_folder(&settings, true));
    assert!(!should_prompt_for_folder(&settings, false));
}
```

- [ ] **Step 2: Run it and watch it fail**

Run: `cargo test -p lajufile-app --lib folder_prompt_only_for_manual_adds_when_enabled`
Expected: compile error, function missing.

- [ ] **Step 3: Implement the flow**

- Add `Browse...` next to the folder field in the Add dialog; it emits `Action::OpenFolderPicker`.
- In `handle_actions`:
  - `OpenFolderPicker`: open `FolderPicker::new(current add_draft folder or settings.download_dir)`.
  - `Add`: when `should_prompt_for_folder(&settings, true)` and `add_draft.folder` is empty, stash `pending_add = Some(draft.into_request())` and open the picker instead of sending; otherwise send as today.
- In `eframe::App::ui`, if `folder_picker` is `Some`, call `show(ctx)`:
  - `Select(path)`: if `pending_add` is `Some`, set `save_dir = Some(path.clone())` and send it; otherwise set `add_draft.folder` to the path. Then clear both fields.
  - `Cancel`: clear `pending_add` and the picker; nothing is queued.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p lajufile-app --lib`
Expected: PASS.

- [ ] **Step 5: Manual smoke**

Run the app, add a URL with a folder picked through Browse, confirm the file lands there and the row's folder matches. Repeat with `ask_where_to_save` on and Cancel, confirm nothing is queued.

- [ ] **Step 6: Commit**

```bash
git add crates/lajufile-app/src/app.rs crates/lajufile-app/src/ui/dialogs.rs crates/lajufile-app/src/ui/mod.rs
git commit -m "feat: browse and ask for save folder"
```

### Task 4: Documentation and verification

**Files:**
- Modify: `README.md`

- [ ] **Step 1: Update README feature list**

Add: "Pick a save folder per download (Browse in Add) and optionally be asked on every manual add (Settings > Ask where to save)."

- [ ] **Step 2: Full verification**

Run: `cargo fmt --check`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`.
Expected: all exit 0.

- [ ] **Step 3: Commit**

```bash
git add README.md
git commit -m "docs: document save folder picker"
```

## Self-Review

- Spec coverage: M5 folder picker (Task 2), Browse in Add and the ask setting (Tasks 1 and 3), browser captures untouched (Global Constraints and Task 3), documentation (Task 4).
- Type consistency: `FolderPicker`/`FolderChoice`/`should_prompt_for_folder` names are used the same way in Tasks 2 and 3.
- Review Focus: each line maps to a test (settings default, cancel path, save_dir in Task 3 smoke, missing directory error in Task 2, unchanged default in Task 3).
