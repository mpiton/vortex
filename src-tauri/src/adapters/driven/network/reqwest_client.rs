//! HTTP client adapter using reqwest.
//!
//! Implements the `HttpClient` domain port. Bridges async reqwest calls
//! to the sync port contract using a dedicated OS thread + `Handle::block_on`.
//!
//! # Thread safety constraint
//!
//! The `block_on` helper spawns a new OS thread that blocks on the tokio
//! runtime handle. This is safe from sync call sites (e.g. Tauri command
//! handlers) but must NOT be called from within an async task on the same
//! runtime thread, as that would deadlock.

use std::collections::HashMap;

use tokio::runtime::Handle;
use tracing::debug;

use crate::domain::error::DomainError;
use crate::domain::model::http::HttpResponse;
use crate::domain::ports::driven::http_client::HttpClient;

pub struct ReqwestHttpClient {
    client: reqwest::Client,
}

impl ReqwestHttpClient {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .user_agent("Vortex/0.1")
            .connect_timeout(std::time::Duration::from_secs(30))
            .timeout(std::time::Duration::from_secs(3600))
            .build()
            .expect("failed to create HTTP client");
        Self { client }
    }

    pub fn with_client(client: reqwest::Client) -> Self {
        Self { client }
    }
}

impl Default for ReqwestHttpClient {
    fn default() -> Self {
        Self::new()
    }
}

fn map_reqwest_error(err: reqwest::Error) -> DomainError {
    DomainError::NetworkError(format!("HTTP request failed: {err}"))
}

/// Run an async future on the current tokio runtime from a sync context.
///
/// Spawns a new OS thread that blocks on the runtime handle. This avoids
/// the deadlock risk of calling `block_in_place` on a `current_thread` runtime.
fn block_on<F>(future: F) -> Result<F::Output, DomainError>
where
    F: std::future::Future + Send,
    F::Output: Send,
{
    let handle = Handle::try_current()
        .map_err(|_| DomainError::NetworkError("no tokio runtime available".into()))?;
    std::thread::scope(|s| {
        s.spawn(|| handle.block_on(future))
            .join()
            .map_err(|_| DomainError::NetworkError("HTTP thread panicked".into()))
    })
}

fn parse_headers(headers: &reqwest::header::HeaderMap) -> HashMap<String, Vec<String>> {
    let mut map: HashMap<String, Vec<String>> = HashMap::new();
    for (name, value) in headers {
        let key = name.as_str().to_string();
        if let Ok(v) = value.to_str() {
            map.entry(key).or_default().push(v.to_string());
        }
    }
    map
}

impl HttpClient for ReqwestHttpClient {
    fn head(&self, url: &str) -> Result<HttpResponse, DomainError> {
        debug!(url, "HEAD request");
        let response = block_on(async {
            self.client
                .head(url)
                .send()
                .await
                .map_err(map_reqwest_error)
        })??;

        let status_code = response.status().as_u16();
        let headers = parse_headers(response.headers());

        Ok(HttpResponse {
            status_code,
            headers,
            body: Vec::new(),
        })
    }

    fn get_range(&self, url: &str, start: u64, end: u64) -> Result<Vec<u8>, DomainError> {
        debug!(url, start, end, "GET range request");
        let range_header = format!("bytes={start}-{end}");

        let bytes = block_on(async {
            let response = self
                .client
                .get(url)
                .header(reqwest::header::RANGE, &range_header)
                .send()
                .await
                .map_err(map_reqwest_error)?;

            if !response.status().is_success() {
                return Err(DomainError::NetworkError(format!(
                    "HTTP {} for range request on {url}",
                    response.status()
                )));
            }

            response.bytes().await.map_err(map_reqwest_error)
        })??;

        Ok(bytes.to_vec())
    }

    fn supports_range(&self, url: &str) -> Result<bool, DomainError> {
        let response = self.head(url)?;
        let supported = response
            .header("accept-ranges")
            .map(|v| v.eq_ignore_ascii_case("bytes"))
            .unwrap_or(false);
        Ok(supported)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_head_returns_status_and_headers() {
        let mock_server = MockServer::start().await;

        Mock::given(method("HEAD"))
            .and(path("/file.zip"))
            .respond_with(
                ResponseTemplate::new(200)
                    .append_header("content-length", "1024")
                    .append_header("accept-ranges", "bytes"),
            )
            .mount(&mock_server)
            .await;

        let client = ReqwestHttpClient::new();
        let url = format!("{}/file.zip", mock_server.uri());
        let response = client.head(&url).expect("head should succeed");

        assert_eq!(response.status_code, 200);
        assert_eq!(response.content_length(), Some(1024));
        assert_eq!(response.header("accept-ranges"), Some("bytes"));
        assert!(response.body.is_empty());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_get_range_returns_partial_content() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/file.zip"))
            .and(header("range", "bytes=0-4"))
            .respond_with(ResponseTemplate::new(206).set_body_bytes(b"hello".as_slice()))
            .mount(&mock_server)
            .await;

        let client = ReqwestHttpClient::new();
        let url = format!("{}/file.zip", mock_server.uri());
        let body = client
            .get_range(&url, 0, 4)
            .expect("get_range should succeed");

        assert_eq!(body, b"hello");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_get_range_full_response_when_no_range_support() {
        let mock_server = MockServer::start().await;

        // Server returns 200 (no partial content) — still valid, body returned
        Mock::given(method("GET"))
            .and(path("/file.zip"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"full body".as_slice()))
            .mount(&mock_server)
            .await;

        let client = ReqwestHttpClient::new();
        let url = format!("{}/file.zip", mock_server.uri());
        let body = client
            .get_range(&url, 0, 8)
            .expect("get_range should succeed on 200");

        assert_eq!(body, b"full body");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_supports_range_returns_true() {
        let mock_server = MockServer::start().await;

        Mock::given(method("HEAD"))
            .and(path("/file.zip"))
            .respond_with(ResponseTemplate::new(200).append_header("accept-ranges", "bytes"))
            .mount(&mock_server)
            .await;

        let client = ReqwestHttpClient::new();
        let url = format!("{}/file.zip", mock_server.uri());
        let result = client.supports_range(&url).expect("supports_range");

        assert!(result);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_supports_range_returns_false() {
        let mock_server = MockServer::start().await;

        Mock::given(method("HEAD"))
            .and(path("/file.zip"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&mock_server)
            .await;

        let client = ReqwestHttpClient::new();
        let url = format!("{}/file.zip", mock_server.uri());
        let result = client.supports_range(&url).expect("supports_range");

        assert!(!result);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_head_network_error_returns_domain_error() {
        let client = ReqwestHttpClient::new();
        // Use an invalid URL that will fail to connect
        let result = client.head("http://127.0.0.1:1");

        assert!(result.is_err());
        match result.unwrap_err() {
            DomainError::NetworkError(msg) => {
                assert!(msg.contains("HTTP request failed"));
            }
            other => panic!("expected NetworkError, got {other:?}"),
        }
    }
}
