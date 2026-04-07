//! Port for clipboard monitoring.
//!
//! Watches the system clipboard for URLs that can be added to
//! the download queue automatically.

use crate::domain::error::DomainError;

/// Monitors the system clipboard for downloadable URLs.
///
/// The adapter uses platform-specific APIs (or `arboard`) to
/// poll or watch the clipboard. Detected URLs are reported back
/// so the application layer can trigger the link-grabber flow.
pub trait ClipboardObserver: Send + Sync {
    /// Start watching the clipboard for URL changes.
    fn start(&self) -> Result<(), DomainError>;

    /// Stop watching the clipboard.
    fn stop(&self) -> Result<(), DomainError>;

    /// Get URLs detected since the last call (drains the internal buffer).
    fn get_urls(&self) -> Result<Vec<String>, DomainError>;
}
