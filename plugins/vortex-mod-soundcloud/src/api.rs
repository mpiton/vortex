//! SoundCloud API request / response types.
//!
//! The SoundCloud `/resolve` endpoint returns JSON with very permissive shape
//! — many fields are optional depending on the resource kind and the
//! visibility (public/private, go+/regular). This module models only the
//! subset of fields we care about and tolerates unknown keys.
//!
//! ## HTTP host-function envelope
//!
//! The plugin wraps every outgoing request in an [`HttpRequest`] JSON and
//! expects an [`HttpResponse`] back from the host. The schemas mirror
//! `src-tauri/src/adapters/driven/plugin/host_functions.rs`.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::error::PluginError;

// ── Host function envelope ────────────────────────────────────────────────────

/// Matches `HttpRequest` in `host_functions.rs`.
#[derive(Debug, Serialize)]
pub struct HttpRequest {
    pub method: String,
    pub url: String,
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub headers: HashMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
}

/// Matches `HttpResponse` in `host_functions.rs`.
#[derive(Debug, Deserialize)]
pub struct HttpResponse {
    pub status: u16,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    #[serde(default)]
    pub body: String,
}

impl HttpResponse {
    /// Returns the body if the status is 2xx, else a typed error.
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

pub fn build_resolve_request(original_url: &str, client_id: &str) -> Result<String, PluginError> {
    let resolve_url = format!(
        "https://api-v2.soundcloud.com/resolve?url={}&client_id={}",
        urlencode(original_url),
        urlencode(client_id),
    );
    let req = HttpRequest {
        method: "GET".into(),
        url: resolve_url,
        headers: HashMap::new(),
        body: None,
    };
    Ok(serde_json::to_string(&req)?)
}

pub fn parse_http_response(raw: &str) -> Result<HttpResponse, PluginError> {
    serde_json::from_str(raw).map_err(|e| PluginError::HostResponse(e.to_string()))
}

/// Minimal URL-encode for query parameters (RFC 3986 unreserved + '%').
///
/// A full percent-encoder would pull in an extra dependency for just two
/// call sites; the lookup table here covers every byte the resolve
/// endpoint accepts in a `url=` query string.
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

// ── SoundCloud resource types ─────────────────────────────────────────────────

/// `/resolve` response envelope discriminated by the `kind` field.
///
/// Known kinds: `track`, `playlist`, `user`. Unknown kinds are mapped to
/// [`ResolveResponse::Unknown`] so the plugin can surface a clear error
/// instead of panicking.
#[derive(Debug, Deserialize)]
#[serde(tag = "kind")]
pub enum ResolveResponse {
    #[serde(rename = "track")]
    Track(Track),
    #[serde(rename = "playlist")]
    Playlist(Playlist),
    #[serde(rename = "user")]
    User(User),
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Deserialize)]
pub struct Track {
    pub id: u64,
    pub title: String,
    #[serde(default)]
    pub duration: Option<u64>,
    #[serde(default)]
    pub permalink_url: Option<String>,
    #[serde(default)]
    pub artwork_url: Option<String>,
    #[serde(default)]
    pub user: Option<TrackUser>,
    #[serde(default)]
    pub streamable: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct TrackUser {
    pub username: String,
}

#[derive(Debug, Deserialize)]
pub struct Playlist {
    pub id: u64,
    pub title: String,
    #[serde(default)]
    pub permalink_url: Option<String>,
    #[serde(default)]
    pub artwork_url: Option<String>,
    #[serde(default)]
    pub tracks: Vec<Track>,
    #[serde(default)]
    pub track_count: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub struct User {
    pub id: u64,
    pub username: String,
    #[serde(default)]
    pub permalink_url: Option<String>,
    #[serde(default)]
    pub avatar_url: Option<String>,
}

pub fn parse_resolve_response(body: &str) -> Result<ResolveResponse, PluginError> {
    serde_json::from_str(body).map_err(|e| PluginError::ParseJson(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const TRACK_JSON: &str = r#"{
        "kind": "track",
        "id": 12345,
        "title": "Flickermood",
        "duration": 225000,
        "permalink_url": "https://soundcloud.com/forss/flickermood",
        "artwork_url": "https://i1.sndcdn.com/artworks-12345.jpg",
        "streamable": true,
        "user": { "username": "Forss" }
    }"#;

    const PLAYLIST_JSON: &str = r#"{
        "kind": "playlist",
        "id": 99,
        "title": "Soulhack",
        "permalink_url": "https://soundcloud.com/forss/sets/soulhack",
        "tracks": [
            {"kind": "track", "id": 1, "title": "Flickermood"},
            {"kind": "track", "id": 2, "title": "Journeyman"}
        ],
        "track_count": 2
    }"#;

    const USER_JSON: &str = r#"{
        "kind": "user",
        "id": 42,
        "username": "forss",
        "permalink_url": "https://soundcloud.com/forss",
        "avatar_url": "https://i1.sndcdn.com/avatars-42.jpg"
    }"#;

    const UNKNOWN_KIND_JSON: &str = r#"{"kind": "system-playlist", "id": 1}"#;

    #[test]
    fn parse_track_response() {
        let resolved = parse_resolve_response(TRACK_JSON).unwrap();
        match resolved {
            ResolveResponse::Track(t) => {
                assert_eq!(t.id, 12345);
                assert_eq!(t.title, "Flickermood");
                assert_eq!(t.duration, Some(225000));
                assert_eq!(t.user.unwrap().username, "Forss");
                assert!(t.artwork_url.is_some());
            }
            other => panic!("expected Track, got {other:?}"),
        }
    }

    #[test]
    fn parse_playlist_response() {
        let resolved = parse_resolve_response(PLAYLIST_JSON).unwrap();
        match resolved {
            ResolveResponse::Playlist(p) => {
                assert_eq!(p.id, 99);
                assert_eq!(p.title, "Soulhack");
                assert_eq!(p.tracks.len(), 2);
                assert_eq!(p.track_count, Some(2));
            }
            other => panic!("expected Playlist, got {other:?}"),
        }
    }

    #[test]
    fn parse_user_response() {
        let resolved = parse_resolve_response(USER_JSON).unwrap();
        match resolved {
            ResolveResponse::User(u) => {
                assert_eq!(u.username, "forss");
            }
            other => panic!("expected User, got {other:?}"),
        }
    }

    #[test]
    fn parse_unknown_kind_falls_through() {
        let resolved = parse_resolve_response(UNKNOWN_KIND_JSON).unwrap();
        assert!(matches!(resolved, ResolveResponse::Unknown));
    }

    #[test]
    fn parse_resolve_rejects_malformed_json() {
        let err = parse_resolve_response("not json").unwrap_err();
        assert!(matches!(err, PluginError::ParseJson(_)));
    }

    #[test]
    fn http_response_2xx_returns_body() {
        let resp = HttpResponse {
            status: 200,
            headers: HashMap::new(),
            body: "ok".into(),
        };
        assert_eq!(resp.into_success_body().unwrap(), "ok");
    }

    #[test]
    fn http_response_401_is_private() {
        let resp = HttpResponse {
            status: 401,
            headers: HashMap::new(),
            body: "forbidden".into(),
        };
        assert!(matches!(
            resp.into_success_body().unwrap_err(),
            PluginError::Private(_)
        ));
    }

    #[test]
    fn http_response_500_is_http_status_error() {
        let resp = HttpResponse {
            status: 500,
            headers: HashMap::new(),
            body: "boom".into(),
        };
        match resp.into_success_body().unwrap_err() {
            PluginError::HttpStatus { status, .. } => assert_eq!(status, 500),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn urlencode_roundtrips_safe_chars() {
        assert_eq!(urlencode("abc-_.~"), "abc-_.~");
        assert_eq!(
            urlencode("https://soundcloud.com/a/b"),
            "https%3A%2F%2Fsoundcloud.com%2Fa%2Fb"
        );
    }

    #[test]
    fn build_resolve_request_encodes_target() {
        let req_str =
            build_resolve_request("https://soundcloud.com/forss/flickermood", "abc123").unwrap();
        assert!(req_str.contains("\"method\":\"GET\""));
        assert!(req_str.contains("client_id=abc123"));
        assert!(req_str.contains("url=https%3A%2F%2Fsoundcloud.com%2Fforss%2Fflickermood"));
    }
}
