//! Quality selection logic for YouTube formats.
//!
//! Matches user preferences (quality, container format, audio-only) against
//! the list of available [`FormatEntry`]s and picks the best match. Pure
//! logic — natively testable without WASM or subprocess.

use serde::{Deserialize, Serialize};

use crate::error::PluginError;
use crate::metadata::{FormatEntry, FormatKind};

/// User-facing quality preference expressed as a vertical resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Quality {
    #[serde(rename = "360p")]
    P360,
    #[serde(rename = "480p")]
    P480,
    #[serde(rename = "720p")]
    P720,
    #[serde(rename = "1080p")]
    P1080,
    #[serde(rename = "1440p")]
    P1440,
    #[serde(rename = "2160p")]
    P2160,
    #[serde(rename = "4320p")]
    P4320,
    /// Always pick the highest available vertical resolution.
    #[serde(rename = "best")]
    Best,
}

impl Quality {
    /// Target height in pixels, or `None` for [`Quality::Best`].
    pub fn target_height(self) -> Option<u32> {
        match self {
            Self::P360 => Some(360),
            Self::P480 => Some(480),
            Self::P720 => Some(720),
            Self::P1080 => Some(1080),
            Self::P1440 => Some(1440),
            Self::P2160 => Some(2160),
            Self::P4320 => Some(4320),
            Self::Best => None,
        }
    }

    /// Parse a string like `"1080p"` or `"best"`.
    pub fn from_label(label: &str) -> Option<Self> {
        match label.trim().to_ascii_lowercase().as_str() {
            "360p" => Some(Self::P360),
            "480p" => Some(Self::P480),
            "720p" => Some(Self::P720),
            "1080p" => Some(Self::P1080),
            "1440p" | "2k" => Some(Self::P1440),
            "2160p" | "4k" => Some(Self::P2160),
            "4320p" | "8k" => Some(Self::P4320),
            "best" | "highest" => Some(Self::Best),
            _ => None,
        }
    }
}

/// User preferences passed to [`select_best_format`].
#[derive(Debug, Clone)]
pub struct SelectionPrefs {
    pub quality: Quality,
    pub preferred_container: Option<String>,
    pub audio_only: bool,
}

impl Default for SelectionPrefs {
    fn default() -> Self {
        Self {
            quality: Quality::P1080,
            preferred_container: Some("mp4".into()),
            audio_only: false,
        }
    }
}

/// Pick the best format from a list given a user's preferences.
///
/// Strategy:
///   1. If `audio_only`, restrict to [`FormatKind::AudioOnly`] and return the one
///      with highest audio bitrate.
///   2. Otherwise, restrict to [`FormatKind::Muxed`] + [`FormatKind::VideoOnly`].
///      Pick the format whose height is closest to (but not exceeding) the target,
///      falling back to the highest available below the target, or the smallest
///      above if none below.
///   3. Within the matching height bucket, prefer the user's container (`mp4`/`webm`/`mkv`).
pub fn select_best_format<'a>(
    formats: &'a [FormatEntry],
    prefs: &SelectionPrefs,
) -> Result<&'a FormatEntry, PluginError> {
    if formats.is_empty() {
        return Err(PluginError::NoMatchingFormat);
    }

    if prefs.audio_only {
        return pick_audio_only(formats);
    }

    pick_video(formats, prefs)
}

fn pick_audio_only(formats: &[FormatEntry]) -> Result<&FormatEntry, PluginError> {
    formats
        .iter()
        .filter(|f| f.kind == FormatKind::AudioOnly)
        .max_by(|a, b| {
            a.abr
                .unwrap_or(0.0)
                .partial_cmp(&b.abr.unwrap_or(0.0))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .ok_or(PluginError::NoMatchingFormat)
}

fn pick_video<'a>(
    formats: &'a [FormatEntry],
    prefs: &SelectionPrefs,
) -> Result<&'a FormatEntry, PluginError> {
    let video_formats: Vec<&FormatEntry> = formats
        .iter()
        .filter(|f| {
            matches!(f.kind, FormatKind::Muxed | FormatKind::VideoOnly) && f.height.is_some()
        })
        .collect();

    if video_formats.is_empty() {
        return Err(PluginError::NoMatchingFormat);
    }

    let target = prefs.quality.target_height();

    let chosen_height = match target {
        None => video_formats
            .iter()
            .filter_map(|f| f.height)
            .max()
            .ok_or(PluginError::NoMatchingFormat)?,
        Some(target) => {
            // Largest height ≤ target, or smallest height > target if none below.
            let below_or_eq = video_formats
                .iter()
                .filter_map(|f| f.height)
                .filter(|&h| h <= target)
                .max();
            match below_or_eq {
                Some(h) => h,
                None => video_formats
                    .iter()
                    .filter_map(|f| f.height)
                    .min()
                    .ok_or(PluginError::NoMatchingFormat)?,
            }
        }
    };

    // Prefer user's container within the chosen height bucket.
    let at_height: Vec<&FormatEntry> = video_formats
        .into_iter()
        .filter(|f| f.height == Some(chosen_height))
        .collect();

    if let Some(container) = &prefs.preferred_container {
        if let Some(best) = at_height
            .iter()
            .find(|f| f.ext.eq_ignore_ascii_case(container))
        {
            return Ok(best);
        }
    }

    at_height
        .into_iter()
        .next()
        .ok_or(PluginError::NoMatchingFormat)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn fmt(id: &str, ext: &str, height: Option<u32>, kind: FormatKind) -> FormatEntry {
        FormatEntry {
            format_id: id.into(),
            ext: ext.into(),
            height,
            width: None,
            vcodec: None,
            acodec: None,
            fps: None,
            filesize: None,
            abr: None,
            vbr: None,
            kind,
        }
    }

    fn audio_fmt(id: &str, ext: &str, abr: f64) -> FormatEntry {
        FormatEntry {
            format_id: id.into(),
            ext: ext.into(),
            height: None,
            width: None,
            vcodec: None,
            acodec: None,
            fps: None,
            filesize: None,
            abr: Some(abr),
            vbr: None,
            kind: FormatKind::AudioOnly,
        }
    }

    #[test]
    fn quality_from_label_parses_common_values() {
        assert_eq!(Quality::from_label("1080p"), Some(Quality::P1080));
        assert_eq!(Quality::from_label("4K"), Some(Quality::P2160));
        assert_eq!(Quality::from_label("best"), Some(Quality::Best));
    }

    #[test]
    fn quality_from_label_returns_none_for_unknown() {
        assert_eq!(Quality::from_label("42p"), None);
    }

    #[test]
    fn returns_error_when_no_formats_available() {
        let result = select_best_format(&[], &SelectionPrefs::default());
        assert!(matches!(result, Err(PluginError::NoMatchingFormat)));
    }

    #[test]
    fn picks_exact_quality_match_when_available() {
        let formats = vec![
            fmt("18", "mp4", Some(360), FormatKind::Muxed),
            fmt("22", "mp4", Some(720), FormatKind::Muxed),
            fmt("137", "mp4", Some(1080), FormatKind::VideoOnly),
        ];
        let prefs = SelectionPrefs {
            quality: Quality::P1080,
            preferred_container: Some("mp4".into()),
            audio_only: false,
        };
        let chosen = select_best_format(&formats, &prefs).unwrap();
        assert_eq!(chosen.format_id, "137");
    }

    #[test]
    fn falls_back_to_highest_below_target_when_exact_missing() {
        let formats = vec![
            fmt("18", "mp4", Some(360), FormatKind::Muxed),
            fmt("22", "mp4", Some(720), FormatKind::Muxed),
        ];
        let prefs = SelectionPrefs {
            quality: Quality::P1080,
            ..SelectionPrefs::default()
        };
        let chosen = select_best_format(&formats, &prefs).unwrap();
        assert_eq!(chosen.format_id, "22");
    }

    #[test]
    fn falls_back_up_when_only_higher_than_target_exists() {
        let formats = vec![fmt("137", "mp4", Some(1080), FormatKind::VideoOnly)];
        let prefs = SelectionPrefs {
            quality: Quality::P360,
            ..SelectionPrefs::default()
        };
        let chosen = select_best_format(&formats, &prefs).unwrap();
        assert_eq!(chosen.format_id, "137");
    }

    #[test]
    fn best_quality_picks_highest_resolution() {
        let formats = vec![
            fmt("18", "mp4", Some(360), FormatKind::Muxed),
            fmt("137", "mp4", Some(1080), FormatKind::VideoOnly),
            fmt("313", "webm", Some(2160), FormatKind::VideoOnly),
        ];
        let prefs = SelectionPrefs {
            quality: Quality::Best,
            preferred_container: None,
            audio_only: false,
        };
        let chosen = select_best_format(&formats, &prefs).unwrap();
        assert_eq!(chosen.format_id, "313");
    }

    #[test]
    fn prefers_user_container_within_height_bucket() {
        let formats = vec![
            fmt("248", "webm", Some(1080), FormatKind::VideoOnly),
            fmt("137", "mp4", Some(1080), FormatKind::VideoOnly),
        ];
        let prefs = SelectionPrefs {
            quality: Quality::P1080,
            preferred_container: Some("mp4".into()),
            audio_only: false,
        };
        let chosen = select_best_format(&formats, &prefs).unwrap();
        assert_eq!(chosen.format_id, "137");
    }

    #[test]
    fn falls_back_to_first_container_when_preferred_missing_at_height() {
        let formats = vec![fmt("248", "webm", Some(1080), FormatKind::VideoOnly)];
        let prefs = SelectionPrefs {
            quality: Quality::P1080,
            preferred_container: Some("mp4".into()),
            audio_only: false,
        };
        let chosen = select_best_format(&formats, &prefs).unwrap();
        assert_eq!(chosen.format_id, "248");
    }

    #[test]
    fn audio_only_picks_highest_bitrate() {
        let formats = vec![
            audio_fmt("139", "m4a", 48.0),
            audio_fmt("140", "m4a", 128.0),
            audio_fmt("141", "m4a", 256.0),
        ];
        let prefs = SelectionPrefs {
            audio_only: true,
            ..SelectionPrefs::default()
        };
        let chosen = select_best_format(&formats, &prefs).unwrap();
        assert_eq!(chosen.format_id, "141");
    }

    #[test]
    fn audio_only_ignores_video_formats() {
        let formats = vec![
            fmt("137", "mp4", Some(1080), FormatKind::VideoOnly),
            audio_fmt("140", "m4a", 128.0),
        ];
        let prefs = SelectionPrefs {
            audio_only: true,
            ..SelectionPrefs::default()
        };
        let chosen = select_best_format(&formats, &prefs).unwrap();
        assert_eq!(chosen.format_id, "140");
    }

    #[test]
    fn audio_only_errors_when_no_audio_formats() {
        let formats = vec![fmt("137", "mp4", Some(1080), FormatKind::VideoOnly)];
        let prefs = SelectionPrefs {
            audio_only: true,
            ..SelectionPrefs::default()
        };
        assert!(matches!(
            select_best_format(&formats, &prefs),
            Err(PluginError::NoMatchingFormat)
        ));
    }

    #[test]
    fn errors_when_no_video_formats_for_video_request() {
        let formats = vec![audio_fmt("140", "m4a", 128.0)];
        let prefs = SelectionPrefs::default();
        assert!(matches!(
            select_best_format(&formats, &prefs),
            Err(PluginError::NoMatchingFormat)
        ));
    }
}
