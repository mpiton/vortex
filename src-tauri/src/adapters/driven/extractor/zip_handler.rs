//! ZIP archive handler adapter.
//!
//! Provides extraction and content listing for ZIP files using the `zip` crate.
//! Implements path traversal protection via `enclosed_name()` check.

use std::fs;
use std::path::Path;
use std::time::Instant;

use tracing::{debug, warn};
use zip::ZipArchive;

use crate::domain::error::DomainError;
use crate::domain::model::archive::{ArchiveEntry, ExtractSummary};

/// Check that no ancestor between dest_dir and target is a symlink.
fn reject_symlinked_ancestors(dest_dir: &Path, target: &Path) -> bool {
    let mut current = dest_dir.to_path_buf();
    if let Ok(rel) = target.strip_prefix(dest_dir) {
        for component in rel.parent().into_iter().flat_map(|p| p.components()) {
            current.push(component);
            if current.symlink_metadata().is_ok_and(|m| m.is_symlink()) {
                return true; // Found a symlinked ancestor
            }
        }
    }
    false
}

/// ZIP archive handler — extract and list ZIP files.
#[derive(Default)]
pub struct ZipHandler;

impl ZipHandler {
    pub fn new() -> Self {
        Self
    }

    /// Extract a ZIP archive to the destination directory.
    ///
    /// Returns an `ExtractSummary` with file count, byte total, duration, and warnings.
    /// Skips entries with path traversal attempts (via `enclosed_name()`).
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

        // Open the ZIP file
        let file = fs::File::open(file_path)
            .map_err(|e| DomainError::StorageError(format!("failed to open ZIP: {}", e)))?;

        let mut archive = ZipArchive::new(file)
            .map_err(|e| DomainError::StorageError(format!("invalid ZIP: {}", e)))?;

        let entry_count = archive.len();
        debug!("ZIP contains {} entries", entry_count);

        // Extract each entry
        for i in 0..entry_count {
            let mut entry = if let Some(pwd) = password {
                archive
                    .by_index_decrypt(i, pwd.as_bytes())
                    .map_err(|e| DomainError::StorageError(format!("ZIP decrypt error: {}", e)))?
            } else {
                archive
                    .by_index(i)
                    .map_err(|e| DomainError::StorageError(format!("ZIP read error: {}", e)))?
            };

            // Security: check for path traversal attacks
            let enclosed = match entry.enclosed_name() {
                Some(path) => path,
                None => {
                    warn!("skipping entry with path traversal: {}", entry.name());
                    warnings.push(format!("skipped suspicious path: {}", entry.name()));
                    continue;
                }
            };

            let target_path = dest_dir.join(enclosed);

            // Reject symlinks: ancestors AND target itself
            if reject_symlinked_ancestors(dest_dir, &target_path) || target_path.is_symlink() {
                warn!(
                    "skipping entry with symlinked ancestor: {}",
                    target_path.display()
                );
                warnings.push(format!("skipped symlinked path: {}", target_path.display()));
                continue;
            }

            // Extract directory or file
            if entry.is_dir() {
                if let Err(e) = fs::create_dir_all(&target_path) {
                    warn!("failed to create dir {}: {}", target_path.display(), e);
                    warnings.push(format!("failed to create dir: {}", e));
                    continue;
                }
            } else {
                // Create parent directories
                if let Some(parent) = target_path.parent()
                    && let Err(e) = fs::create_dir_all(parent)
                {
                    warn!("failed to create parent dirs: {}", e);
                    warnings.push(format!("failed to create parent dirs: {}", e));
                    continue;
                }

                // Extract file
                let mut file = match fs::File::create(&target_path) {
                    Ok(f) => f,
                    Err(e) => {
                        warn!("failed to create file {}: {}", target_path.display(), e);
                        warnings.push(format!("failed to create file: {}", e));
                        continue;
                    }
                };

                let bytes_written = match std::io::copy(&mut entry, &mut file) {
                    Ok(n) => n,
                    Err(e) => {
                        warn!("failed to write file {}: {}", target_path.display(), e);
                        warnings.push(format!("failed to extract file: {}", e));
                        continue;
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
        }

        let duration_ms = start.elapsed().as_millis() as u64;

        Ok(ExtractSummary {
            extracted_files,
            extracted_bytes,
            duration_ms,
            warnings,
        })
    }

    /// List the contents of a ZIP archive.
    ///
    /// Returns entries with path, size, is_dir, and modified timestamp (if available).
    pub fn list_contents(
        &self,
        file_path: &Path,
        password: Option<&str>,
    ) -> Result<Vec<ArchiveEntry>, DomainError> {
        let file = fs::File::open(file_path)
            .map_err(|e| DomainError::StorageError(format!("failed to open ZIP: {}", e)))?;

        let mut archive = ZipArchive::new(file)
            .map_err(|e| DomainError::StorageError(format!("invalid ZIP: {}", e)))?;

        let mut entries = Vec::new();

        for i in 0..archive.len() {
            let entry = if let Some(pwd) = password {
                archive
                    .by_index_decrypt(i, pwd.as_bytes())
                    .map_err(|e| DomainError::StorageError(format!("ZIP decrypt error: {}", e)))?
            } else {
                archive
                    .by_index(i)
                    .map_err(|e| DomainError::StorageError(format!("ZIP read error: {}", e)))?
            };

            // Security: skip suspicious paths
            let enclosed = match entry.enclosed_name() {
                Some(path) => path,
                None => {
                    debug!("skipping entry with path traversal: {}", entry.name());
                    continue;
                }
            };

            // ZIP datetime conversion requires the `time` crate for accurate
            // timestamps. Return None rather than an incorrect approximation.
            let modified_timestamp: Option<i64> = None;

            entries.push(ArchiveEntry {
                path: enclosed.to_path_buf(),
                is_dir: entry.is_dir(),
                size: entry.size(),
                modified_timestamp,
            });
        }

        Ok(entries)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;
    use std::path::PathBuf;
    use tempfile::TempDir;

    /// Helper to create a simple test ZIP file in memory, write it to disk, and return the path.
    fn create_test_zip(
        contents: &[(&str, &[u8])],
    ) -> Result<(TempDir, PathBuf), Box<dyn std::error::Error>> {
        let temp_dir = TempDir::new()?;
        let zip_path = temp_dir.path().join("test.zip");

        let zip_file = fs::File::create(&zip_path)?;
        let mut writer = zip::ZipWriter::new(zip_file);

        for (name, data) in contents {
            let options = zip::write::FileOptions::<zip::write::ExtendedFileOptions>::default()
                .compression_method(zip::CompressionMethod::Deflated);
            writer.start_file(*name, options)?;
            writer.write_all(data)?;
        }
        writer.finish()?;

        Ok((temp_dir, zip_path))
    }

    #[test]
    fn test_extract_zip_simple() -> Result<(), Box<dyn std::error::Error>> {
        // Create a test ZIP with two files
        let (temp_dir, zip_path) = create_test_zip(&[
            ("file1.txt", b"hello world"),
            ("file2.txt", b"goodbye world"),
        ])?;

        // Create extraction destination
        let extract_dir = temp_dir.path().join("extracted");
        fs::create_dir_all(&extract_dir)?;

        // Extract
        let handler = ZipHandler::new();
        let summary = handler.extract(&zip_path, &extract_dir, None)?;

        // Verify extraction
        assert_eq!(summary.extracted_files, 2);
        assert_eq!(summary.extracted_bytes, 24); // "hello world" (11) + "goodbye world" (13)
        assert!(summary.warnings.is_empty());

        // Verify files exist
        assert!(extract_dir.join("file1.txt").exists());
        assert!(extract_dir.join("file2.txt").exists());

        // Verify content
        let content1 = fs::read_to_string(extract_dir.join("file1.txt"))?;
        assert_eq!(content1, "hello world");

        let content2 = fs::read_to_string(extract_dir.join("file2.txt"))?;
        assert_eq!(content2, "goodbye world");

        Ok(())
    }

    #[test]
    fn test_list_contents_zip() -> Result<(), Box<dyn std::error::Error>> {
        // Create a test ZIP with various entries
        let (_temp_dir, zip_path) =
            create_test_zip(&[("docs/readme.txt", b"readme content"), ("data.json", b"{}")])?;

        // List contents
        let handler = ZipHandler::new();
        let entries = handler.list_contents(&zip_path, None)?;

        // Verify entries
        assert_eq!(entries.len(), 2);

        // Find the readme entry
        let readme = entries
            .iter()
            .find(|e| e.path.to_string_lossy().contains("readme.txt"))
            .expect("readme.txt not found");
        assert!(!readme.is_dir);
        assert_eq!(readme.size, 14);

        // Find the data.json entry
        let json = entries
            .iter()
            .find(|e| e.path.to_string_lossy().contains("data.json"))
            .expect("data.json not found");
        assert!(!json.is_dir);
        assert_eq!(json.size, 2);

        Ok(())
    }

    #[test]
    fn test_extract_zip_with_directories() -> Result<(), Box<dyn std::error::Error>> {
        let (temp_dir, zip_path) =
            create_test_zip(&[("dir/", b""), ("dir/file.txt", b"nested file")])?;

        let extract_dir = temp_dir.path().join("extracted");
        fs::create_dir_all(&extract_dir)?;

        let handler = ZipHandler::new();
        let summary = handler.extract(&zip_path, &extract_dir, None)?;

        // Should extract the directory and the file
        assert_eq!(summary.extracted_files, 1); // Only count regular files, not dirs
        assert!(extract_dir.join("dir").is_dir());
        assert!(extract_dir.join("dir/file.txt").exists());

        Ok(())
    }
}
