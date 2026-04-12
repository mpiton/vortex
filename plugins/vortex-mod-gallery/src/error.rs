//! Plugin error type.

use thiserror::Error;

/// Errors raised by the Gallery plugin.
#[derive(Debug, Error)]
pub enum PluginError {
    #[error("Gallery JSON parse error: {0}")]
    ParseJson(String),

    #[error("JSON error: {0}")]
    SerdeJson(#[from] serde_json::Error),

    #[error("Gallery API returned status {status}: {message}")]
    HttpStatus { status: u16, message: String },

    #[error("host function response invalid: {0}")]
    HostResponse(String),

    #[error("URL is not a recognised gallery resource: {0}")]
    UnsupportedUrl(String),

    #[error("invalid min_resolution '{0}' — expected WxH")]
    InvalidMinResolution(String),
}
