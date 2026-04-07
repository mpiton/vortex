//! Repository for download history records.
//!
//! Tracks completed (and failed) downloads for the history view.

use crate::domain::error::DomainError;
use crate::domain::model::download::DownloadId;
use crate::domain::model::views::HistoryEntry;

/// Persists and queries download history.
///
/// History entries are created when a download completes or is
/// permanently removed. The history is append-mostly with a
/// time-based cleanup mechanism.
pub trait HistoryRepository: Send + Sync {
    /// Record a completed download in history.
    fn record(&self, entry: &HistoryEntry) -> Result<(), DomainError>;

    /// Get the most recent history entries.
    fn find_recent(&self, limit: usize) -> Result<Vec<HistoryEntry>, DomainError>;

    /// Find history entries for a specific download.
    fn find_by_download(&self, id: DownloadId) -> Result<Vec<HistoryEntry>, DomainError>;

    /// Delete history entries older than the given timestamp.
    fn delete_older_than(&self, before_timestamp: u64) -> Result<u64, DomainError>;
}
