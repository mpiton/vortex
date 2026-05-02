//! HTTP response types for the `HttpClient` port.
//!
//! Minimal representations of HTTP responses and headers,
//! using only `std` types. The actual HTTP implementation is
//! provided by the reqwest adapter.

use std::collections::HashMap;

/// An HTTP response returned by the `HttpClient` port.
///
/// Contains status code, headers, and the response body as raw bytes.
/// The domain uses this to inspect HTTP metadata (content-length,
/// accept-ranges) without depending on any HTTP library.
#[derive(Debug, Clone, PartialEq)]
pub struct HttpResponse {
    pub status_code: u16,
    pub headers: HashMap<String, Vec<String>>,
    pub body: Vec<u8>,
}

impl HttpResponse {
    /// Returns `true` if the response status is in the 2xx range.
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status_code)
    }

    /// Looks up the first header value by name (case-insensitive).
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .and_then(|(_, v)| v.first())
            .map(|s| s.as_str())
    }

    /// Returns all values for a header name (case-insensitive).
    pub fn header_all(&self, name: &str) -> Vec<&str> {
        self.headers
            .iter()
            .filter(|(k, _)| k.eq_ignore_ascii_case(name))
            .flat_map(|(_, v)| v.iter().map(|s| s.as_str()))
            .collect()
    }

    /// Returns the Content-Length header value, if present and valid.
    pub fn content_length(&self) -> Option<u64> {
        self.header("content-length")?.trim().parse().ok()
    }

    /// `true` when the server advertises HTTP byte-range support via
    /// `Accept-Ranges: bytes`. Mirrors the reqwest-side helper used by
    /// the segmented engine so all probe flows agree on resumability.
    pub fn accept_ranges_bytes(&self) -> bool {
        self.header("accept-ranges")
            .map(|v| v.eq_ignore_ascii_case("bytes"))
            .unwrap_or(false)
    }
}
