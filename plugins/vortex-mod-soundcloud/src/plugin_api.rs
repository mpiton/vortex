//! WASM-only module: `#[plugin_fn]` exports and `#[host_fn]` imports.
//!
//! Gated behind `cfg(target_family = "wasm")` because the macros emit
//! code that only compiles for a WASM target.

use extism_pdk::*;

use crate::api::{
    build_resolve_request, parse_http_response, parse_resolve_response, ResolveResponse,
};
use crate::error::PluginError;
use crate::{
    build_playlist_response, build_single_track_response, ensure_playlist, ensure_soundcloud_url,
    ensure_track, handle_can_handle, handle_supports_playlist, response_to_extract_links,
};

// ── Host function imports ─────────────────────────────────────────────────────

#[host_fn]
extern "ExtismHost" {
    /// JSON in → JSON out — see `HttpRequest` / `HttpResponse` envelopes.
    fn http_request(req: String) -> String;
    fn get_config(key: String) -> String;
}

// ── Plugin function exports ───────────────────────────────────────────────────

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
    ensure_soundcloud_url(&url).map_err(error_to_fn_error)?;

    let resolved = resolve(&url)?;
    let response = response_to_extract_links(resolved).map_err(error_to_fn_error)?;
    Ok(serde_json::to_string(&response)?)
}

#[plugin_fn]
pub fn extract_playlist(url: String) -> FnResult<String> {
    ensure_playlist(&url).map_err(error_to_fn_error)?;

    let resolved = resolve(&url)?;
    let response = match resolved {
        ResolveResponse::Playlist(p) => build_playlist_response(p),
        // Artist profiles need a second call; for now we surface a clear
        // error so the UI can paginate via a follow-up call when that
        // endpoint support lands.
        ResolveResponse::User(u) => {
            return Err(error_to_fn_error(PluginError::UnsupportedUrl(format!(
                "artist profile '{}' — artist pagination not yet implemented",
                u.username
            ))))
        }
        ResolveResponse::Track(_) => {
            return Err(error_to_fn_error(PluginError::UnsupportedUrl(
                "single track cannot be extracted as playlist".into(),
            )))
        }
        ResolveResponse::Unknown => {
            return Err(error_to_fn_error(PluginError::UnsupportedUrl(
                "unknown resource kind".into(),
            )))
        }
    };
    Ok(serde_json::to_string(&response)?)
}

#[plugin_fn]
pub fn extract_track(url: String) -> FnResult<String> {
    ensure_track(&url).map_err(error_to_fn_error)?;

    let resolved = resolve(&url)?;
    let response = match resolved {
        ResolveResponse::Track(t) => build_single_track_response(t),
        _ => {
            return Err(error_to_fn_error(PluginError::UnsupportedUrl(
                "resolved resource is not a track".into(),
            )))
        }
    };
    Ok(serde_json::to_string(&response)?)
}

// ── Host function wiring ──────────────────────────────────────────────────────

/// Issue a `/resolve` call against api-v2.soundcloud.com via the host and
/// return the parsed envelope.
fn resolve(url: &str) -> FnResult<ResolveResponse> {
    let client_id = read_client_id();
    let req_json = build_resolve_request(url, &client_id).map_err(error_to_fn_error)?;
    // SAFETY: `http_request` is resolved by the Vortex plugin host at
    // load time (see src-tauri/src/adapters/driven/plugin/host_functions.rs:
    // `make_http_request_function`). Invariants:
    //   1. The host registers `http_request` in the `ExtismHost` namespace
    //      before any `#[plugin_fn]` export is callable — a missing
    //      symbol would abort `Plugin::new` in extism_loader.rs.
    //   2. The ABI is `(I64) -> I64` — a single u64 Extism memory handle
    //      in, a single u64 handle out. The `#[host_fn]` macro marshals
    //      `String` to/from the memory handle.
    //   3. The host enforces capability `http=true` from the manifest
    //      before invoking the implementation; rejections return an
    //      error which `?` propagates safely.
    //   4. Inputs and outputs are owned, serialisable JSON strings — no
    //      aliasing or mutability concerns.
    let resp_json = unsafe { http_request(req_json)? };
    let response = parse_http_response(&resp_json).map_err(error_to_fn_error)?;
    let body = response.into_success_body().map_err(error_to_fn_error)?;
    parse_resolve_response(&body).map_err(error_to_fn_error)
}

/// Read the `client_id` config value. Returns an empty string if the
/// host has not yet wired `get_config` (forward-compatible with the
/// manifest parser, which currently ignores `[config]`).
fn read_client_id() -> String {
    // SAFETY: `get_config` is registered host-side before plugin exports
    // run (see src-tauri/src/adapters/driven/plugin/host_functions.rs:
    // `make_get_config_function`). Invariants:
    //   1. The symbol is registered in the `ExtismHost` namespace
    //      before any `#[plugin_fn]` export is callable.
    //   2. The ABI is `(I64) -> I64`; the `#[host_fn]` macro marshals
    //      `String` in/out.
    //   3. A missing key or transient error returns the empty default
    //      so the plugin still builds the URL — the host surfaces the
    //      401/403 via `http_request`, which `HttpResponse::into_success_body`
    //      maps to `PluginError::Private` and the user sees a clear
    //      "SoundCloud resource is private" error.
    //   4. Inputs/outputs are owned JSON strings — no aliasing concerns.
    unsafe { get_config("client_id".to_string()) }.unwrap_or_default()
}

fn error_to_fn_error(err: PluginError) -> WithReturnCode<extism_pdk::Error> {
    extism_pdk::Error::msg(err.to_string()).into()
}
