//! Write repository for the `Download` aggregate (CQRS write side).
//!
//! Manipulates domain entities directly. Used by command handlers
//! to load, persist, and delete downloads.

use crate::domain::error::DomainError;
use crate::domain::model::download::{Download, DownloadId, DownloadState};

/// Persists and retrieves `Download` aggregates.
///
/// This is the **write** repository in the CQRS pattern. It works with
/// full domain entities, not flattened views. For read-optimized queries,
/// see `DownloadReadRepository`.
pub trait DownloadRepository: Send + Sync {
    /// Find a download by its unique identifier.
    fn find_by_id(&self, id: DownloadId) -> Result<Option<Download>, DomainError>;

    /// Persist a download (insert or update).
    fn save(&self, download: &Download) -> Result<(), DomainError>;

    /// Persist a failed download and store its raw backend error string.
    fn save_failed(&self, download: &Download, _error_message: &str) -> Result<(), DomainError> {
        self.save(download)
    }

    /// Delete a download by its identifier.
    fn delete(&self, id: DownloadId) -> Result<(), DomainError>;

    /// Find all downloads in a given state.
    fn find_by_state(&self, state: DownloadState) -> Result<Vec<Download>, DomainError>;
}
