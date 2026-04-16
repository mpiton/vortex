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
    pub state: DownloadState,
    pub progress_percent: f64,
    pub speed_bytes_per_sec: u64,
    pub downloaded_bytes: u64,
    pub total_bytes: Option<u64>,
    pub eta_seconds: Option<u64>,
    pub segments: Vec<SegmentView>,
    pub checksum_expected: Option<String>,
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
    pub download_id: DownloadId,
    pub file_name: String,
    pub url: String,
    pub total_bytes: u64,
    pub completed_at: u64,
    pub duration_seconds: u64,
    pub avg_speed: u64,
    pub destination_path: String,
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
