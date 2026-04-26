//! Port for opening external URLs in the user's default web browser.
//!
//! Separate from [`super::FileOpener`] because the underlying mechanism is
//! different on every platform (Linux: `xdg-open`, macOS: `open`, Windows:
//! `cmd /C start ""`) and the inputs (URL string vs filesystem path) have
//! distinct validation rules.

use crate::domain::error::DomainError;

pub trait UrlOpener: Send + Sync {
    /// Open `url` with the OS-default browser.
    ///
    /// `url` must already be a fully-formed `http(s)://` URL — the caller
    /// is responsible for encoding query strings and validating origin.
    /// Implementations must reject any other scheme to avoid handing
    /// `javascript:` / `file://` payloads to the OS launcher.
    fn open_url(&self, url: &str) -> Result<(), DomainError>;
}
