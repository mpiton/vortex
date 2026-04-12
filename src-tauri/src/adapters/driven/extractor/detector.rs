//! Archive format detection by magic bytes and file extension fallback.
//!
//! Reads the first 8 bytes (or more) from a file to detect the archive format.
//! Falls back to extension-based detection if magic bytes don't match.

use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

use crate::domain::error::DomainError;
use crate::domain::model::archive::ArchiveFormat;

/// Detects archive format by reading magic bytes or falling back to file extension.
///
/// # Errors
///
/// Returns `DomainError::StorageError` if the file cannot be read.
pub fn detect_format(file_path: &Path) -> Result<Option<ArchiveFormat>, DomainError> {
    match detect_by_magic_bytes(file_path) {
        Ok(Some(format)) => Ok(Some(format)),
        Ok(None) => Ok(extension_matches(file_path)),
        Err(e) => Err(e),
    }
}

/// Detects archive format by reading and comparing magic bytes.
fn detect_by_magic_bytes(file_path: &Path) -> Result<Option<ArchiveFormat>, DomainError> {
    let mut file = File::open(file_path)
        .map_err(|e| DomainError::StorageError(format!("failed to open file: {}", e)))?;

    let mut buffer = [0u8; 8];
    let bytes_read = file
        .read(&mut buffer)
        .map_err(|e| DomainError::StorageError(format!("failed to read file: {}", e)))?;

    // ZIP: PK\x03\x04 (local file header), PK\x05\x06 (empty archive / end of central dir),
    // PK\x07\x08 (spanned archive)
    if bytes_read >= 4
        && buffer[0] == 0x50
        && buffer[1] == 0x4B
        && matches!(buffer[2], 0x03 | 0x05 | 0x07)
    {
        return Ok(Some(ArchiveFormat::Zip));
    }

    // RAR v5: [0x52, 0x61, 0x72, 0x21, 0x1A, 0x07, 0x01, 0x00]
    if bytes_read >= 8 && buffer == [0x52, 0x61, 0x72, 0x21, 0x1A, 0x07, 0x01, 0x00] {
        return Ok(Some(ArchiveFormat::RarV5));
    }

    // RAR v4: [0x52, 0x61, 0x72, 0x21, 0x1A, 0x07, 0x00]
    if bytes_read >= 7 && buffer[0..7] == [0x52, 0x61, 0x72, 0x21, 0x1A, 0x07, 0x00] {
        return Ok(Some(ArchiveFormat::RarV4));
    }

    // 7z: [0x37, 0x7A, 0xBC, 0xAF, 0x27, 0x1C]
    if bytes_read >= 6 && buffer[0..6] == [0x37, 0x7A, 0xBC, 0xAF, 0x27, 0x1C] {
        return Ok(Some(ArchiveFormat::SevenZ));
    }

    // TAR: check "ustar" at offset 257
    if is_tar(file_path)? {
        return Ok(Some(ArchiveFormat::Tar));
    }

    Ok(None)
}

/// Checks if a file is a TAR by reading bytes 257..262 for the "ustar" signature.
fn is_tar(file_path: &Path) -> Result<bool, DomainError> {
    let mut file = File::open(file_path)
        .map_err(|e| DomainError::StorageError(format!("failed to open file: {}", e)))?;

    let mut buffer = [0u8; 262];
    match file.read_exact(&mut buffer) {
        Ok(()) => Ok(&buffer[257..262] == b"ustar"),
        Err(ref e) if e.kind() == io::ErrorKind::UnexpectedEof => {
            // File is too short to contain TAR signature
            Ok(false)
        }
        Err(e) => Err(DomainError::StorageError(format!(
            "failed to read file: {}",
            e
        ))),
    }
}

/// Matches archive format by file extension (case-insensitive).
fn extension_matches(path: &Path) -> Option<ArchiveFormat> {
    let path_str = path.to_string_lossy().to_lowercase();

    // Check multi-part extensions first (tar variants)
    if path_str.ends_with(".tar.gz") || path_str.ends_with(".tgz") {
        return Some(ArchiveFormat::TarGz);
    }
    if path_str.ends_with(".tar.bz2") || path_str.ends_with(".tbz2") {
        return Some(ArchiveFormat::TarBz2);
    }
    if path_str.ends_with(".tar.xz") || path_str.ends_with(".txz") {
        return Some(ArchiveFormat::TarXz);
    }
    if path_str.ends_with(".tar.zst") || path_str.ends_with(".tar.zstd") {
        return Some(ArchiveFormat::TarZstd);
    }

    // Single-part extensions (case-insensitive)
    let ext = path.extension()?.to_string_lossy().to_lowercase();
    match ext.as_str() {
        "tar" => Some(ArchiveFormat::Tar),
        "zip" => Some(ArchiveFormat::Zip),
        "rar" => Some(ArchiveFormat::RarV5), // Default to v5 for .rar extension
        "7z" => Some(ArchiveFormat::SevenZ),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::path::PathBuf;
    use tempfile::NamedTempFile;

    fn create_file_with_magic(magic_bytes: &[u8]) -> NamedTempFile {
        let mut file = NamedTempFile::new().expect("failed to create temp file");
        file.write_all(magic_bytes)
            .expect("failed to write magic bytes");
        file.flush().expect("failed to flush file");
        file
    }

    #[test]
    fn test_detect_zip_by_magic_bytes() {
        let file = create_file_with_magic(&[0x50, 0x4B, 0x03, 0x04, 0xFF, 0xFF, 0xFF, 0xFF]);
        let result = detect_format(file.path()).expect("should detect zip");
        assert_eq!(result, Some(ArchiveFormat::Zip));
    }

    #[test]
    fn test_detect_rar_v5_by_magic_bytes() {
        let magic = &[0x52, 0x61, 0x72, 0x21, 0x1A, 0x07, 0x01, 0x00];
        let file = create_file_with_magic(magic);
        let result = detect_format(file.path()).expect("should detect rar v5");
        assert_eq!(result, Some(ArchiveFormat::RarV5));
    }

    #[test]
    fn test_detect_rar_v4_by_magic_bytes() {
        let magic = &[0x52, 0x61, 0x72, 0x21, 0x1A, 0x07, 0x00];
        let file = create_file_with_magic(magic);
        let result = detect_format(file.path()).expect("should detect rar v4");
        assert_eq!(result, Some(ArchiveFormat::RarV4));
    }

    #[test]
    fn test_detect_7z_by_magic_bytes() {
        let magic = &[0x37, 0x7A, 0xBC, 0xAF, 0x27, 0x1C, 0xFF, 0xFF];
        let file = create_file_with_magic(magic);
        let result = detect_format(file.path()).expect("should detect 7z");
        assert_eq!(result, Some(ArchiveFormat::SevenZ));
    }

    #[test]
    fn test_detect_tar_by_extension() {
        let _file = create_file_with_magic(&[0xFF; 8]);
        let path = PathBuf::from("/tmp/archive.tar");
        let result = extension_matches(&path);
        assert_eq!(result, Some(ArchiveFormat::Tar));
    }

    #[test]
    fn test_detect_tar_gz_by_extension() {
        let path = PathBuf::from("/tmp/archive.tar.gz");
        let result = extension_matches(&path);
        assert_eq!(result, Some(ArchiveFormat::TarGz));
    }

    #[test]
    fn test_detect_tgz_by_extension() {
        let path = PathBuf::from("/tmp/archive.tgz");
        let result = extension_matches(&path);
        assert_eq!(result, Some(ArchiveFormat::TarGz));
    }

    #[test]
    fn test_detect_tar_bz2_by_extension() {
        let path = PathBuf::from("/tmp/archive.tar.bz2");
        let result = extension_matches(&path);
        assert_eq!(result, Some(ArchiveFormat::TarBz2));
    }

    #[test]
    fn test_detect_tbz2_by_extension() {
        let path = PathBuf::from("/tmp/archive.tbz2");
        let result = extension_matches(&path);
        assert_eq!(result, Some(ArchiveFormat::TarBz2));
    }

    #[test]
    fn test_detect_tar_xz_by_extension() {
        let path = PathBuf::from("/tmp/archive.tar.xz");
        let result = extension_matches(&path);
        assert_eq!(result, Some(ArchiveFormat::TarXz));
    }

    #[test]
    fn test_detect_txz_by_extension() {
        let path = PathBuf::from("/tmp/archive.txz");
        let result = extension_matches(&path);
        assert_eq!(result, Some(ArchiveFormat::TarXz));
    }

    #[test]
    fn test_detect_tar_zstd_by_extension() {
        let path = PathBuf::from("/tmp/archive.tar.zstd");
        let result = extension_matches(&path);
        assert_eq!(result, Some(ArchiveFormat::TarZstd));
    }

    #[test]
    fn test_detect_tar_zst_by_extension() {
        let path = PathBuf::from("/tmp/archive.tar.zst");
        let result = extension_matches(&path);
        assert_eq!(result, Some(ArchiveFormat::TarZstd));
    }

    #[test]
    fn test_detect_zip_by_extension() {
        let path = PathBuf::from("/tmp/archive.zip");
        let result = extension_matches(&path);
        assert_eq!(result, Some(ArchiveFormat::Zip));
    }

    #[test]
    fn test_detect_rar_by_extension() {
        let path = PathBuf::from("/tmp/archive.rar");
        let result = extension_matches(&path);
        assert_eq!(result, Some(ArchiveFormat::RarV5));
    }

    #[test]
    fn test_detect_7z_by_extension() {
        let path = PathBuf::from("/tmp/archive.7z");
        let result = extension_matches(&path);
        assert_eq!(result, Some(ArchiveFormat::SevenZ));
    }

    #[test]
    fn test_extension_case_insensitive() {
        let path = PathBuf::from("/tmp/archive.TAR.GZ");
        let result = extension_matches(&path);
        assert_eq!(result, Some(ArchiveFormat::TarGz));
    }

    #[test]
    fn test_unknown_extension_returns_none() {
        let path = PathBuf::from("/tmp/archive.txt");
        let result = extension_matches(&path);
        assert_eq!(result, None);
    }

    #[test]
    fn test_no_extension_returns_none() {
        let path = PathBuf::from("/tmp/archive");
        let result = extension_matches(&path);
        assert_eq!(result, None);
    }

    #[test]
    fn test_nonexistent_file_returns_storage_error() {
        let path = PathBuf::from("/nonexistent/archive.zip");
        let result = detect_format(&path);
        assert!(result.is_err());
        match result {
            Err(DomainError::StorageError(msg)) => {
                assert!(msg.contains("failed to open file"));
            }
            _ => panic!("expected StorageError"),
        }
    }

    #[test]
    fn test_magic_bytes_takes_precedence_over_extension() {
        let file = create_file_with_magic(&[0x50, 0x4B, 0x03, 0x04, 0xFF, 0xFF, 0xFF, 0xFF]);
        // Even with .rar extension, magic bytes should detect as ZIP
        let result = detect_format(file.path()).expect("should detect zip");
        assert_eq!(result, Some(ArchiveFormat::Zip));
    }
}
