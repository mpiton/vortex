//! WASM-only module: `#[plugin_fn]` exports and `#[host_fn]` imports.

use extism_pdk::*;

use crate::error::PluginError;
use crate::parser::{
    build_oembed_request, build_player_config_request, extract_player_config_from_html,
    parse_http_response, parse_oembed, parse_player_config,
};
use crate::url_matcher::extract_video_id;
use crate::{
    build_media_variants_response, build_single_video_response, ensure_single_video,
    ensure_vimeo_url, filter_audio_only, handle_can_handle, handle_supports_playlist,
};

#[host_fn]
extern "ExtismHost" {
    fn http_request(req: String) -> String;
    fn get_config(key: String) -> String;
}

#[plugin_fn]
pub fn can_handle(url: String) -> FnResult<String> {
    Ok(handle_can_handle(&url))
}

#[plugin_fn]
pub fn supports_playlist(url: String) -> FnResult<String> {
    Ok(handle_supports_playlist(&url))
}

#[plugin_fn]
pub fn extract_links(url: String) -> FnResult<String> {
    ensure_vimeo_url(&url).map_err(error_to_fn_error)?;

    let oembed = fetch_oembed(&url)?;
    let response = build_single_video_response(oembed);
    Ok(serde_json::to_string(&response)?)
}

#[plugin_fn]
pub fn get_media_variants(url: String) -> FnResult<String> {
    ensure_single_video(&url).map_err(error_to_fn_error)?;

    let video_id = extract_video_id(&url)
        .ok_or_else(|| error_to_fn_error(PluginError::UnsupportedUrl(url.clone())))?;
    let config = fetch_player_config(&video_id)?;
    let variants = build_media_variants_response(config);
    let filtered = if audio_only_preference() {
        filter_audio_only(variants)
    } else {
        variants
    };
    Ok(serde_json::to_string(&filtered)?)
}

#[plugin_fn]
pub fn extract_playlist(_url: String) -> FnResult<String> {
    // Showcase / album extraction is not implemented in the MVP — the
    // oEmbed endpoint does not enumerate showcase entries and the
    // relevant API endpoint requires an access token. Return a clear
    // error so the UI can surface an appropriate message.
    Err(error_to_fn_error(PluginError::UnsupportedUrl(
        "showcase extraction is not implemented yet".into(),
    )))
}

// ── Host function wiring ──────────────────────────────────────────────────────

fn fetch_oembed(video_url: &str) -> FnResult<crate::parser::OembedResponse> {
    let req = build_oembed_request(video_url).map_err(error_to_fn_error)?;
    // SAFETY: `http_request` is resolved by the Vortex plugin host at
    // load time (see src-tauri/src/adapters/driven/plugin/host_functions.rs:
    // `make_http_request_function`). Invariants:
    //   1. The host registers `http_request` in the `ExtismHost`
    //      namespace before any `#[plugin_fn]` export is callable.
    //   2. The ABI is `(I64) -> I64`; the `#[host_fn]` macro marshals
    //      `String` in/out through Extism memory handles.
    //   3. The host gates the call on the `http` capability from
    //      `plugin.toml`; rejections return an error which `?` surfaces.
    //   4. Inputs/outputs are owned JSON strings — no aliasing.
    let raw = unsafe { http_request(req)? };
    let resp = parse_http_response(&raw).map_err(error_to_fn_error)?;
    let body = resp.into_success_body().map_err(error_to_fn_error)?;
    parse_oembed(&body).map_err(error_to_fn_error)
}

fn fetch_player_config(video_id: &str) -> FnResult<crate::parser::PlayerConfig> {
    let req = build_player_config_request(video_id).map_err(error_to_fn_error)?;
    // SAFETY: identical host-function invariants to `fetch_oembed`
    // above — the host-side symbol, ABI, capability gate, and owned
    // JSON I/O all apply unchanged. See `fetch_oembed` for the full
    // list.
    let raw = unsafe { http_request(req)? };
    let resp = parse_http_response(&raw).map_err(error_to_fn_error)?;
    let body = resp.into_success_body().map_err(error_to_fn_error)?;

    // Vimeo returns JSON directly for /config. If the body happens to be
    // an HTML page (e.g. geo-blocked fallback) try to extract the config
    // block before giving up.
    match parse_player_config(&body) {
        Ok(cfg) => Ok(cfg),
        Err(_) => {
            let json = extract_player_config_from_html(&body).map_err(error_to_fn_error)?;
            parse_player_config(json).map_err(error_to_fn_error)
        }
    }
}

fn audio_only_preference() -> bool {
    // SAFETY: `get_config` is registered host-side before plugin exports
    // run (see src-tauri/src/adapters/driven/plugin/host_functions.rs:
    // `make_get_config_function`). Invariants:
    //   1. The symbol is registered in the `ExtismHost` namespace
    //      before any `#[plugin_fn]` export is callable.
    //   2. The ABI is `(I64) -> I64`; the `#[host_fn]` macro marshals
    //      `String` in/out.
    //   3. A missing key or transient error yields the empty default
    //      which falls through to `false` — the documented default for
    //      `extract_audio_only`.
    //   4. Inputs/outputs are owned JSON strings — no aliasing concerns.
    let value = unsafe { get_config("extract_audio_only".to_string()) }.unwrap_or_default();
    matches!(value.to_ascii_lowercase().as_str(), "true" | "1" | "yes")
}

fn error_to_fn_error(err: PluginError) -> WithReturnCode<extism_pdk::Error> {
    extism_pdk::Error::msg(err.to_string()).into()
}
