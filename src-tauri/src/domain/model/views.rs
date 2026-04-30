//! Read model types for CQRS query results.
//!
//! These are flattened views optimized for display, not domain aggregates.
//! They are returned by read repositories. Fields are public
//! because these are data-only DTOs with no invariants to protect.

use std::collections::HashMap;

use super::download::{DownloadId, DownloadState};
use super::segment::SegmentState;

/// Flattened view of a download for list display.
///
/// Produced by `DownloadReadRepository` from optimized SQL queries.
/// Contains pre-computed fields (progress, speed, ETA) so the frontend
/// does not need to derive them from the raw aggregate.
#[derive(Debug, Clone, PartialEq)]
pub struct DownloadView {
    pub id: DownloadId,
    pub file_name: String,
    pub url: String,
    /// The origin hostname (e.g. "www.youtube.com") — distinct from the
    /// effective download URL which may point to a CDN (e.g. googlevideo.com).
    pub source_hostname: String,
    pub state: DownloadState,
    pub progress_percent: f64,
    pub speed_bytes_per_sec: u64,
    pub downloaded_bytes: u64,
    pub total_bytes: Option<u64>,
    pub eta_seconds: Option<u64>,
    pub segments_active: u32,
    pub segments_total: u32,
    pub module_name: Option<String>,
    pub account_name: Option<String>,
    pub error_message: Option<String>,
    pub priority: u8,
    /// Manual queue ordering. Lower values = earlier in queue. Zero = default
    /// (falls back to `created_at` ordering).
    pub queue_position: i64,
    pub created_at: u64,
}

/// Detailed view of a single download, including segment breakdown.
///
/// Used by the detail panel / side panel UI. Includes all fields from
/// `DownloadView` plus per-segment progress and metadata.
#[derive(Debug, Clone, PartialEq)]
pub struct DownloadDetailView {
    pub id: DownloadId,
    pub file_name: String,
    pub url: String,
    pub source_hostname: String,
    pub state: DownloadState,
    pub progress_percent: f64,
    pub speed_bytes_per_sec: u64,
    pub downloaded_bytes: u64,
    pub total_bytes: Option<u64>,
    pub eta_seconds: Option<u64>,
    pub segments: Vec<SegmentView>,
    pub checksum_expected: Option<String>,
    pub checksum_computed: Option<String>,
    pub checksum_algorithm: Option<String>,
    pub destination_path: String,
    pub module_name: Option<String>,
    pub account_name: Option<String>,
    pub resume_supported: bool,
    pub retry_count: u32,
    pub max_retries: u32,
    pub created_at: u64,
    pub updated_at: u64,
}

/// Flattened view of a single segment within a download.
#[derive(Debug, Clone, PartialEq)]
pub struct SegmentView {
    pub id: u32,
    pub start_byte: u64,
    pub end_byte: u64,
    pub downloaded_bytes: u64,
    pub state: SegmentState,
}

/// A record in the download history (completed or failed downloads).
#[derive(Debug, Clone, PartialEq)]
pub struct HistoryEntry {
    /// Primary key assigned by the history store. `0` when the entry has
    /// not yet been persisted (constructed in-memory before `record`).
    pub id: u64,
    pub download_id: DownloadId,
    pub file_name: String,
    pub url: String,
    pub total_bytes: u64,
    pub completed_at: u64,
    pub duration_seconds: u64,
    pub avg_speed: u64,
    pub destination_path: String,
}

/// Filter criteria for history list queries.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct HistoryFilter {
    /// Include entries with `completed_at >= date_from` (Unix seconds).
    pub date_from: Option<u64>,
    /// Include entries with `completed_at <= date_to` (Unix seconds).
    pub date_to: Option<u64>,
    /// Case-insensitive exact match against the URL's host component (the
    /// authority between `://` and the next `/`, stripped of userinfo and
    /// port). Blank or whitespace-only values are treated as "no filter".
    pub hostname: Option<String>,
}

/// Sort field for history list queries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HistorySortField {
    #[default]
    CompletedAt,
    FileName,
    TotalBytes,
    DurationSeconds,
}

/// Combined sort specification for history queries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HistorySort {
    pub field: HistorySortField,
    pub direction: SortDirection,
}

/// Aggregated download statistics.
#[derive(Debug, Clone, PartialEq)]
pub struct StatsView {
    pub total_downloaded_bytes: u64,
    pub total_files: u64,
    pub avg_speed: u64,
    pub peak_speed: u64,
    pub success_rate: f64,
    pub daily_volumes: Vec<DailyVolume>,
    pub top_hosts: Vec<HostStats>,
}

/// Download volume for a single day.
#[derive(Debug, Clone, PartialEq)]
pub struct DailyVolume {
    pub date: String,
    pub bytes: u64,
    pub count: u64,
}

/// Download statistics per host.
#[derive(Debug, Clone, PartialEq)]
pub struct HostStats {
    pub hostname: String,
    pub total_bytes: u64,
    pub download_count: u64,
}

/// Time window for statistics queries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StatsPeriod {
    Last7Days,
    Last30Days,
    #[default]
    AllTime,
}

impl StatsPeriod {
    /// Number of days covered by this period, or `None` for all-time.
    pub fn window_days(self) -> Option<u32> {
        match self {
            StatsPeriod::Last7Days => Some(7),
            StatsPeriod::Last30Days => Some(30),
            StatsPeriod::AllTime => None,
        }
    }
}

/// Download usage per resolving module (plugin).
#[derive(Debug, Clone, PartialEq)]
pub struct ModuleStats {
    pub module_name: String,
    pub download_count: u64,
    pub total_bytes: u64,
}

/// Filter criteria for download list queries.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DownloadFilter {
    pub state: Option<DownloadState>,
    pub search: Option<String>,
    pub host: Option<String>,
}

/// Sort field for download list queries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortField {
    #[default]
    CreatedAt,
    FileName,
    FileSize,
    Progress,
    Speed,
    State,
    QueuePosition,
}

/// Sort direction (ascending or descending).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortDirection {
    #[default]
    Ascending,
    Descending,
}

/// Combined sort specification: field + direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SortOrder {
    pub field: SortField,
    pub direction: SortDirection,
}

/// Count of downloads grouped by state.
pub type StateCountMap = HashMap<DownloadState, usize>;

/// Aggregated read view of a `Package` aggregate.
///
/// Produced by [`PackageReadRepository`](crate::domain::ports::driven::PackageReadRepository)
/// from a single `LEFT JOIN` between `packages` and `downloads` so the
/// child statistics (`downloads_count`, `total_bytes`, `progress_percent`)
/// are computed SQL-side. Avoids the N+1 round-trip the UI would otherwise
/// pay when listing dozens of packages.
#[derive(Debug, Clone, PartialEq)]
pub struct PackageView {
    pub id: String,
    pub name: String,
    /// Lowercase wire form (`container`, `playlist`, `manual`, `split_archive`).
    pub source_type: String,
    pub folder_path: Option<String>,
    pub auto_extract: bool,
    pub priority: u8,
    pub created_at: u64,
    /// Number of downloads currently attached via `downloads.package_id`.
    pub downloads_count: u64,
    /// Aggregate of member `downloads.total_bytes`. Members in state
    /// `Completed` count for `COALESCE(total_bytes, downloaded_bytes)` so
    /// the value matches what each row's per-download view reports
    /// (Completed = 100% regardless of the persisted bytes); other
    /// members with `total_bytes = NULL` contribute `0`. `0` overall
    /// when the package has no members. Mirrored on the numerator side
    /// in `downloaded_bytes` so `progress_percent` cannot exceed 100%.
    pub total_bytes: u64,
    /// Aggregate of member `downloads.downloaded_bytes`. Members in
    /// state `Completed` count for `COALESCE(total_bytes,
    /// downloaded_bytes)` (their full size when known, otherwise their
    /// persisted bytes), other members count for the persisted
    /// `downloaded_bytes`. `0` when the package has no members.
    pub downloaded_bytes: u64,
    /// Aggregate progress in `[0.0, 100.0]`, rounded to one decimal. `0.0`
    /// when no member contributes a known total. Mirrors the formula
    /// applied to individual downloads in `download_read_repo` so the UI
    /// stays consistent across rows.
    pub progress_percent: f64,
    /// `true` when at least one member download has a `Completed` state
    /// **and** every other member is also `Completed`. `false` when the
    /// package is empty or any member is still pending/failed/active.
    pub all_completed: bool,
}

/// Filter combinable on the `find_packages` read repository call.
///
/// Each field is optional. When `name_q` is set the implementation
/// performs a Unicode-aware case-insensitive substring (fuzzy) match
/// against `packages.name` — the comparison happens after the SQL fetch
/// so `LIKE` wildcards (`%`, `_`) are treated literally and non-ASCII
/// characters case-fold correctly (e.g. `café` matches `CAFÉ`). Blank
/// or whitespace-only values are treated as "no filter". When
/// `source_type` is set it constrains by the lowercase wire form
/// (`container`, `playlist`, `manual`, `split_archive`). Both fields
/// combine with AND when present.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PackageFilter {
    pub source_type: Option<String>,
    pub name_q: Option<String>,
}
