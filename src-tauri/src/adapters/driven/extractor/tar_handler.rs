//! TAR archive handler adapter.
//!
//! Provides extraction and content listing for TAR archives using the `tar` crate.
//! Supports multiple compression formats: plain TAR, gzip, bzip2, xz, and zstd.

use std::fs;
use std::io::Read;
use std::path::Path;
use std::time::Instant;

use tracing::{debug, warn};

use crate::domain::error::DomainError;
use crate::domain::model::archive::{ArchiveEntry, ArchiveFormat, ExtractSummary};

/// TAR archive handler — extract and list TAR files (plain and compressed).
#[derive(Default)]
pub struct TarHandler;

impl TarHandler {
    pub fn new() -> Self {
        Self
    }

    /// Extract a TAR archive to the destination directory.
    ///
    /// Returns an `ExtractSummary` with file count, byte total, duration, and warnings.
    /// Supports plain TAR and compressed variants (gz, bz2, xz, zstd).
    pub fn extract(
        &self,
        file_path: &Path,
        dest_dir: &Path,
        format: ArchiveFormat,
    ) -> Result<ExtractSummary, DomainError> {
        let start = Instant::now();
        let mut warnings = Vec::new();

        // Open and decode the archive
        let decoder = Self::open_decoder(file_path, format)?;
        let mut archive = tar::Archive::new(decoder);

        // Extract all entries
        match archive.unpack(dest_dir) {
            Ok(()) => {
                debug!("TAR extraction completed successfully");
            }
            Err(e) => {
                let msg = format!("failed to unpack TAR: {}", e);
                warn!("{}", msg);
                warnings.push(msg);
            }
        }

        // Count extracted files and bytes by walking the destination directory
        let (extracted_files, extracted_bytes) = Self::count_extracted(dest_dir).unwrap_or((0, 0));

        let duration_ms = start.elapsed().as_millis() as u64;

        Ok(ExtractSummary {
            extracted_files,
            extracted_bytes,
            duration_ms,
            warnings,
        })
    }

    /// List the contents of a TAR archive.
    ///
    /// Returns entries with path, size, is_dir, and modified timestamp (if available).
    pub fn list_contents(
        &self,
        file_path: &Path,
        format: ArchiveFormat,
    ) -> Result<Vec<ArchiveEntry>, DomainError> {
        let decoder = Self::open_decoder(file_path, format)?;
        let mut archive = tar::Archive::new(decoder);

        let mut entries = Vec::new();

        for entry_result in archive
            .entries()
            .map_err(|e| DomainError::StorageError(format!("failed to read TAR entries: {}", e)))?
        {
            let entry = entry_result.map_err(|e| {
                DomainError::StorageError(format!("failed to read TAR entry: {}", e))
            })?;

            let header = entry.header();
            let path = entry
                .path()
                .map_err(|e| DomainError::StorageError(format!("invalid TAR entry path: {}", e)))?
                .to_path_buf();

            let is_dir = header.entry_type().is_dir();

            let size = entry.size();

            // Extract modified timestamp (if available)
            let modified_timestamp = header.mtime().ok().map(|t| t as i64);

            entries.push(ArchiveEntry {
                path,
                is_dir,
                size,
                modified_timestamp,
            });
        }

        Ok(entries)
    }

    /// Open a TAR decoder based on the archive format.
    ///
    /// Returns a boxed `Read` trait object pointing to the appropriate decompressor.
    fn open_decoder(file_path: &Path, format: ArchiveFormat) -> Result<Box<dyn Read>, DomainError> {
        let file = fs::File::open(file_path)
            .map_err(|e| DomainError::StorageError(format!("failed to open TAR file: {}", e)))?;

        match format {
            ArchiveFormat::Tar => Ok(Box::new(file)),
            ArchiveFormat::TarGz => {
                let decoder = flate2::read::GzDecoder::new(file);
                Ok(Box::new(decoder))
            }
            ArchiveFormat::TarBz2 => {
                let decoder = bzip2::read::BzDecoder::new(file);
                Ok(Box::new(decoder))
            }
            ArchiveFormat::TarXz => {
                let decoder = xz2::read::XzDecoder::new(file);
                Ok(Box::new(decoder))
            }
            ArchiveFormat::TarZstd => {
                let decoder = zstd::Decoder::new(file).map_err(|e| {
                    DomainError::StorageError(format!("failed to create zstd decoder: {}", e))
                })?;
                Ok(Box::new(decoder))
            }
            _ => Err(DomainError::StorageError(format!(
                "unsupported format for TAR handler: {}",
                format
            ))),
        }
    }

    /// Count extracted files and total bytes in a directory tree.
    fn count_extracted(root: &Path) -> Result<(usize, u64), DomainError> {
        let mut file_count = 0usize;
        let mut total_bytes = 0u64;

        for entry_result in fs::read_dir(root)
            .map_err(|e| DomainError::StorageError(format!("failed to read directory: {}", e)))?
        {
            let entry = entry_result.map_err(|e| {
                DomainError::StorageError(format!("failed to read dir entry: {}", e))
            })?;

            let metadata = entry.metadata().map_err(|e| {
                DomainError::StorageError(format!("failed to read metadata: {}", e))
            })?;

            if metadata.is_dir() {
                // Recursively count subdirectories
                let (sub_files, sub_bytes) = Self::count_extracted(&entry.path())?;
                file_count += sub_files;
                total_bytes += sub_bytes;
            } else {
                file_count += 1;
                total_bytes += metadata.len();
            }
        }

        Ok((file_count, total_bytes))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tempfile::TempDir;

    /// Helper to create a simple test TAR file and return the path.
    fn create_test_tar(
        contents: &[(&str, &[u8])],
    ) -> Result<(TempDir, PathBuf), Box<dyn std::error::Error>> {
        let temp_dir = TempDir::new()?;
        let tar_path = temp_dir.path().join("test.tar");

        let tar_file = fs::File::create(&tar_path)?;
        let mut builder = tar::Builder::new(tar_file);

        for (name, data) in contents {
            let mut header = tar::Header::new_gnu();
            header.set_size(data.len() as u64);
            builder.append_data(&mut header, *name, &data[..])?;
        }

        builder.finish()?;

        Ok((temp_dir, tar_path))
    }

    /// Helper to create a test TAR.GZ file.
    fn create_test_tar_gz(
        contents: &[(&str, &[u8])],
    ) -> Result<(TempDir, PathBuf), Box<dyn std::error::Error>> {
        let temp_dir = TempDir::new()?;
        let tar_gz_path = temp_dir.path().join("test.tar.gz");

        let tar_file = fs::File::create(&tar_gz_path)?;
        let gz_encoder = flate2::write::GzEncoder::new(tar_file, flate2::Compression::default());
        let mut builder = tar::Builder::new(gz_encoder);

        for (name, data) in contents {
            let mut header = tar::Header::new_gnu();
            header.set_size(data.len() as u64);
            builder.append_data(&mut header, *name, &data[..])?;
        }

        builder.finish()?;

        Ok((temp_dir, tar_gz_path))
    }

    #[test]
    fn test_extract_tar_simple() -> Result<(), Box<dyn std::error::Error>> {
        let (temp_dir, tar_path) = create_test_tar(&[
            ("file1.txt", b"hello world"),
            ("file2.txt", b"goodbye world"),
        ])?;

        let extract_dir = temp_dir.path().join("extracted");
        fs::create_dir_all(&extract_dir)?;

        let handler = TarHandler::new();
        let summary = handler.extract(&tar_path, &extract_dir, ArchiveFormat::Tar)?;

        assert_eq!(summary.extracted_files, 2);
        assert_eq!(summary.extracted_bytes, 24);
        assert!(summary.warnings.is_empty());

        assert!(extract_dir.join("file1.txt").exists());
        assert!(extract_dir.join("file2.txt").exists());

        let content1 = fs::read_to_string(extract_dir.join("file1.txt"))?;
        assert_eq!(content1, "hello world");

        Ok(())
    }

    #[test]
    fn test_extract_tar_gz() -> Result<(), Box<dyn std::error::Error>> {
        let (temp_dir, tar_gz_path) = create_test_tar_gz(&[("data.txt", b"compressed content")])?;

        let extract_dir = temp_dir.path().join("extracted");
        fs::create_dir_all(&extract_dir)?;

        let handler = TarHandler::new();
        let summary = handler.extract(&tar_gz_path, &extract_dir, ArchiveFormat::TarGz)?;

        assert_eq!(summary.extracted_files, 1);
        assert!(summary.warnings.is_empty());

        let content = fs::read_to_string(extract_dir.join("data.txt"))?;
        assert_eq!(content, "compressed content");

        Ok(())
    }

    #[test]
    fn test_list_contents_tar() -> Result<(), Box<dyn std::error::Error>> {
        let (_temp_dir, tar_path) =
            create_test_tar(&[("readme.txt", b"readme content"), ("data.json", b"{}")])?;

        let handler = TarHandler::new();
        let entries = handler.list_contents(&tar_path, ArchiveFormat::Tar)?;

        assert_eq!(entries.len(), 2);

        let readme = entries
            .iter()
            .find(|e| e.path.to_string_lossy().contains("readme.txt"))
            .expect("readme.txt not found");
        assert!(!readme.is_dir);
        assert_eq!(readme.size, 14);

        let json = entries
            .iter()
            .find(|e| e.path.to_string_lossy().contains("data.json"))
            .expect("data.json not found");
        assert!(!json.is_dir);
        assert_eq!(json.size, 2);

        Ok(())
    }
}
