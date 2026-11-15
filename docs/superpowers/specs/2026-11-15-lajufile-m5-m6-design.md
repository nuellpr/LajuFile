# LajuFile M5/M6 - save location and video download design

Status: draft for approval. Date: 2026-11-15.

## Intent

Two gaps after M4:

1. The user cannot choose where a download is saved; the Add dialog only shows
   the default folder, and there is no way to be asked at download time.
2. LajuFile cannot download YouTube videos the way IDM does: a button on the
   watch page, a list of resolutions (1440p down to audio only), and the same
   choice when a YouTube URL is pasted into Add.

The reference is IDM: pick a folder per download, and a video module that lists
formats and downloads the chosen one.

## Goals

- M5
  - A Browse button in the Add dialog that opens a folder picker, so a
    single download can go anywhere.
  - A setting "Ask where to save" (default off). When on, every manual Add
    opens the save dialog before the download is queued.
  - The chosen folder is persisted per download in `save_path`, as today.
- M6
  - Managed tools: `yt-dlp.exe` and `ffmpeg.exe` downloaded once into
    `%APPDATA%\LajuFile\tools`, never committed to the repo.
  - Pasting a YouTube URL into Add loads the available formats and shows a
    quality dropdown (video 1440/1080/720/480/360/240, audio only).
  - A content-script button on YouTube watch pages ("Download with LajuFile")
    that shows the same quality list and sends the choice to the app.
  - Video jobs appear in the normal download list with progress, pause/cancel,
    and a completed file in the chosen folder.

## Non-goals

- Prompting for a folder on every browser capture. The bridge returns in 5 s;
  waiting for a dialog would time the request out, and a cancelled dialog would
  leave the browser copy already removed. Captures keep using the default
  folder, and the user can move the file later.
- Subtitles, thumbnails, chapters, playlists, and DRM-protected media.
- A general purpose site grabber (IDM's Site Grabber is out of scope).
- Auto-updating yt-dlp/ffmpeg on a schedule. A manual "Update tools" action is
  enough for v1.

## Anti-goals

- Blocking the UI thread while yt-dlp resolves formats or downloads.
- Writing downloaded tools or video output into the git repository.
- Replacing the existing HTTP download path for normal files. Video is a new
  path next to it, not a rewrite.

## Constraints

- Windows 10/11, Rust 2024, egui 0.37, existing `lajufile-core` and
  `lajufile-app` layout.
- No new heavyweight dependency for the folder picker. The network was
  unreliable in M4 (tower-http 0.7 could not be fetched), so M5 must build with
  the crates already in the lockfile where possible.
- YouTube splits video and audio above 720p, so merging needs ffmpeg.
- Cookies for age-restricted or account-only videos are out of scope for v1;
  yt-dlp fails and the app shows the error.

## Approaches considered

### Save location

- **A1 (chosen): folder picker in Add + "Ask where to save" for manual Add.**
  Small, no bridge changes, and the user keeps full control for the downloads
  they start by hand.
- A2: also prompt for Edge captures. Rejected: the bridge call would block on a
  dialog, time out after 5 s, and the extension would leave the browser copy
  in place while LajuFile may have queued the file.

### Video download

- **B1 (chosen): yt-dlp + ffmpeg as managed tools.** Same approach as IDM's
  separate video module. Handles signature changes, merges 1440p video with
  audio, and lists formats with one `-J` call.
- B2: yt-dlp only. Rejected: no merge above 720p, which is the main reason the
  user asked for a resolution list.
- B3: pure Rust extractor. Rejected: breaks whenever YouTube changes its
  player, and the user wants it to work like IDM now.

## Design

### M5: save location

- `Settings` gains `ask_where_to_save: bool` (default false, `#[serde(default)]`).
  Settings dialog gets a checkbox under Behavior.
- The Add dialog's save row gets a **Browse...** button next to the path field.
- Folder picker: a small egui modal (`FolderPicker`) with a path box, a list of
  sub-directories, Drive roots, Up, New folder, and Select. It runs inside the
  app process, so it needs no new crate and no COM dialog. It remembers the
  last folder used in the session.
- Flow when `ask_where_to_save` is on: Add download validates the URL, then
  opens the picker prefilled with `settings.download_dir`; Cancel aborts the
  add. When off, the existing path is used.
- The Add dialog keeps its explicit folder field; the picker writes into it.
- No engine change: `AddRequest.save_dir` already carries the folder.

### M6: video downloads

- Tools manager in `lajufile-app`:
  - `tools_dir() = %APPDATA%\LajuFile\tools`.
  - `ensure_tools()` checks `yt-dlp.exe` and `ffmpeg.exe`; if missing, shows a
    modal explaining the ~70 MB download and starts a background download with
    progress in the status bar. Cancel is supported.
  - Settings shows the tool paths and an **Update tools** button.
  - Downloads use pinned release URLs; failures surface as a toast, and the
    video path stays disabled until both tools exist.
- Format listing:
  - `yt-dlp.exe -J --no-playlist <url>` on the engine runtime; parse
    `formats` and `title` into `VideoFormat { id, label, ext, height, filesize }`.
  - Build a short list: `best` (highest video+audio), 1440p, 1080p, 720p, 480p,
    360p, 240p, and **Audio only (m4a)**. Entries with no video stream are
    audio only.
  - Cache the result per URL for the session.
- Video job:
  - New `kind` column on downloads (`http` default, `video`) plus
    `video_format` text, added with a schema migration that fills existing rows
    with `http`.
  - `start_download` branches: `http` keeps the current segmented path; `video`
    spawns `yt-dlp.exe -f <selector> --merge-output-format mp4 -o <temp>
    <url>` with `--newline` progress, parses `[download]  42.3%` from stdout,
    and reports `done/total` through the existing `Event::Progress`.
  - Pause aborts the child process; resume re-runs yt-dlp with `--continue`.
    Cancel aborts and deletes the `.part` files.
  - Completion renames the merged file to `save_path` without overwriting, as
    the HTTP path does. Size and ETA come from the parsed total.
- Add dialog: when the URL host is `youtube.com` or `youtu.be`, a "Quality"
  dropdown appears after the probe; it is populated by the format listing.
  Add download sends the chosen format id.
- Extension:
  - `content.js` on `https://www.youtube.com/*` adds a small button next to
    the player: "Download with LajuFile".
  - Clicking it asks the background worker for formats via a new
    `POST /video/formats` bridge endpoint, shows the same list, and sends the
    choice with `POST /video`.
  - `POST /video` creates a `kind = video` row. It returns 201/409/422 like the
    file endpoint, and the page button never cancels a browser download.
- Bridge: `/video/formats` and `/video` share the token check, the 64 KiB body
  limit, and the CORS layer with `/downloads`. Format lookup has a 15 s timeout.

## What is left to the builder

- Exact yt-dlp selectors per quality and the fallback when a requested height is
  missing.
- Whether ffmpeg ships as a single `ffmpeg.exe` or the essentials build, and the
  pinned URLs and checksums.
- Progress parsing details and the timeout for a stalled yt-dlp process.
- Folder picker edge cases: empty drives, permission denied, long paths.
- Migration test for the `kind` column on an existing database.
