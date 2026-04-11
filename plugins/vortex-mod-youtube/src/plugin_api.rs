//! WASM-only module: `#[plugin_fn]` exports and `#[host_fn]` imports.
//!
//! This module is gated behind `cfg(target_family = "wasm")` because the
//! macros emit code that only compiles for a WASM target (e.g. `cdylib`
//! exports, `extern "ExtismHost"` linkage). Pure logic lives in sibling
//! modules so that it can be unit-tested natively.

use extism_pdk::*;

use crate::error::PluginError;
use crate::extractor::{
    build_subprocess_request, parse_subprocess_response, yt_dlp_args_for_playlist,
    yt_dlp_args_for_single_video,
};
use crate::metadata::{parse_flat_playlist, parse_single_video};
use crate::url_matcher::{classify_url, UrlKind};
use crate::{
    build_media_variants_response, build_playlist_response, build_single_video_response,
    ensure_youtube_url, handle_can_handle, handle_supports_playlist,
};

// ── Host function imports ─────────────────────────────────────────────────────

#[host_fn]
extern "ExtismHost" {
    /// JSON in → JSON out — see `SubprocessRequest` / `SubprocessResponse`.
    fn run_subprocess(req: String) -> String;
}

// ── Plugin function exports ───────────────────────────────────────────────────

/// Returns `"true"` if the URL is any form of recognised YouTube resource.
#[plugin_fn]
pub fn can_handle(url: String) -> FnResult<String> {
    Ok(handle_can_handle(&url))
}

/// Returns `"true"` if the URL refers to a playlist or channel.
#[plugin_fn]
pub fn supports_playlist(url: String) -> FnResult<String> {
    Ok(handle_supports_playlist(&url))
}

/// Extract media links from a single video or playlist URL.
///
/// Dispatches to `yt-dlp --dump-json` (single video) or
/// `yt-dlp --dump-json --flat-playlist` (playlist / channel).
#[plugin_fn]
pub fn extract_links(url: String) -> FnResult<String> {
    ensure_youtube_url(&url).map_err(error_to_fn_error)?;

    let kind = classify_url(&url);
    let response = match kind {
        UrlKind::Playlist | UrlKind::Channel => {
            let stdout = call_yt_dlp(yt_dlp_args_for_playlist(&url))?;
            let playlist = parse_flat_playlist(&stdout).map_err(error_to_fn_error)?;
            build_playlist_response(playlist)
        }
        UrlKind::Video | UrlKind::Shorts => {
            let stdout = call_yt_dlp(yt_dlp_args_for_single_video(&url))?;
            let video = parse_single_video(&stdout).map_err(error_to_fn_error)?;
            build_single_video_response(video)
        }
        UrlKind::Unknown => {
            return Err(error_to_fn_error(PluginError::UnsupportedUrl(url)));
        }
    };

    Ok(serde_json::to_string(&response)?)
}

/// List available media formats for a single video URL.
#[plugin_fn]
pub fn get_media_variants(url: String) -> FnResult<String> {
    ensure_youtube_url(&url).map_err(error_to_fn_error)?;

    let stdout = call_yt_dlp(yt_dlp_args_for_single_video(&url))?;
    let video = parse_single_video(&stdout).map_err(error_to_fn_error)?;
    let variants = build_media_variants_response(video);
    Ok(serde_json::to_string(&variants)?)
}

/// Extract a flat playlist listing.
#[plugin_fn]
pub fn extract_playlist(url: String) -> FnResult<String> {
    ensure_youtube_url(&url).map_err(error_to_fn_error)?;

    let stdout = call_yt_dlp(yt_dlp_args_for_playlist(&url))?;
    let playlist = parse_flat_playlist(&stdout).map_err(error_to_fn_error)?;
    let response = build_playlist_response(playlist);
    Ok(serde_json::to_string(&response)?)
}

// ── Host function wiring ──────────────────────────────────────────────────────

fn call_yt_dlp(args: Vec<String>) -> FnResult<String> {
    let req_json = build_subprocess_request(args).map_err(error_to_fn_error)?;
    // SAFETY: `run_subprocess` is resolved by the Vortex plugin host at
    // load time (see src-tauri/src/adapters/driven/plugin/host_functions.rs:
    // `make_run_subprocess_function`). Invariants:
    //   1. The host registers the symbol `run_subprocess` in the
    //      `ExtismHost` namespace before any `#[plugin_fn]` export is
    //      callable — a missing symbol would abort `Plugin::new` in
    //      extism_loader.rs and prevent the plugin from being loaded.
    //   2. The ABI is `(I64) -> I64` — a single u64 Extism memory
    //      handle in, a single u64 handle out. The `#[host_fn]` macro
    //      generates the correct marshalling from `String` to/from the
    //      memory handle.
    //   3. Host-side capability check rejects calls when the plugin
    //      manifest does not declare `subprocess:yt-dlp`; the host
    //      returns an error, which the `?` below propagates safely.
    //   4. `run_subprocess` has no aliasing or mutability concerns —
    //      inputs and outputs are owned, serialisable JSON strings.
    let resp_json = unsafe { run_subprocess(req_json)? };
    parse_subprocess_response(&resp_json).map_err(error_to_fn_error)
}

fn error_to_fn_error(err: PluginError) -> WithReturnCode<extism_pdk::Error> {
    extism_pdk::Error::msg(err.to_string()).into()
}
