//! 7z archive handler adapter.
//!
//! Provides extraction and content listing for 7z files using the `sevenz-rust2` crate.
//! Supports password-protected archives.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use sevenz_rust2::{ArchiveReader, Password};
use tracing::{debug, warn};

use crate::domain::error::DomainError;
use crate::domain::model::archive::{ArchiveEntry, ExtractSummary};

/// 7z archive handler — extract and list 7z files.
#[derive(Default)]
pub struct SevenZHandler;

impl SevenZHandler {
    pub fn new() -> Self {
        Self
    }

    /// Extract a 7z archive to the destination directory.
    ///
    /// Returns an `ExtractSummary` with file count, byte total, duration, and warnings.
    /// If an entry extraction fails, it is added to warnings and extraction continues.
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

        // Ensure destination directory exists
        fs::create_dir_all(dest_dir)
            .map_err(|e| DomainError::StorageError(format!("failed to create dest dir: {}", e)))?;

        // Open the 7z archive
        let password_obj = match password {
            Some(pwd) => Password::from(pwd),
            None => Password::empty(),
        };

        let mut reader = ArchiveReader::open(file_path, password_obj)
            .map_err(|e| DomainError::StorageError(format!("failed to open 7z: {}", e)))?;

        let file_count = reader.archive().files.len();
        debug!("7z contains {} entries", file_count);

        // Extract each entry
        let result = reader.for_each_entries(|entry, reader| {
            let entry_path = entry.name();
            debug!("processing entry: {}", entry_path);

            // Parse the path and protect against traversal attacks
            let safe_path = match parse_safe_path(entry_path) {
                Some(path) => path,
                None => {
                    warn!("skipping entry with path traversal: {}", entry_path);
                    warnings.push(format!("skipped suspicious path: {}", entry_path));
                    return Ok(true);
                }
            };

            let target_path = dest_dir.join(&safe_path);

            // Extract directory or file
            if entry.is_directory {
                if let Err(e) = fs::create_dir_all(&target_path) {
                    warn!("failed to create dir {}: {}", target_path.display(), e);
                    warnings.push(format!("failed to create dir: {}", e));
                    return Ok(true);
                }
            } else {
                // Create parent directories
                if let Some(parent) = target_path.parent()
                    && let Err(e) = fs::create_dir_all(parent)
                {
                    warn!("failed to create parent dirs: {}", e);
                    warnings.push(format!("failed to create parent dirs: {}", e));
                    return Ok(true);
                }

                // Extract file
                let mut file = match fs::File::create(&target_path) {
                    Ok(f) => f,
                    Err(e) => {
                        warn!("failed to create file {}: {}", target_path.display(), e);
                        warnings.push(format!("failed to create file: {}", e));
                        return Ok(true);
                    }
                };

                let bytes_written = match std::io::copy(reader, &mut file) {
                    Ok(n) => n,
                    Err(e) => {
                        warn!("failed to write file {}: {}", target_path.display(), e);
                        warnings.push(format!("failed to extract file: {}", e));
                        return Ok(true);
                    }
                };

                extracted_files += 1;
                extracted_bytes += bytes_written;
                debug!(
                    "extracted {} ({} bytes)",
                    target_path.display(),
                    bytes_written
                );
            }

            Ok(true)
        });

        if let Err(e) = result {
            return Err(DomainError::StorageError(format!(
                "7z extraction failed: {}",
                e
            )));
        }

        let duration_ms = start.elapsed().as_millis() as u64;

        Ok(ExtractSummary {
            extracted_files,
            extracted_bytes,
            duration_ms,
            warnings,
        })
    }

    /// List the contents of a 7z archive.
    ///
    /// Returns entries with path, size, is_dir, and modified timestamp.
    pub fn list_contents(
        &self,
        file_path: &Path,
        password: Option<&str>,
    ) -> Result<Vec<ArchiveEntry>, DomainError> {
        let password_obj = match password {
            Some(pwd) => Password::from(pwd),
            None => Password::empty(),
        };

        let mut reader = ArchiveReader::open(file_path, password_obj)
            .map_err(|e| DomainError::StorageError(format!("failed to open 7z: {}", e)))?;

        let mut entries = Vec::new();

        let result = reader.for_each_entries(|entry, _reader| {
            let entry_path = entry.name();

            // Skip suspicious paths
            let safe_path = match parse_safe_path(entry_path) {
                Some(path) => path,
                None => {
                    debug!("skipping entry with path traversal: {}", entry_path);
                    return Ok(true);
                }
            };

            entries.push(ArchiveEntry {
                path: safe_path,
                is_dir: entry.is_directory,
                size: entry.size,
                modified_timestamp: None,
            });

            Ok(true)
        });

        if let Err(e) = result {
            return Err(DomainError::StorageError(format!(
                "7z listing failed: {}",
                e
            )));
        }

        Ok(entries)
    }
}

/// Parse a path string and prevent directory traversal attacks.
///
/// Returns `Some(PathBuf)` if the path is safe, or `None` if it attempts traversal.
fn parse_safe_path(path_str: &str) -> Option<PathBuf> {
    let path = PathBuf::from(path_str);

    // Check if path contains .. or absolute components
    for component in path.components() {
        use std::path::Component;
        match component {
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
            Component::CurDir => {
                // Skip . components but continue
            }
            Component::Normal(_) => {
                // Normal component, safe
            }
        }
    }

    Some(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_safe_path_normal() {
        let result = parse_safe_path("docs/readme.txt");
        assert_eq!(result, Some(PathBuf::from("docs/readme.txt")));
    }

    #[test]
    fn test_parse_safe_path_simple_file() {
        let result = parse_safe_path("file.txt");
        assert_eq!(result, Some(PathBuf::from("file.txt")));
    }

    #[test]
    fn test_parse_safe_path_nested() {
        let result = parse_safe_path("a/b/c/file.txt");
        assert_eq!(result, Some(PathBuf::from("a/b/c/file.txt")));
    }

    #[test]
    fn test_parse_safe_path_rejects_parent_dir() {
        let result = parse_safe_path("../etc/passwd");
        assert_eq!(result, None);
    }

    #[test]
    fn test_parse_safe_path_rejects_absolute() {
        let result = parse_safe_path("/etc/passwd");
        assert_eq!(result, None);
    }

    #[test]
    fn test_parse_safe_path_rejects_mixed_traversal() {
        let result = parse_safe_path("docs/../../../etc/passwd");
        assert_eq!(result, None);
    }

    #[test]
    fn test_extract_7z_nonexistent() {
        let handler = SevenZHandler::new();
        let result = handler.extract(
            Path::new("/nonexistent/archive.7z"),
            Path::new("/tmp"),
            None,
        );
        assert!(result.is_err());
        match result {
            Err(DomainError::StorageError(msg)) => {
                assert!(msg.contains("failed to open 7z"));
            }
            _ => panic!("expected StorageError"),
        }
    }

    #[test]
    fn test_list_contents_nonexistent() {
        let handler = SevenZHandler::new();
        let result = handler.list_contents(Path::new("/nonexistent/archive.7z"), None);
        assert!(result.is_err());
        match result {
            Err(DomainError::StorageError(msg)) => {
                assert!(msg.contains("failed to open 7z"));
            }
            _ => panic!("expected StorageError"),
        }
    }

    #[test]
    #[ignore = "creating 7z test archives programmatically is complex; use actual .7z files for real format tests"]
    fn test_extract_7z_simple() {
        // This test would require creating a valid .7z file programmatically.
        // sevenz-rust2 is primarily a decompressor, not a compressor.
        // For integration tests, use pre-built .7z files.
    }

    #[test]
    #[ignore = "creating 7z test archives programmatically is complex; use actual .7z files for real format tests"]
    fn test_list_contents_7z() {
        // This test would require creating a valid .7z file programmatically.
        // For integration tests, use pre-built .7z files.
    }
}
