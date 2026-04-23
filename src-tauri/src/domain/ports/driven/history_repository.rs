//! Repository for download history records.
//!
//! Tracks completed (and failed) downloads for the history view.

use crate::domain::error::DomainError;
use crate::domain::model::download::DownloadId;
use crate::domain::model::views::{HistoryEntry, HistoryFilter, HistorySort};

/// Upper bound enforced by adapters when `limit` is unset or exceeds it.
///
/// Keeps a single IPC response from serialising an unbounded history table.
/// Frontends that need more rows should paginate via `offset`.
pub const MAX_HISTORY_PAGE_SIZE: usize = 500;

/// Upper bound on the number of rows inspected by a single `search` call.
///
/// Searches scan the most recent entries up to this cap, so very old rows
/// may be excluded from matches — acceptable for a user-facing history view.
pub const MAX_HISTORY_SEARCH_RESULTS: usize = 500;

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
    /// Implementations must clamp `limit` to [`MAX_HISTORY_PAGE_SIZE`] and
    /// treat `None` as the same cap. Sorting defaults to `completed_at DESC`.
    /// `HistoryFilter::hostname` matches the URL's host component exactly
    /// (case-insensitive), not an arbitrary substring of the URL.
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
    /// (case-insensitive). Implementations must cap the number of scanned
    /// rows at [`MAX_HISTORY_SEARCH_RESULTS`] to keep IPC payloads bounded.
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
