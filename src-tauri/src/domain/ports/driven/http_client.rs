//! Port for HTTP operations needed by the download engine.
//!
//! Provides HEAD requests (for metadata) and range GET requests
//! (for segmented downloads). The adapter uses reqwest.

use crate::domain::error::DomainError;
use crate::domain::model::http::HttpResponse;

/// HTTP client for download operations.
///
/// Designed for the download engine's needs: probing file metadata
/// via HEAD, checking range support, and fetching byte ranges.
/// General-purpose HTTP (plugin host functions) uses a separate path.
pub trait HttpClient: Send + Sync {
    /// Send a HEAD request to retrieve response headers without body.
    fn head(&self, url: &str) -> Result<HttpResponse, DomainError>;

    /// Fetch a byte range from the URL (inclusive start, inclusive end).
    fn get_range(&self, url: &str, start: u64, end: u64) -> Result<Vec<u8>, DomainError>;

    /// Check whether the server supports HTTP Range requests.
    fn supports_range(&self, url: &str) -> Result<bool, DomainError>;
}
