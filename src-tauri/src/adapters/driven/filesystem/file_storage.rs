//! Production [`FileStorage`] adapter backed by the local filesystem.
//!
//! Handles sparse file pre-allocation, segment writes at byte offsets,
//! and `.vortex-meta` persistence (bincode) for download resume.

use std::fs::{self, File, OpenOptions};
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use tracing::{debug, warn};

use crate::domain::error::DomainError;
use crate::domain::model::meta::DownloadMeta;
use crate::domain::ports::driven::FileStorage;

use super::meta_storage;

/// Counter for unique temporary file names during atomic writes.
static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Filesystem-backed implementation of [`FileStorage`].
///
/// Stateless — every method receives the target path explicitly.
#[derive(Default)]
pub struct FsFileStorage;

impl FsFileStorage {
    pub fn new() -> Self {
        Self
    }
}

/// Build the `.vortex-meta` sidecar path from a download file path.
///
/// Appends `.vortex-meta` as a suffix to the file name, so
/// `/tmp/downloads/file.zip` becomes `/tmp/downloads/file.zip.vortex-meta`.
fn meta_path(download_path: &Path) -> PathBuf {
    let file_name = download_path.file_name().unwrap_or_default().to_os_string();
    let mut meta_name = file_name;
    meta_name.push(".vortex-meta");
    download_path.with_file_name(meta_name)
}

impl FileStorage for FsFileStorage {
    /// Pre-allocate a sparse file at `path` with logical size `size`.
    ///
    /// **Precondition:** the file must not already exist with partial data,
    /// as `File::create` truncates any existing content.
    fn create_file(&self, path: &Path, size: u64) -> Result<(), DomainError> {
        let file = File::create(path).map_err(|e| {
            DomainError::StorageError(format!("failed to create {}: {e}", path.display()))
        })?;
        // set_len creates a sparse file — the OS only allocates blocks
        // as data is actually written, so a 1 GB file uses ~0 bytes on disk.
        file.set_len(size).map_err(|e| {
            DomainError::StorageError(format!(
                "failed to pre-allocate {} ({size} bytes): {e}",
                path.display()
            ))
        })?;
        debug!(path = %path.display(), size, "pre-allocated download file");
        Ok(())
    }

    fn write_segment(&self, path: &Path, offset: u64, data: &[u8]) -> Result<(), DomainError> {
        let mut file = OpenOptions::new().write(true).open(path).map_err(|e| {
            DomainError::StorageError(format!(
                "failed to open {} for writing: {e}",
                path.display()
            ))
        })?;
        file.seek(SeekFrom::Start(offset)).map_err(|e| {
            DomainError::StorageError(format!(
                "failed to seek to offset {offset} in {}: {e}",
                path.display()
            ))
        })?;
        file.write_all(data).map_err(|e| {
            DomainError::StorageError(format!(
                "failed to write {} bytes at offset {offset} in {}: {e}",
                data.len(),
                path.display()
            ))
        })?;
        Ok(())
    }

    fn read_meta(&self, path: &Path) -> Result<Option<DownloadMeta>, DomainError> {
        let mp = meta_path(path);
        // Read directly and handle NotFound — avoids TOCTOU race with delete_meta.
        let data = match fs::read(&mp) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => {
                return Err(DomainError::StorageError(format!(
                    "failed to read {}: {e}",
                    mp.display()
                )));
            }
        };
        match meta_storage::deserialize_meta(&data) {
            Ok(meta) => Ok(Some(meta)),
            Err(e) => {
                warn!(
                    path = %mp.display(),
                    error = %e,
                    "corrupted .vortex-meta file — ignoring and restarting download"
                );
                Ok(None)
            }
        }
    }

    fn write_meta(&self, path: &Path, meta: &DownloadMeta) -> Result<(), DomainError> {
        let mp = meta_path(path);
        let data = meta_storage::serialize_meta(meta)?;

        // Atomic write: write to a uniquely-named temporary file then rename,
        // so a crash during write never leaves a half-written .vortex-meta,
        // and concurrent writers don't clobber each other's temp files.
        let n = TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let tmp = mp.with_extension(format!("vortex-meta.{n}.tmp"));
        fs::write(&tmp, &data).map_err(|e| {
            DomainError::StorageError(format!("failed to write {}: {e}", tmp.display()))
        })?;
        fs::rename(&tmp, &mp).map_err(|e| {
            // Clean up orphaned tmp on rename failure
            let _ = fs::remove_file(&tmp);
            DomainError::StorageError(format!(
                "failed to rename {} → {}: {e}",
                tmp.display(),
                mp.display()
            ))
        })?;
        Ok(())
    }

    fn delete_meta(&self, path: &Path) -> Result<(), DomainError> {
        let mp = meta_path(path);
        match fs::remove_file(&mp) {
            Ok(()) => {
                debug!(path = %mp.display(), "deleted .vortex-meta");
                Ok(())
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(DomainError::StorageError(format!(
                "failed to delete {}: {e}",
                mp.display()
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::model::download::DownloadId;
    use crate::domain::model::meta::SegmentMeta;
    use std::io::Read as _;

    fn make_meta() -> DownloadMeta {
        DownloadMeta {
            download_id: DownloadId(99),
            url: "https://example.com/large.bin".to_string(),
            file_name: "large.bin".to_string(),
            total_bytes: Some(2_000_000),
            segments: vec![
                SegmentMeta {
                    id: 0,
                    start_byte: 0,
                    end_byte: 999_999,
                    downloaded_bytes: 500_000,
                    completed: false,
                },
                SegmentMeta {
                    id: 1,
                    start_byte: 1_000_000,
                    end_byte: 1_999_999,
                    downloaded_bytes: 1_000_000,
                    completed: true,
                },
            ],
            checksum_expected: None,
            created_at: 1_700_000_000,
            updated_at: 1_700_002_000,
        }
    }

    #[test]
    fn test_create_file_preallocates_correct_size() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file_path = dir.path().join("download.bin");
        let storage = FsFileStorage::new();

        storage
            .create_file(&file_path, 1_048_576)
            .expect("create_file should succeed");

        let metadata = fs::metadata(&file_path).expect("file should exist");
        assert_eq!(metadata.len(), 1_048_576);
    }

    #[test]
    fn test_write_segment_at_offset() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file_path = dir.path().join("download.bin");
        let storage = FsFileStorage::new();

        storage
            .create_file(&file_path, 100)
            .expect("create_file should succeed");

        storage
            .write_segment(&file_path, 10, b"hello")
            .expect("write_segment should succeed");

        let mut buf = vec![0u8; 100];
        let mut f = File::open(&file_path).expect("open");
        f.read_exact(&mut buf).expect("read");

        assert_eq!(&buf[10..15], b"hello");
        assert_eq!(&buf[0..10], &[0u8; 10]);
        assert_eq!(&buf[15..20], &[0u8; 5]);
    }

    #[test]
    fn test_write_and_read_meta_round_trip() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file_path = dir.path().join("download.bin");
        let storage = FsFileStorage::new();

        let original = make_meta();
        storage
            .write_meta(&file_path, &original)
            .expect("write_meta should succeed");

        let restored = storage
            .read_meta(&file_path)
            .expect("read_meta should succeed")
            .expect("meta should exist");

        assert_eq!(original, restored);
    }

    #[test]
    fn test_delete_meta_removes_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file_path = dir.path().join("download.bin");
        let storage = FsFileStorage::new();

        storage
            .write_meta(&file_path, &make_meta())
            .expect("write_meta should succeed");

        let mp = meta_path(&file_path);
        assert!(mp.exists(), ".vortex-meta should exist before delete");

        storage
            .delete_meta(&file_path)
            .expect("delete_meta should succeed");

        assert!(!mp.exists(), ".vortex-meta should be gone after delete");
    }

    #[test]
    fn test_read_meta_missing_file_returns_none() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file_path = dir.path().join("nonexistent.bin");
        let storage = FsFileStorage::new();

        let result = storage
            .read_meta(&file_path)
            .expect("read_meta should succeed even when file is missing");

        assert!(result.is_none());
    }

    #[test]
    fn test_delete_meta_missing_file_succeeds() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file_path = dir.path().join("nonexistent.bin");
        let storage = FsFileStorage::new();

        storage
            .delete_meta(&file_path)
            .expect("delete_meta on missing file should succeed");
    }

    #[test]
    fn test_write_segment_concurrent_multiple_offsets() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file_path = dir.path().join("download.bin");
        let storage = FsFileStorage::new();

        storage.create_file(&file_path, 300).expect("create_file");

        storage
            .write_segment(&file_path, 0, &[0xAA; 100])
            .expect("segment 0");
        storage
            .write_segment(&file_path, 100, &[0xBB; 100])
            .expect("segment 1");
        storage
            .write_segment(&file_path, 200, &[0xCC; 100])
            .expect("segment 2");

        let data = fs::read(&file_path).expect("read file");
        assert_eq!(&data[0..100], &[0xAA; 100]);
        assert_eq!(&data[100..200], &[0xBB; 100]);
        assert_eq!(&data[200..300], &[0xCC; 100]);
    }

    #[test]
    fn test_read_meta_corrupted_returns_none() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file_path = dir.path().join("download.bin");
        let mp = meta_path(&file_path);

        fs::write(&mp, b"not valid bincode").expect("write garbage");

        let storage = FsFileStorage::new();
        let result = storage
            .read_meta(&file_path)
            .expect("read_meta should succeed even with corruption");

        assert!(
            result.is_none(),
            "corrupted meta should return None, not error"
        );
    }

    #[test]
    fn test_meta_path_handles_various_paths() {
        let p = meta_path(Path::new("/tmp/downloads/file.zip"));
        assert_eq!(p, PathBuf::from("/tmp/downloads/file.zip.vortex-meta"));

        let p = meta_path(Path::new("/tmp/downloads/file"));
        assert_eq!(p, PathBuf::from("/tmp/downloads/file.vortex-meta"));
    }
}
