//! Gallery URL detection and provider routing.
//!
//! Each recognised URL is routed to exactly one [`Provider`] based on a
//! host + path shape check. Unknown URLs fall through to
//! [`Provider::Generic`] only if the URL scheme is http(s); everything
//! else is rejected.

use std::sync::OnceLock;

use regex::Regex;

/// Gallery provider for a given URL.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    let path_only = path.split('?').next().unwrap_or("").trim_end_matches('/');

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
    let path_only = path.split('?').next().unwrap_or("").trim_end_matches('/');
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
    let path_only = path.split('?').next().unwrap_or("").trim_end_matches('/');
    if reddit_regex().is_match(path_only) {
        Some(format!("https://www.reddit.com{path_only}.json"))
    } else {
        None
    }
}

pub fn extract_flickr_album_id(url: &str) -> Option<(String, String)> {
    let (_, path) = validate_and_split(url)?;
    let path_only = path.split('?').next().unwrap_or("").trim_end_matches('/');
    let caps = flickr_regex().captures(path_only)?;
    let user = caps.get(1)?.as_str().to_string();
    let album = caps.get(2)?.as_str().to_string();
    Some((user, album))
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

fn imgur_regex() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^/(?:a|gallery)/([A-Za-z0-9]+)").unwrap())
}

fn reddit_regex() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^/r/[A-Za-z0-9_]+/comments/[A-Za-z0-9]+").unwrap())
}

fn flickr_regex() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^/photos/([^/]+)/albums/(\d+)").unwrap())
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
    let host = authority_no_user
        .split(':')
        .next()
        .unwrap_or(authority_no_user);
    if host.is_empty() {
        return None;
    }
    Some((host.to_ascii_lowercase(), path_and_query))
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
    fn is_recognised_provider_rejects_generic() {
        assert!(is_recognised_provider("https://imgur.com/a/abc"));
        assert!(!is_recognised_provider("https://example.com/page"));
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
