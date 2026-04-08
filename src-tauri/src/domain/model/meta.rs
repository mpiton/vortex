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
#[derive(Clone, PartialEq)]
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

impl std::fmt::Debug for DownloadMeta {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DownloadMeta")
            .field("download_id", &self.download_id)
            .field("url", &"<redacted>")
            .field("file_name", &self.file_name)
            .field("total_bytes", &self.total_bytes)
            .field("segments", &self.segments.len())
            .field("created_at", &self.created_at)
            .field("updated_at", &self.updated_at)
            .finish()
    }
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
