//! Port for archive extraction operations.
//!
//! Implementations live in `adapters::driven::extractor`.

use std::path::Path;

use crate::domain::error::DomainError;
use crate::domain::model::archive::{ArchiveEntry, ArchiveFormat, ExtractSummary};

/// Driven port for detecting, listing, and extracting archives.
///
/// All methods are synchronous because archive extraction is CPU-bound I/O.
/// Command handlers should call these inside `spawn_blocking`.
pub trait ArchiveExtractor: Send + Sync {
    /// Detect the archive format by reading magic bytes and extension.
    /// Returns `None` if the file is not a recognized archive.
    fn detect_format(&self, file_path: &Path) -> Result<Option<ArchiveFormat>, DomainError>;

    /// Check whether this extractor can handle the given file.
    fn can_extract(&self, file_path: &Path) -> Result<bool, DomainError>;

    /// Extract an archive to the destination directory.
    /// If `password` is `Some`, attempt password-protected extraction.
    fn extract(
        &self,
        file_path: &Path,
        dest_dir: &Path,
        password: Option<&str>,
    ) -> Result<ExtractSummary, DomainError>;

    /// List archive contents without extracting.
    fn list_contents(
        &self,
        file_path: &Path,
        password: Option<&str>,
    ) -> Result<Vec<ArchiveEntry>, DomainError>;

    /// Detect whether the file is part of a split archive.
    /// Returns `Some(parts)` with all segment paths if split, `None` if single file.
    fn detect_segments(
        &self,
        file_path: &Path,
    ) -> Result<Option<Vec<std::path::PathBuf>>, DomainError>;
}
