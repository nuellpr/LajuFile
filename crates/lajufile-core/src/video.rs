//! Video format listing for yt-dlp sources.

use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoFormat {
    pub id: String,
    pub ext: String,
    pub vcodec: String,
    pub acodec: String,
    pub height: Option<u32>,
    pub fps: Option<u32>,
    pub has_video: bool,
    pub has_audio: bool,
    pub filesize: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QualityChoice {
    pub label: String,
    pub selector: String,
    pub fps: Option<u32>,
    pub size: Option<u64>,
}

/// Selector used when no quality is picked: best video plus best audio.
pub const DEFAULT_SELECTOR: &str = "bv*+ba/b";

/// What `yt-dlp -J` reports before a download: the title and the quality list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoListing {
    pub title: String,
    pub choices: Vec<QualityChoice>,
}

pub fn listing(json: &Value) -> VideoListing {
    VideoListing {
        title: json
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or("video")
            .to_string(),
        choices: quality_choices(&parse_formats(json)),
    }
}

/// Formats from a `yt-dlp -J` document, keeping only entries with a format id.
pub fn parse_formats(json: &Value) -> Vec<VideoFormat> {
    let Some(formats) = json.get("formats").and_then(Value::as_array) else {
        return Vec::new();
    };
    let duration = json
        .get("duration")
        .and_then(Value::as_f64)
        .filter(|secs| secs.is_finite() && *secs > 0.0);
    formats
        .iter()
        .filter_map(|format| {
            let id = format.get("format_id")?.as_str()?.to_string();
            let vcodec = format
                .get("vcodec")
                .and_then(Value::as_str)
                .unwrap_or("none")
                .to_string();
            let acodec = format
                .get("acodec")
                .and_then(Value::as_str)
                .unwrap_or("none")
                .to_string();
            let filesize = format
                .get("filesize")
                .and_then(Value::as_u64)
                .or_else(|| format.get("filesize_approx").and_then(Value::as_u64))
                .or_else(|| {
                    let tbr_kbps = format.get("tbr").and_then(Value::as_f64)?;
                    let secs = duration?;
                    if tbr_kbps.is_finite() && tbr_kbps > 0.0 {
                        Some(((tbr_kbps * 1000.0 / 8.0) * secs).round() as u64)
                    } else {
                        None
                    }
                });
            Some(VideoFormat {
                id,
                ext: format
                    .get("ext")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                has_video: vcodec != "none",
                has_audio: acodec != "none",
                vcodec,
                acodec,
                height: format
                    .get("height")
                    .and_then(Value::as_u64)
                    .and_then(|height| u32::try_from(height).ok()),
                fps: format.get("fps").and_then(|value| {
                    value
                        .as_u64()
                        .and_then(|fps| u32::try_from(fps).ok())
                        .or_else(|| value.as_f64().map(|fps| fps.round() as u32))
                }),
                filesize,
            })
        })
        .collect()
}

fn preferred_audio_size(formats: &[VideoFormat]) -> Option<u64> {
    formats
        .iter()
        .filter(|f| !f.has_video && f.has_audio)
        .filter_map(|f| {
            let size = f.filesize?;
            let pref = u8::from(f.ext.eq_ignore_ascii_case("m4a") || f.acodec.starts_with("mp4a"));
            Some((pref, size))
        })
        .max()
        .map(|(_, size)| size)
}

fn estimated_height_size(
    formats: &[VideoFormat],
    height: u32,
    audio_bytes: Option<u64>,
) -> Option<u64> {
    let best = formats
        .iter()
        .filter(|f| f.has_video && f.height == Some(height))
        .filter_map(|f| {
            let size = f.filesize?;
            let h264 = f.vcodec.starts_with("avc1") || f.vcodec.starts_with("h264");
            let mp4 = f.ext.eq_ignore_ascii_case("mp4");
            let pref = (u8::from(h264) * 2) + u8::from(mp4);
            Some((pref, size, f.has_audio))
        })
        .max_by_key(|(pref, size, _)| (*pref, *size))?;
    if best.2 {
        Some(best.1)
    } else {
        Some(best.1.saturating_add(audio_bytes.unwrap_or(0)))
    }
}

/// Resolution choices, highest first, then audio only.
pub fn quality_choices(formats: &[VideoFormat]) -> Vec<QualityChoice> {
    let mut heights: Vec<u32> = formats
        .iter()
        .filter(|format| format.has_video)
        .filter_map(|format| format.height)
        .collect();
    heights.sort_unstable();
    heights.dedup();
    heights.reverse();

    let audio_bytes = preferred_audio_size(formats);
    let mut choices = Vec::new();
    for height in heights {
        let fps = formats
            .iter()
            .filter(|format| format.has_video && format.height == Some(height))
            .filter_map(|format| format.fps)
            .max();
        choices.push(QualityChoice {
            label: format!("{height}p"),
            selector: selector_for_height(height),
            fps,
            size: estimated_height_size(formats, height, audio_bytes),
        });
    }
    if formats.iter().any(|format| format.has_audio) {
        choices.push(QualityChoice {
            label: "Audio only (m4a)".to_string(),
            selector: "ba[ext=m4a]/ba".to_string(),
            fps: None,
            size: audio_bytes,
        });
    }
    choices
}

/// Selector that merges the best audio with video at or below `height`.
pub fn selector_for_height(height: u32) -> String {
    format!("bv*[height<={height}]+ba/b[height<={height}]/bv*+ba")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample() -> Value {
        json!({
            "duration": 100.0,
            "formats": [
                {"format_id": "140", "ext": "m4a", "vcodec": "none", "acodec": "mp4a", "filesize": 1000},
                {"format_id": "137", "ext": "mp4", "height": 1080, "fps": 60, "vcodec": "avc1", "acodec": "none", "filesize": 50000},
                {"format_id": "248", "ext": "webm", "height": 1080, "vcodec": "vp9", "acodec": "none", "filesize": 40000},
                {"format_id": "136", "ext": "mp4", "height": 720, "vcodec": "avc1", "acodec": "none", "tbr": 800.0},
                {"format_id": "18", "ext": "mp4", "height": 360, "vcodec": "avc1", "acodec": "mp4a", "filesize": 8000}
            ]
        })
    }

    #[test]
    fn parses_formats_and_flags() {
        let formats = parse_formats(&sample());
        assert_eq!(formats.len(), 5);
        assert!(formats[0].has_audio && !formats[0].has_video);
        assert_eq!(formats[1].height, Some(1080));
        assert_eq!(formats[0].filesize, Some(1000));
        assert_eq!(formats[3].filesize, Some(10_000_000));
    }

    #[test]
    fn choices_are_descending_then_audio() {
        let choices = quality_choices(&parse_formats(&sample()));
        let labels: Vec<&str> = choices.iter().map(|choice| choice.label.as_str()).collect();
        assert_eq!(labels, vec!["1080p", "720p", "360p", "Audio only (m4a)"]);
        assert_eq!(
            choices[0].selector,
            "bv*[height<=1080]+ba/b[height<=1080]/bv*+ba"
        );
        assert_eq!(choices[0].fps, Some(60));
        assert_eq!(choices[0].size, Some(51_000));
        assert_eq!(choices[1].size, Some(10_001_000));
        assert_eq!(choices[2].size, Some(8_000));
        assert_eq!(choices[3].size, Some(1_000));
        assert_eq!(choices[3].selector, "ba[ext=m4a]/ba");
    }

    #[test]
    fn missing_formats_yield_no_choices() {
        assert!(parse_formats(&json!({})).is_empty());
        assert!(quality_choices(&[]).is_empty());
    }

    #[test]
    fn listing_keeps_the_title_and_choices() {
        let mut sample = sample();
        sample["title"] = json!("A clip");
        let parsed = listing(&sample);
        assert_eq!(parsed.title, "A clip");
        assert_eq!(parsed.choices.len(), 4);
        assert_eq!(listing(&json!({})).title, "video");
    }
}
