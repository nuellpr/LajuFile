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

fn validate_tar_listing(archive: &Path) -> Result<(), String> {
    let names_out = Command::new("tar")
        .arg("-tf")
        .arg(archive)
        .output()
        .map_err(|e| format!("tar list failed: {e}"))?;
    if !names_out.status.success() {
        return Err("Archive listing failed (corrupt or unsupported archive)".to_string());
    }
    for line in String::from_utf8_lossy(&names_out.stdout).lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if !is_safe_archive_entry(trimmed) {
            return Err(format!("Blocked unsafe archive entry: {trimmed}"));
        }
    }
    let verbose_out = Command::new("tar")
        .arg("-tvf")
        .arg(archive)
        .output()
        .map_err(|e| format!("tar inspect failed: {e}"))?;
    if verbose_out.status.success() {
        for line in String::from_utf8_lossy(&verbose_out.stdout).lines() {
            let trimmed = line.trim_start();
            if trimmed.starts_with('l') || trimmed.starts_with('h') {
                return Err("Blocked archive containing symlink/hardlink entries".to_string());
            }
        }
    }
    Ok(())
}

#[cfg(all(unix, not(target_os = "macos")))]
fn validate_zip_listing_unix(archive: &Path) -> Result<(), String> {
    let out = Command::new("unzip")
        .arg("-Z1")
        .arg(archive)
        .output()
        .map_err(|e| format!("unzip list failed: {e}"))?;
    if !out.status.success() {
        return Err("ZIP listing failed".to_string());
    }
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        let trimmed = line.trim();
        if !trimmed.is_empty() && !is_safe_archive_entry(trimmed) {
            return Err(format!("Blocked unsafe ZIP entry: {trimmed}"));
        }
    }
    let verbose = Command::new("unzip")
        .arg("-Z")
        .arg(archive)
        .output()
        .map_err(|e| format!("unzip inspect failed: {e}"))?;
    if verbose.status.success() {
        for line in String::from_utf8_lossy(&verbose.stdout).lines() {
            if line.trim_start().starts_with('l') {
                return Err("Blocked ZIP containing symlink entries".to_string());
            }
        }
    }
    Ok(())
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
        ArchiveKind::Tar => Command::new("tar")
            .arg("-xf")
            .arg(archive)
            .arg("-C")
            .arg(&dest)
            .status()
            .map_err(|e| format!("tar extract failed: {e}"))?,
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
                Command::new("tar")
                    .arg("-xf")
                    .arg(archive)
                    .arg("-C")
                    .arg(&dest)
                    .status()
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
}
