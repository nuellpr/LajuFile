//! Video format listing for yt-dlp sources.

use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoFormat {
    pub id: String,
    pub ext: String,
    pub height: Option<u32>,
    pub has_video: bool,
    pub has_audio: bool,
    pub filesize: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QualityChoice {
    pub label: String,
    pub selector: String,
}

/// Formats from a `yt-dlp -J` document, keeping only entries with a format id.
pub fn parse_formats(json: &Value) -> Vec<VideoFormat> {
    let Some(formats) = json.get("formats").and_then(Value::as_array) else {
        return Vec::new();
    };
    formats
        .iter()
        .filter_map(|format| {
            let id = format.get("format_id")?.as_str()?.to_string();
            Some(VideoFormat {
                id,
                ext: format
                    .get("ext")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                height: format
                    .get("height")
                    .and_then(Value::as_u64)
                    .and_then(|height| u32::try_from(height).ok()),
                has_video: format
                    .get("vcodec")
                    .and_then(Value::as_str)
                    .is_some_and(|codec| codec != "none"),
                has_audio: format
                    .get("acodec")
                    .and_then(Value::as_str)
                    .is_some_and(|codec| codec != "none"),
                filesize: format
                    .get("filesize")
                    .and_then(Value::as_u64)
                    .or_else(|| format.get("filesize_approx").and_then(Value::as_u64)),
            })
        })
        .collect()
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

    let mut choices = Vec::new();
    for height in heights {
        choices.push(QualityChoice {
            label: format!("{height}p"),
            selector: selector_for_height(height),
        });
    }
    if formats.iter().any(|format| format.has_audio) {
        choices.push(QualityChoice {
            label: "Audio only (m4a)".to_string(),
            selector: "ba[ext=m4a]/ba".to_string(),
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
            "formats": [
                {"format_id": "140", "ext": "m4a", "vcodec": "none", "acodec": "mp4a", "filesize": 100},
                {"format_id": "137", "ext": "mp4", "height": 1080, "vcodec": "avc1", "acodec": "none"},
                {"format_id": "248", "ext": "webm", "height": 1080, "vcodec": "vp9", "acodec": "none"},
                {"format_id": "136", "ext": "mp4", "height": 720, "vcodec": "avc1", "acodec": "none"},
                {"format_id": "18", "ext": "mp4", "height": 360, "vcodec": "avc1", "acodec": "mp4a"}
            ]
        })
    }

    #[test]
    fn parses_formats_and_flags() {
        let formats = parse_formats(&sample());
        assert_eq!(formats.len(), 5);
        assert!(formats[0].has_audio && !formats[0].has_video);
        assert_eq!(formats[1].height, Some(1080));
        assert_eq!(formats[0].filesize, Some(100));
    }

    #[test]
    fn choices_are_descending_then_audio() {
        let choices = quality_choices(&parse_formats(&sample()));
        let labels: Vec<&str> = choices.iter().map(|choice| choice.label.as_str()).collect();
        assert_eq!(labels, vec!["1080p", "720p", "360p", "Audio only (m4a)"]);
        assert_eq!(choices[0].selector, "bv*[height<=1080]+ba/b[height<=1080]/bv*+ba");
        assert_eq!(choices[3].selector, "ba[ext=m4a]/ba");
    }

    #[test]
    fn missing_formats_yield_no_choices() {
        assert!(parse_formats(&json!({})).is_empty());
        assert!(quality_choices(&[]).is_empty());
    }
}
