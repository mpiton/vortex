//! Native unit tests for the pure-logic IPC entry points.
//!
//! The actual `#[plugin_fn]` wrappers live in `plugin_api.rs` (WASM only),
//! but they simply call these pure helpers, so covering them here is enough
//! to guarantee behaviour without running a WASM runtime.

#![cfg(test)]

use crate::metadata::{parse_flat_playlist, parse_single_video};
use crate::url_matcher::UrlKind;
use crate::{
    build_media_variants_response, build_playlist_response, build_single_video_response,
    ensure_playlist_or_channel, ensure_single_video, ensure_youtube_url, handle_can_handle,
    handle_supports_playlist,
};

const SINGLE_VIDEO_JSON: &str = r#"{
    "id": "dQw4w9WgXcQ",
    "title": "Never Gonna Give You Up",
    "description": "",
    "duration": 212,
    "webpage_url": "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
    "thumbnail": "https://i.ytimg.com/vi/dQw4w9WgXcQ/maxresdefault.jpg",
    "uploader": "Rick Astley",
    "formats": [
        {"format_id": "137", "ext": "mp4", "height": 1080, "vcodec": "avc1", "acodec": "none"},
        {"format_id": "140", "ext": "m4a", "vcodec": "none", "acodec": "mp4a.40.2", "abr": 128.0}
    ]
}"#;

const PLAYLIST_JSONL: &str = "{\"id\":\"v1\",\"title\":\"First\",\"webpage_url\":\"https://www.youtube.com/watch?v=v1\",\"playlist_id\":\"PL1\",\"playlist\":\"Demo\"}\n{\"id\":\"v2\",\"title\":\"Second\",\"url\":\"https://www.youtube.com/watch?v=v2\"}";

#[test]
fn can_handle_returns_true_for_youtube() {
    assert_eq!(
        handle_can_handle("https://www.youtube.com/watch?v=dQw4w9WgXcQ"),
        "true"
    );
}

#[test]
fn can_handle_returns_false_for_other_hosts() {
    assert_eq!(handle_can_handle("https://vimeo.com/12345"), "false");
}

#[test]
fn supports_playlist_true_for_playlist_url() {
    assert_eq!(
        handle_supports_playlist("https://www.youtube.com/playlist?list=PLxyz"),
        "true"
    );
}

#[test]
fn supports_playlist_true_for_channel_url() {
    assert_eq!(
        handle_supports_playlist("https://www.youtube.com/@MrBeast"),
        "true"
    );
}

#[test]
fn supports_playlist_false_for_single_video() {
    assert_eq!(
        handle_supports_playlist("https://www.youtube.com/watch?v=dQw4w9WgXcQ"),
        "false"
    );
}

#[test]
fn build_single_video_response_serialises_to_video_kind() {
    let info = parse_single_video(SINGLE_VIDEO_JSON).unwrap();
    let resp = build_single_video_response(info);
    let json = serde_json::to_string(&resp).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed["kind"], "video");
    assert_eq!(parsed["videos"].as_array().unwrap().len(), 1);
    assert_eq!(parsed["videos"][0]["id"], "dQw4w9WgXcQ");
    assert_eq!(parsed["videos"][0]["title"], "Never Gonna Give You Up");
    assert_eq!(parsed["videos"][0]["duration"], 212);
}

#[test]
fn build_playlist_response_preserves_entries() {
    let pl = parse_flat_playlist(PLAYLIST_JSONL).unwrap();
    let resp = build_playlist_response(pl);
    assert_eq!(resp.kind, "playlist");
    assert_eq!(resp.videos.len(), 2);
    assert_eq!(resp.videos[0].id, "v1");
    assert_eq!(resp.videos[1].id, "v2");
}

#[test]
fn build_media_variants_response_lists_both_formats() {
    let info = parse_single_video(SINGLE_VIDEO_JSON).unwrap();
    let resp = build_media_variants_response(info);
    let json = serde_json::to_string(&resp).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed["variants"].as_array().unwrap().len(), 2);
    assert_eq!(parsed["variants"][0]["format_id"], "137");
    assert_eq!(parsed["variants"][0]["kind"], "video_only");
    assert_eq!(parsed["variants"][1]["kind"], "audio_only");
}

#[test]
fn ensure_youtube_url_returns_kind_for_valid_url() {
    assert_eq!(
        ensure_youtube_url("https://youtu.be/dQw4w9WgXcQ").unwrap(),
        UrlKind::Video
    );
    assert_eq!(
        ensure_youtube_url("https://www.youtube.com/shorts/abcDEF12345").unwrap(),
        UrlKind::Shorts
    );
    assert_eq!(
        ensure_youtube_url("https://www.youtube.com/playlist?list=PLxyz").unwrap(),
        UrlKind::Playlist
    );
}

#[test]
fn ensure_youtube_url_err_for_invalid_url() {
    assert!(ensure_youtube_url("https://vimeo.com/12345").is_err());
}

#[test]
fn ensure_single_video_rejects_playlist() {
    assert!(
        ensure_single_video("https://www.youtube.com/playlist?list=PLxyz").is_err(),
        "playlist URL should be rejected by single-video guard"
    );
}

#[test]
fn ensure_single_video_accepts_video_and_shorts() {
    assert_eq!(
        ensure_single_video("https://youtu.be/dQw4w9WgXcQ").unwrap(),
        UrlKind::Video
    );
    assert_eq!(
        ensure_single_video("https://www.youtube.com/shorts/abcDEF12345").unwrap(),
        UrlKind::Shorts
    );
}

#[test]
fn ensure_playlist_or_channel_rejects_single_video() {
    assert!(
        ensure_playlist_or_channel("https://youtu.be/dQw4w9WgXcQ").is_err(),
        "single video URL should be rejected by playlist guard"
    );
}

#[test]
fn ensure_playlist_or_channel_accepts_both_kinds() {
    assert_eq!(
        ensure_playlist_or_channel("https://www.youtube.com/playlist?list=PLxyz").unwrap(),
        UrlKind::Playlist
    );
    assert_eq!(
        ensure_playlist_or_channel("https://www.youtube.com/@MrBeast").unwrap(),
        UrlKind::Channel
    );
}

#[test]
fn can_handle_rejects_unknown_youtube_path() {
    // /embed/... is a YouTube host but not a supported kind — must not
    // advertise support for it, otherwise dispatcher will later fail.
    assert_eq!(
        handle_can_handle("https://www.youtube.com/embed/dQw4w9WgXcQ"),
        "false"
    );
}
