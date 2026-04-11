//! yt-dlp JSON parsing and media classification.
//!
//! Parses the output of `yt-dlp --dump-json` (single video) and
//! `yt-dlp --dump-json --flat-playlist` (playlist entries).
//!
//! Pure logic — unit-testable natively without yt-dlp or WASM.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::error::PluginError;

// ── Domain types ──────────────────────────────────────────────────────────────

/// Full metadata for a single YouTube video.
#[derive(Debug, Clone, Serialize)]
pub struct VideoInfo {
    pub id: String,
    pub title: String,
    pub description: String,
    pub duration: Option<u64>,
    pub upload_date: Option<String>,
    pub view_count: Option<u64>,
    pub uploader: Option<String>,
    pub webpage_url: String,
    pub thumbnail: Option<String>,
    pub formats: Vec<FormatEntry>,
    pub subtitles: HashMap<String, Vec<SubtitleTrack>>,
    pub automatic_captions: HashMap<String, Vec<SubtitleTrack>>,
}

/// A single downloadable format variant.
#[derive(Debug, Clone, Serialize)]
pub struct FormatEntry {
    pub format_id: String,
    pub ext: String,
    pub height: Option<u32>,
    pub width: Option<u32>,
    pub vcodec: Option<String>,
    pub acodec: Option<String>,
    pub fps: Option<f64>,
    pub filesize: Option<u64>,
    pub abr: Option<f64>,
    pub vbr: Option<f64>,
    pub kind: FormatKind,
}

/// Classification of a [`FormatEntry`] based on its codecs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FormatKind {
    /// Has both audio and video tracks (muxed).
    Muxed,
    /// Video track only, no audio.
    VideoOnly,
    /// Audio track only, no video.
    AudioOnly,
    /// Unknown or incomplete metadata.
    Unknown,
}

/// A subtitle track entry in one language.
#[derive(Debug, Clone, Serialize)]
pub struct SubtitleTrack {
    pub ext: String,
    pub url: Option<String>,
    pub name: Option<String>,
}

/// A playlist as produced by `--flat-playlist --dump-json`.
#[derive(Debug, Clone, Serialize)]
pub struct Playlist {
    pub id: Option<String>,
    pub title: Option<String>,
    pub entries: Vec<PlaylistEntry>,
}

/// One entry in a flat playlist.
#[derive(Debug, Clone, Serialize)]
pub struct PlaylistEntry {
    pub id: String,
    pub title: Option<String>,
    pub url: String,
    pub duration: Option<u64>,
    pub thumbnail: Option<String>,
}

// ── Raw yt-dlp JSON shapes (deserialize) ──────────────────────────────────────

#[derive(Debug, Deserialize)]
struct RawVideo {
    id: String,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    duration: Option<serde_json::Value>,
    #[serde(default)]
    upload_date: Option<String>,
    #[serde(default)]
    view_count: Option<u64>,
    #[serde(default)]
    uploader: Option<String>,
    #[serde(default)]
    webpage_url: Option<String>,
    #[serde(default)]
    thumbnail: Option<String>,
    #[serde(default)]
    formats: Vec<RawFormat>,
    #[serde(default)]
    subtitles: HashMap<String, Vec<RawSubtitle>>,
    #[serde(default)]
    automatic_captions: HashMap<String, Vec<RawSubtitle>>,
}

#[derive(Debug, Deserialize)]
struct RawFormat {
    format_id: String,
    #[serde(default)]
    ext: Option<String>,
    #[serde(default)]
    height: Option<u32>,
    #[serde(default)]
    width: Option<u32>,
    #[serde(default)]
    vcodec: Option<String>,
    #[serde(default)]
    acodec: Option<String>,
    #[serde(default)]
    fps: Option<f64>,
    #[serde(default)]
    filesize: Option<u64>,
    #[serde(default)]
    abr: Option<f64>,
    #[serde(default)]
    vbr: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct RawSubtitle {
    #[serde(default)]
    ext: Option<String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawPlaylistEntry {
    id: String,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    webpage_url: Option<String>,
    #[serde(default)]
    duration: Option<serde_json::Value>,
    #[serde(default)]
    thumbnail: Option<String>,
    #[serde(default)]
    playlist_id: Option<String>,
    #[serde(default)]
    playlist: Option<String>,
}

/// Envelope yt-dlp sometimes emits for channels: a single JSON object with
/// an `entries` array rather than JSONL.
#[derive(Debug, Deserialize)]
struct RawPlaylistEnvelope {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    title: Option<String>,
    entries: Vec<RawPlaylistEntry>,
}

// ── Parsing ───────────────────────────────────────────────────────────────────

/// Parse the JSON output of `yt-dlp --dump-json <url>` for a single video.
pub fn parse_single_video(json: &str) -> Result<VideoInfo, PluginError> {
    let raw: RawVideo = serde_json::from_str(json)?;

    let formats = raw.formats.into_iter().map(into_format_entry).collect();
    let subtitles = convert_subtitles(raw.subtitles);
    let automatic_captions = convert_subtitles(raw.automatic_captions);
    // yt-dlp always populates `webpage_url` for successful extractions, but
    // when a downstream tool feeds us a sparse dump we derive a canonical
    // watch URL from the video id rather than emit an empty string silently.
    let webpage_url = raw
        .webpage_url
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| canonical_watch_url(&raw.id));

    Ok(VideoInfo {
        id: raw.id,
        title: raw.title.unwrap_or_default(),
        description: raw.description.unwrap_or_default(),
        duration: parse_duration(raw.duration),
        upload_date: raw.upload_date,
        view_count: raw.view_count,
        uploader: raw.uploader,
        webpage_url,
        thumbnail: raw.thumbnail,
        formats,
        subtitles,
        automatic_captions,
    })
}

/// Canonical YouTube watch URL for a given video id — used as a fallback
/// when yt-dlp omits `webpage_url`/`url`.
fn canonical_watch_url(id: &str) -> String {
    format!("https://www.youtube.com/watch?v={id}")
}

/// Parse the output of `yt-dlp --dump-json --flat-playlist <url>`.
///
/// yt-dlp emits **either** one JSON object per line (JSONL — typical for
/// playlists) **or** a single JSON object with an `entries` array (envelope
/// format — often used for channels, depending on yt-dlp version and URL
/// shape). Both shapes are accepted.
///
/// Detection is defensive: we attempt envelope deserialisation first when
/// the input is a single top-level JSON object, and silently fall back to
/// JSONL parsing on any failure. This avoids the previous heuristic where a
/// substring match on `"entries"` (which could appear as a string value, not
/// a key) would commit to envelope parsing and fail the whole playlist.
pub fn parse_flat_playlist(output: &str) -> Result<Playlist, PluginError> {
    let trimmed = output.trim_start();
    if trimmed.is_empty() {
        return Ok(Playlist {
            id: None,
            title: None,
            entries: Vec::new(),
        });
    }

    // Only consider the envelope shape when the output is a single JSON
    // object. Multi-line outputs are always JSONL by definition.
    let single_line_object =
        trimmed.starts_with('{') && trimmed.lines().filter(|l| !l.trim().is_empty()).count() == 1;

    if single_line_object {
        if let Ok(env) = serde_json::from_str::<RawPlaylistEnvelope>(trimmed) {
            return Ok(from_envelope(env));
        }
        // Not an envelope after all — fall through to JSONL parsing of the
        // same single line (it may be a single-entry flat playlist).
    }

    parse_jsonl_playlist(output)
}

fn from_envelope(env: RawPlaylistEnvelope) -> Playlist {
    let entries = env.entries.into_iter().map(into_playlist_entry).collect();
    Playlist {
        id: env.id,
        title: env.title,
        entries,
    }
}

fn parse_jsonl_playlist(jsonl: &str) -> Result<Playlist, PluginError> {
    let mut entries = Vec::new();
    let mut playlist_id = None;
    let mut playlist_title = None;

    for (idx, line) in jsonl.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let raw: RawPlaylistEntry = serde_json::from_str(trimmed)
            .map_err(|e| PluginError::ParseJson(format!("playlist line {}: {}", idx + 1, e)))?;

        if playlist_id.is_none() {
            playlist_id = raw.playlist_id.clone();
        }
        if playlist_title.is_none() {
            playlist_title = raw.playlist.clone();
        }

        entries.push(into_playlist_entry(raw));
    }

    Ok(Playlist {
        id: playlist_id,
        title: playlist_title,
        entries,
    })
}

fn into_playlist_entry(raw: RawPlaylistEntry) -> PlaylistEntry {
    // Prefer webpage_url > url > canonical fallback from the video id. yt-dlp
    // omits both fields for placeholder entries (private/deleted/region-locked
    // videos) and emitting an empty string silently would produce unusable
    // MediaLink.url values downstream — deriving a canonical watch URL keeps
    // the link at least navigable and debuggable.
    let url = raw
        .webpage_url
        .filter(|s| !s.is_empty())
        .or_else(|| raw.url.filter(|s| !s.is_empty()))
        .unwrap_or_else(|| canonical_watch_url(&raw.id));

    PlaylistEntry {
        id: raw.id,
        title: raw.title,
        url,
        duration: parse_duration(raw.duration),
        thumbnail: raw.thumbnail,
    }
}

// ── Conversion helpers ────────────────────────────────────────────────────────

fn into_format_entry(raw: RawFormat) -> FormatEntry {
    let kind = classify_format(raw.vcodec.as_deref(), raw.acodec.as_deref());
    FormatEntry {
        format_id: raw.format_id,
        ext: raw.ext.unwrap_or_default(),
        height: raw.height,
        width: raw.width,
        vcodec: raw.vcodec,
        acodec: raw.acodec,
        fps: raw.fps,
        filesize: raw.filesize,
        abr: raw.abr,
        vbr: raw.vbr,
        kind,
    }
}

/// Classify a format based on its codecs.
///
/// yt-dlp uses the literal string `"none"` to indicate absence of a track.
pub fn classify_format(vcodec: Option<&str>, acodec: Option<&str>) -> FormatKind {
    let has_video = codec_present(vcodec);
    let has_audio = codec_present(acodec);
    match (has_video, has_audio) {
        (true, true) => FormatKind::Muxed,
        (true, false) => FormatKind::VideoOnly,
        (false, true) => FormatKind::AudioOnly,
        (false, false) => FormatKind::Unknown,
    }
}

fn codec_present(codec: Option<&str>) -> bool {
    match codec {
        Some(c) => !c.is_empty() && !c.eq_ignore_ascii_case("none"),
        None => false,
    }
}

fn convert_subtitles(
    raw: HashMap<String, Vec<RawSubtitle>>,
) -> HashMap<String, Vec<SubtitleTrack>> {
    raw.into_iter()
        .map(|(lang, tracks)| {
            let converted = tracks
                .into_iter()
                .map(|t| SubtitleTrack {
                    ext: t.ext.unwrap_or_default(),
                    url: t.url,
                    name: t.name,
                })
                .collect();
            (lang, converted)
        })
        .collect()
}

fn parse_duration(value: Option<serde_json::Value>) -> Option<u64> {
    value.and_then(|v| match v {
        serde_json::Value::Number(n) => n.as_f64().and_then(seconds_to_u64),
        serde_json::Value::String(s) => s.parse::<f64>().ok().and_then(seconds_to_u64),
        _ => None,
    })
}

/// Convert a floating-point seconds value to a whole-second `u64`.
///
/// Rejects negative values (yt-dlp may emit `-1` as a sentinel for live
/// streams or unknown durations) and NaN/infinity. Without this guard the
/// saturating `as u64` cast would silently coerce `-1.0` into `Some(0)`,
/// indistinguishable from a legitimate 0-second video.
fn seconds_to_u64(f: f64) -> Option<u64> {
    if f.is_nan() || f.is_infinite() || f < 0.0 {
        None
    } else {
        Some(f.round() as u64)
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const SINGLE_VIDEO_JSON: &str = r#"{
        "id": "dQw4w9WgXcQ",
        "title": "Rick Astley - Never Gonna Give You Up",
        "description": "The official video",
        "duration": 212,
        "upload_date": "20091025",
        "view_count": 1500000000,
        "uploader": "Rick Astley",
        "webpage_url": "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
        "thumbnail": "https://i.ytimg.com/vi/dQw4w9WgXcQ/maxresdefault.jpg",
        "formats": [
            {
                "format_id": "18",
                "ext": "mp4",
                "height": 360,
                "width": 640,
                "vcodec": "avc1.42001E",
                "acodec": "mp4a.40.2",
                "fps": 25.0,
                "filesize": 10485760
            },
            {
                "format_id": "137",
                "ext": "mp4",
                "height": 1080,
                "width": 1920,
                "vcodec": "avc1.640028",
                "acodec": "none",
                "fps": 25.0,
                "vbr": 2500.0
            },
            {
                "format_id": "140",
                "ext": "m4a",
                "vcodec": "none",
                "acodec": "mp4a.40.2",
                "abr": 128.0
            }
        ],
        "subtitles": {
            "en": [{"ext": "vtt", "url": "https://example.com/sub.vtt", "name": "English"}]
        },
        "automatic_captions": {
            "en": [{"ext": "json3", "url": "https://example.com/auto.json3"}]
        }
    }"#;

    #[test]
    fn parses_single_video_basic_fields() {
        let info = parse_single_video(SINGLE_VIDEO_JSON).expect("parse should succeed");
        assert_eq!(info.id, "dQw4w9WgXcQ");
        assert_eq!(info.title, "Rick Astley - Never Gonna Give You Up");
        assert_eq!(info.duration, Some(212));
        assert_eq!(info.view_count, Some(1500000000));
        assert_eq!(info.uploader.as_deref(), Some("Rick Astley"));
    }

    #[test]
    fn parses_single_video_formats_with_classification() {
        let info = parse_single_video(SINGLE_VIDEO_JSON).unwrap();
        assert_eq!(info.formats.len(), 3);
        assert_eq!(info.formats[0].kind, FormatKind::Muxed);
        assert_eq!(info.formats[1].kind, FormatKind::VideoOnly);
        assert_eq!(info.formats[2].kind, FormatKind::AudioOnly);
    }

    #[test]
    fn parses_single_video_subtitles() {
        let info = parse_single_video(SINGLE_VIDEO_JSON).unwrap();
        assert_eq!(info.subtitles.len(), 1);
        assert!(info.subtitles.contains_key("en"));
        assert_eq!(info.subtitles["en"][0].ext, "vtt");
        assert_eq!(info.automatic_captions["en"][0].ext, "json3");
    }

    #[test]
    fn returns_error_on_invalid_json() {
        let result = parse_single_video("{ not json");
        assert!(matches!(result, Err(PluginError::SerdeJson(_))));
    }

    #[test]
    fn parses_envelope_format_with_entries_array() {
        let envelope = r#"{"id":"UCxyz","title":"Channel X","entries":[{"id":"v1","title":"First","webpage_url":"https://www.youtube.com/watch?v=v1"},{"id":"v2","title":"Second","webpage_url":"https://www.youtube.com/watch?v=v2"}]}"#;
        let pl = parse_flat_playlist(envelope).unwrap();
        assert_eq!(pl.id.as_deref(), Some("UCxyz"));
        assert_eq!(pl.title.as_deref(), Some("Channel X"));
        assert_eq!(pl.entries.len(), 2);
        assert_eq!(pl.entries[0].id, "v1");
        assert_eq!(pl.entries[1].id, "v2");
    }

    #[test]
    fn single_line_with_entries_as_value_parses_as_jsonl() {
        // A JSONL line where "entries" is a string value, not a key. The old
        // heuristic (substring check) committed to envelope parsing and
        // failed. The new defensive path should fall back to JSONL.
        let line = r#"{"id":"abc12345678","title":"entries","webpage_url":"https://www.youtube.com/watch?v=abc12345678"}"#;
        let pl = parse_flat_playlist(line).unwrap();
        assert_eq!(pl.entries.len(), 1);
        assert_eq!(pl.entries[0].title.as_deref(), Some("entries"));
    }

    #[test]
    fn playlist_entry_missing_url_fallbacks_to_canonical_watch_url() {
        let line = r#"{"id":"abc12345678","title":"Placeholder"}"#;
        let pl = parse_flat_playlist(line).unwrap();
        assert_eq!(pl.entries.len(), 1);
        assert_eq!(
            pl.entries[0].url,
            "https://www.youtube.com/watch?v=abc12345678"
        );
    }

    #[test]
    fn single_video_missing_webpage_url_fallbacks_to_canonical() {
        let json = r#"{"id":"abc12345678"}"#;
        let info = parse_single_video(json).unwrap();
        assert_eq!(
            info.webpage_url,
            "https://www.youtube.com/watch?v=abc12345678"
        );
    }

    #[test]
    fn parses_negative_duration_as_none() {
        let json = r#"{"id":"abc12345678","duration":-1}"#;
        let info = parse_single_video(json).unwrap();
        assert_eq!(info.duration, None);
    }

    #[test]
    fn parses_nan_duration_as_none() {
        // serde_json rejects NaN in strict number form, so test the string
        // branch directly.
        let json = r#"{"id":"abc12345678","duration":"nan"}"#;
        let info = parse_single_video(json).unwrap();
        assert_eq!(info.duration, None);
    }

    #[test]
    fn parses_empty_output_as_empty_playlist() {
        let pl = parse_flat_playlist("").unwrap();
        assert!(pl.entries.is_empty());
    }

    #[test]
    fn handles_missing_optional_fields() {
        let minimal = r#"{"id":"abc12345678"}"#;
        let info = parse_single_video(minimal).unwrap();
        assert_eq!(info.id, "abc12345678");
        assert_eq!(info.title, "");
        assert!(info.formats.is_empty());
        assert!(info.subtitles.is_empty());
    }

    #[test]
    fn classifies_audio_only_when_vcodec_none() {
        assert_eq!(
            classify_format(Some("none"), Some("mp4a.40.2")),
            FormatKind::AudioOnly
        );
    }

    #[test]
    fn classifies_video_only_when_acodec_none() {
        assert_eq!(
            classify_format(Some("avc1"), Some("none")),
            FormatKind::VideoOnly
        );
    }

    #[test]
    fn classifies_muxed_when_both_codecs_present() {
        assert_eq!(
            classify_format(Some("avc1"), Some("mp4a.40.2")),
            FormatKind::Muxed
        );
    }

    #[test]
    fn classifies_unknown_when_both_codecs_missing() {
        assert_eq!(classify_format(None, None), FormatKind::Unknown);
    }

    #[test]
    fn classifies_unknown_when_both_codecs_none() {
        assert_eq!(
            classify_format(Some("none"), Some("none")),
            FormatKind::Unknown
        );
    }

    const FLAT_PLAYLIST_JSONL: &str = "{\"id\":\"abc12345678\",\"title\":\"First\",\"webpage_url\":\"https://www.youtube.com/watch?v=abc12345678\",\"duration\":120,\"playlist_id\":\"PLxyz\",\"playlist\":\"My List\"}\n{\"id\":\"def12345678\",\"title\":\"Second\",\"url\":\"https://www.youtube.com/watch?v=def12345678\",\"duration\":240}";

    #[test]
    fn parses_flat_playlist_multiple_entries() {
        let pl = parse_flat_playlist(FLAT_PLAYLIST_JSONL).unwrap();
        assert_eq!(pl.entries.len(), 2);
        assert_eq!(pl.entries[0].id, "abc12345678");
        assert_eq!(pl.entries[0].title.as_deref(), Some("First"));
        assert_eq!(pl.entries[1].id, "def12345678");
    }

    #[test]
    fn captures_playlist_id_and_title_from_first_entry() {
        let pl = parse_flat_playlist(FLAT_PLAYLIST_JSONL).unwrap();
        assert_eq!(pl.id.as_deref(), Some("PLxyz"));
        assert_eq!(pl.title.as_deref(), Some("My List"));
    }

    #[test]
    fn uses_webpage_url_over_url_when_both_present() {
        let line =
            r#"{"id":"abc12345678","webpage_url":"https://webpage/","url":"https://fallback/"}"#;
        let pl = parse_flat_playlist(line).unwrap();
        assert_eq!(pl.entries[0].url, "https://webpage/");
    }

    #[test]
    fn skips_empty_lines_in_playlist() {
        let jsonl = "\n{\"id\":\"abc12345678\"}\n\n";
        let pl = parse_flat_playlist(jsonl).unwrap();
        assert_eq!(pl.entries.len(), 1);
    }

    #[test]
    fn returns_error_on_invalid_playlist_line() {
        let result = parse_flat_playlist("{\"id\":\"x\"}\n{invalid");
        assert!(matches!(result, Err(PluginError::ParseJson(_))));
    }

    #[test]
    fn parses_duration_from_float() {
        let json = r#"{"id":"abc12345678","duration":212.5}"#;
        let info = parse_single_video(json).unwrap();
        assert_eq!(info.duration, Some(213));
    }
}
