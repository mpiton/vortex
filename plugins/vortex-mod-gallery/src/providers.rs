//! Provider-specific parsers: Imgur, Reddit, Flickr, generic HTML.
//!
//! Each provider takes a raw API body (or HTML page for Generic) and
//! returns a list of [`ImageLink`]s. The providers are kept pure so they
//! can be unit-tested with hardcoded fixtures.

use std::collections::HashMap;
use std::sync::OnceLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::error::PluginError;
use crate::link::ImageLink;

// ── Host function envelope ────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct HttpRequest {
    pub method: String,
    pub url: String,
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub headers: HashMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct HttpResponse {
    pub status: u16,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    #[serde(default)]
    pub body: String,
}

impl HttpResponse {
    pub fn into_success_body(self) -> Result<String, PluginError> {
        if (200..300).contains(&self.status) {
            Ok(self.body)
        } else {
            Err(PluginError::HttpStatus {
                status: self.status,
                message: truncate(&self.body, 256),
            })
        }
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        let mut cut = max;
        while !s.is_char_boundary(cut) && cut > 0 {
            cut -= 1;
        }
        format!("{}…", &s[..cut])
    }
}

pub fn parse_http_response(raw: &str) -> Result<HttpResponse, PluginError> {
    serde_json::from_str(raw).map_err(|e| PluginError::HostResponse(e.to_string()))
}

// ── Imgur ────────────────────────────────────────────────────────────────────

/// Matches Imgur API v3 `/album/<id>/images` JSON shape.
#[derive(Debug, Deserialize)]
struct ImgurAlbumResponse {
    data: Vec<ImgurImage>,
    status: u16,
    #[serde(default)]
    success: bool,
}

#[derive(Debug, Deserialize)]
struct ImgurImage {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    link: Option<String>,
    #[serde(default)]
    width: Option<u32>,
    #[serde(default)]
    height: Option<u32>,
}

pub fn build_imgur_album_request(album_id: &str, client_id: &str) -> Result<String, PluginError> {
    let url = format!("https://api.imgur.com/3/album/{album_id}/images");
    let mut headers = HashMap::new();
    headers.insert("Authorization".into(), format!("Client-ID {client_id}"));
    let req = HttpRequest {
        method: "GET".into(),
        url,
        headers,
        body: None,
    };
    Ok(serde_json::to_string(&req)?)
}

pub fn parse_imgur_album(raw: &str) -> Result<Vec<ImageLink>, PluginError> {
    let parsed: ImgurAlbumResponse =
        serde_json::from_str(raw).map_err(|e| PluginError::ParseJson(e.to_string()))?;
    if !parsed.success || !(200..300).contains(&parsed.status) {
        return Err(PluginError::HttpStatus {
            status: parsed.status,
            message: "Imgur API returned success=false".into(),
        });
    }
    Ok(parsed
        .data
        .into_iter()
        .filter_map(|img| {
            img.link.map(|url| ImageLink {
                url,
                width: img.width,
                height: img.height,
                title: img.title.or(img.id),
                filename: None,
            })
        })
        .collect())
}

// ── Reddit ───────────────────────────────────────────────────────────────────

/// Minimal subset of the Reddit listing JSON: the root is a 2-element
/// array where the first element contains the post and the second the
/// comment tree.
#[derive(Debug, Deserialize)]
struct RedditListing {
    data: RedditListingData,
}

#[derive(Debug, Deserialize)]
struct RedditListingData {
    #[serde(default)]
    children: Vec<RedditChild>,
}

#[derive(Debug, Deserialize)]
struct RedditChild {
    data: RedditPost,
}

#[derive(Debug, Deserialize)]
struct RedditPost {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    is_gallery: Option<bool>,
    #[serde(default)]
    media_metadata: Option<HashMap<String, RedditMediaMeta>>,
    #[serde(default)]
    preview: Option<RedditPreview>,
}

#[derive(Debug, Deserialize)]
struct RedditMediaMeta {
    #[serde(default)]
    s: Option<RedditMediaSource>,
}

#[derive(Debug, Deserialize)]
struct RedditMediaSource {
    #[serde(default)]
    u: Option<String>,
    #[serde(default)]
    x: Option<u32>,
    #[serde(default)]
    y: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct RedditPreview {
    #[serde(default)]
    images: Vec<RedditPreviewImage>,
}

#[derive(Debug, Deserialize)]
struct RedditPreviewImage {
    #[serde(default)]
    source: Option<RedditPreviewSource>,
}

#[derive(Debug, Deserialize)]
struct RedditPreviewSource {
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    width: Option<u32>,
    #[serde(default)]
    height: Option<u32>,
}

pub fn build_reddit_request(permalink_json: &str) -> Result<String, PluginError> {
    let mut headers = HashMap::new();
    headers.insert("User-Agent".into(), "vortex-gallery-plugin/1.0".into());
    let req = HttpRequest {
        method: "GET".into(),
        url: permalink_json.to_string(),
        headers,
        body: None,
    };
    Ok(serde_json::to_string(&req)?)
}

pub fn parse_reddit_submission(raw: &str) -> Result<Vec<ImageLink>, PluginError> {
    let listings: Vec<RedditListing> =
        serde_json::from_str(raw).map_err(|e| PluginError::ParseJson(e.to_string()))?;
    let Some(root) = listings.first() else {
        return Ok(Vec::new());
    };
    let Some(child) = root.data.children.first() else {
        return Ok(Vec::new());
    };
    let post = &child.data;
    let title = post.title.clone();

    // Case 1: native Reddit gallery (`is_gallery=true` + `media_metadata`)
    if post.is_gallery.unwrap_or(false) {
        if let Some(meta) = &post.media_metadata {
            let mut links: Vec<ImageLink> = meta
                .values()
                .filter_map(|item| {
                    let s = item.s.as_ref()?;
                    s.u.as_ref().map(|u| ImageLink {
                        url: unescape_amp(u),
                        width: s.x,
                        height: s.y,
                        title: title.clone(),
                        filename: None,
                    })
                })
                .collect();
            // media_metadata is a HashMap with no guaranteed order; sort
            // by URL so the output is deterministic across runs.
            links.sort_by(|a, b| a.url.cmp(&b.url));
            return Ok(links);
        }
    }

    // Case 2: single-image post — prefer preview (carries dimensions)
    if let Some(preview) = &post.preview {
        if let Some(img) = preview.images.first() {
            if let Some(src) = &img.source {
                if let Some(url) = &src.url {
                    return Ok(vec![ImageLink {
                        url: unescape_amp(url),
                        width: src.width,
                        height: src.height,
                        title,
                        filename: None,
                    }]);
                }
            }
        }
    }

    // Case 3: fallback — the submission URL points directly at an image
    if let Some(url) = &post.url {
        if looks_like_image_url(url) {
            return Ok(vec![ImageLink {
                url: url.clone(),
                width: None,
                height: None,
                title,
                filename: None,
            }]);
        }
    }

    Ok(Vec::new())
}

fn unescape_amp(url: &str) -> String {
    url.replace("&amp;", "&")
}

fn looks_like_image_url(url: &str) -> bool {
    // Strip the query and fragment before inspecting the extension so
    // that `https://cdn/example.jpg?sig=xyz#frag` is still recognised.
    let stripped = url
        .split('#')
        .next()
        .unwrap_or("")
        .split('?')
        .next()
        .unwrap_or("");
    let lower = stripped.to_ascii_lowercase();
    lower.ends_with(".jpg")
        || lower.ends_with(".jpeg")
        || lower.ends_with(".png")
        || lower.ends_with(".gif")
        || lower.ends_with(".webp")
        || lower.ends_with(".avif")
}

// ── Flickr ───────────────────────────────────────────────────────────────────

/// Matches Flickr REST `flickr.photosets.getPhotos` JSON response when
/// `format=json&nojsoncallback=1` is passed.
///
/// `photoset` is optional because Flickr's `{"stat":"fail"}` error
/// envelopes (bad API key, private album, non-existent photoset) omit
/// the field entirely — a mandatory field would surface those as JSON
/// parse failures instead of clean provider errors.
#[derive(Debug, Deserialize)]
struct FlickrResponse {
    #[serde(default)]
    photoset: Option<FlickrPhotoset>,
    #[serde(default)]
    stat: Option<String>,
    #[serde(default)]
    code: Option<u16>,
    #[serde(default)]
    message: Option<String>,
}

#[derive(Debug, Deserialize)]
struct FlickrPhotoset {
    #[serde(default)]
    photo: Vec<FlickrPhoto>,
}

#[derive(Debug, Deserialize)]
struct FlickrPhoto {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    url_o: Option<String>,
    #[serde(default)]
    url_l: Option<String>,
    #[serde(default)]
    height_o: Option<serde_json::Value>,
    #[serde(default)]
    width_o: Option<serde_json::Value>,
}

pub fn build_flickr_request(album_id: &str, api_key: &str) -> Result<String, PluginError> {
    // URL-encode user-controlled config values so that a key or album
    // id containing `&` or `=` cannot corrupt the query string. The
    // `album_id` is matched by `(\d+)` in `url_matcher.rs` so it is
    // safe by construction, but encoding it costs nothing and matches
    // the hardening applied to SoundCloud and Vimeo.
    let url = format!(
        "https://api.flickr.com/services/rest/?method=flickr.photosets.getPhotos&api_key={}&photoset_id={}&format=json&nojsoncallback=1&extras=url_o%2Curl_l",
        urlencode_query(api_key),
        urlencode_query(album_id),
    );
    let req = HttpRequest {
        method: "GET".into(),
        url,
        headers: HashMap::new(),
        body: None,
    };
    Ok(serde_json::to_string(&req)?)
}

/// Minimal percent-encoder for query-string values. Identical in spirit
/// to the one used by the SoundCloud and Vimeo plugins; duplicated here
/// intentionally because sharing would force a separate sdk crate.
fn urlencode_query(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{:02X}", b));
        }
    }
    out
}

pub fn parse_flickr_photoset(raw: &str) -> Result<Vec<ImageLink>, PluginError> {
    let parsed: FlickrResponse =
        serde_json::from_str(raw).map_err(|e| PluginError::ParseJson(e.to_string()))?;

    // Check the API envelope status BEFORE touching `photoset`, so that
    // `{"stat":"fail"}` responses surface as a provider error with the
    // Flickr error code / message instead of an unwrap panic or a
    // misleading JSON parse failure.
    if parsed.stat.as_deref() == Some("fail") {
        return Err(PluginError::HttpStatus {
            status: parsed.code.unwrap_or(400),
            message: parsed
                .message
                .unwrap_or_else(|| "Flickr API returned stat=fail".into()),
        });
    }
    if !matches!(parsed.stat.as_deref(), Some("ok") | None) {
        return Err(PluginError::HttpStatus {
            status: 400,
            message: format!("Flickr stat={:?}", parsed.stat),
        });
    }

    // `photoset` is now only absent for malformed success envelopes —
    // treat that as an empty album rather than an error.
    let Some(photoset) = parsed.photoset else {
        return Ok(Vec::new());
    };

    Ok(photoset
        .photo
        .into_iter()
        .filter_map(|p| {
            let url = p.url_o.or(p.url_l)?;
            Some(ImageLink {
                url,
                width: extract_dim(&p.width_o),
                height: extract_dim(&p.height_o),
                title: p.title.or_else(|| p.id.clone()),
                filename: None,
            })
        })
        .collect())
}

/// Flickr returns image dimensions as either a JSON number or a string
/// depending on the extras requested — handle both.
fn extract_dim(value: &Option<serde_json::Value>) -> Option<u32> {
    match value {
        Some(serde_json::Value::Number(n)) => n.as_u64().map(|v| v as u32),
        Some(serde_json::Value::String(s)) => s.parse().ok(),
        _ => None,
    }
}

// ── Generic HTML fallback ────────────────────────────────────────────────────

fn img_src_regex() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        // Match <img ... src="..." ...> and capture the src value.
        // Uses [^>]* rather than .*? to avoid backtracking on large pages.
        Regex::new(r#"(?i)<img\b[^>]*\bsrc\s*=\s*["']([^"']+)["']"#).unwrap()
    })
}

pub fn build_generic_request(page_url: &str) -> Result<String, PluginError> {
    let mut headers = HashMap::new();
    headers.insert("User-Agent".into(), "vortex-gallery-plugin/1.0".into());
    let req = HttpRequest {
        method: "GET".into(),
        url: page_url.to_string(),
        headers,
        body: None,
    };
    Ok(serde_json::to_string(&req)?)
}

/// Scrape `<img>` tags from an HTML page. Relative URLs are resolved
/// against `base_url`:
///
/// - absolute URLs are passed through verbatim
/// - protocol-relative URLs (`//cdn.example.com/a.jpg`) inherit the
///   **scheme of the page URL** (not a hardcoded `https:`)
/// - root-relative paths (`/foo.png`) are resolved against the origin
/// - page-relative paths (`images/4.jpg`) are resolved against the
///   page's **directory** (everything up to and including the last
///   `/`) so `<img src="a.jpg">` on `https://example.com/gallery/p.html`
///   becomes `https://example.com/gallery/a.jpg`, not
///   `https://example.com/a.jpg`
///
/// Non-http(s) URL schemes like `data:`, `blob:`, `javascript:`,
/// `mailto:` are dropped before resolution.
pub fn parse_generic_html(html: &str, base_url: &str) -> Vec<ImageLink> {
    let ctx = UrlContext::from_page_url(base_url);
    img_src_regex()
        .captures_iter(html)
        .filter_map(|c| c.get(1).map(|m| m.as_str().to_string()))
        .filter(|raw| !has_non_http_scheme(raw))
        .map(|raw| ctx.resolve(&raw))
        .filter(|url| is_http_url(url))
        .map(|url| ImageLink {
            url,
            width: None,
            height: None,
            title: None,
            filename: None,
        })
        .collect()
}

/// Parsed view of a page URL, split into the pieces the generic
/// resolver actually needs.
#[derive(Debug, Default)]
struct UrlContext {
    /// `"http"` or `"https"`, lowercased. Empty if the input wasn't
    /// a well-formed http(s) URL — the resolver then degrades to
    /// leaving relative URLs untouched (and they get dropped by
    /// `is_http_url`).
    scheme: String,
    /// `<scheme>://<host>` — no path, no query, no fragment.
    origin: String,
    /// `<scheme>://<host>/<dir>/` — the page directory, always ending
    /// in `/`. Used for page-relative resolution.
    base_dir: String,
}

impl UrlContext {
    fn from_page_url(url: &str) -> Self {
        let (scheme, rest) = match url.split_once("://") {
            Some((s, r)) => (s.to_ascii_lowercase(), r),
            None => return Self::default(),
        };
        if !matches!(scheme.as_str(), "http" | "https") {
            return Self::default();
        }
        // `rest` looks like `host/path?q#f` or just `host`.
        let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
        let host = &rest[..authority_end];
        if host.is_empty() {
            return Self::default();
        }
        let origin = format!("{scheme}://{host}");

        // Extract the path (before `?` and `#`), then keep everything
        // up to and including the last `/` as the base directory.
        let path_start = authority_end;
        let after_authority = &rest[path_start..];
        let path_only = after_authority
            .split('#')
            .next()
            .unwrap_or("")
            .split('?')
            .next()
            .unwrap_or("");
        let dir = match path_only.rfind('/') {
            Some(idx) => &path_only[..=idx],
            None => "/",
        };
        let base_dir = format!("{origin}{dir}");

        Self {
            scheme,
            origin,
            base_dir,
        }
    }

    fn resolve(&self, raw: &str) -> String {
        if raw.starts_with("http://") || raw.starts_with("https://") {
            raw.to_string()
        } else if let Some(tail) = raw.strip_prefix("//") {
            // Protocol-relative: inherit the page scheme instead of
            // hardcoding https so http-only pages keep working.
            let scheme = if self.scheme.is_empty() {
                "https"
            } else {
                &self.scheme
            };
            format!("{scheme}://{tail}")
        } else if raw.starts_with('/') {
            // Root-relative: attach to the origin.
            format!("{}{}", self.origin, raw)
        } else if self.base_dir.is_empty() {
            // No base directory to resolve against — return the raw
            // path; `is_http_url` will drop it.
            raw.to_string()
        } else {
            // Page-relative: attach to the page directory so nested
            // pages keep their asset paths intact.
            format!("{}{}", self.base_dir, raw)
        }
    }
}

/// Return true if the raw href is a non-resolvable scheme such as
/// `data:`, `blob:`, `javascript:`, `mailto:`, `tel:`, `file:`. These
/// must never be prefixed with an origin during relative resolution.
fn has_non_http_scheme(raw: &str) -> bool {
    // A scheme is `<alpha>[<alnum/+/-/.>]*:` at the start of the URL.
    // If it matches *and* it is not `http` or `https`, reject.
    let colon = match raw.find(':') {
        Some(i) => i,
        None => return false,
    };
    // Rule out `//` (protocol-relative) which has no scheme prefix.
    if raw.starts_with("//") {
        return false;
    }
    let scheme = &raw[..colon];
    // Use `map_or` rather than `unwrap()` so that reordering this
    // check ahead of the `is_empty()` guard cannot introduce a panic.
    if !scheme
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic())
    {
        return false;
    }
    if !scheme
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
    {
        return false;
    }
    let lower = scheme.to_ascii_lowercase();
    lower != "http" && lower != "https"
}

fn is_http_url(url: &str) -> bool {
    url.starts_with("http://") || url.starts_with("https://")
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Imgur ──────────────────────────────────────────────────────────────
    const IMGUR_ALBUM_JSON: &str = r#"{
        "data": [
            {
                "id": "img1",
                "title": "first",
                "link": "https://i.imgur.com/img1.jpg",
                "width": 1920,
                "height": 1080
            },
            {
                "id": "img2",
                "title": null,
                "link": "https://i.imgur.com/img2.png",
                "width": 800,
                "height": 600
            }
        ],
        "status": 200,
        "success": true
    }"#;

    const IMGUR_FAILED_JSON: &str = r#"{
        "data": [],
        "status": 404,
        "success": false
    }"#;

    #[test]
    fn imgur_album_extracts_all_images() {
        let links = parse_imgur_album(IMGUR_ALBUM_JSON).unwrap();
        assert_eq!(links.len(), 2);
        assert_eq!(links[0].url, "https://i.imgur.com/img1.jpg");
        assert_eq!(links[0].width, Some(1920));
        assert_eq!(links[0].height, Some(1080));
        assert_eq!(links[0].title.as_deref(), Some("first"));
    }

    #[test]
    fn imgur_failed_response_maps_to_http_error() {
        let err = parse_imgur_album(IMGUR_FAILED_JSON).unwrap_err();
        assert!(matches!(err, PluginError::HttpStatus { status: 404, .. }));
    }

    #[test]
    fn build_imgur_request_sets_client_id_header() {
        let req = build_imgur_album_request("abc123", "MY_CLIENT").unwrap();
        assert!(req.contains("https://api.imgur.com/3/album/abc123/images"));
        assert!(req.contains("Authorization"));
        assert!(req.contains("Client-ID MY_CLIENT"));
    }

    // ── Reddit ─────────────────────────────────────────────────────────────
    const REDDIT_GALLERY_JSON: &str = r#"[
        {"data": {"children": [
            {"data": {
                "title": "cool pics",
                "is_gallery": true,
                "media_metadata": {
                    "id1": {"s": {"u": "https://preview.redd.it/a.jpg?sig=1&amp;s=2", "x": 1200, "y": 800}, "m": "image/jpg"},
                    "id2": {"s": {"u": "https://preview.redd.it/b.jpg", "x": 1920, "y": 1080}, "m": "image/jpg"}
                }
            }}
        ]}},
        {"data": {"children": []}}
    ]"#;

    const REDDIT_SINGLE_IMAGE_JSON: &str = r#"[
        {"data": {"children": [
            {"data": {
                "title": "neato",
                "url": "https://i.redd.it/example.png",
                "preview": {
                    "images": [
                        {"source": {"url": "https://preview.redd.it/example.png?sig=xyz", "width": 800, "height": 600}}
                    ]
                }
            }}
        ]}},
        {"data": {"children": []}}
    ]"#;

    #[test]
    fn reddit_gallery_extracts_all_images_in_order() {
        let links = parse_reddit_submission(REDDIT_GALLERY_JSON).unwrap();
        assert_eq!(links.len(), 2);
        // Unescaped ampersand roundtrip
        assert_eq!(links[0].url, "https://preview.redd.it/a.jpg?sig=1&s=2");
        assert_eq!(links[0].width, Some(1200));
        assert_eq!(links[0].height, Some(800));
    }

    #[test]
    fn reddit_single_image_uses_preview_source() {
        let links = parse_reddit_submission(REDDIT_SINGLE_IMAGE_JSON).unwrap();
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].width, Some(800));
        assert_eq!(links[0].height, Some(600));
    }

    #[test]
    fn reddit_empty_listing_is_not_an_error() {
        let raw = r#"[{"data": {"children": []}}, {"data": {"children": []}}]"#;
        let links = parse_reddit_submission(raw).unwrap();
        assert!(links.is_empty());
    }

    // ── Flickr ─────────────────────────────────────────────────────────────
    const FLICKR_SET_JSON: &str = r#"{
        "photoset": {
            "id": "72177",
            "photo": [
                {
                    "id": "1",
                    "title": "pic1",
                    "url_o": "https://live.staticflickr.com/1.jpg",
                    "width_o": "4000",
                    "height_o": "3000"
                },
                {
                    "id": "2",
                    "title": "pic2",
                    "url_l": "https://live.staticflickr.com/2_l.jpg",
                    "width_o": 2048,
                    "height_o": 1365
                }
            ]
        },
        "stat": "ok"
    }"#;

    const FLICKR_ERROR_JSON: &str = r#"{
        "photoset": {"photo": []},
        "stat": "fail"
    }"#;

    #[test]
    fn flickr_photoset_extracts_all_photos() {
        let links = parse_flickr_photoset(FLICKR_SET_JSON).unwrap();
        assert_eq!(links.len(), 2);
        assert_eq!(links[0].width, Some(4000));
        assert_eq!(links[0].height, Some(3000));
        assert_eq!(links[1].url, "https://live.staticflickr.com/2_l.jpg");
    }

    #[test]
    fn flickr_error_stat_is_rejected() {
        let err = parse_flickr_photoset(FLICKR_ERROR_JSON).unwrap_err();
        assert!(matches!(err, PluginError::HttpStatus { .. }));
    }

    #[test]
    fn build_flickr_request_encodes_extras() {
        let req = build_flickr_request("72177", "KEY").unwrap();
        assert!(req.contains("photoset_id=72177"));
        assert!(req.contains("api_key=KEY"));
        assert!(req.contains("extras=url_o%2Curl_l"));
    }

    // ── Generic HTML ───────────────────────────────────────────────────────
    #[test]
    fn generic_html_scrapes_img_tags_page_relative() {
        let html = r#"
            <html>
              <body>
                <img src="https://cdn.example.com/1.jpg" alt="one">
                <img src='/rel/2.png' alt="two">
                <img src="//cdn2.example.com/3.webp" alt="three">
                <img src="data:image/png;base64,...">
                <img class="logo" src="relative/4.gif">
              </body>
            </html>
        "#;
        // The page lives in `/gallery/` so `relative/4.gif` should
        // resolve against that directory, not the origin root.
        let links = parse_generic_html(html, "https://example.com/gallery/page.html");
        let urls: Vec<_> = links.iter().map(|l| l.url.as_str()).collect();
        assert_eq!(
            urls,
            vec![
                "https://cdn.example.com/1.jpg",
                "https://example.com/rel/2.png",
                "https://cdn2.example.com/3.webp",
                "https://example.com/gallery/relative/4.gif",
            ]
        );
    }

    #[test]
    fn generic_html_protocol_relative_inherits_http_scheme() {
        // Page served over plain HTTP must NOT upgrade protocol-relative
        // images to https — that would break http-only assets.
        let html = r#"<img src="//cdn.example.com/a.jpg">"#;
        let links = parse_generic_html(html, "http://example.com/page");
        assert_eq!(links[0].url, "http://cdn.example.com/a.jpg");
    }

    #[test]
    fn generic_html_root_page_page_relative_uses_origin_root() {
        // When the page has no directory segment, relative paths
        // resolve against `/` directly.
        let html = r#"<img src="foo.jpg">"#;
        let links = parse_generic_html(html, "https://example.com");
        assert_eq!(links[0].url, "https://example.com/foo.jpg");
    }

    #[test]
    fn url_context_ignores_query_and_fragment_on_base() {
        let ctx = UrlContext::from_page_url("https://example.com/a/b?q=1#f");
        assert_eq!(ctx.origin, "https://example.com");
        assert_eq!(ctx.base_dir, "https://example.com/a/");
    }

    #[test]
    fn has_non_http_scheme_rejects_data_javascript_mailto() {
        assert!(has_non_http_scheme("data:image/png;base64,AAAA"));
        assert!(has_non_http_scheme("javascript:alert(1)"));
        assert!(has_non_http_scheme("mailto:a@b.com"));
        assert!(has_non_http_scheme("blob:https://x"));
        assert!(!has_non_http_scheme("http://x/y"));
        assert!(!has_non_http_scheme("https://x/y"));
        assert!(!has_non_http_scheme("//cdn.example.com/a.jpg"));
        assert!(!has_non_http_scheme("/relative"));
        assert!(!has_non_http_scheme("no-colon-here"));
    }

    #[test]
    fn flickr_failure_envelope_surfaces_as_provider_error() {
        // Bad API key / private album / missing set → `stat: "fail"`
        // with no `photoset` field. Must map to PluginError::HttpStatus.
        let raw = r#"{
            "stat": "fail",
            "code": 100,
            "message": "Invalid API Key"
        }"#;
        let err = parse_flickr_photoset(raw).unwrap_err();
        match err {
            PluginError::HttpStatus { status, message } => {
                assert_eq!(status, 100);
                assert_eq!(message, "Invalid API Key");
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn reddit_fallback_detects_image_with_query_string() {
        // Reddit single-image submissions without a `preview` field
        // use the submission URL directly. That URL may carry a CDN
        // signing query string, so the extension check must ignore it.
        let raw = r#"[
            {"data": {"children": [
                {"data": {
                    "title": "shot",
                    "url": "https://i.redd.it/example.png?sig=abc123"
                }}
            ]}},
            {"data": {"children": []}}
        ]"#;
        let links = parse_reddit_submission(raw).unwrap();
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].url, "https://i.redd.it/example.png?sig=abc123");
    }

    #[test]
    fn http_response_parse_envelope() {
        let raw = r#"{"status": 200, "headers": {}, "body": "ok"}"#;
        let resp = parse_http_response(raw).unwrap();
        assert_eq!(resp.status, 200);
    }
}
