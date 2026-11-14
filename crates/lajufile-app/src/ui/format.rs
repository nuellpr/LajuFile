//! Formatting helpers for the download table.

/// Formats a byte count using binary units.
pub fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let units = ["KB", "MB", "GB", "TB"];
    let mut value = bytes as f64 / 1024.0;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < units.len() {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.1} {}", units[unit])
}

/// Formats an ETA in seconds, or an em dash when unknown.
pub fn format_eta(seconds: Option<u64>) -> String {
    let Some(seconds) = seconds.filter(|seconds| *seconds > 0) else {
        return "—".to_string();
    };
    match seconds {
        seconds if seconds < 60 => format!("{seconds}s"),
        seconds if seconds < 3_600 => format!("{}m {}s", seconds / 60, seconds % 60),
        seconds if seconds < 86_400 => {
            format!("{}h {}m", seconds / 3_600, (seconds % 3_600) / 60)
        }
        seconds => format!("{}d {}h", seconds / 86_400, (seconds % 86_400) / 3_600),
    }
}

/// Formats a bytes-per-second speed.
pub fn format_speed(bytes_per_second: Option<u64>) -> String {
    match bytes_per_second {
        Some(0) | None => "—".to_string(),
        Some(speed) => format!("{}/s", format_size(speed)),
    }
}

/// Estimates remaining seconds; `None` when size or speed is unknown/zero.
pub fn eta_seconds(size: Option<u64>, downloaded: u64, speed: u64) -> Option<u64> {
    if speed == 0 {
        return None;
    }
    size.map(|size| size.saturating_sub(downloaded) / speed)
}

/// Formats a creation timestamp relative to `now_ms`.
pub fn format_relative_time(now_ms: i64, then_ms: i64) -> String {
    let seconds = (now_ms - then_ms).max(0) / 1_000;
    match seconds {
        0..=59 => "just now".to_string(),
        60..=3_599 => format!("{}m ago", seconds / 60),
        3_600..=86_399 => format!("{}h ago", seconds / 3_600),
        _ => format!("{}d ago", seconds / 86_400),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_sizes() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(1536), "1.5 KB");
        assert_eq!(format_size(4_700_000_000), "4.4 GB");
    }

    #[test]
    fn formats_eta() {
        assert_eq!(format_eta(None), "—");
        assert_eq!(format_eta(Some(0)), "—");
        assert_eq!(format_eta(Some(125)), "2m 5s");
    }

    #[test]
    fn eta_is_none_without_size_or_speed() {
        assert_eq!(eta_seconds(None, 0, 100), None);
        assert_eq!(eta_seconds(Some(1_000), 0, 0), None);
        assert_eq!(eta_seconds(Some(1_000), 400, 100), Some(6));
    }

    #[test]
    fn formats_relative_time() {
        assert_eq!(format_relative_time(1_000_000, 999_000), "just now");
        assert_eq!(format_relative_time(1_000_000, 700_000), "5m ago");
        assert_eq!(format_relative_time(1_000_000, -6_200_000), "2h ago");
    }
}
