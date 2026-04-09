//! Host function implementations for WASM plugins.

use std::collections::HashMap;
use std::io::Read;
use std::net::{IpAddr, ToSocketAddrs};
use std::process::Child;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use super::capabilities::PluginHostContext;

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
struct SubprocessRequest {
    binary: String,
    #[serde(default)]
    args: Vec<String>,
    timeout_ms: Option<u64>,
}

#[derive(Serialize)]
struct SubprocessResponse {
    exit_code: i32,
    stdout: String,
    stderr: String,
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

struct CapturedOutput {
    bytes: Vec<u8>,
    truncated: bool,
}

const MAX_HTTP_BODY_BYTES: u64 = 100 * 1024 * 1024;
const MAX_SUBPROCESS_OUTPUT_BYTES: usize = 1024 * 1024;
const SUBPROCESS_POLL_INTERVAL: Duration = Duration::from_millis(25);

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

/// Reject URLs targeting internal/loopback networks (SSRF protection).
fn validate_url_not_internal(url: &reqwest::Url) -> Result<(), extism::Error> {
    if let Some(host) = url.host_str() {
        // Reject localhost variants
        if host == "localhost" || host.ends_with(".localhost") {
            return Err(anyhow::anyhow!(
                "http_request: requests to localhost are forbidden"
            ));
        }

        if let Ok(ip) = host.parse::<IpAddr>() {
            if is_forbidden_ip(&ip) {
                return Err(anyhow::anyhow!(
                    "http_request: requests to internal networks are forbidden"
                ));
            }
            return Ok(());
        }

        let port = url
            .port_or_known_default()
            .ok_or_else(|| anyhow::anyhow!("http_request: URL is missing a known port"))?;

        let resolved_ips = (host, port)
            .to_socket_addrs()
            .map_err(|e| anyhow::anyhow!("http_request: failed to resolve host '{host}': {e}"))?
            .map(|socket_addr| socket_addr.ip())
            .collect::<Vec<_>>();

        if resolved_ips.is_empty() {
            return Err(anyhow::anyhow!(
                "http_request: host '{host}' did not resolve to any addresses"
            ));
        }

        if resolved_ips.iter().any(is_forbidden_ip) {
            return Err(anyhow::anyhow!(
                "http_request: requests to internal networks are forbidden"
            ));
        }
    }
    Ok(())
}

fn is_forbidden_ip(ip: &IpAddr) -> bool {
    ip.is_loopback() || ip.is_unspecified() || is_private_ip(ip) || is_link_local(ip)
}

fn is_private_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let octets = v4.octets();
            octets[0] == 10
                || (octets[0] == 172 && (16..=31).contains(&octets[1]))
                || (octets[0] == 192 && octets[1] == 168)
        }
        IpAddr::V6(v6) => {
            let segments = v6.segments();
            // fc00::/7 (unique local)
            (segments[0] & 0xfe00) == 0xfc00
        }
    }
}

fn is_link_local(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let octets = v4.octets();
            // 169.254.0.0/16 (includes AWS metadata 169.254.169.254)
            octets[0] == 169 && octets[1] == 254
        }
        IpAddr::V6(v6) => {
            let segments = v6.segments();
            // fe80::/10
            (segments[0] & 0xffc0) == 0xfe80
        }
    }
}

/// Validate subprocess binary name has no path components.
fn validate_binary_name(binary: &str) -> Result<(), extism::Error> {
    if binary.is_empty()
        || binary.contains('/')
        || binary.contains('\\')
        || binary.contains("..")
        || binary.contains('\0')
    {
        return Err(anyhow::anyhow!(
            "run_subprocess: invalid binary name '{binary}'"
        ));
    }
    Ok(())
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

fn read_stream_capped<R: Read>(mut reader: R, max_bytes: usize) -> std::io::Result<CapturedOutput> {
    let mut bytes = Vec::new();
    let mut truncated = false;
    let mut chunk = [0_u8; 8192];

    loop {
        let read = reader.read(&mut chunk)?;
        if read == 0 {
            break;
        }

        let remaining = max_bytes.saturating_sub(bytes.len());
        let to_copy = remaining.min(read);
        if to_copy > 0 {
            bytes.extend_from_slice(&chunk[..to_copy]);
        }
        if to_copy < read || bytes.len() >= max_bytes {
            truncated = true;
        }
    }

    Ok(CapturedOutput { bytes, truncated })
}

fn spawn_output_reader<T>(
    stream: Option<T>,
) -> std::thread::JoinHandle<std::io::Result<CapturedOutput>>
where
    T: Read + Send + 'static,
{
    std::thread::spawn(move || match stream {
        Some(stream) => read_stream_capped(stream, MAX_SUBPROCESS_OUTPUT_BYTES),
        None => Ok(CapturedOutput {
            bytes: Vec::new(),
            truncated: false,
        }),
    })
}

fn decode_captured_output(output: CapturedOutput) -> String {
    let mut text = String::from_utf8_lossy(&output.bytes).into_owned();
    if output.truncated {
        text.push_str("\n[truncated]");
    }
    text
}

fn collect_subprocess_output(
    stdout_handle: std::thread::JoinHandle<std::io::Result<CapturedOutput>>,
    stderr_handle: std::thread::JoinHandle<std::io::Result<CapturedOutput>>,
) -> Result<(String, String), extism::Error> {
    let stdout = stdout_handle
        .join()
        .map_err(|_| anyhow::anyhow!("run_subprocess: stdout reader thread panicked"))?
        .map_err(|e| anyhow::anyhow!("run_subprocess: failed to read stdout: {e}"))?;
    let stderr = stderr_handle
        .join()
        .map_err(|_| anyhow::anyhow!("run_subprocess: stderr reader thread panicked"))?
        .map_err(|e| anyhow::anyhow!("run_subprocess: failed to read stderr: {e}"))?;

    Ok((
        decode_captured_output(stdout),
        decode_captured_output(stderr),
    ))
}

fn wait_for_child_with_timeout(
    child: &mut Child,
    timeout: Duration,
) -> Result<(std::process::ExitStatus, bool), extism::Error> {
    let started_at = Instant::now();

    loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|e| anyhow::anyhow!("run_subprocess: failed to poll child status: {e}"))?
        {
            return Ok((status, false));
        }

        if started_at.elapsed() >= timeout {
            child.kill().map_err(|e| {
                anyhow::anyhow!("run_subprocess: failed to kill timed out process: {e}")
            })?;
            let status = child.wait().map_err(|e| {
                anyhow::anyhow!("run_subprocess: failed to reap timed out process: {e}")
            })?;
            return Ok((status, true));
        }

        std::thread::sleep(SUBPROCESS_POLL_INTERVAL);
    }
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
            match req.level.as_str() {
                "error" => tracing::error!(plugin = plugin_name, "{}", req.message),
                "warn" => tracing::warn!(plugin = plugin_name, "{}", req.message),
                "debug" => tracing::debug!(plugin = plugin_name, "{}", req.message),
                _ => tracing::info!(plugin = plugin_name, "{}", req.message),
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
            validate_url_not_internal(&url)?;

            // F6: Minimize mutex scope — clone the client, then release the lock
            let client = {
                let guard = ud.get()?;
                let ctx = guard
                    .lock()
                    .map_err(|_| anyhow::anyhow!("http_request: mutex poisoned"))?;
                ctx.shared.http_client().clone()
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
            let (store, plugin_name) = {
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

                let store = ctx.shared.credential_store().cloned().ok_or_else(|| {
                    anyhow::anyhow!("get_credential: no credential store configured")
                })?;
                (store, ctx.plugin_name.clone())
            };

            let cred = store
                .get(&plugin_name)
                .map_err(|e| anyhow::anyhow!("get_credential: store error: {e}"))?
                .ok_or_else(|| anyhow::anyhow!("get_credential: no credential found"))?;

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

/// Run a subprocess binary declared in the plugin's `subprocess:` capabilities.
pub fn make_run_subprocess_function(
    user_data: extism::UserData<PluginHostContext>,
) -> extism::Function {
    extism::Function::new(
        "run_subprocess",
        [extism::ValType::I64],
        [extism::ValType::I64],
        user_data,
        |plugin, inputs, outputs, ud| {
            let input = read_input_string(plugin, inputs)?;
            let req: SubprocessRequest = serde_json::from_str(&input)
                .map_err(|e| anyhow::anyhow!("run_subprocess: invalid JSON: {e}"))?;

            // F4: Validate binary name has no path components
            validate_binary_name(&req.binary)?;

            // F6: Minimize mutex scope — check capability then release
            {
                let guard = ud.get()?;
                let ctx = guard
                    .lock()
                    .map_err(|_| anyhow::anyhow!("run_subprocess: mutex poisoned"))?;

                let required_cap = format!("subprocess:{}", req.binary);
                if !ctx.capabilities.iter().any(|c| c == &required_cap) {
                    return Err(anyhow::anyhow!(
                        "run_subprocess: binary '{}' not listed in plugin capabilities",
                        req.binary
                    ));
                }
            } // Mutex released here — subprocess runs without holding the lock

            let timeout = std::time::Duration::from_millis(req.timeout_ms.unwrap_or(60_000));
            let binary = req.binary.clone();

            let mut child = std::process::Command::new(&req.binary)
                .args(&req.args)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .map_err(|e| {
                    anyhow::anyhow!("run_subprocess: failed to spawn '{}': {e}", req.binary)
                })?;

            let stdout_handle = spawn_output_reader(child.stdout.take());
            let stderr_handle = spawn_output_reader(child.stderr.take());

            let (status, timed_out) = wait_for_child_with_timeout(&mut child, timeout)?;
            let (stdout, stderr) = collect_subprocess_output(stdout_handle, stderr_handle)?;

            if timed_out {
                return Err(anyhow::anyhow!(
                    "run_subprocess: '{}' timed out after {}ms",
                    binary,
                    timeout.as_millis()
                ));
            }

            let resp = SubprocessResponse {
                exit_code: status.code().unwrap_or(-1),
                stdout,
                stderr,
            };
            let json = serde_json::to_string(&resp).map_err(|e| {
                anyhow::anyhow!("run_subprocess: failed to serialize response: {e}")
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
    fn test_subprocess_request_deserialization() {
        let json = r#"{"binary":"yt-dlp","args":["--no-playlist","https://example.com"],"timeout_ms":5000}"#;
        let req: SubprocessRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.binary, "yt-dlp");
        assert_eq!(req.args, vec!["--no-playlist", "https://example.com"]);
        assert_eq!(req.timeout_ms, Some(5000));
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
    fn test_subprocess_binary_validation() {
        let ctx_caps: Vec<String> = vec!["subprocess:ffmpeg".to_string()];
        let required = "yt-dlp";
        let cap_key = format!("subprocess:{required}");
        assert!(!ctx_caps.iter().any(|c| c == &cap_key));

        let ctx_caps2: Vec<String> = vec![
            "subprocess:ffmpeg".to_string(),
            "subprocess:yt-dlp".to_string(),
        ];
        assert!(ctx_caps2.iter().any(|c| c == &cap_key));
    }
}
