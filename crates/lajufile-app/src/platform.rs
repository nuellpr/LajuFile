//! OS-native helpers for single-instance activation, window focus, and anti-sleep.

use std::io::Write;
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
#[cfg(any(windows, unix, test))]
use std::path::Path;
#[cfg(any(windows, test))]
use std::path::PathBuf;
use std::time::Duration;

use lajufile_core::paths;
use lajufile_core::settings::{QueueCompleteAction, Settings};

/// Holds the OS single-instance lock for the lifetime of the desktop process.
pub struct SingleInstanceGuard {
    #[cfg(windows)]
    handle: windows_sys::Win32::Foundation::HANDLE,
    #[cfg(not(windows))]
    _lock_file: Option<std::fs::File>,
}

impl SingleInstanceGuard {
    pub fn acquire() -> Option<Self> {
        #[cfg(windows)]
        {
            use windows_sys::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError};
            use windows_sys::Win32::System::Threading::CreateMutexW;

            let name: Vec<u16> = "Local\\LajuFileSingleInstance\0".encode_utf16().collect();
            let handle = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
            if handle.is_null() {
                return Some(Self { handle });
            }
            if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
                unsafe { CloseHandle(handle) };
                if let Some(settings) = load_existing_settings() {
                    let _ = signal_existing_instance(&settings);
                }
                return None;
            }
            Some(Self { handle })
        }
        #[cfg(not(windows))]
        {
            #[cfg(unix)]
            {
                let dir = paths::data_dir();
                if paths::ensure_private_dir(&dir).is_ok() {
                    match acquire_unix_lock(&dir.join(".lajufile.lock")) {
                        Ok(file) => {
                            return Some(Self {
                                _lock_file: Some(file),
                            });
                        }
                        Err(true) => {
                            if let Some(settings) = load_existing_settings() {
                                let _ = probe_wake(&settings);
                            }
                            return None;
                        }
                        Err(false) => {}
                    }
                }
            }
            if let Some(settings) = load_existing_settings()
                && probe_wake(&settings)
            {
                return None;
            }
            Some(Self { _lock_file: None })
        }
    }
}

#[cfg(unix)]
fn acquire_unix_lock(path: &Path) -> Result<std::fs::File, bool> {
    use std::os::unix::fs::OpenOptionsExt;
    use std::os::unix::io::AsRawFd;

    unsafe extern "C" {
        fn flock(fd: std::os::raw::c_int, operation: std::os::raw::c_int) -> std::os::raw::c_int;
    }
    const LOCK_EX_NB: std::os::raw::c_int = 2 | 4;

    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(path)
        .map_err(|_| false)?;
    if unsafe { flock(file.as_raw_fd(), LOCK_EX_NB) } == 0 {
        return Ok(file);
    }
    let err = std::io::Error::last_os_error();
    let contended =
        err.kind() == std::io::ErrorKind::WouldBlock || matches!(err.raw_os_error(), Some(11 | 35));
    Err(contended)
}

#[cfg(windows)]
impl Drop for SingleInstanceGuard {
    fn drop(&mut self) {
        if !self.handle.is_null() {
            unsafe {
                windows_sys::Win32::Foundation::CloseHandle(self.handle);
            }
            self.handle = std::ptr::null_mut();
        }
    }
}

fn load_existing_settings() -> Option<Settings> {
    let dir = paths::data_dir();
    let path = paths::settings_path(&dir);
    let raw = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

/// Grants foreground activation (on Windows) and posts an authenticated `/wake`
/// request to an already-running local bridge instance.
pub fn signal_existing_instance(settings: &Settings) -> bool {
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::UI::WindowsAndMessaging::{ASFW_ANY, AllowSetForegroundWindow};
        AllowSetForegroundWindow(ASFW_ANY);
    }
    probe_wake(settings)
}

pub fn request_graceful_shutdown() -> bool {
    load_existing_settings().is_some_and(|settings| post_bridge_command(&settings, "/shutdown"))
}

fn probe_wake(settings: &Settings) -> bool {
    post_bridge_command(settings, "/wake")
}

fn post_bridge_command(settings: &Settings, path: &str) -> bool {
    if settings.bridge_port == 0 || settings.bridge_token.len() != 64 {
        return false;
    }
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, settings.bridge_port));
    let timeout = Duration::from_millis(800);
    let Ok(mut stream) = TcpStream::connect_timeout(&addr, timeout) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(timeout));
    let _ = stream.set_write_timeout(Some(timeout));
    let req = format!(
        "POST {path} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Length: 0\r\nx-lajufile-token: {}\r\nConnection: close\r\n\r\n",
        settings.bridge_port, settings.bridge_token
    );
    if stream.write_all(req.as_bytes()).is_err() {
        return false;
    }
    let mut buf = [0u8; 64];
    let Ok(read) = std::io::Read::read(&mut stream, &mut buf) else {
        return false;
    };
    let status = String::from_utf8_lossy(&buf[..read]);
    status
        .split_whitespace()
        .nth(1)
        .is_some_and(|code| code == "200")
}

#[cfg(windows)]
struct FindOwnWindowCtx<'a> {
    pid: u32,
    title: &'a [u16],
    found: windows_sys::Win32::Foundation::HWND,
}

#[cfg(windows)]
unsafe extern "system" fn enum_own_window(
    hwnd: windows_sys::Win32::Foundation::HWND,
    lparam: windows_sys::Win32::Foundation::LPARAM,
) -> windows_sys::core::BOOL {
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetWindowTextW, GetWindowThreadProcessId};
    let ctx = unsafe { &mut *(lparam as *mut FindOwnWindowCtx<'_>) };
    let mut win_pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, &mut win_pid) };
    if win_pid != ctx.pid {
        return 1;
    }
    let mut buf = [0u16; 256];
    let len = unsafe { GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32) };
    if len > 0 && &buf[..len as usize] == ctx.title {
        ctx.found = hwnd;
        return 0;
    }
    1
}

/// Restores and activates this process's window matching `title` on Windows.
pub fn focus_window_by_title(title: &str) {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            EnumWindows, IsIconic, IsWindowVisible, SW_RESTORE, SW_SHOW, SetForegroundWindow,
            ShowWindow,
        };
        let title_utf16: Vec<u16> = title.encode_utf16().collect();
        let mut ctx = FindOwnWindowCtx {
            pid: std::process::id(),
            title: &title_utf16,
            found: std::ptr::null_mut(),
        };
        unsafe {
            EnumWindows(
                Some(enum_own_window),
                (&mut ctx as *mut FindOwnWindowCtx<'_>) as isize,
            );
        }
        if !ctx.found.is_null() {
            unsafe {
                if IsIconic(ctx.found) != 0 {
                    ShowWindow(ctx.found, SW_RESTORE);
                } else if IsWindowVisible(ctx.found) == 0 {
                    ShowWindow(ctx.found, SW_SHOW);
                }
                SetForegroundWindow(ctx.found);
            }
        }
    }
    #[cfg(not(windows))]
    let _ = title;
}

fn set_prevent_sleep_os(prevent: bool) {
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::System::Power::{
            ES_CONTINUOUS, ES_SYSTEM_REQUIRED, SetThreadExecutionState,
        };
        let flags = if prevent {
            ES_CONTINUOUS | ES_SYSTEM_REQUIRED
        } else {
            ES_CONTINUOUS
        };
        SetThreadExecutionState(flags);
    }
    #[cfg(not(windows))]
    {
        use std::process::{Child, Command, Stdio};
        use std::sync::Mutex;

        static SLEEP_INHIBITOR: Mutex<Option<Child>> = Mutex::new(None);
        let Ok(mut guard) = SLEEP_INHIBITOR.lock() else {
            return;
        };
        if prevent {
            if let Some(child) = guard.as_mut()
                && child.try_wait().ok().flatten().is_none()
            {
                return;
            }
            #[cfg(target_os = "macos")]
            let spawned = Command::new("caffeinate")
                .args(["-i", "-s", "-w", &std::process::id().to_string()])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .ok();
            #[cfg(not(target_os = "macos"))]
            let spawned = Command::new("systemd-inhibit")
                .args([
                    "--what=idle:sleep",
                    "--who=LajuFile",
                    "--why=Downloading files",
                    "--mode=block",
                    "sh",
                    "-c",
                    "read _",
                ])
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .ok();
            *guard = spawned;
        } else if let Some(mut child) = guard.take() {
            drop(child.stdin.take());
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// Tracks system sleep inhibition state cleanly and releases it when idle or dropped.
pub struct SleepGuard {
    active: bool,
    apply: Box<dyn FnMut(bool) + Send>,
}

impl Default for SleepGuard {
    fn default() -> Self {
        Self::new()
    }
}

impl SleepGuard {
    pub fn new() -> Self {
        Self::with_handler(set_prevent_sleep_os)
    }

    pub fn with_handler(handler: impl FnMut(bool) + Send + 'static) -> Self {
        Self {
            active: false,
            apply: Box::new(handler),
        }
    }

    pub fn sync(&mut self, should_prevent: bool) {
        if self.active != should_prevent {
            (self.apply)(should_prevent);
            self.active = should_prevent;
        }
    }
}

impl Drop for SleepGuard {
    fn drop(&mut self) {
        self.sync(false);
    }
}

/// Ensures the desktop process does not keep a protected Windows directory (such as
/// `C:\Windows\System32` inherited from Startup `Run` keys) as its working directory.
pub fn ensure_writable_cwd() {
    #[cfg(windows)]
    {
        let switch = match std::env::current_dir() {
            Ok(cwd) => is_protected_windows_cwd(&cwd, &protected_windows_roots()),
            Err(_) => true,
        };
        if switch {
            let data = paths::data_dir();
            if paths::ensure_dir(&data).is_ok() && std::env::set_current_dir(&data).is_ok() {
                return;
            }
            let tmp = std::env::temp_dir();
            let _ = std::fs::create_dir_all(&tmp);
            let _ = std::env::set_current_dir(tmp);
        }
    }
}

#[cfg(windows)]
fn protected_windows_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    for key in ["SystemRoot", "WINDIR", "ProgramFiles", "ProgramFiles(x86)"] {
        if let Some(val) = std::env::var_os(key)
            && !val.is_empty()
        {
            roots.push(PathBuf::from(val));
        }
    }
    if roots.is_empty() {
        roots.push(PathBuf::from(r"C:\Windows"));
    }
    roots
}

#[cfg(any(windows, test))]
fn is_protected_windows_cwd(cwd: &Path, roots: &[PathBuf]) -> bool {
    let norm = |p: &Path| {
        p.to_string_lossy()
            .trim_end_matches(['\\', '/'])
            .to_ascii_lowercase()
    };
    let cwd_norm = norm(cwd);
    roots.iter().any(|root| {
        let r = norm(root);
        !r.is_empty()
            && (cwd_norm == r
                || cwd_norm
                    .strip_prefix(&r)
                    .is_some_and(|rest| rest.starts_with('\\') || rest.starts_with('/')))
    })
}

/// Hints the OS to reclaim unused working-set pages after startup, tray hide,
/// or unloading heavy fallback fonts.
pub fn trim_process_memory() {
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::System::Threading::{GetCurrentProcess, SetProcessWorkingSetSize};
        let _ = SetProcessWorkingSetSize(GetCurrentProcess(), usize::MAX, usize::MAX);
    }
}

/// Returns local wall-clock minutes since midnight (`0..1440`) for the download scheduler.
pub fn local_minutes_since_midnight() -> Option<u16> {
    #[cfg(windows)]
    {
        #[repr(C)]
        struct SystemTime {
            year: u16,
            month: u16,
            day_of_week: u16,
            day: u16,
            hour: u16,
            minute: u16,
            second: u16,
            millis: u16,
        }
        unsafe extern "system" {
            fn GetLocalTime(lp_system_time: *mut SystemTime);
        }
        let mut st = SystemTime {
            year: 0,
            month: 0,
            day_of_week: 0,
            day: 0,
            hour: 0,
            minute: 0,
            second: 0,
            millis: 0,
        };
        unsafe { GetLocalTime(&mut st) };
        Some(st.hour * 60 + st.minute)
    }
    #[cfg(unix)]
    {
        #[allow(dead_code)]
        #[repr(C)]
        struct Tm {
            tm_sec: std::ffi::c_int,
            tm_min: std::ffi::c_int,
            tm_hour: std::ffi::c_int,
            tm_mday: std::ffi::c_int,
            tm_mon: std::ffi::c_int,
            tm_year: std::ffi::c_int,
            tm_wday: std::ffi::c_int,
            tm_yday: std::ffi::c_int,
            tm_isdst: std::ffi::c_int,
            tm_gmtoff: std::ffi::c_long,
            tm_zone: *const std::ffi::c_char,
            _pad: [u64; 4],
        }
        unsafe extern "C" {
            fn time(tloc: *mut std::ffi::c_long) -> std::ffi::c_long;
            fn localtime_r(timep: *const std::ffi::c_long, result: *mut Tm) -> *mut Tm;
        }
        let now = unsafe { time(std::ptr::null_mut()) };
        if now < 0 {
            return None;
        }
        let mut tm = std::mem::MaybeUninit::<Tm>::zeroed();
        let ptr = unsafe { localtime_r(&now, tm.as_mut_ptr()) };
        if ptr.is_null() {
            return None;
        }
        let tm = unsafe { tm.assume_init() };
        if (0..24).contains(&tm.tm_hour) && (0..60).contains(&tm.tm_min) {
            Some((tm.tm_hour as u16) * 60 + (tm.tm_min as u16))
        } else {
            None
        }
    }
    #[cfg(all(not(windows), not(unix)))]
    {
        None
    }
}

/// Triggers an OS power action after all queued downloads finish.
pub fn trigger_power_action(action: QueueCompleteAction) {
    use std::process::Command;
    match action {
        QueueCompleteAction::None | QueueCompleteAction::ExitApp => {}
        QueueCompleteAction::Sleep => {
            #[cfg(windows)]
            unsafe {
                // Direct Win32 call avoids rundll32 passing HWND as bHibernate (which hibernates).
                let _ = windows_sys::Win32::System::Power::SetSuspendState(false, false, false);
            }
            #[cfg(target_os = "macos")]
            {
                let _ = Command::new("pmset").arg("sleepnow").spawn();
            }
            #[cfg(all(unix, not(target_os = "macos")))]
            {
                let _ = Command::new("systemctl").arg("suspend").spawn();
            }
        }
        QueueCompleteAction::Shutdown => {
            #[cfg(windows)]
            {
                let mut cmd = Command::new("shutdown");
                cmd.args(["/s", "/t", "5", "/c", "LajuFile downloads completed"]);
                crate::tools::no_window(&mut cmd);
                let _ = cmd.spawn();
            }
            #[cfg(target_os = "macos")]
            {
                let _ = Command::new("osascript")
                    .args(["-e", "tell application \"System Events\" to shut down"])
                    .spawn();
            }
            #[cfg(all(unix, not(target_os = "macos")))]
            {
                let _ = Command::new("systemctl").arg("poweroff").spawn();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;

    #[test]
    fn sleep_guard_deduplicates_transitions_and_resets_on_drop() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let log_inner = Arc::clone(&log);
        {
            let mut guard = SleepGuard::with_handler(move |state| {
                log_inner.lock().unwrap().push(state);
            });
            guard.sync(false);
            guard.sync(true);
            guard.sync(true);
            guard.sync(false);
            guard.sync(true);
        }
        assert_eq!(*log.lock().unwrap(), vec![true, false, true, false]);
    }

    #[test]
    fn protected_windows_cwd_detects_system_directories_only() {
        let roots = vec![
            PathBuf::from(r"C:\WINDOWS"),
            PathBuf::from(r"C:\Program Files"),
        ];
        assert!(is_protected_windows_cwd(
            Path::new(r"C:\Windows\system32"),
            &roots
        ));
        assert!(is_protected_windows_cwd(Path::new(r"C:\windows"), &roots));
        assert!(is_protected_windows_cwd(
            Path::new(r"C:\Program Files\LajuFile"),
            &roots
        ));
        assert!(!is_protected_windows_cwd(
            Path::new(r"C:\WindowsOld\work"),
            &roots
        ));
        assert!(!is_protected_windows_cwd(Path::new(r"D:\LajuFile"), &roots));
    }

    #[cfg(unix)]
    #[test]
    fn unix_flock_blocks_second_holder_until_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let lock_path = dir.path().join(".lajufile.lock");
        let first = acquire_unix_lock(&lock_path).expect("first lock should succeed");
        assert!(acquire_unix_lock(&lock_path).unwrap_err());
        drop(first);
        assert!(acquire_unix_lock(&lock_path).is_ok());
    }

    #[test]
    fn local_minutes_since_midnight_is_within_day_bounds() {
        let mins = local_minutes_since_midnight().expect("local clock available");
        assert!(mins < 24 * 60);
    }
}
