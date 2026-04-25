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

    /// Return `Ok(true)` when `path` points to an existing file or directory.
    /// Used by the `change_directory` handler to decide whether to skip the
    /// body move (e.g. for `Queued` items whose engine has not started yet).
    ///
    /// Returns an error when the underlying syscall fails for a reason other
    /// than "missing entry" (e.g. permission denied, broken symlink loop).
    /// Callers MUST surface those rather than treating them as "missing",
    /// otherwise they risk skipping a move whose source is actually present
    /// but unreadable — which would leave the storage state inconsistent.
    ///
    /// The default uses `Path::try_exists` so I/O errors surface as `Err`
    /// instead of being silently coerced into `false` (which is what the
    /// older `Path::exists()` would have done). In-memory test stubs are
    /// expected to override with their own tracker.
    fn file_exists(&self, path: &Path) -> Result<bool, DomainError> {
        path.try_exists().map_err(|e| {
            DomainError::StorageError(format!(
                "failed to probe existence of {}: {e}",
                path.display()
            ))
        })
    }

    /// Relocate `from` to `to`, creating any missing parent directories.
    ///
    /// Implementations must handle cross-filesystem moves transparently
    /// (i.e. fall back to copy + remove when an in-place rename would cross
    /// device boundaries) and must roll back any partial destination on
    /// failure so the caller can retry without leaving orphaned files.
    ///
    /// The default returns an error so an adapter that forgets to override
    /// surfaces the gap loudly instead of silently succeeding while leaving
    /// the file behind.
    fn move_file(&self, _from: &Path, _to: &Path) -> Result<(), DomainError> {
        Err(DomainError::StorageError(
            "FileStorage::move_file is not implemented for this adapter".into(),
        ))
    }

    /// Relocate the `.vortex-meta` sidecar associated with `from` so it sits
    /// next to `to`. Silently succeeds when the source sidecar is missing
    /// (the file may have been completed and its meta already deleted).
    ///
    /// The default returns an error for the same reason as `move_file`: a
    /// missing override should surface as a failure, not as a silent no-op
    /// that leaves the sidecar stranded at the old path.
    fn move_meta(&self, _from: &Path, _to: &Path) -> Result<(), DomainError> {
        Err(DomainError::StorageError(
            "FileStorage::move_meta is not implemented for this adapter".into(),
        ))
    }
}
