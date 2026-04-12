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
    // Vimeo's `/config` endpoint returns strict JSON, so the happy
    // path is a direct `serde_json::from_str`. But the HTML-embedded
    // player config (the fallback path used when /config is blocked
    // or geo-rewritten) is a JavaScript object literal, and that
    // format tolerates single-quoted strings — serde_json does not.
    //
    // When the strict parse fails, attempt a best-effort normalisation
    // from JS object literal → JSON: convert unescaped `'` tokens
    // outside already-double-quoted strings into `"`. The result is
    // then re-parsed with serde_json. The normalisation is safe in
    // the sense that a well-formed JSON input passes through
    // unchanged (no `'` outside strings, so nothing to rewrite).
    match serde_json::from_str(raw) {
        Ok(cfg) => Ok(cfg),
        Err(_) => {
            let normalised = js_object_literal_to_json(raw);
            serde_json::from_str(&normalised).map_err(|e| PluginError::ParseJson(e.to_string()))
        }
    }
}

/// Convert a JavaScript object literal into valid JSON by rewriting
/// single-quoted string delimiters to double quotes.
///
/// The scanner walks the input **by `char`** (not by byte) so that
/// non-ASCII metadata embedded in the player config — e.g. a video
/// title like `"Éclair — intro"` with accented characters, emoji, or
/// full-width punctuation — round-trips through the rewrite intact.
/// Iterating bytes and casting each to `char` would corrupt any
/// multi-byte UTF-8 code unit by splitting it across multiple
/// 1-character pushes.
///
/// State tracks whether we are currently inside a `"`-delimited
/// string (so `"don't"` is not rewritten) and whether the previous
/// character was a backslash (so `\'` inside a single-quoted string
/// keeps its meaning as an escaped quote). When a `'` is encountered
/// outside a double-quoted string, the scanner toggles an `in_single`
/// flag and emits `"` instead. Escape sequences inside a
/// single-quoted string are re-emitted verbatim, except that `\'`
/// becomes `'` (a literal apostrophe inside what is now a
/// double-quoted string).
///
/// This handles the shapes the balanced-brace extractor can return:
/// - pure JSON (pass-through — no `'` to rewrite)
/// - JS object with single-quoted strings (`{'url':'a.mp4'}`)
/// - mixed (`{'a':"b",'c':1}`)
///
/// It does **not** handle keyword identifiers as keys
/// (`{url: 'a'}` — no quotes around `url`), because Vimeo's player
/// config always quotes its keys. If that ever changes, extend this
/// function to also rewrite `[A-Za-z_][A-Za-z0-9_]*\s*:` key shapes.
fn js_object_literal_to_json(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut in_double = false;
    let mut in_single = false;
    let mut escaped = false;

    for c in input.chars() {
        if escaped {
            // Inside a single-quoted string, `\'` collapses to `'`
            // (literal apostrophe). Inside a double-quoted string,
            // every escape is preserved verbatim.
            if in_single && c == '\'' {
                out.push('\'');
            } else {
                out.push('\\');
                out.push(c);
            }
            escaped = false;
            continue;
        }
        match c {
            '\\' if in_double || in_single => {
                escaped = true;
            }
            '"' if !in_single => {
                in_double = !in_double;
                out.push('"');
            }
            '\'' if !in_double => {
                // Toggle the single-quote state and emit a double
                // quote in its place.
                in_single = !in_single;
                out.push('"');
            }
            // Inside a single-quoted string, a literal `"` character
            // must be escaped when emitted into the JSON output so
            // the reparser does not see it as an end-of-string.
            '"' if in_single => {
                out.push_str("\\\"");
            }
            _ => out.push(c),
        }
    }
    out
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
    // Both markers require an identifier boundary on **both** sides,
    // so that similarly named variables like `window.playerConfigVersion`
    // or `mywindow.playerConfig` do not match before the real
    // assignment.
    //
    // Additionally, for the `CANONICAL` marker we insist on an `=`
    // operator between the end of the needle and the next `{`. This
    // rejects non-assignment references such as
    // `console.log(window.playerConfig)` which happen to appear
    // before the real assignment in the HTML. The `FALLBACK` needle
    // already contains the `=`, so the gap check is a no-op for it.
    const CANONICAL: &str = "window.playerConfig";
    const FALLBACK: &str = "playerConfig =";
    let (start_marker, needle_len) =
        find_assignment_marker(html, CANONICAL, RequireAssignment::Yes)
            .or_else(|| find_assignment_marker(html, FALLBACK, RequireAssignment::No))
            .ok_or(PluginError::PlayerConfigNotFound)?;

    // Find the first `{` after the marker that is outside any string
    // literal. A plain `rest.find('{')` would pick up `{` inside a
    // string like `"style={...}"`, pointing the balanced-brace scanner
    // at the wrong position. Since the marker search guarantees we are
    // outside a string at `needle_end`, the walk starts clean.
    let needle_end = start_marker + needle_len;
    let brace_start = find_brace_outside_strings(html.as_bytes(), needle_end)
        .ok_or(PluginError::PlayerConfigNotFound)?;

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

/// Whether the caller requires the gap between the needle end and
/// the next `{` to contain an `=` operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RequireAssignment {
    /// Scan the gap for an `=` operator. Use this when the needle
    /// itself is bare (e.g. `window.playerConfig`).
    Yes,
    /// The needle already contains the `=` operator — skip the gap
    /// scan. Use this for markers like `playerConfig =`.
    No,
}

/// Find the first `needle` occurrence in `haystack` that:
///
/// 1. is **outside** any JavaScript string literal (both `"` and `'`
///    delimiters are tracked from position 0 so the quote state is
///    never lost — a marker that appears inside a string like
///    `"debug: window.playerConfig = ..."` is skipped);
/// 2. is bounded on **both** sides by non-identifier characters; and
/// 3. if `require_assignment == Yes`, is followed (before the first
///    `{` that is also outside any string) by a bare `=` operator
///    that is not part of `==`, `===`, `!=`, `!==`, `<=`, `>=`, `=>`.
///
/// The function does a **single pass** over the haystack, tracking
/// JavaScript string state throughout, so there is no need to slice
/// a gap and re-parse it. This eliminates the class of bugs where
/// a gap substring resets quote state at its start.
///
/// Returns `(byte_offset, needle_length)`.
fn find_assignment_marker(
    haystack: &str,
    needle: &str,
    require_assignment: RequireAssignment,
) -> Option<(usize, usize)> {
    let bytes = haystack.as_bytes();
    let needle_bytes = needle.as_bytes();
    let nlen = needle_bytes.len();
    if nlen == 0 || bytes.len() < nlen {
        return None;
    }

    let mut in_double = false;
    let mut in_single = false;
    let mut escaped = false;
    let mut i = 0;

    while i < bytes.len() {
        let b = bytes[i];

        // Handle escape inside strings.
        if escaped {
            escaped = false;
            i += 1;
            continue;
        }
        let in_str = in_double || in_single;
        if in_str && b == b'\\' {
            escaped = true;
            i += 1;
            continue;
        }
        if b == b'"' && !in_single {
            in_double = !in_double;
            i += 1;
            continue;
        }
        if b == b'\'' && !in_double {
            in_single = !in_single;
            i += 1;
            continue;
        }
        // Skip all bytes inside strings — the needle, `=`, and `{`
        // must all be outside strings to count.
        if in_str {
            i += 1;
            continue;
        }

        // Outside any string: check if needle starts here.
        if i + nlen <= bytes.len() && bytes[i..i + nlen] == *needle_bytes {
            let abs = i;
            let after = abs + nlen;

            // Left boundary.
            let left_ok = abs == 0 || !is_js_ident_continue(bytes[abs - 1]);
            // Right boundary.
            let right_ok = bytes.get(after).is_none_or(|b| !is_js_ident_continue(*b));

            if left_ok && right_ok {
                let assignment_ok = match require_assignment {
                    RequireAssignment::No => true,
                    RequireAssignment::Yes => {
                        // Continue the *same* string-state walk from
                        // `after` (which is guaranteed outside any
                        // string at this point) to find the first `{`
                        // outside strings, checking for a bare `=`
                        // along the way.
                        gap_has_assignment_then_brace(bytes, after)
                    }
                };
                if assignment_ok {
                    return Some((abs, nlen));
                }
            }
            // Skip past the needle so the outer loop resumes after it
            // (prevents re-matching the same position).
            i = after;
            continue;
        }

        i += 1;
    }
    None
}

/// Starting from `start` (guaranteed outside any string by the caller),
/// walk `bytes` tracking JS string state. Return `true` if a bare `=`
/// (outside strings, not part of `==`/`===`/`!=`/`!==`/`<=`/`>=`/`=>`)
/// is found before the first `{` (also outside strings). Return `false`
/// if `{` arrives before `=`, or if there is no `{` at all.
fn gap_has_assignment_then_brace(bytes: &[u8], start: usize) -> bool {
    let mut in_double = false;
    let mut in_single = false;
    let mut escaped = false;
    let mut found_eq = false;
    let mut i = start;

    while i < bytes.len() {
        let b = bytes[i];
        if escaped {
            escaped = false;
            i += 1;
            continue;
        }
        let in_str = in_double || in_single;
        if in_str && b == b'\\' {
            escaped = true;
            i += 1;
            continue;
        }
        if b == b'"' && !in_single {
            in_double = !in_double;
            i += 1;
            continue;
        }
        if b == b'\'' && !in_double {
            in_single = !in_single;
            i += 1;
            continue;
        }
        if in_str {
            i += 1;
            continue;
        }
        // Outside any string.
        if b == b'{' {
            return found_eq;
        }
        if b == b'=' {
            let prev = if i == start { 0 } else { bytes[i - 1] };
            let next = bytes.get(i + 1).copied().unwrap_or(0);
            if !matches!(prev, b'=' | b'!' | b'<' | b'>') && !matches!(next, b'=' | b'>') {
                found_eq = true;
            }
        }
        i += 1;
    }
    false
}

/// Find the first `{` in `bytes` starting from `start` that is
/// outside any JS string literal. Returns `None` if there is no `{`
/// outside strings. The caller must ensure `start` is outside a
/// string (this invariant is upheld by `find_assignment_marker`,
/// which only yields needle positions that are outside strings).
fn find_brace_outside_strings(bytes: &[u8], start: usize) -> Option<usize> {
    let mut in_double = false;
    let mut in_single = false;
    let mut escaped = false;
    let mut i = start;
    while i < bytes.len() {
        let b = bytes[i];
        if escaped {
            escaped = false;
            i += 1;
            continue;
        }
        let in_str = in_double || in_single;
        if in_str && b == b'\\' {
            escaped = true;
            i += 1;
            continue;
        }
        if b == b'"' && !in_single {
            in_double = !in_double;
            i += 1;
            continue;
        }
        if b == b'\'' && !in_double {
            in_single = !in_single;
            i += 1;
            continue;
        }
        if !in_str && b == b'{' {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// Standalone wrapper around `gap_has_assignment_then_brace` for
/// direct unit testing. The gap is treated as starting outside any
/// string — the `find_assignment_marker` single-pass scan guarantees
/// this invariant for real call sites. A trailing `{` sentinel is
/// appended so the helper can terminate.
#[cfg(test)]
fn gap_contains_assignment(gap: &str) -> bool {
    let with_brace = format!("{gap}{{");
    gap_has_assignment_then_brace(with_brace.as_bytes(), 0)
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
    fn parse_player_config_accepts_single_quoted_js_literal() {
        // Vimeo's HTML-embedded player config can be a JS object
        // literal with single-quoted strings. `parse_player_config`
        // must normalise this into JSON before handing it to serde.
        let raw = r#"{
            'request': {
                'files': {
                    'progressive': [
                        {
                            'profile': 164,
                            'quality': '720p',
                            'width': 1280,
                            'height': 720,
                            'fps': 24.0,
                            'mime': 'video/mp4',
                            'url': 'https://vod.vimeo.com/720.mp4'
                        }
                    ]
                }
            }
        }"#;
        let c = parse_player_config(raw).unwrap();
        assert_eq!(c.request.files.progressive.len(), 1);
        assert_eq!(c.request.files.progressive[0].quality, "720p");
        assert_eq!(
            c.request.files.progressive[0].url,
            "https://vod.vimeo.com/720.mp4"
        );
    }

    #[test]
    fn parse_player_config_accepts_mixed_quoting() {
        let raw = r#"{
            "request": {
                "files": {
                    'progressive': [
                        {"profile": 1, "quality": "360p", "url": 'https://vod.vimeo.com/360.mp4'}
                    ]
                }
            }
        }"#;
        let c = parse_player_config(raw).unwrap();
        assert_eq!(
            c.request.files.progressive[0].url,
            "https://vod.vimeo.com/360.mp4"
        );
    }

    #[test]
    fn js_object_literal_preserves_double_quoted_apostrophe() {
        let input = r#"{"title":"don't stop"}"#;
        let out = js_object_literal_to_json(input);
        // Strict JSON pass-through — no `'` outside strings, nothing rewritten.
        assert_eq!(out, input);
    }

    #[test]
    fn js_object_literal_preserves_utf8_content() {
        // A title with accented characters, em-dashes, and emoji must
        // round-trip through the rewrite without corruption. Iterating
        // bytes and casting each to char would split multi-byte UTF-8
        // sequences across multiple `push` calls.
        let input = r#"{'title':'Éclair — intro 🎬','n':1}"#;
        let out = js_object_literal_to_json(input);
        assert_eq!(out, r#"{"title":"Éclair — intro 🎬","n":1}"#);
        // And it should parse as valid JSON.
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["title"], "Éclair — intro 🎬");
    }

    #[test]
    fn js_object_literal_preserves_utf8_inside_double_quoted() {
        // Double-quoted strings must also round-trip UTF-8 intact.
        let input = r#"{"title":"Élodie: «bonjour»"}"#;
        let out = js_object_literal_to_json(input);
        assert_eq!(out, r#"{"title":"Élodie: «bonjour»"}"#);
    }

    #[test]
    fn parse_player_config_accepts_js_literal_with_utf8() {
        let raw = r#"{
            'request': {
                'files': {
                    'progressive': [
                        {
                            'quality': '720p',
                            'url': 'https://vod.vimeo.com/720.mp4'
                        }
                    ]
                }
            },
            'video': {
                'title': 'Éclair — intro 🎬'
            }
        }"#;
        let c = parse_player_config(raw).unwrap();
        assert_eq!(c.request.files.progressive[0].quality, "720p");
        assert_eq!(c.video.unwrap().title.unwrap(), "Éclair — intro 🎬");
    }

    #[test]
    fn js_object_literal_converts_escaped_single_quote() {
        let input = r#"{'title':'it\'s fine'}"#;
        let out = js_object_literal_to_json(input);
        assert_eq!(out, r#"{"title":"it's fine"}"#);
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
    fn extract_player_config_rejects_left_boundary_violation() {
        // `mywindow.playerConfig` must not match `window.playerConfig`
        // because the byte before `window` is an identifier character.
        let html = r#"
            <script>
              mywindow.playerConfig = {"decoy": true};
              window.playerConfig = {"real": true};
            </script>
        "#;
        let json = extract_player_config_from_html(html).unwrap();
        assert_eq!(json, r#"{"real": true}"#);
    }

    #[test]
    fn extract_player_config_rejects_equality_comparison() {
        // `window.playerConfig === null` is not an assignment, but
        // the old `gap.contains('=')` check would accept it because
        // `===` contains `=`. The new `gap_contains_assignment`
        // helper rejects this.
        let html = r#"
            <script>
              if (window.playerConfig === null) { legacy(); }
              window.playerConfig = {"real": true};
            </script>
        "#;
        let json = extract_player_config_from_html(html).unwrap();
        assert_eq!(json, r#"{"real": true}"#);
    }

    #[test]
    fn extract_player_config_rejects_loose_equality_and_inequality() {
        let html_eq = r#"
            <script>
              if (window.playerConfig == null) { legacy(); }
              window.playerConfig = {"real": true};
            </script>
        "#;
        assert_eq!(
            extract_player_config_from_html(html_eq).unwrap(),
            r#"{"real": true}"#
        );

        let html_neq = r#"
            <script>
              if (window.playerConfig !== null) { /* ... */ }
              window.playerConfig = {"real": true};
            </script>
        "#;
        assert_eq!(
            extract_player_config_from_html(html_neq).unwrap(),
            r#"{"real": true}"#
        );
    }

    #[test]
    fn extract_player_config_rejects_arrow_function_reference() {
        // `window.playerConfig => { ... }` is syntactically nonsense,
        // but a gap with `=>` must still be rejected as non-assignment.
        let html = r#"
            <script>
              const cb = window.playerConfig => { legacy(); };
              window.playerConfig = {"real": true};
            </script>
        "#;
        let json = extract_player_config_from_html(html).unwrap();
        assert_eq!(json, r#"{"real": true}"#);
    }

    #[test]
    fn gap_contains_assignment_accepts_bare_equal() {
        assert!(gap_contains_assignment(" = "));
        assert!(gap_contains_assignment("="));
        assert!(gap_contains_assignment("\t= \n"));
    }

    #[test]
    fn gap_contains_assignment_ignores_equals_inside_string_literals() {
        // `=` inside a double-quoted string literal is not an
        // assignment — it is data. The scanner must ignore it so
        // that a decoy `"window.playerConfig = ..."` string cannot
        // fool the marker check.
        assert!(!gap_contains_assignment(
            r#" msg = "not = here" "#.split('=').next().unwrap()
        ));
        assert!(!gap_contains_assignment(r#" "has = inside" "#));
        assert!(!gap_contains_assignment(r#" 'single = quoted' "#));
        // A real `=` *after* the string must still be detected.
        assert!(gap_contains_assignment(r#" "prefix" = "#));
    }

    #[test]
    fn gap_contains_assignment_handles_escaped_quotes() {
        // Escaped quote inside a string does not close the string,
        // so the `=` that follows is still inside the literal.
        assert!(!gap_contains_assignment(r#" "it \"= inside\"" "#));
    }

    #[test]
    fn extract_player_config_skips_marker_inside_already_open_string() {
        // The marker `window.playerConfig` appears inside a string
        // that was already open before the marker starts. The
        // single-pass scanner must track string state from position 0
        // so it knows the marker is still inside the string, even
        // though `gap_contains_assignment` starts clean.
        let html = r#"
            <script>
              var x = "testing window.playerConfig = {bad: true} still in string";
              window.playerConfig = {"real": true};
            </script>
        "#;
        let json = extract_player_config_from_html(html).unwrap();
        assert_eq!(json, r#"{"real": true}"#);
    }

    #[test]
    fn extract_player_config_skips_decoy_inside_string_literal() {
        // A JavaScript snippet that embeds the playerConfig marker
        // inside a string literal must not be picked as the real
        // assignment site. The decoy `=` inside the string is now
        // ignored, so the balanced-brace scanner reaches the real
        // assignment below.
        let html = r#"
            <script>
              const msg = "debug: window.playerConfig = bogus";
              window.playerConfig = {"real": true};
            </script>
        "#;
        let json = extract_player_config_from_html(html).unwrap();
        assert_eq!(json, r#"{"real": true}"#);
    }

    #[test]
    fn gap_contains_assignment_rejects_comparisons_and_arrows() {
        assert!(!gap_contains_assignment(" == "));
        assert!(!gap_contains_assignment(" === "));
        assert!(!gap_contains_assignment(" != "));
        assert!(!gap_contains_assignment(" !== "));
        assert!(!gap_contains_assignment(" <= "));
        assert!(!gap_contains_assignment(" >= "));
        assert!(!gap_contains_assignment(" => "));
        assert!(!gap_contains_assignment(""));
        assert!(!gap_contains_assignment("no equals here"));
    }

    #[test]
    fn extract_player_config_rejects_non_assignment_reference() {
        // A reference like `console.log(window.playerConfig)` appears
        // before the real assignment. The scanner must walk past it
        // because the gap between the needle end and the next `{`
        // does not contain an `=` operator.
        let html = r#"
            <script>
              console.log(window.playerConfig);
              function f() {}
              window.playerConfig = {"real": true};
            </script>
        "#;
        let json = extract_player_config_from_html(html).unwrap();
        assert_eq!(json, r#"{"real": true}"#);
    }

    #[test]
    fn extract_player_config_fallback_marker_still_works() {
        // The FALLBACK marker `playerConfig =` already contains `=`
        // so the gap check is skipped — it must still find the
        // assignment.
        let html = r#"<script>playerConfig = {"fallback": true};</script>"#;
        let json = extract_player_config_from_html(html).unwrap();
        assert_eq!(json, r#"{"fallback": true}"#);
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
