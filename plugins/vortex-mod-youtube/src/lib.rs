//! Vortex YouTube WASM plugin.
//!
//! Implements the CrawlerModule contract expected by the Vortex plugin host:
//! - `can_handle(url)` → `"true"` / `"false"`
//! - `supports_playlist(url)` → `"true"` / `"false"`
//! - `extract_links(url)` → JSON string describing the resolved media
//! - `get_media_variants(url)` → JSON string listing available formats
//! - `extract_playlist(url)` → JSON string with flat playlist entries
//!
//! Inputs are UTF-8 strings (the URL). Outputs are UTF-8 strings that the
//! host passes through `plugin.call::<&str, &str>(func, input)`.

pub mod error;
pub mod extractor;
pub mod metadata;
pub mod quality_manager;
pub mod url_matcher;

#[cfg(test)]
mod ipc_tests;

// The `plugin_api` module exports `#[plugin_fn]`-decorated functions and the
// host-function imports. It is only compiled when targeting WASM, because
// `extism-pdk`'s macros emit code that is not valid for native builds.
#[cfg(target_family = "wasm")]
mod plugin_api;

use serde::Serialize;

use crate::error::PluginError;
use crate::metadata::{FormatKind, Playlist, VideoInfo};
use crate::url_matcher::UrlKind;

// ── IPC DTOs ──────────────────────────────────────────────────────────────────

/// Returned by `extract_links` — describes the resolved media resource.
#[derive(Debug, Serialize)]
pub struct ExtractLinksResponse {
    pub kind: &'static str,
    pub videos: Vec<MediaLink>,
}

/// A single resolved media link.
#[derive(Debug, Serialize)]
pub struct MediaLink {
    pub id: String,
    pub title: String,
    pub url: String,
    pub duration: Option<u64>,
    pub uploader: Option<String>,
    pub thumbnail: Option<String>,
}

/// Returned by `get_media_variants` — describes selectable formats.
#[derive(Debug, Serialize)]
pub struct MediaVariantsResponse {
    pub variants: Vec<MediaVariant>,
}

/// A single selectable format variant exposed to the UI.
#[derive(Debug, Serialize)]
pub struct MediaVariant {
    pub format_id: String,
    pub kind: FormatKind,
    pub ext: String,
    pub height: Option<u32>,
    pub fps: Option<f64>,
    pub vcodec: Option<String>,
    pub acodec: Option<String>,
    pub abr: Option<f64>,
    pub filesize: Option<u64>,
}

// ── Pure business logic (native-testable) ────────────────────────────────────

/// Returns `"true"` if the URL is one of the kinds this plugin can actually
/// route to an extraction function.
///
/// Uses [`url_matcher::classify_url`] directly rather than
/// [`url_matcher::is_youtube_url`] so that the routing contract stays in sync
/// with the `extract_*` / `get_media_variants` handlers: adding a new
/// [`UrlKind`] variant later will force an explicit decision here instead of
/// silently accepting it.
pub fn handle_can_handle(url: &str) -> String {
    let kind = url_matcher::classify_url(url);
    bool_to_string(matches!(
        kind,
        UrlKind::Video | UrlKind::Shorts | UrlKind::Playlist | UrlKind::Channel
    ))
}

/// Returns `"true"` if the URL refers to a playlist or channel.
pub fn handle_supports_playlist(url: &str) -> String {
    let kind = url_matcher::classify_url(url);
    bool_to_string(matches!(kind, UrlKind::Playlist | UrlKind::Channel))
}

fn bool_to_string(b: bool) -> String {
    if b {
        "true".into()
    } else {
        "false".into()
    }
}

/// Build the [`ExtractLinksResponse`] from a parsed single video.
pub fn build_single_video_response(video: VideoInfo) -> ExtractLinksResponse {
    let link = MediaLink {
        id: video.id,
        title: video.title,
        url: video.webpage_url,
        duration: video.duration,
        uploader: video.uploader,
        thumbnail: video.thumbnail,
    };
    ExtractLinksResponse {
        kind: "video",
        videos: vec![link],
    }
}

/// Build the [`ExtractLinksResponse`] from a parsed playlist.
pub fn build_playlist_response(playlist: Playlist) -> ExtractLinksResponse {
    let videos = playlist
        .entries
        .into_iter()
        .map(|entry| MediaLink {
            id: entry.id,
            title: entry.title.unwrap_or_default(),
            url: entry.url,
            duration: entry.duration,
            uploader: None,
            thumbnail: entry.thumbnail,
        })
        .collect();
    ExtractLinksResponse {
        kind: "playlist",
        videos,
    }
}

/// Build the [`MediaVariantsResponse`] from a parsed single video.
pub fn build_media_variants_response(video: VideoInfo) -> MediaVariantsResponse {
    let variants = video
        .formats
        .into_iter()
        .map(|f| MediaVariant {
            format_id: f.format_id,
            kind: f.kind,
            ext: f.ext,
            height: f.height,
            fps: f.fps,
            vcodec: f.vcodec,
            acodec: f.acodec,
            abr: f.abr,
            filesize: f.filesize,
        })
        .collect();
    MediaVariantsResponse { variants }
}

/// Reject URLs that do not map to a supported [`UrlKind`] before spending a
/// subprocess call on them. Returns the classified kind on success so that
/// callers can dispatch on it without re-running [`url_matcher::classify_url`].
pub fn ensure_youtube_url(url: &str) -> Result<UrlKind, PluginError> {
    let kind = url_matcher::classify_url(url);
    match kind {
        UrlKind::Video | UrlKind::Shorts | UrlKind::Playlist | UrlKind::Channel => Ok(kind),
        UrlKind::Unknown => Err(PluginError::UnsupportedUrl(url.to_string())),
    }
}

/// Reject URLs that are not a single-video resource (video or short).
/// Used as a pre-check by `get_media_variants`.
pub fn ensure_single_video(url: &str) -> Result<UrlKind, PluginError> {
    let kind = url_matcher::classify_url(url);
    match kind {
        UrlKind::Video | UrlKind::Shorts => Ok(kind),
        _ => Err(PluginError::UnsupportedUrl(url.to_string())),
    }
}

/// Reject URLs that are not a playlist or channel resource.
/// Used as a pre-check by `extract_playlist`.
pub fn ensure_playlist_or_channel(url: &str) -> Result<UrlKind, PluginError> {
    let kind = url_matcher::classify_url(url);
    match kind {
        UrlKind::Playlist | UrlKind::Channel => Ok(kind),
        _ => Err(PluginError::UnsupportedUrl(url.to_string())),
    }
}
