//! Archive extraction adapter — composite extractor implementing the domain port.
//!
//! Delegates to format-specific handlers (ZIP, TAR, RAR, 7z) and supports
//! recursive extraction and split archive detection.

// Module is fully implemented but not yet wired into the IPC layer.
#![allow(dead_code)]

pub mod detector;
pub mod rar_handler;
pub mod segmentation;
pub mod seven_z_handler;
pub mod tar_handler;
pub mod zip_handler;

use std::path::{Path, PathBuf};
use std::time::Instant;

use tracing::{debug, info};

use crate::domain::error::DomainError;
use crate::domain::model::archive::{
    ArchiveEntry, ArchiveFormat, ExtractSummary, ExtractionConfig,
};
use crate::domain::ports::driven::ArchiveExtractor;

use self::rar_handler::RarHandler;
use self::seven_z_handler::SevenZHandler;
use self::tar_handler::TarHandler;
use self::zip_handler::ZipHandler;

/// Composite archive extractor implementing the `ArchiveExtractor` port.
///
/// Routes extraction to the appropriate format handler based on detected format.
/// Supports optional recursive extraction (archives within archives).
pub struct VortexArchiveExtractor {
    config: ExtractionConfig,
    zip: ZipHandler,
    tar: TarHandler,
    rar: RarHandler,
    seven_z: SevenZHandler,
}

impl VortexArchiveExtractor {
    pub fn new(config: ExtractionConfig) -> Self {
        Self {
            config,
            zip: ZipHandler::new(),
            tar: TarHandler::new(),
            rar: RarHandler,
            seven_z: SevenZHandler::new(),
        }
    }

    /// Extract an archive using the appropriate handler for the detected format.
    fn extract_by_format(
        &self,
        format: ArchiveFormat,
        file_path: &Path,
        dest_dir: &Path,
        password: Option<&str>,
    ) -> Result<ExtractSummary, DomainError> {
        match format {
            ArchiveFormat::Zip => self.zip.extract(file_path, dest_dir, password),
            ArchiveFormat::Tar
            | ArchiveFormat::TarGz
            | ArchiveFormat::TarBz2
            | ArchiveFormat::TarXz
            | ArchiveFormat::TarZstd => self.tar.extract(file_path, dest_dir, format),
            ArchiveFormat::RarV4 | ArchiveFormat::RarV5 => {
                self.rar.extract(file_path, dest_dir, password)
            }
            ArchiveFormat::SevenZ => self.seven_z.extract(file_path, dest_dir, password),
        }
    }

    /// List contents using the appropriate handler for the detected format.
    fn list_by_format(
        &self,
        format: ArchiveFormat,
        file_path: &Path,
        password: Option<&str>,
    ) -> Result<Vec<ArchiveEntry>, DomainError> {
        match format {
            ArchiveFormat::Zip => self.zip.list_contents(file_path, password),
            ArchiveFormat::Tar
            | ArchiveFormat::TarGz
            | ArchiveFormat::TarBz2
            | ArchiveFormat::TarXz
            | ArchiveFormat::TarZstd => self.tar.list_contents(file_path, format),
            ArchiveFormat::RarV4 | ArchiveFormat::RarV5 => {
                self.rar.list_contents(file_path, password)
            }
            ArchiveFormat::SevenZ => self.seven_z.list_contents(file_path, password),
        }
    }

    /// Recursively extract archives found within the extracted output.
    fn extract_recursive(
        &self,
        dest_dir: &Path,
        password: Option<&str>,
        depth: u32,
    ) -> Result<Vec<String>, DomainError> {
        if depth >= self.config.max_recursion_depth {
            return Ok(vec![format!(
                "max recursion depth ({}) reached",
                self.config.max_recursion_depth
            )]);
        }

        let mut warnings = Vec::new();
        let nested_archives = find_archives_in_dir(dest_dir)?;

        for archive_path in nested_archives {
            info!(
                "recursive extraction (depth {}): {}",
                depth + 1,
                archive_path.display()
            );

            let nested_dest = archive_path.with_extension("");
            std::fs::create_dir_all(&nested_dest).map_err(|e| {
                DomainError::StorageError(format!("failed to create nested dest dir: {}", e))
            })?;

            let format = match detector::detect_format(&archive_path)? {
                Some(f) => f,
                None => continue,
            };

            match self.extract_by_format(format, &archive_path, &nested_dest, password) {
                Ok(summary) => {
                    warnings.extend(summary.warnings);
                    // Recurse deeper into the newly extracted directory
                    let nested_warnings =
                        self.extract_recursive(&nested_dest, password, depth + 1)?;
                    warnings.extend(nested_warnings);
                }
                Err(e) => {
                    warnings.push(format!(
                        "failed to extract nested archive {}: {}",
                        archive_path.display(),
                        e
                    ));
                }
            }
        }

        Ok(warnings)
    }
}

impl ArchiveExtractor for VortexArchiveExtractor {
    fn detect_format(&self, file_path: &Path) -> Result<Option<ArchiveFormat>, DomainError> {
        detector::detect_format(file_path)
    }

    fn can_extract(&self, file_path: &Path) -> Result<bool, DomainError> {
        Ok(detector::detect_format(file_path)?.is_some())
    }

    fn extract(
        &self,
        file_path: &Path,
        dest_dir: &Path,
        password: Option<&str>,
    ) -> Result<ExtractSummary, DomainError> {
        let start = Instant::now();

        let format = detector::detect_format(file_path)?.ok_or_else(|| {
            DomainError::StorageError(format!(
                "unsupported archive format: {}",
                file_path.display()
            ))
        })?;

        info!(
            "extracting {} ({}) to {}",
            file_path.display(),
            format,
            dest_dir.display()
        );

        std::fs::create_dir_all(dest_dir)
            .map_err(|e| DomainError::StorageError(format!("failed to create dest dir: {}", e)))?;

        let mut summary = self.extract_by_format(format, file_path, dest_dir, password)?;

        // Recursive extraction if enabled
        if self.config.recursive_extraction {
            let recursive_warnings = self.extract_recursive(dest_dir, password, 0)?;
            summary.warnings.extend(recursive_warnings);
        }

        // Override duration to include recursive time
        summary.duration_ms = start.elapsed().as_millis() as u64;

        debug!(
            "extraction complete: {} files, {} bytes, {}ms",
            summary.extracted_files, summary.extracted_bytes, summary.duration_ms
        );

        Ok(summary)
    }

    fn list_contents(
        &self,
        file_path: &Path,
        password: Option<&str>,
    ) -> Result<Vec<ArchiveEntry>, DomainError> {
        let format = detector::detect_format(file_path)?.ok_or_else(|| {
            DomainError::StorageError(format!(
                "unsupported archive format: {}",
                file_path.display()
            ))
        })?;

        self.list_by_format(format, file_path, password)
    }

    fn detect_segments(&self, file_path: &Path) -> Result<Option<Vec<PathBuf>>, DomainError> {
        segmentation::detect_segments(file_path)
    }
}

/// Scan a directory for files that look like archives.
fn find_archives_in_dir(dir: &Path) -> Result<Vec<PathBuf>, DomainError> {
    let mut archives = Vec::new();

    let entries = std::fs::read_dir(dir).map_err(|e| {
        DomainError::StorageError(format!("failed to read dir {}: {}", dir.display(), e))
    })?;

    for entry in entries {
        let entry = entry
            .map_err(|e| DomainError::StorageError(format!("failed to read dir entry: {}", e)))?;

        let path = entry.path();
        if path.is_file() {
            if let Ok(Some(_)) = detector::detect_format(&path) {
                archives.push(path);
            }
        } else if path.is_dir() {
            // Recurse into subdirectories
            let sub_archives = find_archives_in_dir(&path)?;
            archives.extend(sub_archives);
        }
    }

    Ok(archives)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    fn default_extractor() -> VortexArchiveExtractor {
        VortexArchiveExtractor::new(ExtractionConfig::default())
    }

    #[test]
    fn test_can_extract_zip() {
        let temp_dir = TempDir::new().unwrap();
        let zip_path = temp_dir.path().join("test.zip");

        // Create a minimal valid ZIP file
        let file = std::fs::File::create(&zip_path).unwrap();
        let mut writer = zip::ZipWriter::new(file);
        let options = zip::write::FileOptions::<zip::write::ExtendedFileOptions>::default();
        writer.start_file("hello.txt", options).unwrap();
        writer.write_all(b"hello").unwrap();
        writer.finish().unwrap();

        let extractor = default_extractor();
        assert!(extractor.can_extract(&zip_path).unwrap());
    }

    #[test]
    fn test_cannot_extract_text_file() {
        let temp_dir = TempDir::new().unwrap();
        let txt_path = temp_dir.path().join("readme.txt");
        std::fs::write(&txt_path, "just text").unwrap();

        let extractor = default_extractor();
        assert!(!extractor.can_extract(&txt_path).unwrap());
    }

    #[test]
    fn test_extract_zip_via_composite() {
        let temp_dir = TempDir::new().unwrap();
        let zip_path = temp_dir.path().join("test.zip");

        let file = std::fs::File::create(&zip_path).unwrap();
        let mut writer = zip::ZipWriter::new(file);
        let options = zip::write::FileOptions::<zip::write::ExtendedFileOptions>::default();
        writer.start_file("data.txt", options).unwrap();
        writer.write_all(b"test data").unwrap();
        writer.finish().unwrap();

        let extract_dir = temp_dir.path().join("out");
        let extractor = default_extractor();
        let summary = extractor.extract(&zip_path, &extract_dir, None).unwrap();

        assert_eq!(summary.extracted_files, 1);
        assert!(extract_dir.join("data.txt").exists());
    }

    #[test]
    fn test_detect_format_via_composite() {
        let temp_dir = TempDir::new().unwrap();
        let zip_path = temp_dir.path().join("test.zip");

        let file = std::fs::File::create(&zip_path).unwrap();
        let mut writer = zip::ZipWriter::new(file);
        let options = zip::write::FileOptions::<zip::write::ExtendedFileOptions>::default();
        writer.start_file("a.txt", options).unwrap();
        writer.write_all(b"a").unwrap();
        writer.finish().unwrap();

        let extractor = default_extractor();
        let format = extractor.detect_format(&zip_path).unwrap();
        assert_eq!(format, Some(ArchiveFormat::Zip));
    }

    #[test]
    fn test_unsupported_format_returns_error() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("data.bin");
        std::fs::write(&path, b"\x00\x01\x02\x03").unwrap();

        let extractor = default_extractor();
        let result = extractor.extract(&path, temp_dir.path().join("out").as_path(), None);
        assert!(result.is_err());
    }
}
