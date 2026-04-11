//! WASM-only module: `#[plugin_fn]` exports and `#[host_fn]` imports.

use extism_pdk::*;

use crate::error::PluginError;
use crate::providers::{
    build_flickr_request, build_generic_request, build_imgur_album_request, build_reddit_request,
    parse_flickr_photoset, parse_generic_html, parse_http_response, parse_imgur_album,
    parse_reddit_submission,
};
use crate::url_matcher::{
    classify_url, extract_flickr_album_id, extract_imgur_id, extract_reddit_permalink, Provider,
};
use crate::{
    bool_to_string, ensure_recognised_url, finalize_links, handle_can_handle,
    handle_supports_playlist,
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
    let provider = ensure_recognised_url(&url).map_err(error_to_fn_error)?;

    let images = match provider {
        Provider::Imgur => {
            let album = extract_imgur_id(&url)
                .ok_or_else(|| error_to_fn_error(PluginError::UnsupportedUrl(url.clone())))?;
            let client_id = read_config("imgur_client_id");
            let body = http_get(build_imgur_album_request(&album, &client_id))?;
            parse_imgur_album(&body).map_err(error_to_fn_error)?
        }
        Provider::Reddit => {
            let json_url = extract_reddit_permalink(&url)
                .ok_or_else(|| error_to_fn_error(PluginError::UnsupportedUrl(url.clone())))?;
            let body = http_get(build_reddit_request(&json_url))?;
            parse_reddit_submission(&body).map_err(error_to_fn_error)?
        }
        Provider::Flickr => {
            let (_, album) = extract_flickr_album_id(&url)
                .ok_or_else(|| error_to_fn_error(PluginError::UnsupportedUrl(url.clone())))?;
            let api_key = read_config("flickr_api_key");
            let body = http_get(build_flickr_request(&album, &api_key))?;
            parse_flickr_photoset(&body).map_err(error_to_fn_error)?
        }
        Provider::Generic => {
            // `ensure_recognised_url` already rejected Generic — this
            // arm is only reachable if the classifier changes. Surface
            // a clear error so a future contributor gets an obvious
            // nudge rather than silent wrong behaviour.
            return Err(error_to_fn_error(PluginError::UnsupportedUrl(
                "generic HTML fallback is not wired into extract_links".into(),
            )));
        }
    };

    let album_id = album_id_for(provider, &url);
    // Keep the runtime fallback in sync with the `min_resolution`
    // default declared in `plugin.toml` — 800×600. If the manifest is
    // ever edited, update this literal at the same time.
    let min_res = read_config_or("min_resolution", "800x600");
    let auto_name = read_bool_config("auto_name", true);

    let response = finalize_links(provider, &album_id, images, &min_res, auto_name)
        .map_err(error_to_fn_error)?;
    Ok(serde_json::to_string(&response)?)
}

/// Scrape `<img>` tags from an arbitrary HTML page. This entry point is
/// intentionally separate from `extract_links` because the generic
/// fallback must be explicit — the host calls it only when no
/// recognised crawler matched the URL.
#[plugin_fn]
pub fn extract_generic(url: String) -> FnResult<String> {
    // Generic fallback still requires http(s)
    if classify_url(&url).is_none() {
        return Err(error_to_fn_error(PluginError::UnsupportedUrl(url)));
    }

    let body = http_get(build_generic_request(&url))?;
    let images = parse_generic_html(&body, &url);

    // Keep the runtime fallback in sync with the `min_resolution`
    // default declared in `plugin.toml` — 800×600. If the manifest is
    // ever edited, update this literal at the same time.
    let min_res = read_config_or("min_resolution", "800x600");
    let auto_name = read_bool_config("auto_name", true);
    let album_id = "page";

    let response = finalize_links(Provider::Generic, album_id, images, &min_res, auto_name)
        .map_err(error_to_fn_error)?;
    // Override `kind` to signal this came from the generic fallback.
    let json = serde_json::to_string(&response)?;
    Ok(json)
}

#[plugin_fn]
pub fn is_http_url(url: String) -> FnResult<String> {
    Ok(bool_to_string(classify_url(&url).is_some()))
}

// ── Host function wiring ──────────────────────────────────────────────────────

fn http_get(req_json: Result<String, PluginError>) -> FnResult<String> {
    let req = req_json.map_err(error_to_fn_error)?;
    // SAFETY: `http_request` is resolved by the Vortex plugin host at
    // load time (see src-tauri/src/adapters/driven/plugin/host_functions.rs:
    // `make_http_request_function`). Invariants:
    //   1. The host registers `http_request` in the `ExtismHost`
    //      namespace before any `#[plugin_fn]` export is callable — a
    //      missing symbol would abort `Plugin::new` in extism_loader.rs
    //      and prevent the plugin from being loaded.
    //   2. The ABI is `(I64) -> I64` — a single u64 Extism memory
    //      handle in, a single u64 handle out. The `#[host_fn]` macro
    //      marshals `String` to/from the memory handle.
    //   3. The host enforces the `http` capability from the manifest
    //      before invoking the implementation; rejections return an
    //      error which `?` propagates safely.
    //   4. Inputs/outputs are owned, serialisable JSON strings — no
    //      aliasing or mutability concerns.
    let raw = unsafe { http_request(req)? };
    let resp = parse_http_response(&raw).map_err(error_to_fn_error)?;
    resp.into_success_body().map_err(error_to_fn_error)
}

fn album_id_for(provider: Provider, url: &str) -> String {
    match provider {
        Provider::Imgur => extract_imgur_id(url).unwrap_or_default(),
        Provider::Flickr => extract_flickr_album_id(url)
            .map(|(_, a)| a)
            .unwrap_or_default(),
        Provider::Reddit => {
            // Use the final path segment as a readable album id.
            url.trim_end_matches('/')
                .rsplit('/')
                .next()
                .unwrap_or("reddit")
                .to_string()
        }
        Provider::Generic => "page".to_string(),
    }
}

fn read_config(key: &str) -> String {
    // SAFETY: `get_config` is registered host-side before plugin exports
    // run (see src-tauri/src/adapters/driven/plugin/host_functions.rs:
    // `make_get_config_function`). Invariants:
    //   1. The host registers the symbol in the `ExtismHost` namespace
    //      before any `#[plugin_fn]` export is callable — a missing
    //      symbol would abort `Plugin::new` in extism_loader.rs.
    //   2. The ABI is `(I64) -> I64`; the `#[host_fn]` macro marshals
    //      `String` in/out.
    //   3. The host returns an empty string when the key is unknown or
    //      an error for transient failures; both are mapped to the
    //      empty default so the plugin can surface a clean
    //      `PluginError::HttpStatus` downstream.
    //   4. Inputs/outputs are owned JSON strings — no aliasing concerns.
    unsafe { get_config(key.to_string()) }.unwrap_or_default()
}

fn read_config_or(key: &str, default: &str) -> String {
    let v = read_config(key);
    if v.is_empty() {
        default.to_string()
    } else {
        v
    }
}

fn read_bool_config(key: &str, default: bool) -> bool {
    let v = read_config(key);
    match v.to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" => true,
        "false" | "0" | "no" => false,
        _ => default,
    }
}

fn error_to_fn_error(err: PluginError) -> WithReturnCode<extism_pdk::Error> {
    extism_pdk::Error::msg(err.to_string()).into()
}
