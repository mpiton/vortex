//! RAR archive extraction handler using the `unrar` crate.
//!
//! Supports RAR v4/v5 and password-protected archives.
//! Requires libunrar system library.

use std::path::{Path, PathBuf};
use std::time::Instant;

use unrar::Archive;

use crate::domain::error::DomainError;
use crate::domain::model::archive::{ArchiveEntry, ExtractSummary};

/// RAR archive handler for extraction and listing operations.
pub struct RarHandler;

impl RarHandler {
    /// Extracts a RAR archive to the specified destination directory.
    pub fn extract(
        &self,
        file_path: &Path,
        dest_dir: &Path,
        password: Option<&str>,
    ) -> Result<ExtractSummary, DomainError> {
        let start = Instant::now();
        let mut extracted_files = 0usize;
        let mut extracted_bytes = 0u64;
        let mut warnings = Vec::new();

        let archive = if let Some(pwd) = password {
            Archive::with_password(file_path, pwd)
        } else {
            Archive::new(file_path)
        };

        let mut open = archive
            .open_for_processing()
            .map_err(|e| DomainError::StorageError(format!("failed to open RAR archive: {}", e)))?;

        while let Some(header) = open
            .read_header()
            .map_err(|e| DomainError::StorageError(format!("failed to read RAR header: {}", e)))?
        {
            let is_dir = header.entry().is_directory();
            let size = header.entry().unpacked_size;

            if is_dir {
                open = header.skip().map_err(|e| {
                    DomainError::StorageError(format!("failed to skip dir entry: {}", e))
                })?;
                continue;
            }

            extracted_bytes = extracted_bytes.saturating_add(size);

            match header.extract_to(dest_dir) {
                Ok(next) => {
                    extracted_files += 1;
                    open = next;
                }
                Err(e) => {
                    warnings.push(format!("failed to extract entry: {}", e));
                    break;
                }
            }
        }

        let duration_ms = start.elapsed().as_millis() as u64;

        Ok(ExtractSummary {
            extracted_files,
            extracted_bytes,
            duration_ms,
            warnings,
        })
    }

    /// Lists the contents of a RAR archive without extracting.
    pub fn list_contents(
        &self,
        file_path: &Path,
        password: Option<&str>,
    ) -> Result<Vec<ArchiveEntry>, DomainError> {
        let archive = if let Some(pwd) = password {
            Archive::with_password(file_path, pwd)
        } else {
            Archive::new(file_path)
        };

        let mut open = archive.open_for_listing().map_err(|e| {
            DomainError::StorageError(format!("failed to open RAR for listing: {}", e))
        })?;

        let mut entries = Vec::new();

        while let Some(header) = open
            .read_header()
            .map_err(|e| DomainError::StorageError(format!("failed to read RAR entry: {}", e)))?
        {
            let entry = header.entry();

            entries.push(ArchiveEntry {
                path: PathBuf::from(&entry.filename),
                is_dir: entry.is_directory(),
                size: entry.unpacked_size,
                modified_timestamp: None,
            });

            open = header
                .skip()
                .map_err(|e| DomainError::StorageError(format!("failed to skip entry: {}", e)))?;
        }

        Ok(entries)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_list_contents_nonexistent_file() {
        let handler = RarHandler;
        let result = handler.list_contents(Path::new("/tmp/nonexistent_archive_12345.rar"), None);
        assert!(result.is_err());
        match result {
            Err(DomainError::StorageError(msg)) => {
                assert!(msg.contains("failed to open RAR"));
            }
            _ => panic!("expected StorageError"),
        }
    }
}
