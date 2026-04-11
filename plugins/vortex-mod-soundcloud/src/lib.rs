//! Vortex SoundCloud WASM plugin.
//!
//! Implements the CrawlerModule contract expected by the Vortex plugin host:
//! - `can_handle(url)` → `"true"` / `"false"`
//! - `supports_playlist(url)` → `"true"` / `"false"`
//! - `extract_links(url)` → JSON string describing the resolved media
//! - `extract_playlist(url)` → JSON string with flat playlist entries
//!
//! The plugin delegates all network access to the host via `http_request`.
//! Pure parsing / URL-matching logic lives in sibling modules so that it
//! can be unit-tested natively.

pub mod api;
pub mod error;
pub mod url_matcher;

// The `plugin_api` module exports `#[plugin_fn]`-decorated functions and the
// host-function imports. It is only compiled when targeting WASM, because
// `extism-pdk`'s macros emit code that is not valid for native builds.
#[cfg(target_family = "wasm")]
mod plugin_api;

use serde::Serialize;

use crate::api::{Playlist as ApiPlaylist, ResolveResponse, Track};
use crate::error::PluginError;
use crate::url_matcher::UrlKind;

// ── IPC DTOs ──────────────────────────────────────────────────────────────────

/// Returned by `extract_links` — describes the resolved media resource.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct ExtractLinksResponse {
    pub kind: &'static str,
    pub tracks: Vec<MediaLink>,
}

/// A single resolved SoundCloud track entry.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct MediaLink {
    pub id: String,
    pub title: String,
    pub url: String,
    pub artist: Option<String>,
    pub duration_ms: Option<u64>,
    pub artwork_url: Option<String>,
}

// ── Pure business logic (native-testable) ────────────────────────────────────

/// Returns `"true"` if the URL is any form of recognised SoundCloud resource.
///
/// Uses [`url_matcher::classify_url`] directly rather than
/// [`url_matcher::is_soundcloud_url`] so that the routing contract stays in
/// sync with the `extract_*` handlers: adding a new [`UrlKind`] variant
/// later will force an explicit decision here instead of silently
/// accepting it.
pub fn handle_can_handle(url: &str) -> String {
    // Artist profiles are *not* reported as handleable yet because
    // `extract_playlist` currently returns `UnsupportedUrl` for
    // `ResolveResponse::User` — advertising support would produce a
    // false-positive capability detection and a runtime failure.
    // Re-enable `UrlKind::Artist` here once artist pagination is wired.
    let kind = url_matcher::classify_url(url);
    bool_to_string(matches!(kind, UrlKind::Track | UrlKind::Playlist))
}

/// Returns `"true"` only if the URL refers to an explicit playlist /
/// set / likes / tracks / albums collection. Artist profiles are
/// intentionally excluded until artist pagination ships.
pub fn handle_supports_playlist(url: &str) -> String {
    let kind = url_matcher::classify_url(url);
    bool_to_string(matches!(kind, UrlKind::Playlist))
}

fn bool_to_string(b: bool) -> String {
    if b {
        "true".into()
    } else {
        "false".into()
    }
}

/// Reject URLs that are not a supported SoundCloud resource.
///
/// Artist profiles (`UrlKind::Artist`) are not accepted here until the
/// follow-up `/users/<id>/tracks` pagination is implemented — accepting
/// them would make `extract_links` fail with `UnsupportedUrl` *after*
/// the routing contract claimed to handle the URL, which is worse than
/// rejecting early.
pub fn ensure_soundcloud_url(url: &str) -> Result<UrlKind, PluginError> {
    let kind = url_matcher::classify_url(url);
    match kind {
        UrlKind::Track | UrlKind::Playlist => Ok(kind),
        UrlKind::Artist | UrlKind::Unknown => Err(PluginError::UnsupportedUrl(url.to_string())),
    }
}

pub fn ensure_track(url: &str) -> Result<(), PluginError> {
    match url_matcher::classify_url(url) {
        UrlKind::Track => Ok(()),
        _ => Err(PluginError::UnsupportedUrl(url.to_string())),
    }
}

pub fn ensure_playlist(url: &str) -> Result<(), PluginError> {
    match url_matcher::classify_url(url) {
        UrlKind::Playlist => Ok(()),
        _ => Err(PluginError::UnsupportedUrl(url.to_string())),
    }
}

/// Convert an API [`Track`] into a [`MediaLink`] with the artwork
/// upgraded from the default 100×100 thumbnail to `t500x500` if possible.
pub fn track_to_link(track: Track) -> MediaLink {
    MediaLink {
        id: track.id.to_string(),
        title: track.title,
        url: track.permalink_url.unwrap_or_default(),
        artist: track.user.map(|u| u.username),
        duration_ms: track.duration,
        artwork_url: track.artwork_url.map(upgrade_artwork),
    }
}

/// SoundCloud returns small (100×100) artwork URLs by default. The CDN
/// serves higher resolutions when the `-large` marker is replaced with
/// `-t500x500`. Two known URL shapes must be handled:
///
/// - `…/artworks-000-large.jpg` — standard, has a file extension
/// - `…/artworks-000-large` — animated / extensionless variant served
///   by some API responses
///
/// A plain `url.replace("-large", "-t500x500")` would also trigger on
/// `-larger` or `-largest`, which SoundCloud does not use but a future
/// CDN shape might. Guard with a word-boundary check (end-of-string or
/// a `.`, `/`, `?`) so only true `-large` markers are upgraded.
fn upgrade_artwork(url: String) -> String {
    if let Some(idx) = url.find("-large") {
        let after = url
            .as_bytes()
            .get(idx + "-large".len())
            .copied()
            .unwrap_or(0);
        let boundary = matches!(after, 0 | b'.' | b'/' | b'?' | b'#');
        if boundary {
            return format!("{}-t500x500{}", &url[..idx], &url[idx + "-large".len()..]);
        }
    }
    url
}

pub fn build_single_track_response(track: Track) -> ExtractLinksResponse {
    ExtractLinksResponse {
        kind: "track",
        tracks: vec![track_to_link(track)],
    }
}

pub fn build_playlist_response(playlist: ApiPlaylist) -> ExtractLinksResponse {
    ExtractLinksResponse {
        kind: "playlist",
        tracks: playlist.tracks.into_iter().map(track_to_link).collect(),
    }
}

/// Map a resolved response to an [`ExtractLinksResponse`].
///
/// Returns an error for `User` responses because turning an artist
/// profile into a track list requires a second API call (`/users/<id>/tracks`)
/// which the plugin currently delegates to a dedicated code path — the
/// top-level `plugin_api::extract_playlist` handler issues that request.
/// `Unknown` kinds are rejected so that callers get a clear error.
pub fn response_to_extract_links(
    resolved: ResolveResponse,
) -> Result<ExtractLinksResponse, PluginError> {
    match resolved {
        ResolveResponse::Track(t) => Ok(build_single_track_response(t)),
        ResolveResponse::Playlist(p) => Ok(build_playlist_response(p)),
        ResolveResponse::User(u) => Err(PluginError::UnsupportedUrl(format!(
            "artist profile '{}' — call extract_playlist for the track listing",
            u.username
        ))),
        ResolveResponse::Unknown => Err(PluginError::UnsupportedUrl(
            "unknown SoundCloud resource kind".into(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{Track, TrackUser};

    fn sample_track() -> Track {
        Track {
            id: 1,
            title: "Flickermood".into(),
            duration: Some(225_000),
            permalink_url: Some("https://soundcloud.com/forss/flickermood".into()),
            artwork_url: Some("https://i1.sndcdn.com/artworks-12345-large.jpg".into()),
            user: Some(TrackUser {
                username: "Forss".into(),
            }),
            streamable: Some(true),
        }
    }

    #[test]
    fn can_handle_recognises_track() {
        assert_eq!(
            handle_can_handle("https://soundcloud.com/forss/flickermood"),
            "true"
        );
    }

    #[test]
    fn can_handle_rejects_unrelated_host() {
        assert_eq!(handle_can_handle("https://example.com/"), "false");
    }

    #[test]
    fn can_handle_rejects_artist_profile_until_pagination_lands() {
        // Artist profiles are intentionally excluded — extracting them
        // requires a second `/users/<id>/tracks` pagination call which
        // is not implemented yet, so advertising support would produce
        // a false-positive followed by a runtime error.
        assert_eq!(handle_can_handle("https://soundcloud.com/forss"), "false");
    }

    #[test]
    fn can_handle_accepts_on_short_link() {
        assert_eq!(
            handle_can_handle("https://on.soundcloud.com/AbCdEfGhIj"),
            "true"
        );
    }

    #[test]
    fn supports_playlist_true_for_sets() {
        assert_eq!(
            handle_supports_playlist("https://soundcloud.com/forss/sets/soulhack"),
            "true"
        );
    }

    #[test]
    fn supports_playlist_false_for_single_track() {
        assert_eq!(
            handle_supports_playlist("https://soundcloud.com/forss/flickermood"),
            "false"
        );
    }

    #[test]
    fn supports_playlist_false_for_artist_profile() {
        assert_eq!(
            handle_supports_playlist("https://soundcloud.com/forss"),
            "false"
        );
    }

    #[test]
    fn ensure_soundcloud_url_rejects_artist_profile() {
        let err = ensure_soundcloud_url("https://soundcloud.com/forss").unwrap_err();
        assert!(matches!(err, PluginError::UnsupportedUrl(_)));
    }

    #[test]
    fn track_to_link_upgrades_artwork() {
        let link = track_to_link(sample_track());
        assert_eq!(link.id, "1");
        assert_eq!(link.title, "Flickermood");
        assert_eq!(link.artist.as_deref(), Some("Forss"));
        assert_eq!(link.duration_ms, Some(225_000));
        assert_eq!(
            link.artwork_url.as_deref(),
            Some("https://i1.sndcdn.com/artworks-12345-t500x500.jpg"),
            "large artwork marker should be upgraded to t500x500"
        );
    }

    #[test]
    fn track_to_link_preserves_non_large_artwork() {
        let mut t = sample_track();
        t.artwork_url = Some("https://i1.sndcdn.com/artworks-12345-t500x500.jpg".into());
        let link = track_to_link(t);
        assert_eq!(
            link.artwork_url.as_deref(),
            Some("https://i1.sndcdn.com/artworks-12345-t500x500.jpg")
        );
    }

    #[test]
    fn track_to_link_upgrades_artwork_without_extension() {
        let mut t = sample_track();
        t.artwork_url = Some("https://i1.sndcdn.com/artworks-12345-large".into());
        let link = track_to_link(t);
        assert_eq!(
            link.artwork_url.as_deref(),
            Some("https://i1.sndcdn.com/artworks-12345-t500x500"),
            "extensionless -large should also be upgraded"
        );
    }

    #[test]
    fn track_to_link_upgrades_artwork_with_query_string() {
        let mut t = sample_track();
        t.artwork_url = Some("https://i1.sndcdn.com/artworks-12345-large?v=2".into());
        let link = track_to_link(t);
        assert_eq!(
            link.artwork_url.as_deref(),
            Some("https://i1.sndcdn.com/artworks-12345-t500x500?v=2"),
            "query string boundary should still trigger upgrade"
        );
    }

    #[test]
    fn track_to_link_does_not_upgrade_larger_or_largest() {
        let mut t = sample_track();
        t.artwork_url = Some("https://i1.sndcdn.com/artworks-larger.jpg".into());
        let link = track_to_link(t);
        assert_eq!(
            link.artwork_url.as_deref(),
            Some("https://i1.sndcdn.com/artworks-larger.jpg"),
            "-larger must not trigger the word-boundary upgrade"
        );
    }

    #[test]
    fn build_single_track_response_shape() {
        let r = build_single_track_response(sample_track());
        assert_eq!(r.kind, "track");
        assert_eq!(r.tracks.len(), 1);
    }

    #[test]
    fn build_playlist_response_shape() {
        let playlist = ApiPlaylist {
            id: 42,
            title: "Soulhack".into(),
            permalink_url: Some("https://soundcloud.com/forss/sets/soulhack".into()),
            artwork_url: None,
            tracks: vec![sample_track(), sample_track()],
            track_count: Some(2),
        };
        let r = build_playlist_response(playlist);
        assert_eq!(r.kind, "playlist");
        assert_eq!(r.tracks.len(), 2);
    }

    #[test]
    fn ensure_soundcloud_url_rejects_unknown() {
        let err = ensure_soundcloud_url("https://example.com/").unwrap_err();
        assert!(matches!(err, PluginError::UnsupportedUrl(_)));
    }

    #[test]
    fn response_to_extract_links_track_ok() {
        let resp = response_to_extract_links(ResolveResponse::Track(sample_track())).unwrap();
        assert_eq!(resp.kind, "track");
    }

    #[test]
    fn response_to_extract_links_user_routes_to_extract_playlist() {
        let err = response_to_extract_links(ResolveResponse::User(crate::api::User {
            id: 1,
            username: "forss".into(),
            permalink_url: None,
            avatar_url: None,
        }))
        .unwrap_err();
        assert!(matches!(err, PluginError::UnsupportedUrl(_)));
    }

    #[test]
    fn response_to_extract_links_unknown_rejected() {
        let err = response_to_extract_links(ResolveResponse::Unknown).unwrap_err();
        assert!(matches!(err, PluginError::UnsupportedUrl(_)));
    }

    #[test]
    fn json_serialisation_of_extract_links_response() {
        let resp = build_single_track_response(sample_track());
        let json = serde_json::to_string(&resp).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["kind"], "track");
        assert_eq!(parsed["tracks"][0]["title"], "Flickermood");
        assert_eq!(parsed["tracks"][0]["artist"], "Forss");
    }
}
