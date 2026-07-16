//! Host function implementations for WASM plugins.

use std::collections::HashMap;
use std::io::Read;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::capabilities::{CredentialSlot, PluginHostContext};
use super::ytdlp_broker::{
    LegacySubprocessRequest, PluginYtDlpRequest, run_legacy_request, run_plugin_request,
};
use crate::adapters::driven::network::validate_public_url;

// ── JSON types ────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct HttpRequest {
    method: String,
    url: String,
    #[serde(default)]
    headers: HashMap<String, String>,
    body: Option<String>,
}

#[derive(Serialize)]
struct HttpResponse {
    status: u16,
    headers: HashMap<String, String>,
    body: String,
}

#[derive(Deserialize)]
struct LogRequest {
    level: String,
    message: String,
}

#[derive(Deserialize)]
struct ConfigEntry {
    key: String,
    value: String,
}

#[derive(Serialize)]
struct CredentialResponse {
    username: String,
    password: String,
}

const MAX_HTTP_BODY_BYTES: u64 = 100 * 1024 * 1024;

// ── Helpers ───────────────────────────────────────────────────────────────────

fn read_input_string(
    plugin: &mut extism::CurrentPlugin,
    inputs: &[extism::Val],
) -> Result<String, extism::Error> {
    let bytes: Vec<u8> = plugin.memory_get_val(&inputs[0])?;
    String::from_utf8(bytes).map_err(|e| anyhow::anyhow!("invalid utf-8 input: {e}"))
}

fn write_output_string(
    plugin: &mut extism::CurrentPlugin,
    outputs: &mut [extism::Val],
    value: &str,
) -> Result<(), extism::Error> {
    plugin.memory_set_val(&mut outputs[0], value.as_bytes())
}

fn safe_plugin_log_message(
    message: &str,
    credential_slot: &CredentialSlot,
) -> Result<String, extism::Error> {
    let carries_credential = credential_slot
        .lock()
        .map_err(|_| anyhow::anyhow!("log: credential slot poisoned"))?
        .is_some();
    Ok(if carries_credential {
        "plugin log redacted during credential operation".into()
    } else {
        message.to_string()
    })
}

fn read_http_body_capped(
    response: &mut reqwest::blocking::Response,
) -> Result<Vec<u8>, extism::Error> {
    let mut limited_reader = response.take(MAX_HTTP_BODY_BYTES + 1);
    let mut body_bytes = Vec::new();
    limited_reader
        .read_to_end(&mut body_bytes)
        .map_err(|e| anyhow::anyhow!("http_request: failed to read body: {e}"))?;

    if body_bytes.len() as u64 > MAX_HTTP_BODY_BYTES {
        return Err(anyhow::anyhow!(
            "http_request: response body exceeds 100 MB limit"
        ));
    }

    Ok(body_bytes)
}

// ── Host functions ────────────────────────────────────────────────────────────

/// Route plugin log messages through the `tracing` framework.
pub fn make_log_function(user_data: extism::UserData<PluginHostContext>) -> extism::Function {
    extism::Function::new(
        "log",
        [extism::ValType::I64],
        [],
        user_data,
        |plugin, inputs, _outputs, ud| {
            let input = read_input_string(plugin, inputs)?;
            let req: LogRequest = serde_json::from_str(&input)
                .map_err(|e| anyhow::anyhow!("log: invalid JSON: {e}"))?;
            let guard = ud.get()?;
            let ctx = guard
                .lock()
                .map_err(|_| anyhow::anyhow!("log: mutex poisoned"))?;
            let plugin_name = ctx.plugin_name.as_str();
            let message = safe_plugin_log_message(&req.message, &ctx.credential_slot)?;
            match req.level.as_str() {
                "error" => tracing::error!(plugin = plugin_name, "{}", message),
                "warn" => tracing::warn!(plugin = plugin_name, "{}", message),
                "debug" => tracing::debug!(plugin = plugin_name, "{}", message),
                _ => tracing::info!(plugin = plugin_name, "{}", message),
            }
            Ok(())
        },
    )
}

/// Execute an HTTP request using the shared blocking client.
pub fn make_http_request_function(
    user_data: extism::UserData<PluginHostContext>,
) -> extism::Function {
    extism::Function::new(
        "http_request",
        [extism::ValType::I64],
        [extism::ValType::I64],
        user_data,
        |plugin, inputs, outputs, ud| {
            let input = read_input_string(plugin, inputs)?;
            let req: HttpRequest = serde_json::from_str(&input)
                .map_err(|e| anyhow::anyhow!("http_request: invalid JSON: {e}"))?;

            let method = reqwest::Method::from_bytes(req.method.as_bytes())
                .map_err(|_| anyhow::anyhow!("http_request: invalid method: {}", req.method))?;

            let url: reqwest::Url = req
                .url
                .parse()
                .map_err(|e| anyhow::anyhow!("http_request: invalid URL '{}': {e}", req.url))?;

            // F1: SSRF protection — reject internal/loopback destinations
            let resolved_addrs =
                validate_public_url(&url).map_err(|e| anyhow::anyhow!("http_request: {e}"))?;

            // F6: Minimize mutex scope — prepare the client, then release the lock
            let client = {
                let guard = ud.get()?;
                let ctx = guard
                    .lock()
                    .map_err(|_| anyhow::anyhow!("http_request: mutex poisoned"))?;
                match (url.host_str(), resolved_addrs.as_deref()) {
                    (Some(host), Some(addrs)) => {
                        ctx.shared.http_client_for_host(host, addrs).map_err(|e| {
                            anyhow::anyhow!("http_request: failed to build client: {e}")
                        })?
                    }
                    _ => ctx.shared.http_client().clone(),
                }
            }; // Mutex released here — HTTP call runs without holding the lock

            let mut builder = client.request(method, url);
            for (k, v) in &req.headers {
                builder = builder.header(k.as_str(), v.as_str());
            }
            if let Some(body) = req.body {
                builder = builder.body(body);
            }

            let mut response = builder
                .send()
                .map_err(|e| anyhow::anyhow!("http_request: request failed: {e}"))?;

            // F2: Check Content-Length before reading body into memory
            if let Some(len) = response.content_length()
                && len > MAX_HTTP_BODY_BYTES
            {
                return Err(anyhow::anyhow!(
                    "http_request: Content-Length {len} exceeds 100 MB limit"
                ));
            }

            let status = response.status().as_u16();
            let resp_headers: HashMap<String, String> = response
                .headers()
                .iter()
                .map(|(k, v)| (k.as_str().to_string(), v.to_str().unwrap_or("").to_string()))
                .collect();

            let body_bytes = read_http_body_capped(&mut response)?;
            let body = String::from_utf8_lossy(&body_bytes).into_owned();

            let http_resp = HttpResponse {
                status,
                headers: resp_headers,
                body,
            };
            let json = serde_json::to_string(&http_resp)
                .map_err(|e| anyhow::anyhow!("http_request: failed to serialize response: {e}"))?;

            write_output_string(plugin, outputs, &json)
        },
    )
}

/// Read a per-plugin config value by key.
pub fn make_get_config_function(
    user_data: extism::UserData<PluginHostContext>,
) -> extism::Function {
    extism::Function::new(
        "get_config",
        [extism::ValType::I64],
        [extism::ValType::I64],
        user_data,
        |plugin, inputs, outputs, ud| {
            let key = read_input_string(plugin, inputs)?;
            let guard = ud.get()?;
            let ctx = guard
                .lock()
                .map_err(|_| anyhow::anyhow!("get_config: mutex poisoned"))?;

            let value = ctx
                .shared
                .plugin_configs()
                .get(&ctx.plugin_name)
                .and_then(|m| m.get(&key).map(|v| v.clone()))
                .unwrap_or_default();

            write_output_string(plugin, outputs, &value)
        },
    )
}

/// Store a per-plugin config key/value pair.
pub fn make_set_config_function(
    user_data: extism::UserData<PluginHostContext>,
) -> extism::Function {
    extism::Function::new(
        "set_config",
        [extism::ValType::I64],
        [],
        user_data,
        |plugin, inputs, _outputs, ud| {
            let input = read_input_string(plugin, inputs)?;
            let entry: ConfigEntry = serde_json::from_str(&input)
                .map_err(|e| anyhow::anyhow!("set_config: invalid JSON: {e}"))?;

            let guard = ud.get()?;
            let ctx = guard
                .lock()
                .map_err(|_| anyhow::anyhow!("set_config: mutex poisoned"))?;

            ctx.shared
                .plugin_configs()
                .entry(ctx.plugin_name.clone())
                .or_default()
                .insert(entry.key, entry.value);

            Ok(())
        },
    )
}

/// Read a per-plugin ephemeral state value by key.
pub fn make_get_state_function(user_data: extism::UserData<PluginHostContext>) -> extism::Function {
    extism::Function::new(
        "get_state",
        [extism::ValType::I64],
        [extism::ValType::I64],
        user_data,
        |plugin, inputs, outputs, ud| {
            let key = read_input_string(plugin, inputs)?;
            let guard = ud.get()?;
            let ctx = guard
                .lock()
                .map_err(|_| anyhow::anyhow!("get_state: mutex poisoned"))?;

            let value = ctx
                .shared
                .plugin_states()
                .get(&ctx.plugin_name)
                .and_then(|m| m.get(&key).map(|v| v.clone()))
                .unwrap_or_default();

            write_output_string(plugin, outputs, &value)
        },
    )
}

/// Store a per-plugin ephemeral state key/value pair.
pub fn make_set_state_function(user_data: extism::UserData<PluginHostContext>) -> extism::Function {
    extism::Function::new(
        "set_state",
        [extism::ValType::I64],
        [],
        user_data,
        |plugin, inputs, _outputs, ud| {
            let input = read_input_string(plugin, inputs)?;
            let entry: ConfigEntry = serde_json::from_str(&input)
                .map_err(|e| anyhow::anyhow!("set_state: invalid JSON: {e}"))?;

            let guard = ud.get()?;
            let ctx = guard
                .lock()
                .map_err(|_| anyhow::anyhow!("set_state: mutex poisoned"))?;

            ctx.shared
                .plugin_states()
                .entry(ctx.plugin_name.clone())
                .or_default()
                .insert(entry.key, entry.value);

            Ok(())
        },
    )
}

/// Retrieve a credential from the store, scoped to the plugin's own service name.
pub fn make_get_credential_function(
    user_data: extism::UserData<PluginHostContext>,
) -> extism::Function {
    extism::Function::new(
        "get_credential",
        [extism::ValType::I64],
        [extism::ValType::I64],
        user_data,
        |plugin, inputs, outputs, ud| {
            let service = read_input_string(plugin, inputs)?;
            // F3: Scope credential access — plugins can only read credentials
            // matching their own name to prevent cross-plugin credential theft.
            let (slot, store, plugin_name) = {
                let guard = ud.get()?;
                let ctx = guard
                    .lock()
                    .map_err(|_| anyhow::anyhow!("get_credential: mutex poisoned"))?;

                if service != ctx.plugin_name {
                    return Err(anyhow::anyhow!(
                        "get_credential: plugin '{}' cannot access credentials for service '{service}'",
                        ctx.plugin_name
                    ));
                }

                (
                    Arc::clone(&ctx.credential_slot),
                    ctx.shared.credential_store().cloned(),
                    ctx.plugin_name.clone(),
                )
            };

            let scoped = slot
                .lock()
                .map_err(|_| anyhow::anyhow!("get_credential: credential slot poisoned"))?
                .clone();
            let cred = match scoped {
                Some(credential) => credential,
                None => store
                    .ok_or_else(|| {
                        anyhow::anyhow!("get_credential: no credential store configured")
                    })?
                    .get(&plugin_name)
                    .map_err(|e| anyhow::anyhow!("get_credential: store error: {e}"))?
                    .ok_or_else(|| anyhow::anyhow!("get_credential: no credential found"))?,
            };

            let resp = CredentialResponse {
                username: cred.username().to_string(),
                password: cred.password().to_string(),
            };
            let json = serde_json::to_string(&resp).map_err(|e| {
                anyhow::anyhow!("get_credential: failed to serialize response: {e}")
            })?;

            write_output_string(plugin, outputs, &json)
        },
    )
}

/// Run an approved yt-dlp operation built entirely by the host.
pub fn make_run_ytdlp_function(user_data: extism::UserData<PluginHostContext>) -> extism::Function {
    extism::Function::new(
        "run_ytdlp",
        [extism::ValType::I64],
        [extism::ValType::I64],
        user_data,
        |plugin, inputs, outputs, ud| {
            let input = read_input_string(plugin, inputs)?;
            let request: PluginYtDlpRequest = serde_json::from_str(&input)
                .map_err(|e| anyhow::anyhow!("run_ytdlp: invalid JSON: {e}"))?;
            let plugin_name = {
                let guard = ud.get()?;
                let ctx = guard
                    .lock()
                    .map_err(|_| anyhow::anyhow!("run_ytdlp: mutex poisoned"))?;
                if !ctx
                    .capabilities
                    .iter()
                    .any(|cap| cap == "subprocess:yt-dlp")
                {
                    return Err(anyhow::anyhow!("run_ytdlp: capability is not declared"));
                }
                ctx.plugin_name.clone()
            };
            let response = run_plugin_request(&plugin_name, request)?;
            let json = serde_json::to_string(&response)
                .map_err(|e| anyhow::anyhow!("run_ytdlp: failed to serialize response: {e}"))?;
            write_output_string(plugin, outputs, &json)
        },
    )
}

/// Compatibility shim for already-published plugins using the former ABI.
///
/// The broker accepts only the exact historical yt-dlp profiles of official
/// plugins and rebuilds them with the same host-side controls as `run_ytdlp`.
pub fn make_legacy_run_subprocess_function(
    user_data: extism::UserData<PluginHostContext>,
) -> extism::Function {
    extism::Function::new(
        "run_subprocess",
        [extism::ValType::I64],
        [extism::ValType::I64],
        user_data,
        |plugin, inputs, outputs, ud| {
            let input = read_input_string(plugin, inputs)?;
            let request: LegacySubprocessRequest = serde_json::from_str(&input)
                .map_err(|e| anyhow::anyhow!("run_subprocess compatibility: invalid JSON: {e}"))?;
            let plugin_name = {
                let guard = ud.get()?;
                let ctx = guard
                    .lock()
                    .map_err(|_| anyhow::anyhow!("run_subprocess compatibility: mutex poisoned"))?;
                if !ctx
                    .capabilities
                    .iter()
                    .any(|cap| cap == "subprocess:yt-dlp")
                {
                    return Err(anyhow::anyhow!(
                        "run_subprocess compatibility: capability is not declared"
                    ));
                }
                ctx.plugin_name.clone()
            };
            let response = run_legacy_request(&plugin_name, request)?;
            let json = serde_json::to_string(&response).map_err(|e| {
                anyhow::anyhow!("run_subprocess compatibility: failed to serialize response: {e}")
            })?;
            write_output_string(plugin, outputs, &json)
        },
    )
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::driven::plugin::capabilities::SharedHostResources;
    use crate::domain::model::credential::Credential;

    #[test]
    fn test_log_request_deserialization() {
        let json = r#"{"level":"info","message":"hello from plugin"}"#;
        let req: LogRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.level, "info");
        assert_eq!(req.message, "hello from plugin");
    }

    #[test]
    fn test_http_request_deserialization() {
        let json = r#"{"method":"GET","url":"https://example.com","headers":{"Accept":"text/html"},"body":null}"#;
        let req: HttpRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.method, "GET");
        assert_eq!(req.url, "https://example.com");
        assert_eq!(
            req.headers.get("Accept").map(String::as_str),
            Some("text/html")
        );
        assert!(req.body.is_none());
    }

    #[test]
    fn test_get_set_config_round_trip() {
        let shared = std::sync::Arc::new(SharedHostResources::new());
        let plugin_name = "round-trip-plugin";

        shared
            .plugin_configs()
            .entry(plugin_name.to_string())
            .or_default()
            .insert("my_key".to_string(), "my_value".to_string());

        let value = shared
            .plugin_configs()
            .get(plugin_name)
            .and_then(|m| m.get("my_key").map(|v| v.clone()))
            .unwrap_or_default();

        assert_eq!(value, "my_value");
    }

    #[test]
    fn test_get_set_state_round_trip() {
        let shared = std::sync::Arc::new(SharedHostResources::new());
        let plugin_name = "state-plugin";

        shared
            .plugin_states()
            .entry(plugin_name.to_string())
            .or_default()
            .insert("session".to_string(), "abc123".to_string());

        let value = shared
            .plugin_states()
            .get(plugin_name)
            .and_then(|m| m.get("session").map(|v| v.clone()))
            .unwrap_or_default();

        assert_eq!(value, "abc123");
    }

    #[test]
    fn plugin_logs_are_fully_redacted_while_a_credential_is_scoped() {
        let slot = Arc::new(std::sync::Mutex::new(Some(Credential::new(
            "alice",
            "super-secret",
        ))));

        let message = safe_plugin_log_message(
            "Authorization: Bearer super-secret; direct=https://cdn/token",
            &slot,
        )
        .unwrap();

        assert_eq!(message, "plugin log redacted during credential operation");
        assert!(!message.contains("super-secret"));
        assert!(!message.contains("cdn/token"));
    }
}
