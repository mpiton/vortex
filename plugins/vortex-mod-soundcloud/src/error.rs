//! Plugin error type.

use thiserror::Error;

/// Errors raised by the SoundCloud plugin.
#[derive(Debug, Error)]
pub enum PluginError {
    /// SoundCloud API JSON parsing failure with contextual message.
    #[error("SoundCloud JSON parse error: {0}")]
    ParseJson(String),

    /// Direct serde_json failure (no wrapping context needed).
    #[error("JSON error: {0}")]
    SerdeJson(#[from] serde_json::Error),

    /// `http_request` host function returned a non-2xx status.
    #[error("SoundCloud API returned status {status}: {message}")]
    HttpStatus { status: u16, message: String },

    /// Host function returned an invalid response envelope.
    #[error("host function response invalid: {0}")]
    HostResponse(String),

    /// URL could not be classified as a SoundCloud resource.
    #[error("URL is not a recognised SoundCloud resource: {0}")]
    UnsupportedUrl(String),

    /// SoundCloud returned access-denied for a private track.
    #[error("SoundCloud resource is private: {0}")]
    Private(String),
}
