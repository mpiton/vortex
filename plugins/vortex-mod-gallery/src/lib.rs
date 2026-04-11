//! Vortex Gallery WASM plugin.
//!
//! Extracts direct image links from Imgur albums, Reddit galleries,
//! Flickr photosets, and a generic `<img>` fallback for any HTTP page.
//!
//! Implements the CrawlerModule contract:
//! - `can_handle(url)` → `"true"` / `"false"` (recognised providers only)
//! - `extract_links(url)` → JSON string with `ImageLink` entries
//!
//! All network I/O is delegated to the host via `http_request`.

pub mod error;
pub mod filter;
pub mod link;
pub mod providers;
pub mod url_matcher;

#[cfg(target_family = "wasm")]
mod plugin_api;

use serde::Serialize;

use crate::error::PluginError;
// `link::Provider` is the canonical `url_matcher::Provider` re-exported
// through `link.rs` — importing from either path resolves to the same
// type, so there is no `From` conversion needed here.
use crate::link::ImageLink;
use crate::url_matcher::Provider;

// ── IPC DTOs ──────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct ExtractLinksResponse {
    pub kind: &'static str,
    pub provider: Provider,
    pub images: Vec<ImageLink>,
}

// ── Routing helpers ──────────────────────────────────────────────────────────

/// Returns `"true"` if the URL maps to a *recognised* provider. The
/// generic fallback is **not** reported as handleable — that would
/// amount to claiming ownership of every HTTP(S) page on the internet,
/// which would break other plugin ranking heuristics.
pub fn handle_can_handle(url: &str) -> String {
    bool_to_string(url_matcher::is_recognised_provider(url))
}

pub fn handle_supports_playlist(url: &str) -> String {
    // Every recognised provider is a multi-image collection → "true".
    handle_can_handle(url)
}

fn bool_to_string(b: bool) -> String {
    if b {
        "true".into()
    } else {
        "false".into()
    }
}

pub fn ensure_recognised_url(url: &str) -> Result<Provider, PluginError> {
    match url_matcher::classify_url(url) {
        Some(p @ (Provider::Imgur | Provider::Reddit | Provider::Flickr)) => Ok(p),
        _ => Err(PluginError::UnsupportedUrl(url.to_string())),
    }
}

// ── Response building ────────────────────────────────────────────────────────

/// Post-process raw provider images: dedupe, filter by minimum
/// resolution, then optionally attach auto-generated filenames.
pub fn finalize_links(
    provider: Provider,
    album_id: &str,
    images: Vec<ImageLink>,
    min_resolution: &str,
    auto_name_enabled: bool,
) -> Result<ExtractLinksResponse, PluginError> {
    let (min_w, min_h) = filter::parse_min_resolution(min_resolution)?;
    let deduped = filter::dedupe_links(images);
    let filtered = filter::filter_by_min_resolution(deduped, min_w, min_h);

    let images: Vec<ImageLink> = if auto_name_enabled {
        filtered
            .into_iter()
            .enumerate()
            .map(|(idx, mut link)| {
                link.filename = Some(filter::auto_name(provider, album_id, idx, &link.url));
                link
            })
            .collect()
    } else {
        filtered
    };

    Ok(ExtractLinksResponse {
        kind: "gallery",
        provider,
        images,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_images() -> Vec<ImageLink> {
        vec![
            ImageLink {
                url: "https://i.imgur.com/a.jpg".into(),
                width: Some(1920),
                height: Some(1080),
                title: Some("one".into()),
                filename: None,
            },
            ImageLink {
                url: "https://i.imgur.com/a.jpg".into(), // duplicate
                width: Some(1920),
                height: Some(1080),
                title: Some("one".into()),
                filename: None,
            },
            ImageLink {
                url: "https://i.imgur.com/b.png".into(),
                width: Some(400),
                height: Some(300),
                title: Some("small".into()),
                filename: None,
            },
            ImageLink {
                url: "https://i.imgur.com/c.gif".into(),
                width: None,
                height: None,
                title: None,
                filename: None,
            },
        ]
    }

    #[test]
    fn can_handle_true_for_imgur() {
        assert_eq!(handle_can_handle("https://imgur.com/a/abcd"), "true");
    }

    #[test]
    fn can_handle_true_for_reddit() {
        assert_eq!(
            handle_can_handle("https://www.reddit.com/r/pics/comments/1abc/title"),
            "true"
        );
    }

    #[test]
    fn can_handle_true_for_flickr() {
        assert_eq!(
            handle_can_handle("https://www.flickr.com/photos/bob/albums/72177"),
            "true"
        );
    }

    #[test]
    fn can_handle_false_for_unrelated() {
        assert_eq!(handle_can_handle("https://example.com/"), "false");
    }

    #[test]
    fn can_handle_false_for_ftp() {
        assert_eq!(handle_can_handle("ftp://imgur.com/a/abcd"), "false");
    }

    #[test]
    fn finalize_dedupes_filters_and_auto_names() {
        let resp =
            finalize_links(Provider::Imgur, "abcd", sample_images(), "800x600", true).unwrap();
        assert_eq!(resp.kind, "gallery");
        assert_eq!(resp.provider, Provider::Imgur);
        // 4 input → dedupe to 3 → filter drops 400x300 → 2 kept
        assert_eq!(resp.images.len(), 2);
        assert_eq!(
            resp.images[0].filename.as_deref(),
            Some("imgur_abcd_000.jpg")
        );
        assert_eq!(
            resp.images[1].filename.as_deref(),
            Some("imgur_abcd_001.gif")
        );
    }

    #[test]
    fn finalize_without_auto_name_preserves_filename_none() {
        let resp = finalize_links(Provider::Imgur, "abcd", sample_images(), "0x0", false).unwrap();
        assert!(resp.images.iter().all(|img| img.filename.is_none()));
    }

    #[test]
    fn finalize_invalid_min_resolution_errors() {
        let err = finalize_links(Provider::Imgur, "abcd", vec![], "bad", false).unwrap_err();
        assert!(matches!(err, PluginError::InvalidMinResolution(_)));
    }

    #[test]
    fn ensure_recognised_url_rejects_generic_fallback() {
        let err = ensure_recognised_url("https://example.com/page").unwrap_err();
        assert!(matches!(err, PluginError::UnsupportedUrl(_)));
    }

    #[test]
    fn serialisation_of_extract_links_response() {
        let resp = finalize_links(Provider::Flickr, "72177", sample_images(), "0x0", true).unwrap();
        let json = serde_json::to_string(&resp).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["kind"], "gallery");
        assert_eq!(parsed["provider"], "flickr");
        assert_eq!(parsed["images"].as_array().unwrap().len(), 3);
    }
}
