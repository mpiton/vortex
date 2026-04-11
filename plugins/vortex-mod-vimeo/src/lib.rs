//! Vortex Vimeo WASM plugin.
//!
//! Implements the CrawlerModule contract expected by the Vortex plugin host:
//! - `can_handle(url)` → `"true"` / `"false"`
//! - `supports_playlist(url)` → `"true"` / `"false"`
//! - `extract_links(url)` → JSON string describing the resolved media
//! - `get_media_variants(url)` → JSON string listing available formats
//!
//! Network access is delegated to the host via `http_request`.

pub mod error;
pub mod parser;
pub mod url_matcher;

#[cfg(target_family = "wasm")]
mod plugin_api;

use serde::Serialize;

use crate::error::PluginError;
use crate::parser::{OembedResponse, PlayerConfig, ProgressiveEntry};
use crate::url_matcher::UrlKind;

// ── IPC DTOs ──────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct ExtractLinksResponse {
    pub kind: &'static str,
    pub videos: Vec<MediaLink>,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct MediaLink {
    pub id: String,
    pub title: String,
    pub url: String,
    pub description: Option<String>,
    pub uploader: Option<String>,
    pub duration: Option<u64>,
    pub thumbnail: Option<String>,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct MediaVariantsResponse {
    pub variants: Vec<MediaVariant>,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct MediaVariant {
    pub format_id: String,
    pub kind: VariantKind,
    pub ext: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub fps: Option<f64>,
    pub url: String,
}

#[derive(Debug, Serialize, PartialEq, Eq, Clone, Copy)]
#[serde(rename_all = "snake_case")]
pub enum VariantKind {
    Video,
    Audio,
    Adaptive,
}

// ── Routing helpers ──────────────────────────────────────────────────────────

pub fn handle_can_handle(url: &str) -> String {
    // Showcase URLs are intentionally excluded until
    // `extract_playlist` is implemented — advertising support would
    // produce a false-positive followed by a runtime `UnsupportedUrl`.
    bool_to_string(matches!(
        url_matcher::classify_url(url),
        UrlKind::Video | UrlKind::PrivateVideo
    ))
}

pub fn handle_supports_playlist(_url: &str) -> String {
    // Same rationale as `handle_can_handle`: showcase enumeration
    // requires an access-token endpoint that is not wired in this MVP.
    // The URL is intentionally ignored — we unconditionally report
    // `false` so the host never routes showcase URLs to a handler
    // that can only fail. Re-introduce a URL inspection here once
    // `extract_playlist` grows a working showcase backend.
    bool_to_string(false)
}

fn bool_to_string(b: bool) -> String {
    if b {
        "true".into()
    } else {
        "false".into()
    }
}

/// Reject URLs that are not a single-video resource.
///
/// With the current [`UrlKind`] set (`Video`, `PrivateVideo`,
/// `Showcase`, `Unknown`), this is functionally equivalent to
/// [`ensure_single_video`] — both gate on the same variants because
/// showcase extraction is not implemented. The two functions are kept
/// as distinct names for call-site clarity: `ensure_vimeo_url` is
/// used at the top-level routing boundary (e.g. by a future
/// `extract_links` that supports playlists), while
/// `ensure_single_video` is used by handlers that specifically need a
/// single-video resource (`get_media_variants`). When showcase
/// support lands, `ensure_vimeo_url` will accept `Showcase` too and
/// the two will diverge.
pub fn ensure_vimeo_url(url: &str) -> Result<UrlKind, PluginError> {
    ensure_single_video(url)
}

/// Reject URLs that are not a Video or PrivateVideo. Callers that need
/// to operate on a single-video resource (progressive variants, HLS,
/// oEmbed) should call this instead of [`ensure_vimeo_url`] so that
/// future expansion of the routing contract does not accidentally let
/// showcase URLs reach a single-video code path.
pub fn ensure_single_video(url: &str) -> Result<UrlKind, PluginError> {
    match url_matcher::classify_url(url) {
        kind @ (UrlKind::Video | UrlKind::PrivateVideo) => Ok(kind),
        UrlKind::Showcase | UrlKind::Unknown => Err(PluginError::UnsupportedUrl(url.to_string())),
    }
}

// ── Response builders ─────────────────────────────────────────────────────────

/// Build a single-video [`ExtractLinksResponse`] from an oEmbed payload
/// and the **original source URL** the caller resolved against.
///
/// Private share links (`vimeo.com/<id>/<hash>`) carry an auth token in
/// the second path segment — reconstructing the URL from `video_id`
/// alone would drop that hash, and the resulting permalink would no
/// longer open the same video. So the caller must pass the original
/// URL in as `source_url`, and this function preserves it verbatim
/// except when it is empty (in which case it falls back to the
/// `https://vimeo.com/<id>` permalink derived from the oEmbed payload).
pub fn build_single_video_response(
    oembed: OembedResponse,
    source_url: &str,
) -> ExtractLinksResponse {
    let id = oembed.video_id.map(|id| id.to_string()).unwrap_or_default();
    let url = if !source_url.is_empty() {
        source_url.to_string()
    } else if !id.is_empty() {
        format!("https://vimeo.com/{id}")
    } else {
        String::new()
    };
    let link = MediaLink {
        id,
        title: oembed.title,
        url,
        description: oembed.description,
        uploader: oembed.author_name,
        duration: oembed.duration,
        thumbnail: oembed.thumbnail_url,
    };
    ExtractLinksResponse {
        kind: "video",
        videos: vec![link],
    }
}

/// Build the variants list from a parsed player config.
///
/// Progressive MP4 URLs become `Video` variants, while the HLS master
/// manifest is exposed as a single `Adaptive` entry pointing at the
/// default CDN URL (falling back to the first entry if none is flagged
/// default). When `audio_only` is requested the caller post-filters
/// with [`filter_audio_only`].
pub fn build_media_variants_response(config: PlayerConfig) -> MediaVariantsResponse {
    let mut variants: Vec<MediaVariant> = config
        .request
        .files
        .progressive
        .into_iter()
        .map(progressive_to_variant)
        .collect();

    if let Some(hls) = config.request.files.hls {
        if let Some(cdn) = pick_cdn(&hls) {
            variants.push(MediaVariant {
                format_id: "hls".into(),
                kind: VariantKind::Adaptive,
                ext: "m3u8".into(),
                width: None,
                height: None,
                fps: None,
                url: cdn,
            });
        }
    }

    // Deterministic order: progressive in ascending height, adaptive last.
    variants.sort_by_key(|v| match v.kind {
        VariantKind::Audio => (0u8, 0u32),
        VariantKind::Video => (1, v.height.unwrap_or(0)),
        VariantKind::Adaptive => (2, 0),
    });

    MediaVariantsResponse { variants }
}

fn pick_cdn(hls: &parser::HlsEntry) -> Option<String> {
    if let Some(key) = &hls.default_cdn {
        if let Some(entry) = hls.cdns.get(key) {
            return Some(entry.url.clone());
        }
    }
    // `HashMap::values().next()` is non-deterministic: the chosen CDN
    // would change across runs even when the rest of the variant list
    // is intentionally stable. Iterate over the keys and pick the
    // lexicographically smallest one so the fallback is reproducible
    // and matches the sort applied to progressive variants.
    let min_key = hls.cdns.keys().min()?;
    hls.cdns.get(min_key).map(|e| e.url.clone())
}

fn progressive_to_variant(entry: ProgressiveEntry) -> MediaVariant {
    let ext = entry
        .mime
        .as_deref()
        .and_then(|m| m.strip_prefix("video/"))
        .unwrap_or("mp4")
        .to_string();
    MediaVariant {
        format_id: entry.quality.clone(),
        kind: VariantKind::Video,
        ext,
        width: entry.width,
        height: entry.height,
        fps: entry.fps,
        url: entry.url,
    }
}

/// Drop every non-audio-eligible variant (i.e. all progressive video
/// entries) when the user has requested audio-only download. The HLS
/// `Adaptive` stream is preserved because it muxes audio + video and
/// the downstream pipeline demuxes audio from it.
pub fn filter_audio_only(mut response: MediaVariantsResponse) -> MediaVariantsResponse {
    response.variants.retain(|v| v.kind != VariantKind::Video);
    response
}

/// Return the variant closest to (but not exceeding) the user's
/// preferred quality (e.g. `"720p"`). Falls back to the highest
/// available progressive variant, and finally to the HLS stream.
pub fn pick_variant_for_quality<'a>(
    variants: &'a [MediaVariant],
    preferred: &str,
) -> Option<&'a MediaVariant> {
    let target = parse_height(preferred)?;
    let mut best: Option<&MediaVariant> = None;
    for v in variants.iter().filter(|v| v.kind == VariantKind::Video) {
        if let Some(h) = v.height {
            if h <= target {
                best = match best {
                    Some(prev) if prev.height.unwrap_or(0) >= h => Some(prev),
                    _ => Some(v),
                };
            }
        }
    }
    best.or_else(|| variants.iter().find(|v| v.kind == VariantKind::Adaptive))
}

fn parse_height(quality: &str) -> Option<u32> {
    let trimmed = quality.trim_end_matches(['p', 'P']);
    // "2K" ≈ 1080 isn't quite right, but it's what the UI uses to label
    // 1440p; similarly "4K" → 2160. The mapping mirrors the plugin.toml
    // options list.
    match trimmed.to_ascii_uppercase().as_str() {
        "2K" => Some(1440),
        "4K" => Some(2160),
        other => other.parse().ok(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::{
        CdnEntry, FilesConfig, HlsEntry, OembedResponse, PlayerConfig, ProgressiveEntry,
        RequestConfig,
    };
    use std::collections::HashMap;

    fn sample_oembed() -> OembedResponse {
        OembedResponse {
            kind: "video".into(),
            title: "Sintel trailer".into(),
            description: Some("Blender demo".into()),
            author_name: Some("Blender Foundation".into()),
            author_url: Some("https://vimeo.com/blender".into()),
            thumbnail_url: Some("https://i.vimeocdn.com/video/1.jpg".into()),
            duration: Some(52),
            video_id: Some(123_456_789),
        }
    }

    fn sample_progressive(quality: &str, height: u32, url: &str) -> ProgressiveEntry {
        ProgressiveEntry {
            profile: None,
            quality: quality.to_string(),
            width: Some(height * 16 / 9),
            height: Some(height),
            fps: Some(24.0),
            mime: Some("video/mp4".into()),
            url: url.to_string(),
        }
    }

    fn sample_config_with_all() -> PlayerConfig {
        let mut cdns = HashMap::new();
        cdns.insert(
            "akfire".into(),
            CdnEntry {
                url: "https://cdn.vimeocdn.com/master.m3u8".into(),
                avc_url: None,
            },
        );
        PlayerConfig {
            request: RequestConfig {
                files: FilesConfig {
                    progressive: vec![
                        sample_progressive("1080p", 1080, "https://a.mp4"),
                        sample_progressive("360p", 360, "https://b.mp4"),
                        sample_progressive("720p", 720, "https://c.mp4"),
                    ],
                    hls: Some(HlsEntry {
                        cdns,
                        default_cdn: Some("akfire".into()),
                    }),
                    dash: None,
                },
            },
            video: None,
        }
    }

    #[test]
    fn can_handle_recognises_public_video() {
        assert_eq!(handle_can_handle("https://vimeo.com/123456789"), "true");
    }

    #[test]
    fn can_handle_rejects_unknown() {
        assert_eq!(handle_can_handle("https://example.com/"), "false");
    }

    #[test]
    fn supports_playlist_false_for_video() {
        assert_eq!(
            handle_supports_playlist("https://vimeo.com/123456789"),
            "false"
        );
    }

    #[test]
    fn build_single_video_response_populates_fields() {
        let r = build_single_video_response(sample_oembed(), "https://vimeo.com/123456789");
        assert_eq!(r.kind, "video");
        assert_eq!(r.videos.len(), 1);
        let v = &r.videos[0];
        assert_eq!(v.id, "123456789");
        assert_eq!(v.title, "Sintel trailer");
        assert_eq!(v.url, "https://vimeo.com/123456789");
        assert_eq!(v.uploader.as_deref(), Some("Blender Foundation"));
        assert_eq!(v.duration, Some(52));
    }

    #[test]
    fn build_single_video_response_preserves_private_share_hash() {
        // For private share links the hash token must not be dropped.
        let source_url = "https://vimeo.com/123456789/abcdef1234";
        let r = build_single_video_response(sample_oembed(), source_url);
        assert_eq!(
            r.videos[0].url, source_url,
            "private share URL must be preserved verbatim"
        );
    }

    #[test]
    fn build_single_video_response_falls_back_when_source_empty() {
        // When the caller has no source URL (e.g. internal batch),
        // the oEmbed video_id is used to reconstruct a public permalink.
        let r = build_single_video_response(sample_oembed(), "");
        assert_eq!(r.videos[0].url, "https://vimeo.com/123456789");
    }

    #[test]
    fn build_variants_sorted_ascending_height_then_hls() {
        let r = build_media_variants_response(sample_config_with_all());
        let heights: Vec<Option<u32>> = r.variants.iter().map(|v| v.height).collect();
        // Progressive order: 360 → 720 → 1080 → HLS(no height)
        assert_eq!(heights, vec![Some(360), Some(720), Some(1080), None]);
        assert_eq!(r.variants.last().unwrap().kind, VariantKind::Adaptive);
    }

    #[test]
    fn build_variants_ext_derived_from_mime() {
        let r = build_media_variants_response(sample_config_with_all());
        assert_eq!(r.variants[0].ext, "mp4");
    }

    #[test]
    fn filter_audio_only_keeps_only_adaptive() {
        let r = build_media_variants_response(sample_config_with_all());
        let filtered = filter_audio_only(r);
        assert_eq!(filtered.variants.len(), 1);
        assert_eq!(filtered.variants[0].kind, VariantKind::Adaptive);
    }

    #[test]
    fn pick_variant_below_preferred_quality() {
        let r = build_media_variants_response(sample_config_with_all());
        let picked = pick_variant_for_quality(&r.variants, "720p").unwrap();
        assert_eq!(picked.height, Some(720));
    }

    #[test]
    fn pick_variant_works_on_unsorted_slice() {
        // Callers of `pick_variant_for_quality` are not required to
        // sort their input first — `build_media_variants_response`
        // does sort internally, but the helper must remain correct
        // when given an arbitrary slice.
        let variants = vec![
            MediaVariant {
                format_id: "1080p".into(),
                kind: VariantKind::Video,
                ext: "mp4".into(),
                width: Some(1920),
                height: Some(1080),
                fps: Some(24.0),
                url: "a".into(),
            },
            MediaVariant {
                format_id: "360p".into(),
                kind: VariantKind::Video,
                ext: "mp4".into(),
                width: Some(640),
                height: Some(360),
                fps: Some(24.0),
                url: "b".into(),
            },
            MediaVariant {
                format_id: "720p".into(),
                kind: VariantKind::Video,
                ext: "mp4".into(),
                width: Some(1280),
                height: Some(720),
                fps: Some(24.0),
                url: "c".into(),
            },
        ];
        let picked = pick_variant_for_quality(&variants, "720p").unwrap();
        assert_eq!(picked.height, Some(720));
        let picked = pick_variant_for_quality(&variants, "1080p").unwrap();
        assert_eq!(picked.height, Some(1080));
        let picked = pick_variant_for_quality(&variants, "480p").unwrap();
        assert_eq!(
            picked.height,
            Some(360),
            "480p preferred should pick 360p (max height <= target)"
        );
    }

    #[test]
    fn pick_variant_for_2k_maps_to_1440() {
        // no 1440p entry, only 360/720/1080 → 1080 is the closest ≤1440
        let r = build_media_variants_response(sample_config_with_all());
        let picked = pick_variant_for_quality(&r.variants, "2K").unwrap();
        assert_eq!(picked.height, Some(1080));
    }

    #[test]
    fn pick_variant_falls_back_to_adaptive_when_no_progressive_fits() {
        // Preferred quality lower than any progressive → fall back to HLS
        let r = build_media_variants_response(sample_config_with_all());
        let picked = pick_variant_for_quality(&r.variants, "240p").unwrap();
        assert_eq!(picked.kind, VariantKind::Adaptive);
    }

    #[test]
    fn ensure_vimeo_url_rejects_unknown() {
        let err = ensure_vimeo_url("https://example.com/").unwrap_err();
        assert!(matches!(err, PluginError::UnsupportedUrl(_)));
    }

    #[test]
    fn ensure_single_video_rejects_showcase() {
        let err = ensure_single_video("https://vimeo.com/showcase/1").unwrap_err();
        assert!(matches!(err, PluginError::UnsupportedUrl(_)));
    }

    #[test]
    fn private_video_classification_is_accepted_by_can_handle() {
        assert_eq!(
            handle_can_handle("https://vimeo.com/123456789/abcdef1234"),
            "true"
        );
    }

    #[test]
    fn json_serialisation_of_extract_links_response() {
        let r = build_single_video_response(sample_oembed(), "https://vimeo.com/123456789");
        let json = serde_json::to_string(&r).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["kind"], "video");
        assert_eq!(parsed["videos"][0]["title"], "Sintel trailer");
    }

    #[test]
    fn supports_playlist_false_for_showcase_until_implemented() {
        assert_eq!(
            handle_supports_playlist("https://vimeo.com/showcase/98765"),
            "false",
            "Showcase must not be advertised as playlist-supported"
        );
    }

    #[test]
    fn can_handle_rejects_showcase_until_implemented() {
        assert_eq!(
            handle_can_handle("https://vimeo.com/showcase/98765"),
            "false"
        );
    }

    #[test]
    fn ensure_vimeo_url_rejects_showcase() {
        let err = ensure_vimeo_url("https://vimeo.com/showcase/98765").unwrap_err();
        assert!(matches!(err, PluginError::UnsupportedUrl(_)));
    }

    #[test]
    fn pick_cdn_is_deterministic_without_default() {
        // When `default_cdn` is missing, we must pick the
        // lexicographically smallest key so the result is stable
        // across runs.
        let mut cdns = HashMap::new();
        cdns.insert(
            "z_akamai".into(),
            CdnEntry {
                url: "https://z.example/m.m3u8".into(),
                avc_url: None,
            },
        );
        cdns.insert(
            "a_fastly".into(),
            CdnEntry {
                url: "https://a.example/m.m3u8".into(),
                avc_url: None,
            },
        );
        let hls = HlsEntry {
            cdns,
            default_cdn: None,
        };
        // Run multiple times to catch order instability.
        for _ in 0..5 {
            assert_eq!(pick_cdn(&hls).as_deref(), Some("https://a.example/m.m3u8"));
        }
    }
}
