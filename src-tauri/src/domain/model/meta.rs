//! Download metadata for resume support.
//!
//! Stored as `.vortex-meta` files alongside in-progress downloads.
//! Deleted upon successful completion. Used by `FileStorage` port.

use super::download::DownloadId;

/// Metadata persisted alongside an in-progress download.
///
/// Enables resume after application restart. Contains enough state
/// to reconstruct the download progress without re-downloading
/// completed segments.
#[derive(Debug, Clone, PartialEq)]
pub struct DownloadMeta {
    pub download_id: DownloadId,
    pub url: String,
    pub file_name: String,
    pub total_bytes: Option<u64>,
    pub segments: Vec<SegmentMeta>,
    pub checksum_expected: Option<String>,
    pub created_at: u64,
    pub updated_at: u64,
}

/// Per-segment resume state within a download metadata file.
#[derive(Debug, Clone, PartialEq)]
pub struct SegmentMeta {
    pub id: u32,
    pub start_byte: u64,
    pub end_byte: u64,
    pub downloaded_bytes: u64,
    pub completed: bool,
}
