//! Archive extraction domain types.
//!
//! Pure domain models for archive handling — no external dependencies.

use std::path::PathBuf;

/// Supported archive formats, detected by magic bytes or extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveFormat {
    Zip,
    RarV4,
    RarV5,
    SevenZ,
    Tar,
    TarGz,
    TarBz2,
    TarXz,
    TarZstd,
}

impl ArchiveFormat {
    /// Human-readable label for display purposes.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Zip => "ZIP",
            Self::RarV4 => "RAR v4",
            Self::RarV5 => "RAR v5",
            Self::SevenZ => "7z",
            Self::Tar => "TAR",
            Self::TarGz => "TAR.GZ",
            Self::TarBz2 => "TAR.BZ2",
            Self::TarXz => "TAR.XZ",
            Self::TarZstd => "TAR.ZSTD",
        }
    }

    /// Whether this format supports password protection.
    pub fn supports_password(&self) -> bool {
        matches!(self, Self::Zip | Self::RarV4 | Self::RarV5 | Self::SevenZ)
    }
}

impl std::fmt::Display for ArchiveFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

/// Result of an extraction operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractSummary {
    pub extracted_files: usize,
    pub extracted_bytes: u64,
    pub duration_ms: u64,
    pub warnings: Vec<String>,
}

/// A single entry within an archive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveEntry {
    pub path: PathBuf,
    pub is_dir: bool,
    pub size: u64,
    /// Unix timestamp (seconds since epoch), if available.
    pub modified_timestamp: Option<i64>,
}

/// Configuration for the extraction pipeline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractionConfig {
    pub auto_extract: bool,
    pub delete_after_extract: bool,
    pub extraction_folder: Option<PathBuf>,
    pub recursive_extraction: bool,
    pub max_recursion_depth: u32,
}

impl Default for ExtractionConfig {
    fn default() -> Self {
        Self {
            auto_extract: false,
            delete_after_extract: false,
            extraction_folder: None,
            recursive_extraction: false,
            max_recursion_depth: 2,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_archive_format_label() {
        assert_eq!(ArchiveFormat::Zip.label(), "ZIP");
        assert_eq!(ArchiveFormat::RarV5.label(), "RAR v5");
        assert_eq!(ArchiveFormat::TarZstd.label(), "TAR.ZSTD");
    }

    #[test]
    fn test_archive_format_display() {
        assert_eq!(format!("{}", ArchiveFormat::SevenZ), "7z");
    }

    #[test]
    fn test_archive_format_supports_password() {
        assert!(ArchiveFormat::Zip.supports_password());
        assert!(ArchiveFormat::RarV4.supports_password());
        assert!(ArchiveFormat::RarV5.supports_password());
        assert!(ArchiveFormat::SevenZ.supports_password());
        assert!(!ArchiveFormat::Tar.supports_password());
        assert!(!ArchiveFormat::TarGz.supports_password());
    }

    #[test]
    fn test_extract_summary_creation() {
        let summary = ExtractSummary {
            extracted_files: 10,
            extracted_bytes: 1024,
            duration_ms: 500,
            warnings: vec!["skipped empty entry".to_string()],
        };
        assert_eq!(summary.extracted_files, 10);
        assert_eq!(summary.warnings.len(), 1);
    }

    #[test]
    fn test_archive_entry_creation() {
        let entry = ArchiveEntry {
            path: PathBuf::from("docs/readme.txt"),
            is_dir: false,
            size: 256,
            modified_timestamp: Some(1_700_000_000),
        };
        assert!(!entry.is_dir);
        assert_eq!(entry.size, 256);
    }

    #[test]
    fn test_extraction_config_default() {
        let config = ExtractionConfig::default();
        assert!(!config.auto_extract);
        assert!(!config.delete_after_extract);
        assert!(config.extraction_folder.is_none());
        assert!(!config.recursive_extraction);
        assert_eq!(config.max_recursion_depth, 2);
    }
}
