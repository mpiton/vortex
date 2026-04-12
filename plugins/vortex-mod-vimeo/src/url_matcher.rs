//! Vimeo URL detection and classification.
//!
//! ## Accepted URL shapes
//!
//! - `vimeo.com/<id>` — public video
//! - `vimeo.com/<id>/<hash>` — private video share link (hash is a token)
//! - `vimeo.com/showcase/<id>` or `vimeo.com/album/<id>` — playlist
//! - `vimeo.com/ondemand/<slug>` — rejected (paid content, out of scope)
//! - `player.vimeo.com/video/<id>` — embedded player URL
//!
//! `vimeo.com/<user>` artist profiles are rejected here because Vimeo's
//! public HTML for profiles is inconsistent; the MVP focuses on video
//! and showcase extraction.

use std::sync::OnceLock;

use regex::Regex;

/// Kind of Vimeo resource identified from a URL.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UrlKind {
    /// Single video: `vimeo.com/<id>` or `player.vimeo.com/video/<id>`
    Video,
    /// Private video share: `vimeo.com/<id>/<hash>`
    PrivateVideo,
    /// Showcase / album: `vimeo.com/showcase/<id>` or `vimeo.com/album/<id>`
    Showcase,
    /// Not a recognised Vimeo URL.
    Unknown,
}

/// Returns `true` if the URL is any form of recognised Vimeo resource.
pub fn is_vimeo_url(url: &str) -> bool {
    !matches!(classify_url(url), UrlKind::Unknown)
}

/// Classify the URL into a [`UrlKind`].
pub fn classify_url(url: &str) -> UrlKind {
    let Some((host_lower, path)) = validate_and_split(url) else {
        return UrlKind::Unknown;
    };

    if !is_vimeo_host(&host_lower) {
        return UrlKind::Unknown;
    }

    let path_only = normalize_path(path);

    // player.vimeo.com/video/<id>
    if host_lower == "player.vimeo.com" {
        return if player_video_regex().is_match(path_only) {
            UrlKind::Video
        } else {
            UrlKind::Unknown
        };
    }

    // vimeo.com family
    if showcase_or_album_regex().is_match(path_only) {
        return UrlKind::Showcase;
    }
    if private_video_regex().is_match(path_only) {
        return UrlKind::PrivateVideo;
    }
    if video_regex().is_match(path_only) {
        return UrlKind::Video;
    }
    UrlKind::Unknown
}

/// Strip query string, fragment, and trailing slash from a raw
/// path-and-query slice. `path#frag?q` (malformed but tolerated) is
/// handled by splitting on `#` first.
fn normalize_path(path: &str) -> &str {
    let no_frag = path.split('#').next().unwrap_or("");
    let no_query = no_frag.split('?').next().unwrap_or("");
    no_query.trim_end_matches('/')
}

fn is_vimeo_host(host: &str) -> bool {
    matches!(
        host,
        "vimeo.com" | "www.vimeo.com" | "player.vimeo.com" | "m.vimeo.com"
    )
}

// All four URL-classification regexes are compile-time constants:
// `.expect` documents the invariant and honours the crate-wide
// policy that production code paths must not `.unwrap()`.

fn video_regex() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r"^/(\d{6,})$").expect("video_regex: compile-time constant regex must compile")
    })
}

fn private_video_regex() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r"^/(\d{6,})/([a-f0-9]{8,})$")
            .expect("private_video_regex: compile-time constant regex must compile")
    })
}

fn showcase_or_album_regex() -> &'static Regex {
    // Fully anchored — trailing junk like `/foo/bar` after the numeric
    // ID must not match. Callers normalise query/fragment/trailing
    // slash before passing the path to this regex.
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r"^/(?:showcase|album)/(\d+)$")
            .expect("showcase_or_album_regex: compile-time constant regex must compile")
    })
}

fn player_video_regex() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r"^/video/(\d{6,})$")
            .expect("player_video_regex: compile-time constant regex must compile")
    })
}

/// Extract the numeric video ID from a URL or return `None` if the URL is
/// not a video / private-video shape. Used by the oEmbed request builder.
pub fn extract_video_id(url: &str) -> Option<String> {
    let (_, path) = validate_and_split(url)?;
    let path_only = normalize_path(path);

    if let Some(caps) = private_video_regex().captures(path_only) {
        return caps.get(1).map(|m| m.as_str().to_string());
    }
    if let Some(caps) = video_regex().captures(path_only) {
        return caps.get(1).map(|m| m.as_str().to_string());
    }
    if let Some(caps) = player_video_regex().captures(path_only) {
        return caps.get(1).map(|m| m.as_str().to_string());
    }
    None
}

/// Extract the numeric showcase / album ID from a URL or return `None`.
pub fn extract_showcase_id(url: &str) -> Option<String> {
    let (_, path) = validate_and_split(url)?;
    let path_only = normalize_path(path);
    showcase_or_album_regex()
        .captures(path_only)
        .and_then(|c| c.get(1).map(|m| m.as_str().to_string()))
}

fn validate_and_split(url: &str) -> Option<(String, &str)> {
    let (scheme, rest) = url.split_once("://")?;
    if !matches!(scheme.to_ascii_lowercase().as_str(), "http" | "https") {
        return None;
    }
    let (authority, path_and_query) = match rest.find('/') {
        Some(idx) => (&rest[..idx], &rest[idx..]),
        None => (rest, ""),
    };
    let authority_no_user = authority.rsplit('@').next().unwrap_or(authority);
    let host = extract_host(authority_no_user)?;
    Some((host.to_ascii_lowercase(), path_and_query))
}

/// Extract the host portion (without port) from an authority string.
/// Handles both plain hosts/IPv4 and bracketed IPv6 literals — see
/// the equivalent helper in the gallery plugin for the full policy.
fn extract_host(authority: &str) -> Option<&str> {
    if authority.is_empty() {
        return None;
    }
    if authority.starts_with('[') {
        let close = authority.find(']')?;
        Some(&authority[..=close])
    } else {
        let host = authority.split(':').next().unwrap_or(authority);
        if host.is_empty() {
            None
        } else {
            Some(host)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    #[case("https://vimeo.com/123456789", UrlKind::Video)]
    #[case("https://www.vimeo.com/123456789", UrlKind::Video)]
    #[case("https://vimeo.com/123456789/abcdef1234", UrlKind::PrivateVideo)]
    #[case("https://vimeo.com/showcase/98765", UrlKind::Showcase)]
    #[case("https://vimeo.com/album/54321", UrlKind::Showcase)]
    #[case("https://player.vimeo.com/video/123456789", UrlKind::Video)]
    #[case("https://vimeo.com/123456789?autoplay=1", UrlKind::Video)]
    #[case("https://vimeo.com/123456789/", UrlKind::Video)]
    #[case("https://vimeo.com/ondemand/example", UrlKind::Unknown)]
    #[case("https://vimeo.com/user/foo", UrlKind::Unknown)]
    #[case("https://example.com/?u=vimeo.com/123", UrlKind::Unknown)]
    #[case("not a url", UrlKind::Unknown)]
    // Fragment stripping: `#t=30s` timestamps must not reclassify the URL.
    #[case("https://vimeo.com/123456789#t=30", UrlKind::Video)]
    #[case(
        "https://vimeo.com/123456789/abcdef1234#comment",
        UrlKind::PrivateVideo
    )]
    #[case("https://vimeo.com/showcase/98765#intro", UrlKind::Showcase)]
    // Showcase regex is anchored — junk after the numeric id is rejected.
    #[case("https://vimeo.com/showcase/98765/extra/segments", UrlKind::Unknown)]
    fn test_classify_url(#[case] url: &str, #[case] expected: UrlKind) {
        assert_eq!(classify_url(url), expected);
    }

    #[test]
    fn extract_video_id_public() {
        assert_eq!(
            extract_video_id("https://vimeo.com/123456789"),
            Some("123456789".into())
        );
    }

    #[test]
    fn extract_video_id_private() {
        assert_eq!(
            extract_video_id("https://vimeo.com/123456789/abcdef1234"),
            Some("123456789".into())
        );
    }

    #[test]
    fn extract_video_id_player() {
        assert_eq!(
            extract_video_id("https://player.vimeo.com/video/123456789"),
            Some("123456789".into())
        );
    }

    #[test]
    fn extract_video_id_rejects_showcase() {
        assert_eq!(extract_video_id("https://vimeo.com/showcase/1"), None);
    }

    #[test]
    fn extract_showcase_id_matches_showcase_and_album() {
        assert_eq!(
            extract_showcase_id("https://vimeo.com/showcase/98765"),
            Some("98765".into())
        );
        assert_eq!(
            extract_showcase_id("https://vimeo.com/album/54321"),
            Some("54321".into())
        );
    }

    #[test]
    fn is_vimeo_url_sanity() {
        assert!(is_vimeo_url("https://vimeo.com/1234567"));
        assert!(!is_vimeo_url("https://example.com/"));
    }
}
