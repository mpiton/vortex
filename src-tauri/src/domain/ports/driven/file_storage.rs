//! Port for file I/O and download metadata persistence.
//!
//! Handles pre-allocation, segment writes, and `.vortex-meta` files
//! used for download resume.

use std::path::Path;

use crate::domain::error::DomainError;
use crate::domain::model::meta::DownloadMeta;

/// File system operations for downloads.
///
/// The adapter implementation handles platform-specific file I/O,
/// pre-allocation (fallocate/ftruncate), and atomic metadata writes.
pub trait FileStorage: Send + Sync {
    /// Pre-allocate a file at the given path with the specified size.
    fn create_file(&self, path: &Path, size: u64) -> Result<(), DomainError>;

    /// Write segment data at the specified byte offset.
    fn write_segment(&self, path: &Path, offset: u64, data: &[u8]) -> Result<(), DomainError>;

    /// Read the `.vortex-meta` resume metadata for a download.
    fn read_meta(&self, path: &Path) -> Result<Option<DownloadMeta>, DomainError>;

    /// Write (or overwrite) the `.vortex-meta` resume metadata.
    fn write_meta(&self, path: &Path, meta: &DownloadMeta) -> Result<(), DomainError>;

    /// Delete the `.vortex-meta` file (called after successful completion).
    fn delete_meta(&self, path: &Path) -> Result<(), DomainError>;
}
