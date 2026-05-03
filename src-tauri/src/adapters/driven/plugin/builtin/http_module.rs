//! Built-in HTTP/HTTPS module — catch-all for direct downloads.
//!
//! This module handles any URL with http:// or https:// scheme
//! that no WASM plugin has claimed. It is compiled into the Vortex binary
//! (not loaded as WASM) for maximum performance.
//!
//! FTP support is planned but not yet implemented — `can_handle`
//! returns `false` for ftp:// URLs until then.

use std::net::IpAddr;

use reqwest::header::HeaderMap;

use crate::domain::error::DomainError;
use crate::domain::model::link::LinkStatus;
use crate::domain::model::plugin::{PluginCategory, PluginInfo};

/// Built-in HTTP module for direct URL downloads.
pub struct HttpModule {
    client: reqwest::Client,
    ssrf_protection: bool,
}

impl HttpModule {
    /// Create a new HTTP module with SSRF protection enabled.
    ///
    /// Uses a custom redirect policy that validates each redirect
    /// destination against internal network rules.
    pub fn new() -> Result<Self, DomainError> {
        Self::build(true)
    }

    /// Create a module without SSRF protection (for tests with MockServer).
    #[cfg(test)]
    fn new_permissive() -> Result<Self, DomainError> {
        Self::build(false)
    }

    fn build(ssrf_protection: bool) -> Result<Self, DomainError> {
        let mut builder = reqwest::Client::builder().timeout(std::time::Duration::from_secs(30));

        // When SSRF is enabled, disable automatic redirects — we follow them
        // manually in send_head so each hop gets async DNS validation.
        if ssrf_protection {
            builder = builder.redirect(reqwest::redirect::Policy::none());
        } else {
            builder = builder.redirect(reqwest::redirect::Policy::limited(10));
        }

        let client = builder
            .build()
            .map_err(|e| DomainError::NetworkError(format!("failed to build HTTP client: {e}")))?;
        Ok(Self {
            client,
            ssrf_protection,
        })
    }

    /// Returns true if this module can handle the given URL scheme.
    ///
    /// Only http:// and https:// are supported. FTP is planned but
    /// not yet implemented.
    pub fn can_handle(url: &str) -> bool {
        url.starts_with("http://") || url.starts_with("https://")
    }

    /// Check a URL's availability and metadata via a HEAD request.
    ///
    /// Returns `LinkStatus::Online` for 2xx, `LinkStatus::Offline` for 404/410,
    /// `LinkStatus::PremiumOnly` for 401/402 (auth/payment required), and
    /// `LinkStatus::Unknown` for other status codes. Status-code mapping
    /// is delegated to `LinkStatus::from_status_code` so this module and
    /// the `link_check_online` handler stay in sync.
    pub async fn check_link(&self, url: &str) -> Result<LinkStatus, DomainError> {
        let response = self.send_head(url).await?;
        let code = response.status().as_u16();

        Ok(match LinkStatus::from_status_code(code) {
            Some(terminal) => terminal,
            None => LinkStatus::Online {
                filename: extract_filename(response.headers(), url),
                size: parse_content_length(response.headers()),
                resumable: parse_accept_ranges(response.headers()),
            },
        })
    }

    /// Follow redirects and return the final URL.
    pub async fn resolve_download_url(&self, url: &str) -> Result<String, DomainError> {
        let response = self.send_head(url).await?;
        Ok(response.url().to_string())
    }

    /// Returns synthetic plugin info for the built-in HTTP module.
    pub fn plugin_info() -> PluginInfo {
        PluginInfo::new(
            "builtin-http".to_string(),
            env!("CARGO_PKG_VERSION").to_string(),
            "Built-in HTTP/HTTPS direct download module".to_string(),
            "Vortex".to_string(),
            PluginCategory::Utility,
        )
    }

    /// Send a HEAD request with SSRF validation and manual redirect following.
    ///
    /// When SSRF protection is enabled, redirects are followed manually so
    /// each hop gets full async DNS validation (hostname → IP check).
    async fn send_head(&self, url: &str) -> Result<reqwest::Response, DomainError> {
        const MAX_REDIRECTS: u8 = 10;

        let mut current =
            reqwest::Url::parse(url).map_err(|e| DomainError::InvalidUrl(format!("{e}")))?;

        for _ in 0..=MAX_REDIRECTS {
            if self.ssrf_protection {
                validate_not_internal_async(&current).await?;
            }

            let mut builder = self.client.head(current.clone());
            if let Some((user, pass)) = extract_basic_auth(&current) {
                builder = builder.basic_auth(user, Some(pass));
            }

            let response = builder.send().await.map_err(|e| {
                DomainError::NetworkError(format!(
                    "HEAD request failed for {}: {e}",
                    redact_credentials(&current)
                ))
            })?;

            if !self.ssrf_protection || !response.status().is_redirection() {
                return Ok(response);
            }

            // Manual redirect: extract Location, validate, follow
            let location = response
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|v| v.to_str().ok())
                .ok_or_else(|| {
                    DomainError::NetworkError("redirect without Location header".to_string())
                })?;

            current = current
                .join(location)
                .map_err(|e| DomainError::NetworkError(format!("invalid redirect URL: {e}")))?;
        }

        Err(DomainError::NetworkError(
            "too many redirects (max 10)".to_string(),
        ))
    }
}

// ---------------------------------------------------------------------------
// SSRF protection
// ---------------------------------------------------------------------------

/// Reject URLs targeting internal/loopback networks.
///
/// Uses async DNS resolution to avoid blocking the tokio runtime.
/// Fails closed: if DNS resolution fails, the request is rejected.
async fn validate_not_internal_async(url: &reqwest::Url) -> Result<(), DomainError> {
    let host = match url.host_str() {
        Some(h) => h,
        None => return Ok(()),
    };

    if host == "localhost" || host.ends_with(".localhost") {
        return Err(DomainError::ValidationError(
            "requests to localhost are forbidden".to_string(),
        ));
    }

    if let Some(ip) = parse_host_ip(host) {
        if is_forbidden_ip(&ip) {
            return Err(DomainError::ValidationError(
                "requests to internal networks are forbidden".to_string(),
            ));
        }
        return Ok(());
    }

    // Async DNS resolution — fail closed on errors
    let port = url
        .port_or_known_default()
        .ok_or_else(|| DomainError::ValidationError("URL is missing a known port".to_string()))?;

    // Strip brackets for lookup_host (expects "host:port", not "[host]:port")
    let bare_host = host.trim_start_matches('[').trim_end_matches(']');
    let addr_str = format!("{bare_host}:{port}");
    let addrs: Vec<_> = tokio::net::lookup_host(&addr_str)
        .await
        .map_err(|e| {
            DomainError::ValidationError(format!(
                "failed to resolve host '{host}' for SSRF validation: {e}"
            ))
        })?
        .collect();

    if addrs.is_empty() {
        return Err(DomainError::ValidationError(format!(
            "host '{host}' did not resolve to any addresses"
        )));
    }

    if addrs.iter().any(|addr| is_forbidden_ip(&addr.ip())) {
        return Err(DomainError::ValidationError(
            "requests to internal networks are forbidden".to_string(),
        ));
    }

    Ok(())
}

/// Parse an IP address from a URL host string, handling IPv6 brackets.
fn parse_host_ip(host: &str) -> Option<IpAddr> {
    // URL host_str() may return "[fe80::1]" for IPv6 — strip brackets
    let bare = host.trim_start_matches('[').trim_end_matches(']');
    bare.parse().ok()
}

fn is_forbidden_ip(ip: &IpAddr) -> bool {
    let normalized = normalize_ip(ip);
    normalized.is_loopback() || normalized.is_unspecified() || is_private(&normalized)
}

fn normalize_ip(ip: &IpAddr) -> IpAddr {
    match ip {
        IpAddr::V4(v4) => IpAddr::V4(*v4),
        IpAddr::V6(v6) => v6
            .to_ipv4_mapped()
            .map(IpAddr::V4)
            .unwrap_or(IpAddr::V6(*v6)),
    }
}

fn is_private(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            o[0] == 10
                || (o[0] == 172 && (16..=31).contains(&o[1]))
                || (o[0] == 192 && o[1] == 168)
                || (o[0] == 169 && o[1] == 254) // link-local + AWS metadata
        }
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_private(&IpAddr::V4(v4));
            }
            let seg0 = v6.segments()[0];
            (seg0 & 0xfe00) == 0xfc00       // fc00::/7  unique local
                || (seg0 & 0xffc0) == 0xfe80 // fe80::/10 link-local
        }
    }
}

// ---------------------------------------------------------------------------
// URL helpers
// ---------------------------------------------------------------------------

/// Strip credentials from URL for safe logging.
fn redact_credentials(url: &reqwest::Url) -> String {
    let mut redacted = url.clone();
    if !redacted.username().is_empty() {
        let _ = redacted.set_username("***");
        let _ = redacted.set_password(Some("***"));
    }
    redacted.to_string()
}

/// Extract inline basic auth credentials from a parsed URL.
fn extract_basic_auth(url: &reqwest::Url) -> Option<(String, String)> {
    let user = url.username();
    if user.is_empty() {
        return None;
    }
    let pass = url.password().unwrap_or("");
    Some((user.to_string(), pass.to_string()))
}

// ---------------------------------------------------------------------------
// Header parsing
// ---------------------------------------------------------------------------

fn parse_content_length(headers: &HeaderMap) -> Option<u64> {
    headers
        .get(reqwest::header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse().ok())
}

fn parse_accept_ranges(headers: &HeaderMap) -> bool {
    headers
        .get(reqwest::header::ACCEPT_RANGES)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.eq_ignore_ascii_case("bytes"))
        .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Filename extraction
// ---------------------------------------------------------------------------

/// Extract filename from response headers or URL path.
///
/// Priority: Content-Disposition filename* (RFC 5987) → filename → URL path.
/// Returns `None` if no filename can be determined.
/// Path traversal sequences and null bytes are stripped.
fn extract_filename(headers: &HeaderMap, url: &str) -> Option<String> {
    let raw = extract_raw_filename(headers, url)?;
    let sanitized = sanitize_filename(&raw);
    (!sanitized.is_empty()).then_some(sanitized)
}

fn extract_raw_filename(headers: &HeaderMap, url: &str) -> Option<String> {
    if let Some(cd) = headers
        .get(reqwest::header::CONTENT_DISPOSITION)
        .and_then(|v| v.to_str().ok())
    {
        if let Some(name) = parse_filename_star(cd) {
            return Some(name);
        }
        if let Some(name) = parse_filename_plain(cd) {
            return Some(name);
        }
    }

    // Fall back to URL path — require at least one '/' after the authority
    let path = url.split(['?', '#']).next().unwrap_or(url);
    let after_scheme = path.find("://").map(|p| p + 3).unwrap_or(0);
    if !path[after_scheme..].contains('/') {
        return None;
    }
    path.rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

/// Remove path traversal sequences, slashes, and null bytes.
fn sanitize_filename(name: &str) -> String {
    name.replace(['\0', '/', '\\'], "")
        .replace("..", "")
        .trim()
        .to_string()
}

fn parse_filename_plain(cd: &str) -> Option<String> {
    let lower = cd.to_ascii_lowercase();
    let pos = lower.find("filename=")?;
    let after = &cd[pos + "filename=".len()..];
    if after.starts_with('*') {
        return None;
    }
    let name = after.trim_start_matches('"');
    let name = name.split('"').next().unwrap_or(name);
    let name = name.split(';').next().unwrap_or(name).trim();
    (!name.is_empty()).then(|| name.to_string())
}

fn parse_filename_star(cd: &str) -> Option<String> {
    let lower = cd.to_ascii_lowercase();
    let pos = lower.find("filename*=")?;
    let after = &cd[pos + "filename*=".len()..];
    let after = after.trim_start_matches('"');
    let value = after
        .strip_prefix("UTF-8''")
        .or_else(|| after.strip_prefix("utf-8''"))?;
    let encoded = value
        .split(';')
        .next()
        .unwrap_or(value)
        .split('"')
        .next()
        .unwrap_or(value);
    let decoded = percent_decode(encoded);
    (!decoded.is_empty()).then_some(decoded)
}

fn percent_decode(input: &str) -> String {
    let mut result = Vec::new();
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(h), Some(l)) = (from_hex(bytes[i + 1]), from_hex(bytes[i + 2]))
        {
            result.push((h << 4) | l);
            i += 3;
            continue;
        }
        result.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&result).into_owned()
}

fn from_hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// Permissive module for wiremock tests (allows loopback).
    fn module() -> HttpModule {
        HttpModule::new_permissive().expect("test client")
    }

    // ---- can_handle ----

    #[test]
    fn test_can_handle_http_url() {
        assert!(HttpModule::can_handle("http://example.com/file.zip"));
    }

    #[test]
    fn test_can_handle_https_url() {
        assert!(HttpModule::can_handle("https://example.com/file.zip"));
    }

    #[test]
    fn test_can_handle_rejects_ftp() {
        assert!(!HttpModule::can_handle("ftp://ftp.example.com/file.tar.gz"));
    }

    #[test]
    fn test_can_handle_rejects_unknown_scheme() {
        assert!(!HttpModule::can_handle("magnet:?xt=urn:btih:abc123"));
        assert!(!HttpModule::can_handle("ssh://example.com/file"));
        assert!(!HttpModule::can_handle("sftp://example.com/file"));
        assert!(!HttpModule::can_handle("file:///local/path"));
    }

    // ---- SSRF ----

    #[tokio::test]
    async fn test_ssrf_rejects_localhost() {
        let url = reqwest::Url::parse("http://localhost/secret").unwrap();
        assert!(validate_not_internal_async(&url).await.is_err());
    }

    #[tokio::test]
    async fn test_ssrf_rejects_loopback_ip() {
        let url = reqwest::Url::parse("http://127.0.0.1/secret").unwrap();
        assert!(validate_not_internal_async(&url).await.is_err());
    }

    #[tokio::test]
    async fn test_ssrf_rejects_private_ip() {
        for addr in &["10.0.0.1", "172.16.0.1", "192.168.1.1", "169.254.169.254"] {
            let url = reqwest::Url::parse(&format!("http://{addr}/secret")).unwrap();
            assert!(
                validate_not_internal_async(&url).await.is_err(),
                "should reject {addr}"
            );
        }
    }

    #[tokio::test]
    async fn test_ssrf_rejects_ipv6_link_local() {
        let url = reqwest::Url::parse("http://[fe80::1]/secret").unwrap();
        assert!(validate_not_internal_async(&url).await.is_err());
    }

    #[tokio::test]
    async fn test_ssrf_allows_public_ip() {
        let url = reqwest::Url::parse("http://8.8.8.8/file").unwrap();
        assert!(validate_not_internal_async(&url).await.is_ok());
    }

    // ---- filename extraction ----

    #[test]
    fn test_extract_filename_from_content_disposition() {
        let mut headers = HeaderMap::new();
        headers.insert(
            reqwest::header::CONTENT_DISPOSITION,
            "attachment; filename=\"report.pdf\"".parse().unwrap(),
        );
        let name = extract_filename(&headers, "https://example.com/dl");
        assert_eq!(name, Some("report.pdf".to_string()));
    }

    #[test]
    fn test_extract_filename_from_content_disposition_rfc5987() {
        let mut headers = HeaderMap::new();
        headers.insert(
            reqwest::header::CONTENT_DISPOSITION,
            "attachment; filename*=UTF-8''r%C3%A9sum%C3%A9.pdf"
                .parse()
                .unwrap(),
        );
        let name = extract_filename(&headers, "https://example.com/dl");
        assert_eq!(name, Some("résumé.pdf".to_string()));
    }

    #[test]
    fn test_extract_filename_from_url_path() {
        let headers = HeaderMap::new();
        let name = extract_filename(&headers, "https://example.com/files/archive.zip");
        assert_eq!(name, Some("archive.zip".to_string()));
    }

    #[test]
    fn test_extract_filename_from_url_path_with_query_params() {
        let headers = HeaderMap::new();
        let name = extract_filename(
            &headers,
            "https://example.com/download/setup.exe?token=abc&v=2",
        );
        assert_eq!(name, Some("setup.exe".to_string()));
    }

    #[test]
    fn test_extract_filename_fallback_none() {
        let headers = HeaderMap::new();
        let name = extract_filename(&headers, "https://example.com/");
        assert_eq!(name, None);
    }

    #[test]
    fn test_extract_filename_bare_domain_returns_none() {
        let headers = HeaderMap::new();
        let name = extract_filename(&headers, "https://example.com");
        assert_eq!(name, None);
    }

    #[test]
    fn test_sanitize_filename_strips_traversal() {
        assert_eq!(sanitize_filename("../../etc/passwd"), "etcpasswd");
        assert_eq!(sanitize_filename("file\0name.zip"), "filename.zip");
        assert_eq!(sanitize_filename("path/to\\file.zip"), "pathtofile.zip");
    }

    #[test]
    fn test_sanitize_filename_all_dangerous_returns_empty() {
        let headers = HeaderMap::new();
        headers.get(reqwest::header::CONTENT_DISPOSITION);
        // A raw filename of "../" sanitizes to "" → extract_filename returns None
        assert_eq!(sanitize_filename("../"), "");
    }

    // ---- plugin info ----

    #[test]
    fn test_plugin_info_returns_correct_values() {
        let info = HttpModule::plugin_info();
        assert_eq!(info.name(), "builtin-http");
        assert_eq!(info.category(), PluginCategory::Utility);
        assert_eq!(info.author(), "Vortex");
        assert!(!info.version().is_empty());
    }

    // ---- redact credentials ----

    #[test]
    fn test_redact_credentials_strips_auth() {
        let url = reqwest::Url::parse("https://user:secret@example.com/file").unwrap();
        let redacted = redact_credentials(&url);
        assert!(!redacted.contains("secret"));
        assert!(redacted.contains("***"));
    }

    #[test]
    fn test_redact_credentials_noop_without_auth() {
        let url = reqwest::Url::parse("https://example.com/file").unwrap();
        let redacted = redact_credentials(&url);
        assert_eq!(redacted, "https://example.com/file");
    }

    // ---- integration tests with wiremock ----

    #[tokio::test]
    async fn test_check_link_online() {
        let server = MockServer::start().await;
        Mock::given(method("HEAD"))
            .and(path("/file.zip"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("Content-Length", "4096")
                    .insert_header("Accept-Ranges", "bytes"),
            )
            .mount(&server)
            .await;

        let m = module();
        let url = format!("{}/file.zip", server.uri());
        let result = m.check_link(&url).await.unwrap();

        assert_eq!(
            result,
            LinkStatus::Online {
                filename: Some("file.zip".to_string()),
                size: Some(4096),
                resumable: true,
            }
        );
    }

    #[tokio::test]
    async fn test_check_link_offline_404() {
        let server = MockServer::start().await;
        Mock::given(method("HEAD"))
            .and(path("/missing.zip"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;

        let m = module();
        let url = format!("{}/missing.zip", server.uri());
        assert_eq!(m.check_link(&url).await.unwrap(), LinkStatus::Offline);
    }

    #[tokio::test]
    async fn test_check_link_offline_410_gone() {
        let server = MockServer::start().await;
        Mock::given(method("HEAD"))
            .and(path("/gone"))
            .respond_with(ResponseTemplate::new(410))
            .mount(&server)
            .await;

        let m = module();
        let url = format!("{}/gone", server.uri());
        assert_eq!(m.check_link(&url).await.unwrap(), LinkStatus::Offline);
    }

    #[tokio::test]
    async fn test_check_link_unknown_status() {
        let server = MockServer::start().await;
        Mock::given(method("HEAD"))
            .and(path("/error"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;

        let m = module();
        let url = format!("{}/error", server.uri());
        assert_eq!(m.check_link(&url).await.unwrap(), LinkStatus::Unknown);
    }

    #[tokio::test]
    async fn test_check_link_premium_only_401() {
        let server = MockServer::start().await;
        Mock::given(method("HEAD"))
            .and(path("/premium"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;

        let m = module();
        let url = format!("{}/premium", server.uri());
        assert_eq!(m.check_link(&url).await.unwrap(), LinkStatus::PremiumOnly);
    }

    #[tokio::test]
    async fn test_check_link_premium_only_402() {
        let server = MockServer::start().await;
        Mock::given(method("HEAD"))
            .and(path("/payment"))
            .respond_with(ResponseTemplate::new(402))
            .mount(&server)
            .await;

        let m = module();
        let url = format!("{}/payment", server.uri());
        assert_eq!(m.check_link(&url).await.unwrap(), LinkStatus::PremiumOnly);
    }

    #[tokio::test]
    async fn test_redirect_follows_to_final_url() {
        let server = MockServer::start().await;
        Mock::given(method("HEAD"))
            .and(path("/redirect"))
            .respond_with(ResponseTemplate::new(302).insert_header("Location", "/final"))
            .mount(&server)
            .await;
        Mock::given(method("HEAD"))
            .and(path("/final"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;

        let m = module();
        let url = format!("{}/redirect", server.uri());
        let final_url = m.resolve_download_url(&url).await.unwrap();
        assert!(
            final_url.ends_with("/final"),
            "expected /final, got {final_url}"
        );
    }
}
