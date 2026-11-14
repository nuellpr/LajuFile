use std::path::{Path, PathBuf};

use percent_encoding::percent_decode_str;
use url::Url;

pub const MAX_FILE_NAME_CHARS: usize = 180;

const INVALID_CHARS: &[char] = &['<', '>', ':', '"', '/', '\\', '|', '?', '*'];
const MAX_EXTENSION_CHARS: usize = 20;

pub fn sanitize_file_name(name: &str) -> String {
    let replaced: String = name
        .chars()
        .map(|c| {
            if INVALID_CHARS.contains(&c) || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .collect();

    let trimmed = replaced
        .trim_start_matches(' ')
        .trim_end_matches([' ', '.'])
        .to_string();
    let out = if trimmed.is_empty() {
        "download".to_string()
    } else {
        trimmed
    };

    let mut out = truncate_file_name(&out, MAX_FILE_NAME_CHARS);
    if is_reserved_device_name(&out) {
        out.insert(0, '_');
        out = truncate_file_name(&out, MAX_FILE_NAME_CHARS);
    }
    out
}

pub fn truncate_file_name(name: &str, max_chars: usize) -> String {
    if name.chars().count() <= max_chars {
        return name.to_string();
    }
    if max_chars == 0 {
        return String::new();
    }

    let (mut stem, mut ext) = split_name(name);
    if ext.chars().count() > MAX_EXTENSION_CHARS {
        stem = name;
        ext = "";
    }
    let ext_chars = ext.chars().count();
    if ext_chars >= max_chars {
        return name.chars().take(max_chars).collect();
    }

    let stem_limit = max_chars - ext_chars;
    let mut truncated: String = stem.chars().take(stem_limit).collect();
    while truncated.ends_with([' ', '.']) {
        truncated.pop();
    }
    if truncated.is_empty() {
        truncated.push_str("download");
        truncated = truncated.chars().take(stem_limit).collect();
    }
    truncated.push_str(ext);
    truncated
}

fn is_reserved_device_name(name: &str) -> bool {
    let stem = name
        .split('.')
        .next()
        .unwrap_or(name)
        .trim_end_matches([' ', '.'])
        .to_ascii_uppercase();

    matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && matches!(stem.as_bytes()[3], b'1'..=b'9'))
}

pub fn resolve_file_name(
    explicit: Option<&str>,
    content_disposition: Option<&str>,
    url: &Url,
) -> String {
    let from_explicit = explicit
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_owned);

    let from_header = content_disposition
        .and_then(|value| content_disposition::parse_content_disposition(value).filename_full());

    let from_url = url
        .path_segments()
        .and_then(|mut segments| segments.next_back())
        .filter(|segment| !segment.is_empty())
        .map(|segment| percent_decode_str(segment).decode_utf8_lossy().into_owned());

    let raw = from_explicit
        .or(from_header)
        .or(from_url)
        .unwrap_or_else(|| "download".to_string());
    sanitize_file_name(&raw)
}

pub fn dedupe_path(dir: &Path, file_name: &str) -> PathBuf {
    let dir_chars = dir.as_os_str().to_string_lossy().chars().count();
    let max_name = MAX_FILE_NAME_CHARS
        .min(250usize.saturating_sub(dir_chars + 1))
        .max(1);
    let name = truncate_file_name(file_name, max_name);

    let candidate = dir.join(&name);
    if !candidate.exists() && !temp_path_for(&candidate).exists() {
        return candidate;
    }

    let (stem, ext) = split_name(&name);
    for n in 1..=9999u32 {
        let numbered = format!("{stem} ({n}){ext}");
        let candidate = dir.join(truncate_file_name(&numbered, max_name));
        if !candidate.exists() && !temp_path_for(&candidate).exists() {
            return candidate;
        }
    }

    // Exhausting 9999 suffixes in a real download folder is not realistic.
    dir.join(truncate_file_name(
        &format!("{stem} (9999{}){ext}", std::process::id()),
        max_name,
    ))
}

pub fn temp_path_for(final_path: &Path) -> PathBuf {
    let mut temp = final_path.as_os_str().to_os_string();
    temp.push(".lajufile");
    PathBuf::from(temp)
}

fn split_name(name: &str) -> (&str, &str) {
    match name.rfind('.') {
        Some(index) if index > 0 => (&name[..index], &name[index..]),
        _ => (name, ""),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extension_longer_than_limit_is_not_treated_as_extension() {
        let name = format!("file.{}", "x".repeat(30));
        let out = truncate_file_name(&name, 30);
        assert_eq!(out.chars().count(), 30);
    }

    #[test]
    fn truncation_does_not_leave_trailing_dot_or_space() {
        let name = format!("{}.pdf", "a ".repeat(100));
        let out = truncate_file_name(&name, 20);
        assert!(out.ends_with(".pdf"));
        assert!(!out.trim_end_matches(".pdf").ends_with(' '));
    }
}
