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

    /// URL could not be classified as a SoundCloud resource (host
    /// not recognised, malformed path, not SoundCloud at all).
    #[error("URL is not a recognised SoundCloud resource: {0}")]
    UnsupportedUrl(String),

    /// URL was classified as a SoundCloud resource, but the kind is
    /// not supported by the handler that was called — for example,
    /// passing an artist-profile URL to `extract_playlist`, or a
    /// playlist URL to `extract_track`. Carries the detected
    /// [`crate::url_matcher::UrlKind`] so callers can distinguish
    /// "not a SoundCloud URL at all" from "valid SoundCloud URL of
    /// the wrong kind for this operation".
    #[error("SoundCloud resource kind {kind:?} is not supported here: {url}")]
    UnsupportedResourceKind {
        kind: crate::url_matcher::UrlKind,
        url: String,
    },

    /// SoundCloud returned access-denied for a private track.
    #[error("SoundCloud resource is private: {0}")]
    Private(String),
}
