//! Post-extraction filtering and naming helpers.
//!
//! - [`parse_min_resolution`] turns a user-facing `"WxH"` string into a
//!   `(width, height)` tuple.
//! - [`filter_by_min_resolution`] drops images that are known to be
//!   below the threshold. Images with unknown dimensions are kept,
//!   because the HTTP HEAD check to discover them is out of scope for
//!   the plugin (the download engine may re-check later).
//! - [`dedupe_links`] removes duplicate URLs while preserving order.
//! - [`auto_name`] produces a stable filename from provider + id + index.

use crate::error::PluginError;
use crate::link::{ImageLink, Provider};

/// Parse a `"WxH"` string into `(width, height)`.
pub fn parse_min_resolution(input: &str) -> Result<(u32, u32), PluginError> {
    let trimmed = input.trim().to_ascii_lowercase();
    if trimmed.is_empty() || trimmed == "0x0" {
        return Ok((0, 0));
    }
    let (w, h) = trimmed
        .split_once('x')
        .ok_or_else(|| PluginError::InvalidMinResolution(input.to_string()))?;
    let w: u32 = w
        .trim()
        .parse()
        .map_err(|_| PluginError::InvalidMinResolution(input.to_string()))?;
    let h: u32 = h
        .trim()
        .parse()
        .map_err(|_| PluginError::InvalidMinResolution(input.to_string()))?;
    Ok((w, h))
}

/// Drop images strictly smaller than `min_w × min_h`.
///
/// Policy for partial information:
///
/// - Both dimensions unknown → keep (benefit-of-the-doubt; downstream
///   HEAD check can re-verify).
/// - Both known → drop if either axis is below the minimum.
/// - Only one axis known → drop if *that* axis is below its minimum,
///   otherwise keep. A known small width is a firm "too small" signal
///   and should not leak through just because the height is missing.
pub fn filter_by_min_resolution(links: Vec<ImageLink>, min_w: u32, min_h: u32) -> Vec<ImageLink> {
    if min_w == 0 && min_h == 0 {
        return links;
    }
    links
        .into_iter()
        .filter(|l| match (l.width, l.height) {
            (Some(w), Some(h)) => w >= min_w && h >= min_h,
            (Some(w), None) => w >= min_w,
            (None, Some(h)) => h >= min_h,
            (None, None) => true,
        })
        .collect()
}

/// Remove duplicate URLs while preserving first-seen order.
pub fn dedupe_links(links: Vec<ImageLink>) -> Vec<ImageLink> {
    let mut seen = std::collections::HashSet::<String>::new();
    let mut out = Vec::with_capacity(links.len());
    for link in links {
        if seen.insert(link.url.clone()) {
            out.push(link);
        }
    }
    out
}

/// Produce `<provider>_<album>_<index><ext>` as a stable auto-name when
/// `auto_name` is enabled. Index is zero-padded to 3 digits so files
/// sort lexicographically.
pub fn auto_name(provider: Provider, album_id: &str, index: usize, url: &str) -> String {
    let provider = match provider {
        Provider::Imgur => "imgur",
        Provider::Reddit => "reddit",
        Provider::Flickr => "flickr",
        Provider::Generic => "web",
    };
    let ext = guess_ext_from_url(url).unwrap_or("jpg");
    let safe_album = sanitize(album_id);
    format!("{provider}_{safe_album}_{index:03}.{ext}")
}

fn guess_ext_from_url(url: &str) -> Option<&str> {
    let path = url.split('?').next().unwrap_or(url);
    let dot = path.rfind('.')?;
    let ext = &path[dot + 1..];
    if (1..=5).contains(&ext.len()) && ext.chars().all(|c| c.is_ascii_alphanumeric()) {
        Some(ext)
    } else {
        None
    }
}

fn sanitize(input: &str) -> String {
    input
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn link(url: &str, w: Option<u32>, h: Option<u32>) -> ImageLink {
        ImageLink {
            url: url.to_string(),
            width: w,
            height: h,
            title: None,
            filename: None,
        }
    }

    #[test]
    fn parse_min_resolution_happy_path() {
        assert_eq!(parse_min_resolution("800x600").unwrap(), (800, 600));
        assert_eq!(parse_min_resolution("1920X1080").unwrap(), (1920, 1080));
    }

    #[test]
    fn parse_min_resolution_zero_returns_0x0() {
        assert_eq!(parse_min_resolution("0x0").unwrap(), (0, 0));
        assert_eq!(parse_min_resolution("").unwrap(), (0, 0));
    }

    #[test]
    fn parse_min_resolution_invalid() {
        let err = parse_min_resolution("tall").unwrap_err();
        assert!(matches!(err, PluginError::InvalidMinResolution(_)));
    }

    #[test]
    fn filter_by_min_resolution_drops_small_keeps_unknown() {
        let input = vec![
            link("a.jpg", Some(400), Some(300)),  // drop
            link("b.jpg", Some(1200), Some(800)), // keep
            link("c.jpg", None, None),            // keep (unknown)
            link("d.jpg", Some(800), Some(600)),  // keep (exact match)
        ];
        let out = filter_by_min_resolution(input, 800, 600);
        let urls: Vec<_> = out.iter().map(|l| l.url.as_str()).collect();
        assert_eq!(urls, vec!["b.jpg", "c.jpg", "d.jpg"]);
    }

    #[test]
    fn filter_by_min_resolution_drops_known_partial_below_threshold() {
        // A firmly-known small width must drop even if height is
        // unknown — the old policy leaked such images through because
        // "any partial info" was treated as "benefit of the doubt".
        let input = vec![
            link("small-w.jpg", Some(400), None), // drop (width too small)
            link("big-w.jpg", Some(1920), None),  // keep
            link("small-h.jpg", None, Some(300)), // drop (height too small)
            link("big-h.jpg", None, Some(1080)),  // keep
            link("both-none.jpg", None, None),    // keep (unknown)
        ];
        let out = filter_by_min_resolution(input, 800, 600);
        let urls: Vec<_> = out.iter().map(|l| l.url.as_str()).collect();
        assert_eq!(urls, vec!["big-w.jpg", "big-h.jpg", "both-none.jpg"]);
    }

    #[test]
    fn filter_by_min_resolution_zero_is_noop() {
        let input = vec![link("a.jpg", Some(1), Some(1))];
        let out = filter_by_min_resolution(input, 0, 0);
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn dedupe_links_preserves_first_seen_order() {
        let input = vec![
            link("a.jpg", None, None),
            link("b.jpg", None, None),
            link("a.jpg", None, None),
            link("c.jpg", None, None),
        ];
        let out = dedupe_links(input);
        let urls: Vec<_> = out.iter().map(|l| l.url.as_str()).collect();
        assert_eq!(urls, vec!["a.jpg", "b.jpg", "c.jpg"]);
    }

    #[test]
    fn auto_name_pads_index_and_uses_extension() {
        let name = auto_name(Provider::Imgur, "abcd123", 0, "https://i.imgur.com/xyz.png");
        assert_eq!(name, "imgur_abcd123_000.png");
    }

    #[test]
    fn auto_name_defaults_extension_when_missing() {
        let name = auto_name(Provider::Reddit, "1abc", 7, "https://preview.redd.it/noext");
        assert_eq!(name, "reddit_1abc_007.jpg");
    }

    #[test]
    fn auto_name_sanitizes_album_id() {
        let name = auto_name(Provider::Flickr, "72177/bad id", 12, "a.jpeg");
        assert_eq!(name, "flickr_72177_bad_id_012.jpeg");
    }

    #[test]
    fn guess_ext_strips_query_string() {
        assert_eq!(
            guess_ext_from_url("https://x.com/a.webp?sig=abc"),
            Some("webp")
        );
    }
}
