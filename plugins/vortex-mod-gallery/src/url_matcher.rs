//! Gallery URL detection and provider routing.
//!
//! Each recognised URL is routed to exactly one [`Provider`] based on a
//! host + path shape check. Unknown URLs fall through to
//! [`Provider::Generic`] only if the URL scheme is http(s); everything
//! else is rejected.

use std::sync::OnceLock;

use regex::Regex;
use serde::Serialize;

/// Gallery provider for a given URL.
///
/// This is the single canonical `Provider` type for the crate —
/// `link.rs` re-exports it so that all modules (url_matcher, filter,
/// providers, lib.rs) share the same definition and cannot drift.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    /// Imgur album or gallery: `imgur.com/a/<id>` or `imgur.com/gallery/<id>`
    Imgur,
    /// Reddit submission / gallery: `reddit.com/r/<sub>/comments/<id>/…`
    Reddit,
    /// Flickr photoset / album: `flickr.com/photos/<user>/albums/<id>`
    Flickr,
    /// Generic HTML page — parse `<img>` tags.
    Generic,
}

pub fn classify_url(url: &str) -> Option<Provider> {
    let (host_lower, path) = validate_and_split(url)?;
    let path_only = normalize_path(path);

    if is_imgur_host(&host_lower) && imgur_regex().is_match(path_only) {
        return Some(Provider::Imgur);
    }
    if is_reddit_host(&host_lower) && reddit_regex().is_match(path_only) {
        return Some(Provider::Reddit);
    }
    if is_flickr_host(&host_lower) && flickr_regex().is_match(path_only) {
        return Some(Provider::Flickr);
    }
    // Generic fallback: any http(s) URL is eligible, callers decide
    // whether to actually scrape it based on their own policy.
    Some(Provider::Generic)
}

pub fn is_recognised_provider(url: &str) -> bool {
    !matches!(classify_url(url), None | Some(Provider::Generic))
}

pub fn extract_imgur_id(url: &str) -> Option<String> {
    let (_, path) = validate_and_split(url)?;
    let path_only = normalize_path(path);
    imgur_regex()
        .captures(path_only)
        .and_then(|c| c.get(1).map(|m| m.as_str().to_string()))
}

pub fn extract_reddit_permalink(url: &str) -> Option<String> {
    // Reddit JSON endpoint = <permalink>.json
    let (host, path) = validate_and_split(url)?;
    if !is_reddit_host(&host) {
        return None;
    }
    let path_only = normalize_path(path);
    if !reddit_regex().is_match(path_only) {
        return None;
    }
    // Reddit's JSON endpoint is `<permalink>.json`. Users sometimes
    // paste the already-terminated `.json` URL; appending another
    // `.json` would produce a `title.json.json` request that 404s, so
    // we detect that case and pass the path through unchanged.
    let already_json = path_only.ends_with(".json");
    let suffix = if already_json { "" } else { ".json" };
    Some(format!("https://www.reddit.com{path_only}{suffix}"))
}

pub fn extract_flickr_album_id(url: &str) -> Option<(String, String)> {
    let (_, path) = validate_and_split(url)?;
    let path_only = normalize_path(path);
    let caps = flickr_regex().captures(path_only)?;
    let user = caps.get(1)?.as_str().to_string();
    let album = caps.get(2)?.as_str().to_string();
    Some((user, album))
}

/// Strip `?query`, `#fragment`, and trailing `/` from the raw path.
/// Fragments are split first so that `path?q#frag` still works.
fn normalize_path(path: &str) -> &str {
    let no_frag = path.split('#').next().unwrap_or("");
    let no_query = no_frag.split('?').next().unwrap_or("");
    no_query.trim_end_matches('/')
}

fn is_imgur_host(host: &str) -> bool {
    matches!(
        host,
        "imgur.com" | "www.imgur.com" | "i.imgur.com" | "m.imgur.com"
    )
}

fn is_reddit_host(host: &str) -> bool {
    matches!(
        host,
        "reddit.com" | "www.reddit.com" | "old.reddit.com" | "m.reddit.com"
    )
}

fn is_flickr_host(host: &str) -> bool {
    matches!(host, "flickr.com" | "www.flickr.com" | "m.flickr.com")
}

// Each provider regex anchors the captured segment to a **segment
// boundary** — either end-of-string or a `/` — so malformed paths
// like `/gallery/abc-typo` (where `-typo` is supposed to be part of
// the same segment) or `/albums/123junk` are rejected instead of
// producing a partial match that the extractor would happily use.
// Callers pre-normalise the path with `normalize_path`, so fragment
// and query are already stripped by the time the regex runs.

// All three provider regexes are compile-time constants: `.expect`
// documents the invariant and honours the crate-wide policy that
// production code paths must not `.unwrap()`.

fn imgur_regex() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r"^/(?:a|gallery)/([A-Za-z0-9]+)(?:$|/)")
            .expect("imgur_regex: compile-time constant regex must compile")
    })
}

fn reddit_regex() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r"^/r/[A-Za-z0-9_]+/comments/[A-Za-z0-9]+(?:$|/)")
            .expect("reddit_regex: compile-time constant regex must compile")
    })
}

fn flickr_regex() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r"^/photos/([^/]+)/albums/(\d+)(?:$|/)")
            .expect("flickr_regex: compile-time constant regex must compile")
    })
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
///
/// Handles both plain hostnames/IPv4 (`example.com:8080`, `1.2.3.4`)
/// and IPv6 literals (`[::1]:8080`, `[2001:db8::1]`). For IPv6, the
/// host is the substring between `[` and `]`, keeping the brackets
/// so downstream host-allowlist matches still behave as expected.
/// For plain hosts, the host is the substring before the first `:`.
///
/// Returns `None` when the authority is empty or malformed (e.g. a
/// lone `[` with no closing `]`).
fn extract_host(authority: &str) -> Option<&str> {
    if authority.is_empty() {
        return None;
    }
    if authority.starts_with('[') {
        // IPv6 literal — host includes the brackets.
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
    #[case("https://imgur.com/a/abcd123", Some(Provider::Imgur))]
    #[case("https://imgur.com/gallery/XyZ99", Some(Provider::Imgur))]
    #[case(
        "https://www.reddit.com/r/pics/comments/1abc/title/",
        Some(Provider::Reddit)
    )]
    #[case("https://old.reddit.com/r/pics/comments/1abc/", Some(Provider::Reddit))]
    #[case(
        "https://www.flickr.com/photos/bob/albums/72177720313121212",
        Some(Provider::Flickr)
    )]
    #[case("https://example.com/page", Some(Provider::Generic))]
    #[case("ftp://example.com/page", None)]
    #[case("not a url", None)]
    fn test_classify_url(#[case] url: &str, #[case] expected: Option<Provider>) {
        assert_eq!(classify_url(url), expected);
    }

    #[test]
    fn classify_url_rejects_malformed_imgur_with_junk_suffix() {
        // The segment-boundary anchor rejects trailing junk: the
        // `abc` is a valid id but `abc-typo` is not a separate segment.
        assert_eq!(
            classify_url("https://imgur.com/a/abc-typo"),
            Some(Provider::Generic)
        );
    }

    #[test]
    fn classify_url_rejects_malformed_flickr_album_suffix() {
        assert_eq!(
            classify_url("https://www.flickr.com/photos/bob/albums/123junk"),
            Some(Provider::Generic)
        );
    }

    #[test]
    fn classify_url_rejects_malformed_reddit_permalink_suffix() {
        assert_eq!(
            classify_url("https://www.reddit.com/r/pics/comments/1abcjunk-extra"),
            Some(Provider::Generic)
        );
    }

    #[test]
    fn classify_url_accepts_imgur_with_trailing_slash() {
        // The `(?:$|/)` anchor still permits a trailing slash after a
        // valid id segment.
        assert_eq!(
            classify_url("https://imgur.com/a/abcd123/"),
            Some(Provider::Imgur)
        );
    }

    #[test]
    fn is_recognised_provider_rejects_generic() {
        assert!(is_recognised_provider("https://imgur.com/a/abc"));
        assert!(!is_recognised_provider("https://example.com/page"));
    }

    #[test]
    fn extract_host_handles_plain_and_ipv6() {
        // Plain host: everything before the first `:` is the host.
        assert_eq!(extract_host("example.com"), Some("example.com"));
        assert_eq!(extract_host("example.com:8080"), Some("example.com"));
        assert_eq!(extract_host("1.2.3.4:443"), Some("1.2.3.4"));
        // IPv6 literal: host is the substring between `[` and `]`,
        // brackets included.
        assert_eq!(extract_host("[::1]"), Some("[::1]"));
        assert_eq!(extract_host("[::1]:8080"), Some("[::1]"));
        assert_eq!(extract_host("[2001:db8::1]:443"), Some("[2001:db8::1]"));
        // Malformed: lone `[` with no `]` → None.
        assert_eq!(extract_host("[::1"), None);
        // Empty authority → None.
        assert_eq!(extract_host(""), None);
    }

    #[test]
    fn extract_imgur_id_works() {
        assert_eq!(
            extract_imgur_id("https://imgur.com/a/abcd123"),
            Some("abcd123".into())
        );
        assert_eq!(
            extract_imgur_id("https://imgur.com/gallery/XyZ99?foo=bar"),
            Some("XyZ99".into())
        );
    }

    #[test]
    fn extract_reddit_permalink_adds_json_suffix() {
        assert_eq!(
            extract_reddit_permalink("https://www.reddit.com/r/pics/comments/1abc/title/"),
            Some("https://www.reddit.com/r/pics/comments/1abc/title.json".into())
        );
    }

    #[test]
    fn extract_flickr_album_id_tuple() {
        assert_eq!(
            extract_flickr_album_id("https://www.flickr.com/photos/bob/albums/72177720313121212"),
            Some(("bob".into(), "72177720313121212".into()))
        );
    }
}
