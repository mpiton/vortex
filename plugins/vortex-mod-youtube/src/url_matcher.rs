//! YouTube URL detection and classification.
//!
//! Pure logic, no WASM or subprocess required — unit-testable natively.
//!
//! ## Design
//!
//! URL matching is a two-step process:
//!
//! 1. **Host validation** ([`validate_and_split`]) — parses the URL, strips
//!    userinfo and port, lowercases the host, and checks it against an
//!    explicit allowlist ([`is_youtube_host_string`]). This single chokepoint
//!    blocks substring smuggling (`example.com/?next=youtube.com/…`) and
//!    gracefully handles `user:pass@host:port` authorities.
//!
//! 2. **Path-based regex matching** — the remaining path+query is matched
//!    against compiled regexes that only care about the *path*, not the host.
//!    This means `youtube.com`, `www.youtube.com`, `m.youtube.com`,
//!    `music.youtube.com`, `youtube-nocookie.com`, and `youtu.be` all share
//!    the same regex set, and port/userinfo variations cannot break the
//!    pattern match because they have already been stripped by step 1.
//!
//! YouTube video and playlist IDs are case-sensitive (`dQw4w9WgXcQ` differs
//! from `dqw4w9wgxcq`), so the path is passed to the regexes **with case
//! preserved**. Textual keywords (`watch`, `shorts`, `playlist`, `channel`,
//! `user`, `c`) use the `(?i)` inline flag to stay case-insensitive.

use std::sync::OnceLock;

use regex::Regex;

/// Kind of YouTube resource identified from a URL.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UrlKind {
    /// Single video: `youtube.com/watch?v=...` or `youtu.be/...`
    Video,
    /// Short-form video: `youtube.com/shorts/...`
    Shorts,
    /// Playlist: `youtube.com/playlist?list=...`
    Playlist,
    /// Channel or user page: `youtube.com/@handle`, `/channel/`, `/user/`, `/c/`
    Channel,
    /// Not a recognised YouTube URL.
    Unknown,
}

/// Returns `true` if the URL is any form of recognised YouTube resource.
pub fn is_youtube_url(url: &str) -> bool {
    !matches!(classify_url(url), UrlKind::Unknown)
}

/// Classify the URL into a [`UrlKind`].
///
/// Recognises all standard YouTube hosts (youtube.com, www.youtube.com,
/// m.youtube.com, music.youtube.com, youtube-nocookie.com, youtu.be) and
/// common path patterns. Handles URLs with explicit ports (`:443`) and
/// userinfo (`user:pass@`) without breaking pattern matching.
pub fn classify_url(url: &str) -> UrlKind {
    let Some((host_lower, path_and_query)) = validate_and_split(url) else {
        return UrlKind::Unknown;
    };

    // youtu.be short links: the path itself is the video id.
    if host_lower == "youtu.be" {
        return if extract_youtu_be_id(path_and_query).is_some() {
            UrlKind::Video
        } else {
            UrlKind::Unknown
        };
    }

    // youtube.com family: discriminate by path. Order matters — /shorts/ and
    // /playlist must be checked before the generic /watch pattern so that
    // mixed URLs (e.g. /watch?v=X&list=Y) resolve to Video, not Playlist,
    // but dedicated /playlist URLs resolve to Playlist.
    if shorts_id_regex().is_match(path_and_query) {
        return UrlKind::Shorts;
    }
    if playlist_id_regex().is_match(path_and_query) {
        return UrlKind::Playlist;
    }
    if watch_id_regex().is_match(path_and_query) {
        return UrlKind::Video;
    }
    if channel_path_regex().is_match(path_and_query) {
        return UrlKind::Channel;
    }

    UrlKind::Unknown
}

/// Extract the video id from a `watch?v=...`, `youtu.be/...`, or
/// `shorts/...` URL.
///
/// Returns `None` if the URL has no video id or is not hosted on a recognised
/// YouTube domain. The id is returned in its original case.
pub fn extract_video_id(url: &str) -> Option<String> {
    let (host_lower, path_and_query) = validate_and_split(url)?;

    if host_lower == "youtu.be" {
        return extract_youtu_be_id(path_and_query).map(String::from);
    }

    if let Some(caps) = watch_id_regex().captures(path_and_query) {
        return caps.get(1).map(|m| m.as_str().to_string());
    }

    if let Some(caps) = shorts_id_regex().captures(path_and_query) {
        return caps.get(1).map(|m| m.as_str().to_string());
    }

    None
}

/// Extract the playlist id from a `playlist?list=...` URL.
///
/// Host-validated and stripped of userinfo/port, just like
/// [`extract_video_id`]. The id is returned in its original case.
pub fn extract_playlist_id(url: &str) -> Option<String> {
    let (_host_lower, path_and_query) = validate_and_split(url)?;
    playlist_id_regex()
        .captures(path_and_query)
        .and_then(|c| c.get(1).map(|m| m.as_str().to_string()))
}

// ── Internals ─────────────────────────────────────────────────────────────────

/// Parse a URL, validate its host against the YouTube allowlist, and split
/// off the path+query component.
///
/// Returns `Some((lowercased_host, path_and_query))` on success. The path is
/// returned as a borrow of the trimmed input so that case is preserved —
/// video/playlist IDs must round-trip exactly.
///
/// Returns `None` if the URL lacks a scheme separator, has no authority,
/// or the authority's host is not in [`is_youtube_host_string`]. This is
/// the single chokepoint for host validation — every public function in
/// this module routes through it.
fn validate_and_split(url: &str) -> Option<(String, &str)> {
    let trimmed = url.trim();
    let (_scheme, after_scheme) = trimmed.split_once("://")?;
    if after_scheme.is_empty() {
        return None;
    }

    let path_start = after_scheme
        .find(['/', '?', '#'])
        .unwrap_or(after_scheme.len());
    let (authority, path_and_rest) = after_scheme.split_at(path_start);

    // Strip `user:pass@` userinfo if present.
    let host_port = authority
        .rsplit_once('@')
        .map(|(_, rest)| rest)
        .unwrap_or(authority);
    // Strip `:port` suffix if present. IPv6 addresses are bracketed, so a
    // plain `rsplit_once(':')` is enough for YouTube hosts (never IPv6).
    let host = host_port
        .rsplit_once(':')
        .map(|(h, _)| h)
        .unwrap_or(host_port);

    if host.is_empty() {
        return None;
    }

    let host_lower = host.to_ascii_lowercase();
    if !is_youtube_host_string(&host_lower) {
        return None;
    }

    Some((host_lower, path_and_rest))
}

/// Exact-match the (already lowercased) host against recognised YouTube
/// authorities. Substring matching is deliberately avoided — see the
/// SSRF-style concern where `example.com/?next=youtube.com/...` would
/// otherwise be accepted.
fn is_youtube_host_string(host_lower: &str) -> bool {
    matches!(
        host_lower,
        "youtube.com"
            | "www.youtube.com"
            | "m.youtube.com"
            | "music.youtube.com"
            | "youtube-nocookie.com"
            | "www.youtube-nocookie.com"
            | "youtu.be"
    )
}

/// Extract the video id from a `youtu.be` path (`/VIDEO_ID` or
/// `/VIDEO_ID?query`). Returns the slice of `path_and_query` that contains
/// the id — preserving case — on success.
fn extract_youtu_be_id(path_and_query: &str) -> Option<&str> {
    let after_slash = path_and_query.strip_prefix('/')?;
    let id_end = after_slash
        .find(['/', '?', '#'])
        .unwrap_or(after_slash.len());
    let id = &after_slash[..id_end];
    if is_valid_video_id(id) {
        Some(id)
    } else {
        None
    }
}

/// A plausible YouTube id: at least 6 characters from the base64url
/// alphabet minus padding. Real YouTube ids are exactly 11 characters but
/// we stay lenient for forward compatibility.
fn is_valid_video_id(id: &str) -> bool {
    id.len() >= 6
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

// ── Cached path-based regexes ─────────────────────────────────────────────────
//
// These operate on the path+query slice returned by `validate_and_split`,
// not on full URLs. The `(?i)` flag makes the textual keywords
// (`watch`, `shorts`, `playlist`, `channel`, `user`, `c`) case-insensitive
// while preserving the case of the captured id.

fn watch_id_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)^/watch\?(?:[^&#]*&)*v=([A-Za-z0-9_-]{6,})").unwrap()
    })
}

fn shorts_id_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)^/shorts/([A-Za-z0-9_-]{6,})").unwrap())
}

fn playlist_id_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)^/playlist\?(?:[^&#]*&)*list=([A-Za-z0-9_-]+)").unwrap()
    })
}

fn channel_path_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?i)^/(?:@[A-Za-z0-9_.-]+|channel/[A-Za-z0-9_-]+|user/[A-Za-z0-9_-]+|c/[A-Za-z0-9_-]+)",
        )
        .unwrap()
    })
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    #[case("https://www.youtube.com/watch?v=dQw4w9WgXcQ")]
    #[case("https://youtube.com/watch?v=dQw4w9WgXcQ")]
    #[case("https://m.youtube.com/watch?v=dQw4w9WgXcQ")]
    #[case("https://music.youtube.com/watch?v=dQw4w9WgXcQ")]
    #[case("https://youtu.be/dQw4w9WgXcQ")]
    #[case("https://youtube.com/shorts/abcDEF12345")]
    #[case("https://www.youtube.com/playlist?list=PLxxxxxx")]
    #[case("https://www.youtube.com/@MrBeast")]
    #[case("https://www.youtube.com/channel/UC_x5XG1OV2P6uZZ5FSM9Ttw")]
    // youtube-nocookie — both the bare domain and the www. variant
    #[case("https://www.youtube-nocookie.com/watch?v=dQw4w9WgXcQ")]
    #[case("https://youtube-nocookie.com/watch?v=dQw4w9WgXcQ")]
    fn detects_valid_youtube_urls(#[case] url: &str) {
        assert!(is_youtube_url(url), "expected YouTube URL: {url}");
    }

    #[rstest]
    #[case("https://example.com/watch?v=dQw4w9WgXcQ")]
    #[case("https://vimeo.com/12345")]
    #[case("not a url")]
    #[case("")]
    #[case("https://fakeyoutube.com/watch?v=abcdef")]
    // Reject query-string and fragment smuggling — the host parser must look
    // at the real authority, not a substring of the whole URL.
    #[case("https://example.com/?next=https://youtube.com/watch?v=x")]
    #[case("https://example.com/#youtube.com/watch?v=x")]
    #[case("https://evil.com/youtube.com/watch?v=x")]
    #[case("https://youtube.com.evil.com/watch?v=x")]
    fn rejects_non_youtube_urls(#[case] url: &str) {
        assert!(!is_youtube_url(url), "expected non-YouTube URL: {url}");
    }

    #[test]
    fn accepts_host_with_port() {
        assert!(is_youtube_url(
            "https://www.youtube.com:443/watch?v=dQw4w9WgXcQ"
        ));
    }

    #[test]
    fn accepts_host_with_userinfo() {
        assert!(is_youtube_url(
            "https://user:pass@www.youtube.com/watch?v=dQw4w9WgXcQ"
        ));
    }

    #[test]
    fn accepts_trailing_whitespace_in_extract_video_id() {
        // The user pastes with a trailing newline — extraction should still work.
        assert_eq!(
            extract_video_id("  https://www.youtube.com/watch?v=dQw4w9WgXcQ\n"),
            Some("dQw4w9WgXcQ".to_string())
        );
    }

    #[test]
    fn extract_video_id_rejects_non_youtube_host() {
        // Even if the URL looks like a YouTube path, the host must match.
        assert_eq!(
            extract_video_id("https://evil.com/watch?v=dQw4w9WgXcQ"),
            None
        );
    }

    #[test]
    fn extract_playlist_id_rejects_non_youtube_host() {
        assert_eq!(
            extract_playlist_id("https://evil.com/playlist?list=PLxyz"),
            None
        );
    }

    #[test]
    fn classifies_watch_as_video() {
        assert_eq!(
            classify_url("https://www.youtube.com/watch?v=dQw4w9WgXcQ"),
            UrlKind::Video
        );
    }

    #[test]
    fn classifies_youtu_be_as_video() {
        assert_eq!(classify_url("https://youtu.be/dQw4w9WgXcQ"), UrlKind::Video);
    }

    #[test]
    fn classifies_shorts_as_shorts() {
        assert_eq!(
            classify_url("https://www.youtube.com/shorts/abcDEF12345"),
            UrlKind::Shorts
        );
    }

    #[test]
    fn classifies_playlist_as_playlist() {
        assert_eq!(
            classify_url("https://www.youtube.com/playlist?list=PLxyz123"),
            UrlKind::Playlist
        );
    }

    #[test]
    fn classifies_channel_handle_as_channel() {
        assert_eq!(
            classify_url("https://www.youtube.com/@MrBeast"),
            UrlKind::Channel
        );
    }

    #[test]
    fn classifies_channel_id_as_channel() {
        assert_eq!(
            classify_url("https://www.youtube.com/channel/UC_x5XG1OV2P6uZZ5FSM9Ttw"),
            UrlKind::Channel
        );
    }

    #[test]
    fn classifies_unknown_for_non_youtube() {
        assert_eq!(classify_url("https://vimeo.com/12345"), UrlKind::Unknown);
    }

    // ── youtube-nocookie.com — full extraction coverage ──────────────────────

    #[test]
    fn classifies_youtube_nocookie_watch_as_video() {
        assert_eq!(
            classify_url("https://www.youtube-nocookie.com/watch?v=dQw4w9WgXcQ"),
            UrlKind::Video
        );
    }

    #[test]
    fn extracts_video_id_from_youtube_nocookie() {
        assert_eq!(
            extract_video_id("https://www.youtube-nocookie.com/watch?v=dQw4w9WgXcQ"),
            Some("dQw4w9WgXcQ".to_string())
        );
    }

    // ── Port / userinfo consistency between classify and extract ─────────────

    #[test]
    fn extracts_video_id_from_url_with_port() {
        assert_eq!(
            extract_video_id("https://www.youtube.com:443/watch?v=dQw4w9WgXcQ"),
            Some("dQw4w9WgXcQ".to_string())
        );
    }

    #[test]
    fn extracts_video_id_from_url_with_userinfo() {
        assert_eq!(
            extract_video_id("https://user:pass@www.youtube.com/watch?v=dQw4w9WgXcQ"),
            Some("dQw4w9WgXcQ".to_string())
        );
    }

    #[test]
    fn extracts_playlist_id_from_url_with_port() {
        assert_eq!(
            extract_playlist_id("https://www.youtube.com:443/playlist?list=PLxyz123"),
            Some("PLxyz123".to_string())
        );
    }

    // ── Case sensitivity: mixed-case path keywords ───────────────────────────

    #[test]
    fn accepts_mixed_case_watch_keyword() {
        assert_eq!(
            classify_url("https://www.youtube.com/WATCH?v=dQw4w9WgXcQ"),
            UrlKind::Video
        );
    }

    #[test]
    fn preserves_case_of_video_id() {
        // The id is case-sensitive — a lowercased result would be a
        // different video on YouTube.
        assert_eq!(
            extract_video_id("https://www.youtube.com/watch?v=dQw4w9WgXcQ"),
            Some("dQw4w9WgXcQ".to_string())
        );
    }

    // ── Existing extraction tests ────────────────────────────────────────────

    #[test]
    fn extracts_video_id_from_watch_url() {
        assert_eq!(
            extract_video_id("https://www.youtube.com/watch?v=dQw4w9WgXcQ"),
            Some("dQw4w9WgXcQ".to_string())
        );
    }

    #[test]
    fn extracts_video_id_from_watch_url_with_extra_params() {
        assert_eq!(
            extract_video_id("https://www.youtube.com/watch?feature=share&v=dQw4w9WgXcQ&t=5"),
            Some("dQw4w9WgXcQ".to_string())
        );
    }

    #[test]
    fn extracts_video_id_from_youtu_be() {
        assert_eq!(
            extract_video_id("https://youtu.be/dQw4w9WgXcQ"),
            Some("dQw4w9WgXcQ".to_string())
        );
    }

    #[test]
    fn extracts_video_id_from_shorts() {
        assert_eq!(
            extract_video_id("https://www.youtube.com/shorts/abcDEF12345"),
            Some("abcDEF12345".to_string())
        );
    }

    #[test]
    fn extracts_playlist_id() {
        assert_eq!(
            extract_playlist_id("https://www.youtube.com/playlist?list=PLxyz123"),
            Some("PLxyz123".to_string())
        );
    }

    #[test]
    fn returns_none_for_url_without_video_id() {
        assert_eq!(extract_video_id("https://www.youtube.com/@channel"), None);
    }
}
