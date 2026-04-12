//! SoundCloud URL detection and classification.
//!
//! Pure logic, no WASM or HTTP required — unit-testable natively.
//!
//! ## Design
//!
//! SoundCloud URLs are classified based on the number of path segments
//! after the user slug:
//!
//! - `soundcloud.com/<user>` — artist profile (→ Artist)
//! - `soundcloud.com/<user>/<slug>` — track (→ Track)
//! - `soundcloud.com/<user>/sets/<slug>` — playlist / album (→ Playlist)
//! - `soundcloud.com/<user>/likes` — liked tracks collection (→ Playlist)
//!
//! The host allowlist blocks substring smuggling
//! (`example.com/?next=soundcloud.com/foo`).

/// Kind of SoundCloud resource identified from a URL.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UrlKind {
    /// A single track: `soundcloud.com/<user>/<slug>`
    Track,
    /// A playlist / album / likes collection: `soundcloud.com/<user>/sets/<slug>`
    Playlist,
    /// An artist profile: `soundcloud.com/<user>`
    Artist,
    /// Not a recognised SoundCloud URL.
    Unknown,
}

/// Returns `true` if the URL is any form of recognised SoundCloud resource.
pub fn is_soundcloud_url(url: &str) -> bool {
    !matches!(classify_url(url), UrlKind::Unknown)
}

/// Classify the URL into a [`UrlKind`].
///
/// Accepts both `soundcloud.com` and `m.soundcloud.com`. The `api.` and
/// `api-v2.` subdomains are not accepted because they are server-side
/// endpoints, not public URLs the user would paste.
pub fn classify_url(url: &str) -> UrlKind {
    let Some((host_lower, path)) = validate_and_split(url) else {
        return UrlKind::Unknown;
    };

    if !is_soundcloud_host(&host_lower) {
        return UrlKind::Unknown;
    }

    let path_only = normalize_path(path);
    let segments: Vec<&str> = path_only
        .trim_start_matches('/')
        .split('/')
        .filter(|s| !s.is_empty())
        .collect();

    // `on.soundcloud.com/<token>` is a URL-shortener: the single-segment
    // path is a redirect token, not a user slug, so it resolves to a
    // track (the host resolver follows the redirect). Classify it as
    // Track so that `ensure_track` accepts it downstream.
    if host_lower == "on.soundcloud.com" {
        return match segments.as_slice() {
            [_token] => UrlKind::Track,
            _ => UrlKind::Unknown,
        };
    }

    match segments.as_slice() {
        [] => UrlKind::Unknown,
        [_user] => UrlKind::Artist,
        [_user, "sets", _slug] => UrlKind::Playlist,
        [_user, "likes"] | [_user, "reposts"] | [_user, "tracks"] | [_user, "albums"] => {
            UrlKind::Playlist
        }
        [_user, _slug] => UrlKind::Track,
        _ => UrlKind::Unknown,
    }
}

/// Strip the query string, fragment, and trailing slash from a raw
/// path-and-query slice. Fragments are stripped first because a URL of
/// the form `path?q#frag` keeps the fragment *after* the query, and a
/// URL of the form `path#frag?q` (technically malformed but tolerated
/// by some clients) is handled by the same two-pass split.
fn normalize_path(path: &str) -> &str {
    let no_frag = path.split('#').next().unwrap_or("");
    let no_query = no_frag.split('?').next().unwrap_or("");
    no_query.trim_end_matches('/')
}

fn is_soundcloud_host(host: &str) -> bool {
    matches!(
        host,
        "soundcloud.com" | "www.soundcloud.com" | "m.soundcloud.com" | "on.soundcloud.com"
    )
}

/// Split `scheme://host/path?query` into `(host_lowercased, path+query)`.
/// Strips userinfo and port from the authority, rejects non-http(s).
fn validate_and_split(url: &str) -> Option<(String, &str)> {
    let (scheme, rest) = url.split_once("://")?;
    if !matches!(scheme.to_ascii_lowercase().as_str(), "http" | "https") {
        return None;
    }
    let (authority, path_and_query) = match rest.find('/') {
        Some(idx) => (&rest[..idx], &rest[idx..]),
        None => (rest, ""),
    };
    // Strip userinfo (`user:pass@host`) and port, with IPv6-literal
    // awareness so `[::1]:443` does not collapse to `[`.
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
    #[case("https://soundcloud.com/forss/flickermood", UrlKind::Track)]
    #[case("https://soundcloud.com/forss/sets/soulhack", UrlKind::Playlist)]
    #[case("https://soundcloud.com/forss", UrlKind::Artist)]
    #[case("https://soundcloud.com/forss/likes", UrlKind::Playlist)]
    #[case("https://soundcloud.com/forss/tracks", UrlKind::Playlist)]
    #[case("https://soundcloud.com/forss/albums", UrlKind::Playlist)]
    #[case("https://m.soundcloud.com/forss/flickermood", UrlKind::Track)]
    #[case("https://www.soundcloud.com/forss", UrlKind::Artist)]
    #[case(
        "https://soundcloud.com/forss/flickermood?in=foo/sets/bar",
        UrlKind::Track
    )]
    #[case("https://soundcloud.com/forss/flickermood/", UrlKind::Track)]
    #[case("https://example.com/?next=soundcloud.com/forss", UrlKind::Unknown)]
    #[case("https://api.soundcloud.com/tracks/123", UrlKind::Unknown)]
    #[case("ftp://soundcloud.com/forss", UrlKind::Unknown)]
    #[case("not a url", UrlKind::Unknown)]
    // Fragment stripping: collections with `#...` must not be
    // misclassified as tracks just because the path has two segments.
    #[case("https://soundcloud.com/forss/likes#recent", UrlKind::Playlist)]
    #[case("https://soundcloud.com/forss#bio", UrlKind::Artist)]
    #[case("https://soundcloud.com/forss/flickermood#t=30", UrlKind::Track)]
    // on.soundcloud.com short links are redirect tokens → Track
    #[case("https://on.soundcloud.com/AbCdEfGhIj", UrlKind::Track)]
    #[case("https://on.soundcloud.com/AbCdEfGhIj?si=xyz", UrlKind::Track)]
    #[case("https://on.soundcloud.com/", UrlKind::Unknown)]
    fn test_classify_url(#[case] url: &str, #[case] expected: UrlKind) {
        assert_eq!(classify_url(url), expected);
    }

    #[test]
    fn test_is_soundcloud_url_accepts_tracks_and_playlists() {
        assert!(is_soundcloud_url("https://soundcloud.com/a/b"));
        assert!(is_soundcloud_url("https://soundcloud.com/a/sets/b"));
        assert!(!is_soundcloud_url("https://example.com/"));
    }
}
