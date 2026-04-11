//! Plugin error type.

use thiserror::Error;

/// Errors raised by the YouTube plugin.
///
/// Uses `#[from] serde_json::Error` so that callers can propagate JSON
/// errors with `?` while preserving the underlying source chain via
/// `std::error::Error::source()`.
#[derive(Debug, Error)]
pub enum PluginError {
    /// yt-dlp JSON parsing failure with contextual message.
    #[error("yt-dlp JSON parse error: {0}")]
    ParseJson(String),

    /// Direct serde_json failure (no wrapping context needed).
    #[error("JSON error: {0}")]
    SerdeJson(#[from] serde_json::Error),

    /// yt-dlp subprocess returned a non-zero exit code.
    #[error("yt-dlp failed (exit code {exit_code}): {stderr}")]
    Subprocess { exit_code: i32, stderr: String },

    /// Host function returned an invalid response envelope.
    #[error("host function response invalid: {0}")]
    HostResponse(String),

    /// URL could not be classified as a YouTube resource.
    #[error("URL is not a recognised YouTube resource: {0}")]
    UnsupportedUrl(String),

    /// No format matched the user's quality preference.
    #[error("no format matches requested quality")]
    NoMatchingFormat,
}
