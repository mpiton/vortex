//! YouTube URL detection and classification.
//!
//! Pure logic, no WASM or subprocess required — unit-testable natively.

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
/// Recognises all standard YouTube hosts (youtube.com, youtu.be, m.youtube.com,
/// music.youtube.com, www.youtube.com, youtube-nocookie.com) and common path
/// patterns. The URL is first normalised so that userinfo (`user:pass@`) and
/// explicit ports (`:443`) do not break the regex matchers downstream.
pub fn classify_url(url: &str) -> UrlKind {
    let Some(normalized) = normalize_for_matching(url) else {
        return UrlKind::Unknown;
    };

    if short_host_regex().is_match(&normalized) {
        return UrlKind::Video;
    }

    if shorts_path_regex().is_match(&normalized) {
        return UrlKind::Shorts;
    }

    if playlist_path_regex().is_match(&normalized) {
        return UrlKind::Playlist;
    }

    if watch_path_regex().is_match(&normalized) {
        return UrlKind::Video;
    }

    if channel_path_regex().is_match(&normalized) {
        return UrlKind::Channel;
    }

    UrlKind::Unknown
}

/// Rebuild a YouTube URL in canonical form — lowercase scheme+host, no
/// userinfo, no port, path+query preserved — suitable for regex matching.
///
/// Returns `None` when the host is not a recognised YouTube authority or
/// when the URL has no scheme separator. This is the single chokepoint that
/// enforces host validation; all classification and id-extraction funnels
/// through it.
fn normalize_for_matching(url: &str) -> Option<String> {
    let trimmed = url.trim();
    let (scheme, after_scheme) = trimmed.split_once("://")?;
    if after_scheme.is_empty() {
        return None;
    }

    let path_start = after_scheme
        .find(['/', '?', '#'])
        .unwrap_or(after_scheme.len());
    let (authority, path_and_rest) = after_scheme.split_at(path_start);

    let host_port = authority
        .rsplit_once('@')
        .map(|(_, rest)| rest)
        .unwrap_or(authority);
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

    Some(format!(
        "{}://{}{}",
        scheme.to_ascii_lowercase(),
        host_lower,
        path_and_rest.to_ascii_lowercase()
    ))
}

/// Extract the video id from a `watch?v=...`, `youtu.be/...`, or `shorts/...` URL.
///
/// Returns `None` if the URL has no video id or is not hosted on a recognised
/// YouTube domain. The host is parsed and matched exactly — not via substring
/// search — so that a URL like `https://example.com/?next=youtube.com/watch?v=x`
/// cannot leak through. YouTube video ids preserve case, so the lookup is done
/// on the case-preserved input even though the host comparison is lowercased.
pub fn extract_video_id(url: &str) -> Option<String> {
    let trimmed = url.trim();
    let host = extract_host(trimmed)?.to_ascii_lowercase();

    if !is_youtube_host_string(&host) {
        return None;
    }

    if let Some(caps) = watch_id_regex().captures(trimmed) {
        return caps.get(1).map(|m| m.as_str().to_string());
    }

    if let Some(caps) = short_host_id_regex().captures(trimmed) {
        return caps.get(1).map(|m| m.as_str().to_string());
    }

    if let Some(caps) = shorts_id_regex().captures(trimmed) {
        return caps.get(1).map(|m| m.as_str().to_string());
    }

    None
}

/// Extract the playlist id from a `playlist?list=...` URL.
///
/// Trimmed + host-validated in the same way as [`extract_video_id`].
pub fn extract_playlist_id(url: &str) -> Option<String> {
    let trimmed = url.trim();
    let host = extract_host(trimmed)?.to_ascii_lowercase();

    if !is_youtube_host_string(&host) {
        return None;
    }

    playlist_id_regex()
        .captures(trimmed)
        .and_then(|c| c.get(1).map(|m| m.as_str().to_string()))
}

// ── Host detection ────────────────────────────────────────────────────────────

/// Parse the host component out of a URL-shaped string.
///
/// Does the minimum work needed to answer "what authority does this URL
/// refer to": splits on `://`, then on the first `/`, `?`, or `#`, strips
/// any `user:pass@` prefix, and drops an explicit port. Returns `None` if
/// the input has no scheme separator or no authority.
///
/// This deliberately avoids pulling in the `url` crate — it would add ~200 KB
/// to the WASM binary for a job that is a handful of string splits.
fn extract_host(url: &str) -> Option<&str> {
    let after_scheme = url.split_once("://")?.1;
    if after_scheme.is_empty() {
        return None;
    }
    let end = after_scheme
        .find(['/', '?', '#'])
        .unwrap_or(after_scheme.len());
    let authority = &after_scheme[..end];
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
        None
    } else {
        Some(host)
    }
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

// ── Cached regexes ────────────────────────────────────────────────────────────

fn watch_path_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"youtube\.com/watch\?.*v=[A-Za-z0-9_-]{6,}").unwrap())
}

fn watch_id_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"youtube\.com/watch\?(?:[^&]*&)*v=([A-Za-z0-9_-]{6,})").unwrap())
}

fn short_host_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"://youtu\.be/[A-Za-z0-9_-]{6,}").unwrap())
}

fn short_host_id_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"://youtu\.be/([A-Za-z0-9_-]{6,})").unwrap())
}

fn shorts_path_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"youtube\.com/shorts/[A-Za-z0-9_-]{6,}").unwrap())
}

fn shorts_id_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"youtube\.com/shorts/([A-Za-z0-9_-]{6,})").unwrap())
}

fn playlist_path_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"youtube\.com/playlist\?.*list=[A-Za-z0-9_-]+").unwrap())
}

fn playlist_id_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"youtube\.com/playlist\?(?:[^&]*&)*list=([A-Za-z0-9_-]+)").unwrap()
    })
}

fn channel_path_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"youtube\.com/(?:@[A-Za-z0-9_.-]+|channel/[A-Za-z0-9_-]+|user/[A-Za-z0-9_-]+|c/[A-Za-z0-9_-]+)")
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
