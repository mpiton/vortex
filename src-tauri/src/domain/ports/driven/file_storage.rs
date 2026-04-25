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

    /// Return `true` when `path` points to an existing file or directory.
    /// Used by the `change_directory` handler to decide whether to skip the
    /// body move (e.g. for `Queued` items whose engine has not started yet).
    ///
    /// The default implementation defers to the real filesystem so existing
    /// adapters work unchanged; in-memory test stubs override.
    fn file_exists(&self, path: &Path) -> bool {
        path.exists()
    }

    /// Relocate `from` to `to`, creating any missing parent directories.
    ///
    /// Implementations must handle cross-filesystem moves transparently
    /// (i.e. fall back to copy + remove when an in-place rename would cross
    /// device boundaries) and must roll back any partial destination on
    /// failure so the caller can retry without leaving orphaned files.
    ///
    /// The default implementation is a noop suitable for in-memory test
    /// stubs that don't track real on-disk paths. Production adapters MUST
    /// override.
    fn move_file(&self, _from: &Path, _to: &Path) -> Result<(), DomainError> {
        Ok(())
    }

    /// Relocate the `.vortex-meta` sidecar associated with `from` so it sits
    /// next to `to`. Silently succeeds when the source sidecar is missing
    /// (the file may have been completed and its meta already deleted).
    ///
    /// The default implementation is a noop suitable for in-memory test
    /// stubs. Production adapters MUST override.
    fn move_meta(&self, _from: &Path, _to: &Path) -> Result<(), DomainError> {
        Ok(())
    }
}
