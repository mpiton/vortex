//! Port for the download execution engine.
//!
//! Controls the lifecycle of active downloads (start, pause, cancel).
//! The adapter spawns tokio tasks for parallel segment fetching.

use crate::domain::error::DomainError;
use crate::domain::model::download::{Download, DownloadId};

/// Controls active download execution.
///
/// The adapter implementation manages tokio tasks that fetch segments
/// in parallel, report progress via the `EventBus`, and handle
/// errors with retry logic.
///
/// All methods are **fire-and-forget**: they initiate the operation and
/// return immediately. Outcome signaling (progress, completion, failure)
/// happens exclusively through `DomainEvent` published on the `EventBus`.
/// This is intentionally synchronous — the adapter spawns async work
/// internally (e.g., `tokio::spawn`) without exposing async to the domain.
pub trait DownloadEngine: Send + Sync {
    /// Start downloading (or resume) the given download.
    ///
    /// Returns `Ok(())` once the download task is spawned. Actual download
    /// progress and errors are reported via `DomainEvent::DownloadProgress`
    /// and `DomainEvent::DownloadFailed` respectively.
    fn start(&self, download: &Download) -> Result<(), DomainError>;

    /// Pause an active download, preserving segment progress.
    fn pause(&self, id: DownloadId) -> Result<(), DomainError>;

    /// Cancel an active download and discard partial data.
    fn cancel(&self, id: DownloadId) -> Result<(), DomainError>;
}
