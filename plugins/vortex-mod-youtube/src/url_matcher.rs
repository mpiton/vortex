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
/// music.youtube.com, www.youtube.com) and common path patterns.
pub fn classify_url(url: &str) -> UrlKind {
    let lowered = url.trim().to_ascii_lowercase();

    if !has_youtube_host(&lowered) {
        return UrlKind::Unknown;
    }

    if short_host_regex().is_match(&lowered) {
        return UrlKind::Video;
    }

    if shorts_path_regex().is_match(&lowered) {
        return UrlKind::Shorts;
    }

    if playlist_path_regex().is_match(&lowered) {
        return UrlKind::Playlist;
    }

    if watch_path_regex().is_match(&lowered) {
        return UrlKind::Video;
    }

    if channel_path_regex().is_match(&lowered) {
        return UrlKind::Channel;
    }

    UrlKind::Unknown
}

/// Extract the video id from a `watch?v=...`, `youtu.be/...`, or `shorts/...` URL.
///
/// Returns `None` if the URL has no video id. Note that (unlike
/// [`classify_url`]) this function is case-sensitive on the id itself —
/// YouTube ids preserve case.
pub fn extract_video_id(url: &str) -> Option<String> {
    let trimmed = url.trim();

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
pub fn extract_playlist_id(url: &str) -> Option<String> {
    playlist_id_regex()
        .captures(url)
        .and_then(|c| c.get(1).map(|m| m.as_str().to_string()))
}

// ── Host detection ────────────────────────────────────────────────────────────

fn has_youtube_host(lowered: &str) -> bool {
    const HOSTS: &[&str] = &[
        "://youtube.com/",
        "://www.youtube.com/",
        "://m.youtube.com/",
        "://music.youtube.com/",
        "://youtu.be/",
    ];
    HOSTS.iter().any(|h| lowered.contains(h))
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
    fn rejects_non_youtube_urls(#[case] url: &str) {
        assert!(!is_youtube_url(url), "expected non-YouTube URL: {url}");
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
