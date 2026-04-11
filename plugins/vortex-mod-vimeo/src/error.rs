//! Plugin error type.

use thiserror::Error;

/// Errors raised by the Vimeo plugin.
#[derive(Debug, Error)]
pub enum PluginError {
    /// JSON parsing failure with contextual message.
    #[error("Vimeo JSON parse error: {0}")]
    ParseJson(String),

    /// Direct serde_json failure.
    #[error("JSON error: {0}")]
    SerdeJson(#[from] serde_json::Error),

    /// `http_request` host function returned a non-2xx status.
    #[error("Vimeo API returned status {status}: {message}")]
    HttpStatus { status: u16, message: String },

    /// Vimeo player config JSON not found on the page HTML.
    #[error("Vimeo player config not found on page")]
    PlayerConfigNotFound,

    /// Host function returned an invalid response envelope.
    #[error("host function response invalid: {0}")]
    HostResponse(String),

    /// URL could not be classified as a Vimeo resource.
    #[error("URL is not a recognised Vimeo resource: {0}")]
    UnsupportedUrl(String),

    /// Vimeo video is private or requires authentication.
    #[error("Vimeo resource is private: {0}")]
    Private(String),
}
