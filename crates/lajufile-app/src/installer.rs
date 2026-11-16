//! Self-extracting cross-platform installer payload packing, verification, and setup helpers.

use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use lajufile_core::checksum::sha256_bytes;

const MAGIC: &[u8; 12] = b"LAJUFILE_PK1";
const FOOTER_LEN: usize = 64 + 8 + 12;

#[cfg(windows)]
pub const APP_BIN: &str = "lajufile.exe";
#[cfg(not(windows))]
pub const APP_BIN: &str = "lajufile";

#[cfg(windows)]
pub const UNINSTALL_BIN: &str = "uninstall.exe";
#[cfg(not(windows))]
pub const UNINSTALL_BIN: &str = "uninstall";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PayloadEntry {
    pub rel_path: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct InstallOptions {
    pub install_dir: PathBuf,
    pub desktop_shortcut: bool,
    pub start_menu_shortcut: bool,
}

pub fn default_install_dir() -> PathBuf {
    #[cfg(windows)]
    {
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            return PathBuf::from(local).join("Programs").join("LajuFile");
        }
        PathBuf::from(r"C:\Program Files\LajuFile")
    }
    #[cfg(target_os = "macos")]
    {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home)
                .join("Applications")
                .join("LajuFile.app");
        }
        PathBuf::from("/Applications/LajuFile.app")
    }
    #[cfg(all(not(windows), not(target_os = "macos")))]
    {
        if let Some(xdg) = std::env::var_os("XDG_DATA_HOME") {
            return PathBuf::from(xdg).join("lajufile");
        }
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(".local/share/lajufile");
        }
        PathBuf::from("/usr/local/share/lajufile")
    }
}

pub fn launch_installed_path(install_dir: &Path) -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        if install_dir
            .extension()
            .and_then(|s| s.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("app"))
        {
            return install_dir.join("Contents").join("MacOS").join(APP_BIN);
        }
    }
    install_dir.join(APP_BIN)
}

/// Rejects absolute, drive-prefixed, empty, or `..` paths before packing or writing.
pub fn validate_rel_path(rel: &str) -> Result<PathBuf, String> {
    let trimmed = rel.trim().replace('\\', "/");
    if trimmed.is_empty() {
        return Err("empty entry path".to_string());
    }
    if trimmed.starts_with('/') || trimmed.contains(':') {
        return Err(format!("unsafe path in installer payload: {rel}"));
    }
    let mut clean = PathBuf::new();
    for comp in Path::new(&trimmed).components() {
        match comp {
            Component::Normal(part) => clean.push(part),
            _ => return Err(format!("unsafe path component in installer payload: {rel}")),
        }
    }
    if clean.as_os_str().is_empty() {
        return Err(format!("invalid path: {rel}"));
    }
    Ok(clean)
}

pub fn pack_entries(entries: &[(&str, &[u8])]) -> Result<Vec<u8>, String> {
    let count: u32 = entries
        .len()
        .try_into()
        .map_err(|_| "too many installer entries".to_string())?;
    let mut body = Vec::new();
    body.extend_from_slice(&count.to_le_bytes());

    for (rel, data) in entries {
        validate_rel_path(rel)?;
        let normalized = rel.trim().replace('\\', "/");
        let path_bytes = normalized.as_bytes();
        let path_len: u16 = path_bytes
            .len()
            .try_into()
            .map_err(|_| format!("entry path too long: {rel}"))?;
        let data_len = data.len() as u64;
        body.extend_from_slice(&path_len.to_le_bytes());
        body.extend_from_slice(path_bytes);
        body.extend_from_slice(&data_len.to_le_bytes());
        body.extend_from_slice(data);
    }

    let digest = sha256_bytes(&body);
    let body_len = body.len() as u64;
    body.extend_from_slice(digest.as_bytes());
    body.extend_from_slice(&body_len.to_le_bytes());
    body.extend_from_slice(MAGIC);
    Ok(body)
}

/// Parses an embedded payload trailer and returns `(stub_end_offset, entries)`.
pub fn parse_payload(exe_bytes: &[u8]) -> Result<(usize, Vec<PayloadEntry>), String> {
    if exe_bytes.len() < FOOTER_LEN {
        return Err("installer payload is missing (run setup builder first)".to_string());
    }
    let footer_start = exe_bytes.len() - FOOTER_LEN;
    let footer = &exe_bytes[footer_start..];
    if &footer[72..84] != MAGIC {
        return Err("installer payload magic not found".to_string());
    }
    let expected_sha = std::str::from_utf8(&footer[0..64])
        .map_err(|_| "corrupt installer checksum header".to_string())?;
    let body_len = u64::from_le_bytes(footer[64..72].try_into().unwrap()) as usize;
    let body_start = footer_start
        .checked_sub(body_len)
        .ok_or_else(|| "corrupt installer payload length".to_string())?;
    let body = &exe_bytes[body_start..footer_start];

    let actual_sha = sha256_bytes(body);
    if !actual_sha.eq_ignore_ascii_case(expected_sha) {
        return Err("installer payload failed SHA-256 integrity verification".to_string());
    }

    if body.len() < 4 {
        return Err("truncated installer header".to_string());
    }
    let count = u32::from_le_bytes(body[0..4].try_into().unwrap()) as usize;
    let mut pos = 4usize;
    let mut entries = Vec::with_capacity(count);
    for _ in 0..count {
        let path_len = read_u16(body, &mut pos)? as usize;
        let path_slice = read_slice(body, &mut pos, path_len)?;
        let rel_path = std::str::from_utf8(path_slice)
            .map_err(|_| "invalid UTF-8 path in installer payload".to_string())?;
        validate_rel_path(rel_path)?;
        let data_len = read_u64(body, &mut pos)? as usize;
        let data_slice = read_slice(body, &mut pos, data_len)?;
        entries.push(PayloadEntry {
            rel_path: rel_path.to_string(),
            bytes: data_slice.to_vec(),
        });
    }
    if pos != body.len() {
        return Err("unexpected trailing bytes in installer payload".to_string());
    }
    Ok((body_start, entries))
}

fn read_u16(buf: &[u8], pos: &mut usize) -> Result<u16, String> {
    let slice = read_slice(buf, pos, 2)?;
    Ok(u16::from_le_bytes(slice.try_into().unwrap()))
}

fn read_u64(buf: &[u8], pos: &mut usize) -> Result<u64, String> {
    let slice = read_slice(buf, pos, 8)?;
    Ok(u64::from_le_bytes(slice.try_into().unwrap()))
}

fn read_slice<'a>(buf: &'a [u8], pos: &mut usize, len: usize) -> Result<&'a [u8], String> {
    let end = pos
        .checked_add(len)
        .filter(|&e| e <= buf.len())
        .ok_or_else(|| "truncated installer entry".to_string())?;
    let out = &buf[*pos..end];
    *pos = end;
    Ok(out)
}

pub fn build_setup_executable(
    stub_path: &Path,
    lajufile_bin: &Path,
    extension_dir: &Path,
    license_path: &Path,
    readme_path: &Path,
    out_exe: &Path,
) -> Result<usize, String> {
    let stub = fs::read(stub_path).map_err(|e| format!("read stub {stub_path:?}: {e}"))?;
    let app_name = lajufile_bin
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| APP_BIN.to_string());
    let mut owned: Vec<(String, Vec<u8>)> = Vec::new();
    owned.push((
        app_name,
        fs::read(lajufile_bin).map_err(|e| format!("read {lajufile_bin:?}: {e}"))?,
    ));
    collect_dir(extension_dir, "extension", &mut owned)?;
    if license_path.is_file() {
        owned.push((
            "LICENSE".to_string(),
            fs::read(license_path).map_err(|e| format!("read LICENSE: {e}"))?,
        ));
    }
    if readme_path.is_file() {
        owned.push((
            "README.md".to_string(),
            fs::read(readme_path).map_err(|e| format!("read README.md: {e}"))?,
        ));
    }

    let refs: Vec<(&str, &[u8])> = owned
        .iter()
        .map(|(p, b)| (p.as_str(), b.as_slice()))
        .collect();
    let payload = pack_entries(&refs)?;

    if let Some(parent) = out_exe.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("create {parent:?}: {e}"))?;
    }
    let mut combined = Vec::with_capacity(stub.len() + payload.len());
    combined.extend_from_slice(&stub);
    combined.extend_from_slice(&payload);
    fs::write(out_exe, &combined).map_err(|e| format!("write {out_exe:?}: {e}"))?;
    make_executable(out_exe);
    Ok(owned.len())
}

fn collect_dir(dir: &Path, prefix: &str, out: &mut Vec<(String, Vec<u8>)>) -> Result<(), String> {
    let mut entries: Vec<_> = fs::read_dir(dir)
        .map_err(|e| format!("read {dir:?}: {e}"))?
        .flatten()
        .collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        let rel = format!("{prefix}/{name}");
        if path.is_dir() {
            collect_dir(&path, &rel, out)?;
        } else if path.is_file() {
            out.push((
                rel,
                fs::read(&path).map_err(|e| format!("read {path:?}: {e}"))?,
            ));
        }
    }
    Ok(())
}

/// Extracts payload files and writes clean uninstaller stub when provided.
pub fn write_install_files(
    install_dir: &Path,
    entries: &[PayloadEntry],
    uninstaller_stub: Option<&[u8]>,
) -> io::Result<()> {
    fs::create_dir_all(install_dir)?;
    for entry in entries {
        let rel = validate_rel_path(&entry.rel_path)
            .map_err(|msg| io::Error::new(io::ErrorKind::InvalidData, msg))?;
        let dest = install_dir.join(&rel);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&dest, &entry.bytes).or_else(|err| {
            // Brief retry after stopping the running binary so Windows releases the file lock.
            if err.kind() == io::ErrorKind::PermissionDenied {
                for _ in 0..10 {
                    std::thread::sleep(std::time::Duration::from_millis(150));
                    if fs::write(&dest, &entry.bytes).is_ok() {
                        return Ok(());
                    }
                }
            }
            Err(err)
        })?;
        if entry.rel_path == "lajufile" || entry.rel_path == "lajufile.exe" {
            make_executable(&dest);
        }
    }
    if let Some(stub) = uninstaller_stub {
        let uninst = install_dir.join(UNINSTALL_BIN);
        fs::write(&uninst, stub)?;
        make_executable(&uninst);
    }
    Ok(())
}

pub fn perform_install(options: &InstallOptions) -> Result<(), String> {
    let exe_path = std::env::current_exe().map_err(|e| format!("locate setup binary: {e}"))?;
    let exe_bytes = fs::read(&exe_path).map_err(|e| format!("read installer: {e}"))?;
    let (stub_end, entries) = parse_payload(&exe_bytes)?;
    stop_running_lajufile();

    let stub = &exe_bytes[..stub_end];
    if cfg!(target_os = "macos")
        && options
            .install_dir
            .extension()
            .and_then(|s| s.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("app"))
    {
        write_macos_bundle(&options.install_dir, &entries, stub).map_err(|e| e.to_string())?;
    } else {
        write_install_files(&options.install_dir, &entries, Some(stub))
            .map_err(|e| format!("install files: {e}"))?;
    }
    register_installation(options)?;
    Ok(())
}

fn write_macos_bundle(
    bundle_dir: &Path,
    entries: &[PayloadEntry],
    uninstaller_stub: &[u8],
) -> io::Result<()> {
    let contents = bundle_dir.join("Contents");
    let macos_dir = contents.join("MacOS");
    let resources_dir = contents.join("Resources");
    fs::create_dir_all(&macos_dir)?;
    fs::create_dir_all(&resources_dir)?;

    for entry in entries {
        let rel = validate_rel_path(&entry.rel_path)
            .map_err(|msg| io::Error::new(io::ErrorKind::InvalidData, msg))?;
        let is_main_bin = entry.rel_path == "lajufile" || entry.rel_path == "lajufile.exe";
        let dest = if is_main_bin {
            macos_dir.join(APP_BIN)
        } else {
            resources_dir.join(&rel)
        };
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&dest, &entry.bytes).or_else(|err| {
            // Brief retry after stopping the running binary so Windows releases the file lock.
            if err.kind() == io::ErrorKind::PermissionDenied {
                for _ in 0..10 {
                    std::thread::sleep(std::time::Duration::from_millis(150));
                    if fs::write(&dest, &entry.bytes).is_ok() {
                        return Ok(());
                    }
                }
            }
            Err(err)
        })?;
        if is_main_bin {
            make_executable(&dest);
        }
    }
    let uninst = macos_dir.join(UNINSTALL_BIN);
    fs::write(&uninst, uninstaller_stub)?;
    make_executable(&uninst);

    let plist = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <plist version=\"1.0\"><dict>\n\
         <key>CFBundleExecutable</key><string>{APP_BIN}</string>\n\
         <key>CFBundleIdentifier</key><string>io.lajufile.app</string>\n\
         <key>CFBundleName</key><string>LajuFile</string>\n\
         <key>CFBundlePackageType</key><string>APPL</string>\n\
         <key>CFBundleShortVersionString</key><string>{}</string>\n\
         <key>CFBundleVersion</key><string>{}</string>\n\
         <key>NSHighResolutionCapable</key><true/>\n\
         </dict></plist>\n",
        env!("CARGO_PKG_VERSION"),
        env!("CARGO_PKG_VERSION")
    );
    fs::write(contents.join("Info.plist"), plist)?;
    Ok(())
}

pub fn perform_uninstall(install_dir: &Path) -> Result<(), String> {
    stop_running_lajufile();
    unregister_installation();
    for rel in [
        "lajufile.exe",
        "lajufile",
        "lajufile.png",
        "LICENSE",
        "README.md",
        "extension",
        "Contents",
    ] {
        let target = install_dir.join(rel);
        if target.is_dir() {
            let _ = fs::remove_dir_all(&target);
        } else if target.is_file() {
            let _ = fs::remove_file(&target);
        }
    }
    schedule_self_cleanup(install_dir);
    Ok(())
}

#[cfg(unix)]
fn make_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    if let Ok(meta) = fs::metadata(path) {
        let mut perms = meta.permissions();
        perms.set_mode(0o755);
        let _ = fs::set_permissions(path, perms);
    }
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) {}

#[cfg(windows)]
fn stop_running_lajufile() {
    use std::os::windows::process::CommandExt;
    let _ = std::process::Command::new("taskkill")
        .args(["/IM", "lajufile.exe", "/F"])
        .creation_flags(0x0800_0000)
        .output();
}

#[cfg(not(windows))]
fn stop_running_lajufile() {
    let _ = std::process::Command::new("pkill")
        .args(["-x", "lajufile"])
        .output();
}

#[cfg(windows)]
fn register_installation(options: &InstallOptions) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    const UNINSTALL_KEY: &str =
        r"HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\LajuFile";
    let exe = options.install_dir.join(APP_BIN);
    let uninst = options.install_dir.join(UNINSTALL_BIN);
    let dir_str = options.install_dir.to_string_lossy();
    let exe_str = exe.to_string_lossy();
    let uninst_cmd = format!("\"{}\" --uninstall --dir \"{}\"", uninst.display(), dir_str);
    let quiet_cmd = format!(
        "\"{}\" --uninstall --silent --dir \"{}\"",
        uninst.display(),
        dir_str
    );

    let pairs: &[(&str, &str, &str)] = &[
        ("DisplayName", "REG_SZ", "LajuFile"),
        ("DisplayVersion", "REG_SZ", env!("CARGO_PKG_VERSION")),
        ("Publisher", "REG_SZ", "Nuel Wallep"),
        ("InstallLocation", "REG_SZ", dir_str.as_ref()),
        ("DisplayIcon", "REG_SZ", exe_str.as_ref()),
        ("UninstallString", "REG_SZ", &uninst_cmd),
        ("QuietUninstallString", "REG_SZ", &quiet_cmd),
        ("NoModify", "REG_DWORD", "1"),
        ("NoRepair", "REG_DWORD", "1"),
    ];
    for (name, ty, val) in pairs {
        let status = std::process::Command::new("reg.exe")
            .args(["add", UNINSTALL_KEY, "/v", name, "/t", ty, "/d", val, "/f"])
            .creation_flags(0x0800_0000)
            .status()
            .map_err(|e| format!("registry add {name}: {e}"))?;
        if !status.success() {
            return Err(format!("registry write failed for {name}"));
        }
    }

    if options.start_menu_shortcut
        && let Some(appdata) = std::env::var_os("APPDATA")
    {
        let start_dir = PathBuf::from(appdata)
            .join("Microsoft")
            .join("Windows")
            .join("Start Menu")
            .join("Programs");
        let _ = fs::create_dir_all(&start_dir);
        let _ =
            create_windows_shortcut(&start_dir.join("LajuFile.lnk"), &exe, &options.install_dir);
    }
    if options.desktop_shortcut
        && let Some(profile) = std::env::var_os("USERPROFILE")
    {
        let desktop = PathBuf::from(profile).join("Desktop");
        let _ = create_windows_shortcut(&desktop.join("LajuFile.lnk"), &exe, &options.install_dir);
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn register_installation(options: &InstallOptions) -> Result<(), String> {
    let _ = std::process::Command::new("codesign")
        .args(["--force", "--deep", "--sign", "-"])
        .arg(&options.install_dir)
        .output();

    if options.desktop_shortcut
        && let Some(home) = std::env::var_os("HOME")
    {
        let desktop_link = PathBuf::from(home).join("Desktop").join("LajuFile.app");
        let _ = fs::remove_file(&desktop_link);
        let _ = std::os::unix::fs::symlink(&options.install_dir, &desktop_link);
    }
    Ok(())
}

#[cfg(all(not(windows), not(target_os = "macos")))]
fn register_installation(options: &InstallOptions) -> Result<(), String> {
    const ICON_PNG: &[u8] = include_bytes!("../../../assets/icons/icon-256.png");
    let exe = options.install_dir.join(APP_BIN);
    let icon_path = options.install_dir.join("lajufile.png");
    let _ = fs::write(&icon_path, ICON_PNG);

    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        let local_bin = home.join(".local/bin");
        let _ = fs::create_dir_all(&local_bin);
        let bin_link = local_bin.join("lajufile");
        let _ = fs::remove_file(&bin_link);
        let _ = std::os::unix::fs::symlink(&exe, &bin_link);

        let desktop_entry = format!(
            "[Desktop Entry]\nType=Application\nName=LajuFile\nComment=Fast multi-connection download manager\nExec=\"{}\"\nIcon={}\nTerminal=false\nCategories=Network;FileTransfer;\n",
            exe.display(),
            icon_path.display()
        );
        if options.start_menu_shortcut {
            let apps_dir = home.join(".local/share/applications");
            let _ = fs::create_dir_all(&apps_dir);
            let _ = fs::write(apps_dir.join("lajufile.desktop"), &desktop_entry);
        }
        if options.desktop_shortcut {
            let desktop = home.join("Desktop");
            if desktop.is_dir() {
                let path = desktop.join("LajuFile.desktop");
                if fs::write(&path, &desktop_entry).is_ok() {
                    make_executable(&path);
                }
            }
        }
    }
    Ok(())
}

#[cfg(windows)]
fn create_windows_shortcut(lnk: &Path, target: &Path, work_dir: &Path) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    let script = concat!(
        "$ws = New-Object -ComObject WScript.Shell; ",
        "$s = $ws.CreateShortcut($env:LAJU_LNK); ",
        "$s.TargetPath = $env:LAJU_TARGET; ",
        "$s.WorkingDirectory = $env:LAJU_DIR; ",
        "$s.IconLocation = $env:LAJU_TARGET; ",
        "$s.Save()"
    );
    let status = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .env("LAJU_LNK", lnk)
        .env("LAJU_TARGET", target)
        .env("LAJU_DIR", work_dir)
        .creation_flags(0x0800_0000)
        .status()
        .map_err(|e| format!("create shortcut: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err("shortcut creation failed".to_string())
    }
}

#[cfg(windows)]
fn unregister_installation() {
    use std::os::windows::process::CommandExt;
    for (key, value) in [
        (
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\LajuFile",
            None,
        ),
        (
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
            Some("LajuFile"),
        ),
    ] {
        let mut cmd = std::process::Command::new("reg.exe");
        cmd.arg("delete").arg(key);
        if let Some(val) = value {
            cmd.args(["/v", val]);
        }
        cmd.arg("/f").creation_flags(0x0800_0000);
        let _ = cmd.output();
    }
    if let Some(appdata) = std::env::var_os("APPDATA") {
        let _ = fs::remove_file(
            PathBuf::from(appdata)
                .join("Microsoft")
                .join("Windows")
                .join("Start Menu")
                .join("Programs")
                .join("LajuFile.lnk"),
        );
    }
    if let Some(profile) = std::env::var_os("USERPROFILE") {
        let _ = fs::remove_file(PathBuf::from(profile).join("Desktop").join("LajuFile.lnk"));
    }
}

#[cfg(target_os = "macos")]
fn unregister_installation() {
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        let _ = fs::remove_file(home.join("Desktop").join("LajuFile.app"));
        let _ = fs::remove_file(home.join("Library/LaunchAgents/io.lajufile.app.plist"));
    }
}

#[cfg(all(not(windows), not(target_os = "macos")))]
fn unregister_installation() {
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        let _ = fs::remove_file(home.join(".local/bin/lajufile"));
        let _ = fs::remove_file(home.join(".local/share/applications/lajufile.desktop"));
        let _ = fs::remove_file(home.join("Desktop/LajuFile.desktop"));
        let _ = fs::remove_file(home.join(".config/autostart/lajufile.desktop"));
    }
}

#[cfg(windows)]
fn schedule_self_cleanup(install_dir: &Path) {
    use std::os::windows::process::CommandExt;
    let _ = std::process::Command::new("cmd.exe")
        .args([
            "/C",
            "ping",
            "127.0.0.1",
            "-n",
            "2",
            ">nul",
            "&",
            "rmdir",
            "/S",
            "/Q",
        ])
        .arg(install_dir)
        .creation_flags(0x0800_0000)
        .spawn();
}

#[cfg(not(windows))]
fn schedule_self_cleanup(install_dir: &Path) {
    let _ = fs::remove_dir_all(install_dir);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pack_and_parse_round_trips_and_verifies_integrity() {
        let stub = b"MZ_FAKE_STUB_HEADER";
        let payload = pack_entries(&[
            ("lajufile.exe", b"binary-data"),
            ("extension/manifest.json", br#"{"manifest_version":3}"#),
        ])
        .unwrap();

        let mut combined = stub.to_vec();
        combined.extend_from_slice(&payload);

        let (stub_end, entries) = parse_payload(&combined).unwrap();
        assert_eq!(stub_end, stub.len());
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].rel_path, "lajufile.exe");
        assert_eq!(entries[1].rel_path, "extension/manifest.json");

        // Corrupt a single payload byte and verify SHA-256 check rejects it.
        let mut corrupted = combined;
        corrupted[stub.len() + 10] ^= 0xff;
        assert!(parse_payload(&corrupted).is_err());
    }

    #[test]
    fn validate_rel_path_blocks_traversal_and_absolute_paths() {
        for bad in [
            "",
            "../evil.exe",
            "foo/../../evil.exe",
            "/etc/passwd",
            r"C:\Windows\win.ini",
        ] {
            assert!(validate_rel_path(bad).is_err(), "{bad} should fail");
        }
        assert!(validate_rel_path("extension/icons/icon-16.png").is_ok());
    }

    #[test]
    fn write_install_files_extracts_layout_and_uninstaller() {
        let dir = tempfile::tempdir().unwrap();
        let entries = vec![
            PayloadEntry {
                rel_path: APP_BIN.to_string(),
                bytes: b"app".to_vec(),
            },
            PayloadEntry {
                rel_path: "extension/manifest.json".to_string(),
                bytes: b"{}".to_vec(),
            },
        ];
        write_install_files(dir.path(), &entries, Some(b"stub")).unwrap();
        assert_eq!(fs::read(dir.path().join(APP_BIN)).unwrap(), b"app");
        assert_eq!(
            fs::read(dir.path().join("extension/manifest.json")).unwrap(),
            b"{}"
        );
        assert_eq!(fs::read(dir.path().join(UNINSTALL_BIN)).unwrap(), b"stub");
    }

    #[test]
    fn write_macos_bundle_creates_valid_app_structure() {
        let dir = tempfile::tempdir().unwrap();
        let bundle = dir.path().join("LajuFile.app");
        let entries = vec![
            PayloadEntry {
                rel_path: "lajufile".to_string(),
                bytes: b"bin".to_vec(),
            },
            PayloadEntry {
                rel_path: "extension/manifest.json".to_string(),
                bytes: b"{}".to_vec(),
            },
        ];
        write_macos_bundle(&bundle, &entries, b"unstub").unwrap();
        assert!(bundle.join("Contents/Info.plist").is_file());
        assert_eq!(
            fs::read(bundle.join(format!("Contents/MacOS/{APP_BIN}"))).unwrap(),
            b"bin"
        );
        assert_eq!(
            fs::read(bundle.join("Contents/Resources/extension/manifest.json")).unwrap(),
            b"{}"
        );
    }
}
