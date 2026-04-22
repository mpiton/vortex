//! Repository for download history records.
//!
//! Tracks completed (and failed) downloads for the history view.

use crate::domain::error::DomainError;
use crate::domain::model::download::DownloadId;
use crate::domain::model::views::{HistoryEntry, HistoryFilter, HistorySort};

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

    /// List history entries with optional filter, sort and pagination.
    ///
    /// Defaults: sort by `completed_at DESC`, no pagination.
    fn list(
        &self,
        filter: Option<HistoryFilter>,
        sort: Option<HistorySort>,
        limit: Option<usize>,
        offset: Option<usize>,
    ) -> Result<Vec<HistoryEntry>, DomainError>;

    /// Full-text search across file name, URL and destination path.
    ///
    /// Returns entries where any of those columns contain `query`
    /// (case-insensitive).
    fn search(&self, query: &str) -> Result<Vec<HistoryEntry>, DomainError>;

    /// Find a single history entry by its primary key.
    fn find_by_id(&self, id: u64) -> Result<Option<HistoryEntry>, DomainError>;

    /// Delete a single history entry by its primary key.
    ///
    /// Returns `true` if an entry was removed.
    fn delete_by_id(&self, id: u64) -> Result<bool, DomainError>;

    /// Delete every history entry. Returns the number of rows removed.
    fn delete_all(&self) -> Result<u64, DomainError>;

    /// Delete history entries older than the given Unix timestamp in seconds.
    ///
    /// Returns the number of entries deleted.
    fn delete_older_than(&self, before_timestamp: u64) -> Result<u64, DomainError>;
}
