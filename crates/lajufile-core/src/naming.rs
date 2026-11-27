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

fn utf16_len(s: &str) -> usize {
    s.encode_utf16().count()
}

fn take_utf16(s: &str, max_units: usize) -> String {
    let mut out = String::new();
    let mut used = 0usize;
    for c in s.chars() {
        let len = c.len_utf16();
        if used + len > max_units {
            break;
        }
        out.push(c);
        used += len;
    }
    out
}

pub fn truncate_file_name(name: &str, max_chars: usize) -> String {
    if utf16_len(name) <= max_chars {
        return name.to_string();
    }
    if max_chars == 0 {
        return String::new();
    }

    let (mut stem, mut ext) = split_name(name);
    if utf16_len(ext) > MAX_EXTENSION_CHARS {
        stem = name;
        ext = "";
    }
    let ext_units = utf16_len(ext);
    if ext_units >= max_chars {
        return take_utf16(name, max_chars);
    }

    let stem_limit = max_chars - ext_units;
    let mut truncated = take_utf16(stem, stem_limit);
    while truncated.ends_with([' ', '.']) {
        truncated.pop();
    }
    if truncated.is_empty() {
        truncated = take_utf16("download", stem_limit);
    }
    truncated.push_str(ext);
    truncated
}

pub fn is_reserved_device_name(name: &str) -> bool {
    let raw_stem = name
        .split('.')
        .next()
        .unwrap_or(name)
        .trim_end_matches([' ', '.']);
    let upper = raw_stem.to_ascii_uppercase();

    if matches!(
        upper.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) {
        return true;
    }

    if let Some(rest) = upper
        .strip_prefix("COM")
        .or_else(|| upper.strip_prefix("LPT"))
    {
        let mut chars = rest.chars();
        return matches!(
            (chars.next(), chars.next()),
            (Some('1'..='9' | '\u{00b9}' | '\u{00b2}' | '\u{00b3}'), None)
        );
    }
    false
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
    let dir_units = utf16_len(&dir.as_os_str().to_string_lossy());
    let max_name = MAX_FILE_NAME_CHARS
        .min(250usize.saturating_sub(dir_units + 1))
        .max(1);
    let mut name = truncate_file_name(file_name, max_name);
    if is_reserved_device_name(&name) {
        name.insert(0, '_');
        name = truncate_file_name(&name, max_name);
    }

    let candidate = dir.join(&name);
    if !candidate.exists() && !temp_path_for(&candidate).exists() {
        return candidate;
    }

    let (stem, ext) = split_name(&name);
    for n in 1..=9999u32 {
        let suffix = format!(" ({n})");
        let candidate = dir.join(build_numbered_name(stem, ext, &suffix, max_name));
        if !candidate.exists() && !temp_path_for(&candidate).exists() {
            return candidate;
        }
    }

    // Exhausting 9999 suffixes in a real download folder is not realistic.
    let fallback = format!(" (9999{})", std::process::id());
    dir.join(build_numbered_name(stem, ext, &fallback, max_name))
}

fn build_numbered_name(stem: &str, ext: &str, suffix: &str, max_name: usize) -> String {
    let ext_units = utf16_len(ext);
    let suffix_units = utf16_len(suffix);
    let (base, kept_ext) = if !ext.is_empty()
        && ext_units <= MAX_EXTENSION_CHARS
        && ext_units + suffix_units <= max_name
    {
        (stem.to_string(), ext)
    } else {
        (format!("{stem}{ext}"), "")
    };
    let kept_ext_units = utf16_len(kept_ext);
    if suffix_units + kept_ext_units <= max_name {
        let base_budget = max_name - suffix_units - kept_ext_units;
        let truncated_base = take_utf16(&base, base_budget);
        return format!("{truncated_base}{suffix}{kept_ext}");
    }
    let digits: String = suffix.chars().filter(|c| c.is_ascii_digit()).collect();
    if !digits.is_empty() {
        return take_utf16(&digits, max_name);
    }
    take_utf16(suffix, max_name)
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
