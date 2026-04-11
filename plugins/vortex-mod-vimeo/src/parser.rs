//! Vimeo oEmbed + player config parsing.
//!
//! Two data sources are consulted for a video:
//!
//! 1. **oEmbed endpoint** (`https://vimeo.com/api/oembed.json?url=…`):
//!    always-public JSON with title, description, thumbnail, duration,
//!    html embed code. Works for both public and private-link videos.
//!
//! 2. **Player config JSON** (embedded in the video page HTML inside a
//!    `window.playerConfig = {…};` script tag or fetched from
//!    `https://player.vimeo.com/video/<id>/config`): carries the
//!    progressive download URLs and available quality variants.
//!
//! The oEmbed endpoint alone is enough to populate metadata, so the
//! plugin can still return `MediaLink`s when the page HTML is blocked.
//! The quality variants only appear when the player config is available.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::error::PluginError;

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
        } else if self.status == 401 || self.status == 403 {
            Err(PluginError::Private(format!("status {}", self.status)))
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

// ── oEmbed response ───────────────────────────────────────────────────────────

/// Partial mapping of the Vimeo oEmbed JSON schema.
#[derive(Debug, Deserialize, PartialEq, Eq)]
pub struct OembedResponse {
    /// `"video"` for a single video. Other values are treated as errors.
    #[serde(rename = "type")]
    pub kind: String,
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub author_name: Option<String>,
    #[serde(default)]
    pub author_url: Option<String>,
    #[serde(default)]
    pub thumbnail_url: Option<String>,
    #[serde(default)]
    pub duration: Option<u64>,
    #[serde(default)]
    pub video_id: Option<u64>,
}

pub fn parse_oembed(raw: &str) -> Result<OembedResponse, PluginError> {
    let parsed: OembedResponse =
        serde_json::from_str(raw).map_err(|e| PluginError::ParseJson(e.to_string()))?;
    if parsed.kind != "video" {
        return Err(PluginError::UnsupportedUrl(format!(
            "oEmbed kind '{}' is not a video",
            parsed.kind
        )));
    }
    Ok(parsed)
}

// ── Player config ─────────────────────────────────────────────────────────────

/// Partial mapping of the Vimeo player config JSON schema.
///
/// Full schema is huge; only the fields required to enumerate progressive
/// download URLs and the HLS manifest are captured here.
#[derive(Debug, Deserialize)]
pub struct PlayerConfig {
    pub request: RequestConfig,
    #[serde(default)]
    pub video: Option<VideoMeta>,
}

#[derive(Debug, Deserialize)]
pub struct RequestConfig {
    pub files: FilesConfig,
}

#[derive(Debug, Deserialize, Default)]
pub struct FilesConfig {
    #[serde(default)]
    pub progressive: Vec<ProgressiveEntry>,
    #[serde(default)]
    pub hls: Option<HlsEntry>,
    #[serde(default)]
    pub dash: Option<HlsEntry>,
}

#[derive(Debug, Deserialize)]
pub struct ProgressiveEntry {
    pub profile: Option<serde_json::Value>,
    pub quality: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub fps: Option<f64>,
    pub mime: Option<String>,
    pub url: String,
}

#[derive(Debug, Deserialize)]
pub struct HlsEntry {
    #[serde(default)]
    pub cdns: HashMap<String, CdnEntry>,
    #[serde(default)]
    pub default_cdn: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CdnEntry {
    pub url: String,
    #[serde(default)]
    pub avc_url: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct VideoMeta {
    pub id: Option<u64>,
    pub title: Option<String>,
    pub duration: Option<u64>,
    pub thumbs: Option<HashMap<String, String>>,
}

pub fn parse_player_config(raw: &str) -> Result<PlayerConfig, PluginError> {
    serde_json::from_str(raw).map_err(|e| PluginError::ParseJson(e.to_string()))
}

/// Extract the `{…}` block from a `window.playerConfig = {…};` assignment
/// embedded in the Vimeo page HTML.
///
/// Uses a balanced-brace scan rather than a regex because the JSON payload
/// can contain nested braces inside string literals; a naive `.*?` regex
/// would match the first `}` inside a description field.
///
/// Tracks both `"` and `'` as string delimiters so that a JavaScript
/// object with mixed quoting (not strictly JSON but valid JS) still
/// extracts correctly.
///
/// The marker is anchored to `window.playerConfig` / `playerConfig =`
/// rather than the bare word, so a stray `<meta name="playerConfig">`
/// earlier in the document cannot derail the scan.
pub fn extract_player_config_from_html(html: &str) -> Result<&str, PluginError> {
    // Prefer the canonical assignment pattern; fall back to "playerConfig ="
    // in case Vimeo ever drops the `window.` prefix.
    //
    // Both markers require that the next character is not an identifier
    // continuation (alphanumeric or `_`), so that similarly named
    // variables like `window.playerConfigVersion` or
    // `playerConfigDetail =` do not match before the real assignment.
    const CANONICAL: &str = "window.playerConfig";
    const FALLBACK: &str = "playerConfig =";
    let start_marker = find_at_word_boundary(html, CANONICAL)
        .or_else(|| find_at_word_boundary(html, FALLBACK))
        .ok_or(PluginError::PlayerConfigNotFound)?;

    // Find the first `{` after the marker.
    let rest = &html[start_marker..];
    let brace_rel = rest.find('{').ok_or(PluginError::PlayerConfigNotFound)?;
    let brace_start = start_marker + brace_rel;

    // Walk the bytes, counting unescaped braces outside string literals.
    let bytes = html.as_bytes();
    let mut depth = 0i32;
    let mut in_double = false;
    let mut in_single = false;
    let mut escaped = false;
    let mut end = None;
    for (i, &b) in bytes.iter().enumerate().skip(brace_start) {
        if escaped {
            escaped = false;
            continue;
        }
        let in_str = in_double || in_single;
        match b {
            b'\\' if in_str => escaped = true,
            b'"' if !in_single => in_double = !in_double,
            b'\'' if !in_double => in_single = !in_single,
            b'{' if !in_str => depth += 1,
            b'}' if !in_str => {
                depth -= 1;
                if depth == 0 {
                    end = Some(i);
                    break;
                }
            }
            _ => {}
        }
    }
    let end_idx = end.ok_or(PluginError::PlayerConfigNotFound)?;
    Ok(&html[brace_start..=end_idx])
}

// ── Request builders ──────────────────────────────────────────────────────────

/// Return the byte offset of the first occurrence of `needle` in
/// `haystack` that is **not** followed by a JavaScript identifier
/// continuation character (`[A-Za-z0-9_$]`). JavaScript allows `$` as
/// an identifier character, so `window.playerConfig$legacy` must not
/// satisfy the word boundary for `window.playerConfig`. The check
/// uses a right-hand boundary only — the left-hand side does not need
/// one because both markers here start with a literal character
/// (`w` or `p`) that is already preceded by whitespace or punctuation
/// in every realistic HTML context.
fn find_at_word_boundary(haystack: &str, needle: &str) -> Option<usize> {
    let mut start = 0usize;
    while start < haystack.len() {
        let rel = haystack[start..].find(needle)?;
        let abs = start + rel;
        let after = abs + needle.len();
        let next_ok = haystack
            .as_bytes()
            .get(after)
            .is_none_or(|b| !is_js_ident_continue(*b));
        if next_ok {
            return Some(abs);
        }
        start = abs + needle.len();
    }
    None
}

/// JavaScript ASCII identifier-continuation check.
///
/// Full Unicode identifiers are out of scope for the HTML-embedded
/// `playerConfig` marker scan — Vimeo's page always uses plain ASCII
/// for the assignment — but `$` must be included alongside the
/// standard `[A-Za-z0-9_]` class because it is a legal identifier
/// character in JavaScript and appears in minified bundles.
fn is_js_ident_continue(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'$'
}

pub fn build_oembed_request(video_url: &str) -> Result<String, PluginError> {
    let url = format!(
        "https://vimeo.com/api/oembed.json?url={}",
        urlencode(video_url)
    );
    let req = HttpRequest {
        method: "GET".into(),
        url,
        headers: HashMap::new(),
        body: None,
    };
    Ok(serde_json::to_string(&req)?)
}

pub fn build_player_config_request(video_id: &str) -> Result<String, PluginError> {
    let url = format!("https://player.vimeo.com/video/{video_id}/config");
    let req = HttpRequest {
        method: "GET".into(),
        url,
        headers: HashMap::new(),
        body: None,
    };
    Ok(serde_json::to_string(&req)?)
}

pub fn parse_http_response(raw: &str) -> Result<HttpResponse, PluginError> {
    serde_json::from_str(raw).map_err(|e| PluginError::HostResponse(e.to_string()))
}

fn urlencode(s: &str) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

    const OEMBED_JSON: &str = r#"{
        "type": "video",
        "version": "1.0",
        "title": "Sintel trailer",
        "description": "Third open movie by the Blender Foundation.",
        "author_name": "Blender Foundation",
        "author_url": "https://vimeo.com/blender",
        "thumbnail_url": "https://i.vimeocdn.com/video/1.jpg",
        "duration": 52,
        "video_id": 123456789
    }"#;

    const PLAYER_CONFIG_JSON: &str = r#"{
        "request": {
            "files": {
                "progressive": [
                    {
                        "profile": 164,
                        "quality": "360p",
                        "width": 640,
                        "height": 360,
                        "fps": 24.0,
                        "mime": "video/mp4",
                        "url": "https://vod.vimeo.com/360.mp4"
                    },
                    {
                        "profile": 165,
                        "quality": "720p",
                        "width": 1280,
                        "height": 720,
                        "fps": 24.0,
                        "mime": "video/mp4",
                        "url": "https://vod.vimeo.com/720.mp4"
                    },
                    {
                        "profile": 174,
                        "quality": "1080p",
                        "width": 1920,
                        "height": 1080,
                        "fps": 24.0,
                        "mime": "video/mp4",
                        "url": "https://vod.vimeo.com/1080.mp4"
                    }
                ],
                "hls": {
                    "cdns": {
                        "akfire": {
                            "url": "https://akamai.vimeo.com/master.m3u8",
                            "avc_url": "https://akamai.vimeo.com/avc.m3u8"
                        }
                    },
                    "default_cdn": "akfire"
                }
            }
        },
        "video": { "id": 123456789, "title": "Sintel trailer", "duration": 52 }
    }"#;

    #[test]
    fn parse_oembed_accepts_video_type() {
        let r = parse_oembed(OEMBED_JSON).unwrap();
        assert_eq!(r.title, "Sintel trailer");
        assert_eq!(r.duration, Some(52));
        assert_eq!(r.video_id, Some(123456789));
    }

    #[test]
    fn parse_oembed_rejects_non_video_type() {
        let json = r#"{"type": "photo", "title": "x"}"#;
        let err = parse_oembed(json).unwrap_err();
        assert!(matches!(err, PluginError::UnsupportedUrl(_)));
    }

    #[test]
    fn parse_player_config_all_qualities() {
        let c = parse_player_config(PLAYER_CONFIG_JSON).unwrap();
        let qualities: Vec<_> = c
            .request
            .files
            .progressive
            .iter()
            .map(|e| e.quality.as_str())
            .collect();
        assert_eq!(qualities, vec!["360p", "720p", "1080p"]);
        assert!(c.request.files.hls.is_some());
    }

    #[test]
    fn player_config_heights_preserved() {
        let c = parse_player_config(PLAYER_CONFIG_JSON).unwrap();
        let heights: Vec<_> = c
            .request
            .files
            .progressive
            .iter()
            .map(|e| e.height)
            .collect();
        assert_eq!(heights, vec![Some(360), Some(720), Some(1080)]);
    }

    #[test]
    fn extract_player_config_simple_brace_balanced() {
        let html = r#"<html><script>window.playerConfig = {"a":1,"b":{"c":"}"}};</script></html>"#;
        let json = extract_player_config_from_html(html).unwrap();
        assert_eq!(json, r#"{"a":1,"b":{"c":"}"}}"#);
    }

    #[test]
    fn extract_player_config_escaped_quote_in_string() {
        let html = r#"playerConfig = {"title":"he said \"hi\"","n":1};"#;
        let json = extract_player_config_from_html(html).unwrap();
        assert_eq!(json, r#"{"title":"he said \"hi\"","n":1}"#);
    }

    #[test]
    fn extract_player_config_not_found() {
        let html = "<html><body>no config here</body></html>";
        let err = extract_player_config_from_html(html).unwrap_err();
        assert!(matches!(err, PluginError::PlayerConfigNotFound));
    }

    #[test]
    fn extract_player_config_handles_single_quoted_strings() {
        let html = r#"<script>window.playerConfig = {'url':'has}brace','n':1};</script>"#;
        let json = extract_player_config_from_html(html).unwrap();
        assert_eq!(json, r#"{'url':'has}brace','n':1}"#);
    }

    #[test]
    fn extract_player_config_skips_meta_tag_mention() {
        let html = r#"<meta name="playerConfig" content="legacy"><script>window.playerConfig = {"n":1};</script>"#;
        let json = extract_player_config_from_html(html).unwrap();
        assert_eq!(json, r#"{"n":1}"#);
    }

    #[test]
    fn extract_player_config_skips_similar_prefixes() {
        // `window.playerConfigVersion` must NOT be mistaken for the
        // real `window.playerConfig` assignment.
        let html = r#"
            <script>
              window.playerConfigVersion = {"legacy": true};
              window.playerConfig = {"real": true};
            </script>
        "#;
        let json = extract_player_config_from_html(html).unwrap();
        assert_eq!(json, r#"{"real": true}"#);
    }

    #[test]
    fn extract_player_config_rejects_dollar_sign_identifier_continuation() {
        // `$` is a legal JavaScript identifier character, so
        // `window.playerConfig$legacy` must not be mistaken for
        // `window.playerConfig`.
        let html = r#"
            <script>
              window.playerConfig$legacy = {"legacy": true};
              window.playerConfig = {"real": true};
            </script>
        "#;
        let json = extract_player_config_from_html(html).unwrap();
        assert_eq!(json, r#"{"real": true}"#);
    }

    #[test]
    fn extract_player_config_skips_similar_prefixes_for_fallback_marker() {
        // Fallback `playerConfig =` must also observe the word boundary.
        let html = r#"
            <script>
              playerConfigDetail = {"legacy": true};
              playerConfig = {"real": true};
            </script>
        "#;
        let json = extract_player_config_from_html(html).unwrap();
        assert_eq!(json, r#"{"real": true}"#);
    }

    #[test]
    fn build_oembed_request_url_encoded() {
        let req = build_oembed_request("https://vimeo.com/123456789").unwrap();
        assert!(req.contains("\"method\":\"GET\""));
        assert!(req.contains("url=https%3A%2F%2Fvimeo.com%2F123456789"));
    }

    #[test]
    fn build_player_config_request_shape() {
        let req = build_player_config_request("123456789").unwrap();
        assert!(req.contains("https://player.vimeo.com/video/123456789/config"));
    }

    #[test]
    fn http_response_private_when_401() {
        let r = HttpResponse {
            status: 401,
            headers: HashMap::new(),
            body: "x".into(),
        };
        assert!(matches!(
            r.into_success_body().unwrap_err(),
            PluginError::Private(_)
        ));
    }
}
