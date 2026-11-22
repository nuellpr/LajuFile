//! Safe post-download archive extraction (.zip, .tar, .tar.gz, .tgz) with
//! pre-extraction Zip-Slip and symlink-escape validation.

use std::path::{Component, Path, PathBuf};
use std::process::Command;

use lajufile_core::naming::dedupe_path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ArchiveKind {
    Zip,
    Tar,
}

fn archive_kind(path: &Path) -> Option<(ArchiveKind, String)> {
    let name = path.file_name()?.to_str()?;
    let lower = name.to_ascii_lowercase();
    if let Some(stem) = lower.strip_suffix(".tar.gz") {
        let clean = &name[..stem.len()];
        return Some((ArchiveKind::Tar, clean.to_string()));
    }
    if let Some(stem) = lower.strip_suffix(".tgz") {
        let clean = &name[..stem.len()];
        return Some((ArchiveKind::Tar, clean.to_string()));
    }
    if let Some(stem) = lower.strip_suffix(".tar") {
        let clean = &name[..stem.len()];
        return Some((ArchiveKind::Tar, clean.to_string()));
    }
    if let Some(stem) = lower.strip_suffix(".zip") {
        let clean = &name[..stem.len()];
        return Some((ArchiveKind::Zip, clean.to_string()));
    }
    None
}

pub fn is_supported_archive(path: &Path) -> bool {
    archive_kind(path).is_some()
}

/// Returns true only for safe relative archive member paths that cannot escape
/// the destination folder (blocks absolute paths, Windows drive letters, NULs, and `..`).
pub fn is_safe_archive_entry(raw: &str) -> bool {
    let trimmed = raw.trim_end_matches(['\r', '\n']);
    if trimmed.is_empty() || trimmed.contains('\0') {
        return false;
    }
    if trimmed.starts_with('/') || trimmed.starts_with('\\') {
        return false;
    }
    let bytes = trimmed.as_bytes();
    if bytes.len() >= 2 && bytes[1] == b':' {
        return false;
    }
    let normalized = trimmed.replace('\\', "/");
    for segment in normalized.split('/') {
        if segment == ".." {
            return false;
        }
    }
    for component in Path::new(&normalized).components() {
        match component {
            Component::Normal(_) | Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return false,
        }
    }
    true
}

const MAX_ARCHIVE_ENTRIES: usize = 50_000;
const BOMB_CHECK_MIN_BYTES: u64 = 128 * 1024 * 1024;
const MAX_EXPANSION_RATIO: u64 = 100;

fn is_iso_date(token: &str) -> bool {
    let b = token.as_bytes();
    b.len() == 10
        && b[4] == b'-'
        && b[7] == b'-'
        && b[0..4].iter().all(u8::is_ascii_digit)
        && b[5..7].iter().all(u8::is_ascii_digit)
        && b[8..10].iter().all(u8::is_ascii_digit)
}

fn is_day_of_month(token: &str) -> bool {
    let trimmed = token.trim_end_matches(['.', '日']);
    (1..=2).contains(&trimmed.len())
        && trimmed.bytes().all(|b| b.is_ascii_digit())
        && trimmed.parse::<u8>().is_ok_and(|d| (1..=31).contains(&d))
}

fn is_month_token(token: &str) -> bool {
    !token.is_empty() && token.chars().any(char::is_alphabetic)
}

fn is_tar_time_or_year(token: &str) -> bool {
    let year = token.trim_end_matches('年');
    if year.len() == 4 && year.bytes().all(|b| b.is_ascii_digit()) {
        return true;
    }
    let Some((hour, min)) = token.split_once(':') else {
        return false;
    };
    (1..=2).contains(&hour.len())
        && min.len() == 2
        && hour.bytes().all(|b| b.is_ascii_digit())
        && min.bytes().all(|b| b.is_ascii_digit())
}

/// Parses uncompressed byte size from a `tar -tvf` line (both GNU tar and localized BSD tar).
fn parse_tar_verbose_entry_size(line: &str) -> Option<u64> {
    let parts: Vec<&str> = line.split_whitespace().collect();
    for i in 3..parts.len() {
        let Ok(size) = parts[i - 1].parse::<u64>() else {
            continue;
        };
        if is_iso_date(parts[i]) {
            return Some(size);
        }
        if i + 2 < parts.len()
            && is_tar_time_or_year(parts[i + 2])
            && ((is_month_token(parts[i]) && is_day_of_month(parts[i + 1]))
                || (is_day_of_month(parts[i]) && is_month_token(parts[i + 1])))
        {
            return Some(size);
        }
    }
    None
}

fn inspect_tar_verbose_listing(stdout: &str) -> Result<u64, String> {
    let mut total_uncompressed = 0u64;
    for line in stdout.lines() {
        let trimmed = line.trim_start();
        if trimmed.is_empty() {
            continue;
        }
        let Some(kind_char) = trimmed.chars().next() else {
            continue;
        };
        if matches!(kind_char, 'l' | 'h') {
            return Err("Blocked archive containing symlink/hardlink entries".to_string());
        }
        if !matches!(kind_char, '-' | 'd') {
            return Err(format!(
                "Blocked archive containing unsupported entry type '{kind_char}'"
            ));
        }
        if let Some(sz) = parse_tar_verbose_entry_size(trimmed) {
            total_uncompressed = total_uncompressed.saturating_add(sz);
        } else if kind_char == '-' {
            return Err("Archive inspection failed (could not verify entry size)".to_string());
        }
    }
    Ok(total_uncompressed)
}

/// Parses a `unzip -Z` (zipinfo) listing line, returning `Ok(Some(uncompressed_bytes))` for
/// allowed regular/directory entries, `Ok(None)` for header/summary lines, and `Err` for links/devices.
#[cfg_attr(not(all(unix, not(target_os = "macos"))), allow(dead_code))]
fn inspect_zipinfo_line(line: &str) -> Result<Option<u64>, String> {
    let trimmed = line.trim_start();
    if trimmed.is_empty()
        || trimmed.starts_with("Archive:")
        || trimmed.starts_with("Zip file size:")
    {
        return Ok(None);
    }
    let parts: Vec<&str> = trimmed.split_whitespace().collect();
    let Some(mode) = parts.first().copied() else {
        return Ok(None);
    };
    let mode_bytes = mode.as_bytes();
    if mode_bytes.len() < 7 || !matches!(mode_bytes[1], b'r' | b'-' | b'w' | b'x') {
        return Ok(None);
    }
    match mode_bytes[0] {
        b'-' | b'd' => {
            let size = parts
                .get(3)
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(0);
            Ok(Some(size))
        }
        b'l' | b'h' => Err("Blocked ZIP containing symlink/hardlink entries".to_string()),
        _ => Err(format!("Blocked ZIP containing special entry mode: {mode}")),
    }
}

fn check_extract_budget(
    archive_bytes: u64,
    entry_count: usize,
    total_uncompressed: u64,
    space_probe: &Path,
) -> Result<(), String> {
    if entry_count > MAX_ARCHIVE_ENTRIES {
        return Err(format!(
            "Blocked archive with too many entries ({entry_count} > {MAX_ARCHIVE_ENTRIES})"
        ));
    }
    if total_uncompressed > BOMB_CHECK_MIN_BYTES
        && total_uncompressed > archive_bytes.max(1).saturating_mul(MAX_EXPANSION_RATIO)
    {
        return Err(format!(
            "Blocked archive with suspicious decompression ratio ({total_uncompressed} bytes from {archive_bytes} bytes)"
        ));
    }
    if total_uncompressed > 0 {
        lajufile_core::download::ensure_disk_space(space_probe, total_uncompressed)
            .map_err(|e| format!("Archive extraction blocked: {e}"))?;
    }
    Ok(())
}

fn validate_tar_listing(archive: &Path) -> Result<(), String> {
    let archive_bytes = std::fs::metadata(archive)
        .map_err(|e| format!("Archive metadata failed: {e}"))?
        .len();

    let mut names_cmd = Command::new("tar");
    names_cmd.arg("-tf").arg(archive);
    crate::tools::no_window(&mut names_cmd);
    let names_out = names_cmd
        .output()
        .map_err(|e| format!("tar list failed: {e}"))?;
    if !names_out.status.success() {
        return Err("Archive listing failed (corrupt or unsupported archive)".to_string());
    }
    let mut entry_count = 0usize;
    for line in String::from_utf8_lossy(&names_out.stdout).lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        entry_count += 1;
        if entry_count > MAX_ARCHIVE_ENTRIES {
            return Err(format!(
                "Blocked archive with too many entries (>{MAX_ARCHIVE_ENTRIES})"
            ));
        }
        if !is_safe_archive_entry(trimmed) {
            return Err(format!("Blocked unsafe archive entry: {trimmed}"));
        }
    }

    let mut verbose_cmd = Command::new("tar");
    verbose_cmd.arg("-tvf").arg(archive).env("LC_ALL", "C");
    crate::tools::no_window(&mut verbose_cmd);
    let verbose_out = verbose_cmd
        .output()
        .map_err(|e| format!("tar inspect failed: {e}"))?;
    if !verbose_out.status.success() {
        return Err("Archive inspection failed (could not verify links or sizes)".to_string());
    }
    let total_uncompressed =
        inspect_tar_verbose_listing(&String::from_utf8_lossy(&verbose_out.stdout))?;
    check_extract_budget(archive_bytes, entry_count, total_uncompressed, archive)
}

#[cfg(all(unix, not(target_os = "macos")))]
fn validate_zip_listing_unix(archive: &Path) -> Result<(), String> {
    let archive_bytes = std::fs::metadata(archive)
        .map_err(|e| format!("Archive metadata failed: {e}"))?
        .len();

    let out = Command::new("unzip")
        .arg("-Z1")
        .arg(archive)
        .output()
        .map_err(|e| format!("unzip list failed: {e}"))?;
    if !out.status.success() {
        return Err("ZIP listing failed".to_string());
    }
    let mut entry_count = 0usize;
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        entry_count += 1;
        if entry_count > MAX_ARCHIVE_ENTRIES {
            return Err(format!(
                "Blocked ZIP with too many entries (>{MAX_ARCHIVE_ENTRIES})"
            ));
        }
        if !is_safe_archive_entry(trimmed) {
            return Err(format!("Blocked unsafe ZIP entry: {trimmed}"));
        }
    }
    let verbose = Command::new("unzip")
        .arg("-Z")
        .arg(archive)
        .env("LC_ALL", "C")
        .output()
        .map_err(|e| format!("unzip inspect failed: {e}"))?;
    if !verbose.status.success() {
        return Err("ZIP inspection failed (could not verify links or sizes)".to_string());
    }
    let mut total_uncompressed = 0u64;
    for line in String::from_utf8_lossy(&verbose.stdout).lines() {
        if let Some(sz) = inspect_zipinfo_line(line)? {
            total_uncompressed = total_uncompressed.saturating_add(sz);
        }
    }
    check_extract_budget(archive_bytes, entry_count, total_uncompressed, archive)
}

/// Validates and extracts a supported archive into a fresh sibling subfolder.
pub fn extract_archive_safely(archive: &Path) -> Result<PathBuf, String> {
    let (kind, raw_stem) =
        archive_kind(archive).ok_or_else(|| "Unsupported archive type".to_string())?;
    let stem = if raw_stem.trim().is_empty() {
        "extracted".to_string()
    } else {
        raw_stem
    };
    let parent = archive
        .parent()
        .ok_or_else(|| "Archive has no parent directory".to_string())?;

    // Validate every member before creating any output folder.
    match kind {
        ArchiveKind::Tar => validate_tar_listing(archive)?,
        ArchiveKind::Zip => {
            #[cfg(all(unix, not(target_os = "macos")))]
            validate_zip_listing_unix(archive)?;
            #[cfg(not(all(unix, not(target_os = "macos"))))]
            validate_tar_listing(archive)?;
        }
    }

    let dest = dedupe_path(parent, &stem);
    std::fs::create_dir_all(&dest).map_err(|e| format!("Create extract folder failed: {e}"))?;

    let status = match kind {
        ArchiveKind::Tar => {
            let mut cmd = Command::new("tar");
            cmd.arg("-xf").arg(archive).arg("-C").arg(&dest);
            crate::tools::no_window(&mut cmd);
            cmd.status()
                .map_err(|e| format!("tar extract failed: {e}"))?
        }
        ArchiveKind::Zip => {
            #[cfg(all(unix, not(target_os = "macos")))]
            {
                Command::new("unzip")
                    .args(["-q", "-o"])
                    .arg(archive)
                    .arg("-d")
                    .arg(&dest)
                    .status()
                    .map_err(|e| format!("unzip extract failed: {e}"))?
            }
            #[cfg(not(all(unix, not(target_os = "macos"))))]
            {
                let mut cmd = Command::new("tar");
                cmd.arg("-xf").arg(archive).arg("-C").arg(&dest);
                crate::tools::no_window(&mut cmd);
                cmd.status()
                    .map_err(|e| format!("tar zip extract failed: {e}"))?
            }
        }
    };
    if !status.success() {
        let _ = std::fs::remove_dir_all(&dest);
        return Err(format!(
            "Archive extractor exited with code {}",
            status.code().unwrap_or(-1)
        ));
    }
    Ok(dest)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn crc32(data: &[u8]) -> u32 {
        let mut crc = 0xFFFF_FFFFu32;
        for &byte in data {
            crc ^= u32::from(byte);
            for _ in 0..8 {
                let mask = (crc & 1).wrapping_neg();
                crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
            }
        }
        !crc
    }

    /// Builds a valid uncompressed (store-mode) ZIP archive in memory for testing.
    fn build_store_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut central = Vec::new();
        for (name, data) in entries {
            let offset = out.len() as u32;
            let crc = crc32(data);
            let len = data.len() as u32;
            let name_bytes = name.as_bytes();
            let name_len = name_bytes.len() as u16;
            // Local file header
            out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
            out.extend_from_slice(&20u16.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&crc.to_le_bytes());
            out.extend_from_slice(&len.to_le_bytes());
            out.extend_from_slice(&len.to_le_bytes());
            out.extend_from_slice(&name_len.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(name_bytes);
            out.extend_from_slice(data);
            // Central directory header
            central.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
            central.extend_from_slice(&20u16.to_le_bytes());
            central.extend_from_slice(&20u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&crc.to_le_bytes());
            central.extend_from_slice(&len.to_le_bytes());
            central.extend_from_slice(&len.to_le_bytes());
            central.extend_from_slice(&name_len.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u32.to_le_bytes());
            central.extend_from_slice(&offset.to_le_bytes());
            central.extend_from_slice(name_bytes);
        }
        let cd_offset = out.len() as u32;
        let cd_size = central.len() as u32;
        out.extend_from_slice(&central);
        // End of central directory
        out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        out.extend_from_slice(&cd_size.to_le_bytes());
        out.extend_from_slice(&cd_offset.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out
    }

    #[test]
    fn safe_archive_entry_blocks_traversal_and_absolute_paths() {
        assert!(is_safe_archive_entry("readme.txt"));
        assert!(is_safe_archive_entry("sub/dir/file.bin"));
        assert!(!is_safe_archive_entry("../evil.txt"));
        assert!(!is_safe_archive_entry("a/../../evil.txt"));
        assert!(!is_safe_archive_entry("/etc/passwd"));
        assert!(!is_safe_archive_entry("C:\\Windows\\win.ini"));
        assert!(!is_safe_archive_entry(""));
    }

    #[test]
    fn extract_archive_unpacks_safe_zip_and_blocks_zip_slip() {
        let dir = tempfile::tempdir().unwrap();
        let ok_zip = dir.path().join("bundle.zip");
        std::fs::write(
            &ok_zip,
            build_store_zip(&[("nested/hello.txt", b"hello archive")]),
        )
        .unwrap();
        let extracted = extract_archive_safely(&ok_zip).unwrap();
        assert_eq!(
            std::fs::read_to_string(extracted.join("nested/hello.txt")).unwrap(),
            "hello archive"
        );

        let evil_zip = dir.path().join("evil.zip");
        std::fs::write(&evil_zip, build_store_zip(&[("../escape.txt", b"pwn")])).unwrap();
        let err = extract_archive_safely(&evil_zip).unwrap_err();
        assert!(err.contains("unsafe"));
        assert!(!dir.path().join("escape.txt").exists());
    }

    #[test]
    fn parses_tar_and_zipinfo_sizes_and_blocks_symlinks() {
        assert_eq!(
            parse_tar_verbose_entry_size(
                "-rw-r--r--  0 0      0       12345 Nov 22 10:27 dir/hello.txt"
            ),
            Some(12345)
        );
        assert_eq!(
            parse_tar_verbose_entry_size("-rw-r--r-- root/root 54321 2026-01-01 00:00 sub/a.bin"),
            Some(54321)
        );
        assert_eq!(
            parse_tar_verbose_entry_size("-rw-r--r--  1 John Doe staff 999 Jan 01 1970 spaced.bin"),
            Some(999)
        );
        assert_eq!(
            parse_tar_verbose_entry_size("-rw-r--r--  0 0      0       8192 Agt 22 10:27 id.bin"),
            Some(8192)
        );
        assert_eq!(
            parse_tar_verbose_entry_size(
                "-rw-r--r--  0 0      0       4096 22 Okt 2026 dayfirst.bin"
            ),
            Some(4096)
        );
        assert_eq!(
            parse_tar_verbose_entry_size(
                "-rw-r--r--  0 0      0       2048 22. Dez 10:27 german.bin"
            ),
            Some(2048)
        );
        assert_eq!(
            parse_tar_verbose_entry_size(
                "-rw-r--r--  0 0      0       1024 févr. 05 2025 french.bin"
            ),
            Some(1024)
        );
        assert_eq!(
            parse_tar_verbose_entry_size("-rw-r--r--  0 0      0        512 11月 22 10:27 cjk.bin"),
            Some(512)
        );
        assert_eq!(
            inspect_tar_verbose_listing(
                "drwxr-xr-x  0 0      0          0 Agt 22 10:27 folder/\n-rw-r--r--  0 0      0       8192 Agt 22 10:27 folder/id.bin\n"
            )
            .unwrap(),
            8192
        );
        assert!(
            inspect_tar_verbose_listing("-rw-r--r--  0 0      0        ??? bad/date unknown\n")
                .is_err()
        );

        assert_eq!(inspect_zipinfo_line("Archive:  test.zip").unwrap(), None);
        assert_eq!(
            inspect_zipinfo_line(
                "-rw-r--r--  3.0 unx    12345 tx defN 26-Nov-22 10:27 dir/hello.txt"
            )
            .unwrap(),
            Some(12345)
        );
        assert_eq!(
            inspect_zipinfo_line("drwxr-xr-x  3.0 unx        0 bx stor 26-Nov-22 10:27 dir/")
                .unwrap(),
            Some(0)
        );
        assert!(
            inspect_zipinfo_line(
                "lrwxrwxrwx  3.0 unx       12 tx stor 26-Nov-22 10:27 link -> /etc/passwd"
            )
            .is_err()
        );
        assert!(
            inspect_zipinfo_line("crw-r--r--  3.0 unx        0 bx stor 26-Nov-22 10:27 devnull")
                .is_err()
        );
    }

    #[test]
    fn extract_budget_blocks_bombs_and_excessive_entries() {
        let dir = tempfile::tempdir().unwrap();
        let probe = dir.path().join("archive.zip");
        std::fs::write(&probe, b"zip").unwrap();

        // 1 MiB archive expanding to 500 MiB (500x > 100x and > 128 MiB) must be blocked.
        let bomb_err = check_extract_budget(1024 * 1024, 1, 500 * 1024 * 1024, &probe).unwrap_err();
        assert!(bomb_err.contains("ratio"), "unexpected err: {bomb_err}");

        // Too many entries must be blocked.
        let entries_err =
            check_extract_budget(1024, MAX_ARCHIVE_ENTRIES + 1, 1024, &probe).unwrap_err();
        assert!(
            entries_err.contains("entries"),
            "unexpected err: {entries_err}"
        );

        // Normal 10 MiB archive expanding to 20 MiB passes cleanly.
        assert!(check_extract_budget(10 * 1024 * 1024, 25, 20 * 1024 * 1024, &probe).is_ok());

        // Exceeding available disk space must be blocked.
        assert!(check_extract_budget(u64::MAX / 2, 1, u64::MAX, &probe).is_err());
    }
}
