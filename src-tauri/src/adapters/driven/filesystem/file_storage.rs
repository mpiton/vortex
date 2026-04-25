//! Production [`FileStorage`] adapter backed by the local filesystem.
//!
//! Handles sparse file pre-allocation, segment writes at byte offsets,
//! and `.vortex-meta` persistence (bincode) for download resume.

use std::fs::{self, File, OpenOptions};
use std::io;
use std::io::{Read as _, Seek, SeekFrom, Write};
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
    /// Uses `create_new(true)` so the call **fails** if the file already
    /// exists, preventing silent truncation of a partially downloaded file.
    /// Callers should check for `.vortex-meta` and resume instead.
    fn create_file(&self, path: &Path, size: u64) -> Result<(), DomainError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                DomainError::StorageError(format!(
                    "failed to create directory {}: {e}",
                    parent.display()
                ))
            })?;
        }
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(|e| {
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

        let file_len = file.metadata().map(|m| m.len()).map_err(|e| {
            DomainError::StorageError(format!(
                "failed to read metadata for {}: {e}",
                path.display()
            ))
        })?;
        let end = offset.checked_add(data.len() as u64).ok_or_else(|| {
            DomainError::StorageError(format!(
                "write would overflow u64: offset {offset} + {} bytes",
                data.len()
            ))
        })?;
        if end > file_len {
            return Err(DomainError::StorageError(format!(
                "write past EOF: offset {offset} + {} bytes > file size {file_len} in {}",
                data.len(),
                path.display()
            )));
        }

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
        // Open directly and handle NotFound — avoids TOCTOU race with delete_meta.
        let mut file = match File::open(&mp) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => {
                return Err(DomainError::StorageError(format!(
                    "failed to open {}: {e}",
                    mp.display()
                )));
            }
        };

        // Reject oversized files before allocating memory.
        let file_len = file.metadata().map(|m| m.len()).map_err(|e| {
            DomainError::StorageError(format!("failed to read metadata for {}: {e}", mp.display()))
        })?;
        if file_len > meta_storage::MAX_META_SIZE as u64 {
            warn!(
                path = %mp.display(),
                size = file_len,
                "oversized .vortex-meta file — ignoring"
            );
            return Ok(None);
        }

        let mut data = Vec::with_capacity(file_len as usize);
        if let Err(e) = file.read_to_end(&mut data) {
            return Err(DomainError::StorageError(format!(
                "failed to read {}: {e}",
                mp.display()
            )));
        }

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
            let _ = fs::remove_file(&tmp);
            DomainError::StorageError(format!("failed to write {}: {e}", tmp.display()))
        })?;
        fs::rename(&tmp, &mp).map_err(|e| {
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

    fn move_file(&self, from: &Path, to: &Path) -> Result<(), DomainError> {
        if from == to {
            return Ok(());
        }
        ensure_parent_dir(to)?;
        reserve_destination(to)?;

        match rename_or_cross_fs(from, to) {
            Ok(()) => {
                debug!(from = %from.display(), to = %to.display(), "moved file");
                Ok(())
            }
            Err(e) => {
                // The move failed; drop the placeholder so we don't leave a
                // 0-byte orphan behind. Cleanup is best-effort because the
                // user already needs to know about the move failure.
                let _ = fs::remove_file(to);
                Err(DomainError::StorageError(format!(
                    "failed to move {} → {}: {e}",
                    from.display(),
                    to.display()
                )))
            }
        }
    }

    fn move_meta(&self, from: &Path, to: &Path) -> Result<(), DomainError> {
        let from_meta = meta_path(from);
        let to_meta = meta_path(to);
        if from_meta == to_meta {
            return Ok(());
        }
        ensure_parent_dir(&to_meta)?;
        // Reservation can fail with `AlreadyExists`. Source-missing trumps
        // that for sidecars — the FileStorage contract says a missing
        // sidecar is a no-op. Probe the source and only propagate the
        // reservation error when the sidecar is actually there. The probe
        // is best-effort; on its own error we treat the source as present
        // and surface the original reservation failure (cautious default).
        if let Err(reservation_err) = reserve_destination(&to_meta) {
            return match from_meta.try_exists() {
                Ok(false) => Ok(()),
                _ => Err(reservation_err),
            };
        }

        // Don't pre-check `from_meta`: a concurrent process could delete it
        // between the probe and the move, causing this function to spuriously
        // fail and roll back an already-completed body move in the
        // change_directory handler. Attempt the move, then swallow a
        // NotFound source — the sidecar contract says missing = no-op.
        match rename_or_cross_fs(&from_meta, &to_meta) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                // Sidecar absent (or vanished mid-call). Drop the placeholder
                // so we don't leave a 0-byte orphan at the new location.
                let _ = fs::remove_file(&to_meta);
                Ok(())
            }
            Err(e) => {
                let _ = fs::remove_file(&to_meta);
                Err(DomainError::StorageError(format!(
                    "failed to move sidecar {} → {}: {e}",
                    from_meta.display(),
                    to_meta.display()
                )))
            }
        }
    }
}

fn ensure_parent_dir(path: &Path) -> Result<(), DomainError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| {
            DomainError::StorageError(format!(
                "failed to create directory {}: {e}",
                parent.display()
            ))
        })?;
    }
    Ok(())
}

/// Atomically reserve `path` so a concurrent process can't sneak a different
/// file in between an `exists()` check and our subsequent move. Uses
/// `create_new` so the call fails with `AlreadyExists` instead of clobbering.
/// Caller owns the placeholder until the move succeeds (which overwrites it
/// via rename/copy) or fails (in which case caller cleans up).
fn reserve_destination(path: &Path) -> Result<(), DomainError> {
    match OpenOptions::new().write(true).create_new(true).open(path) {
        Ok(_) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Err(
            DomainError::StorageError(format!("destination already exists: {}", path.display())),
        ),
        Err(e) => Err(DomainError::StorageError(format!(
            "failed to reserve destination {}: {e}",
            path.display()
        ))),
    }
}

/// Move `from` → `to`, falling back to copy+delete on cross-device errors.
/// Returns the raw `io::Error` so callers can react to specific kinds —
/// `move_meta` in particular swallows `NotFound` to honour the sidecar
/// "missing = no-op" contract.
///
/// Caller MUST have already reserved the destination via `reserve_destination`
/// (which created an empty placeholder file). `fs::rename` atomically replaces
/// the placeholder; `fs::copy` truncates and overwrites it.
fn rename_or_cross_fs(from: &Path, to: &Path) -> io::Result<()> {
    match fs::rename(from, to) {
        Ok(()) => Ok(()),
        // EXDEV — same operation across mount points needs copy+delete.
        // `ErrorKind::CrossesDevices` is stable since Rust 1.85; the raw
        // os error fallback covers older kernels and other platforms.
        Err(e) if is_cross_device(&e) => copy_then_delete_io(from, to),
        Err(e) => Err(e),
    }
}

/// Returns true when `err` is an EXDEV-style "can't rename across devices"
/// error — the only case where we fall back to copy+delete instead of bailing.
fn is_cross_device(err: &std::io::Error) -> bool {
    // Stable since Rust 1.85. Wrapped in a match because adding a new
    // ErrorKind variant we don't recognise would otherwise be a hard error.
    if matches!(err.kind(), std::io::ErrorKind::CrossesDevices) {
        return true;
    }
    // EXDEV = 18 on Linux/macOS, ERROR_NOT_SAME_DEVICE = 17 on Windows.
    matches!(err.raw_os_error(), Some(18) | Some(17))
}

/// Copy `from` → `to` byte-for-byte, verify both ends are the same size,
/// then delete `from`. Cleans up the partially-written destination on any
/// failure so the source stays intact.
///
/// The size check guards against truncated copies that the OS reported as
/// successful (rare but possible on full disks or interrupted IO). It is
/// cheap and is the bare minimum acceptance criterion for cross-filesystem
/// moves; a content-level checksum would be stronger but is deferred until
/// we actually see a size match coexisting with content corruption.
///
/// Returns the raw `io::Error` so callers can react to specific kinds — in
/// particular, `NotFound` from the source-stat step lets `move_meta` honour
/// its "missing sidecar = no-op" contract without string-matching.
fn copy_then_delete_io(from: &Path, to: &Path) -> io::Result<()> {
    let source_len = match fs::metadata(from) {
        Ok(m) => m.len(),
        Err(e) => {
            // Source vanished (or is unreadable). Drop the placeholder we
            // reserved at `to` so callers don't see a 0-byte orphan.
            let _ = fs::remove_file(to);
            return Err(e);
        }
    };

    if let Err(e) = fs::copy(from, to) {
        let _ = fs::remove_file(to);
        return Err(e);
    }

    let dest_len = match fs::metadata(to) {
        Ok(m) => m.len(),
        Err(e) => {
            let _ = fs::remove_file(to);
            return Err(e);
        }
    };
    if dest_len != source_len {
        let _ = fs::remove_file(to);
        return Err(io::Error::other(format!(
            "copy verification failed: source {source_len} bytes, destination {dest_len} bytes"
        )));
    }

    match fs::remove_file(from) {
        Ok(()) => Ok(()),
        // Source vanished between the verify and the unlink — most likely
        // a concurrent cleanup. We have a verified copy at `to`, so the
        // move is effectively complete; throwing it away here would lose
        // data and break the move_meta NotFound contract.
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => {
            let _ = fs::remove_file(to);
            Err(e)
        }
    }
}

/// Backwards-compatible wrapper that maps the io error into `DomainError`.
/// Used by tests that exercise the cross-FS path directly without going
/// through `move_file`.
#[cfg(test)]
fn copy_then_delete(from: &Path, to: &Path) -> Result<(), DomainError> {
    copy_then_delete_io(from, to).map_err(|e| {
        DomainError::StorageError(format!(
            "failed to copy {} → {}: {e}",
            from.display(),
            to.display()
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::model::download::DownloadId;
    use crate::domain::model::meta::SegmentMeta;

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
    fn test_create_file_fails_if_already_exists() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file_path = dir.path().join("download.bin");
        let storage = FsFileStorage::new();

        storage
            .create_file(&file_path, 100)
            .expect("first create should succeed");

        let result = storage.create_file(&file_path, 100);
        assert!(
            result.is_err(),
            "second create_file on existing file should fail"
        );
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
    fn test_write_segment_rejects_write_past_eof() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file_path = dir.path().join("download.bin");
        let storage = FsFileStorage::new();

        storage.create_file(&file_path, 50).expect("create_file");

        let result = storage.write_segment(&file_path, 40, &[0xAA; 20]);
        assert!(
            result.is_err(),
            "write past EOF (40+20=60 > 50) should fail"
        );
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
    fn test_write_segment_multiple_offsets() {
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

    #[test]
    fn test_move_file_same_filesystem_renames_atomically() {
        let dir = tempfile::tempdir().expect("tempdir");
        let from = dir.path().join("original.bin");
        let to = dir.path().join("subdir").join("renamed.bin");
        fs::write(&from, b"payload").expect("seed file");

        let storage = FsFileStorage::new();
        storage.move_file(&from, &to).expect("move should succeed");

        assert!(!from.exists(), "source must be gone after move");
        assert_eq!(fs::read(&to).expect("read dest"), b"payload");
    }

    #[test]
    fn test_move_file_creates_missing_parent_directories() {
        let dir = tempfile::tempdir().expect("tempdir");
        let from = dir.path().join("file.bin");
        let to = dir.path().join("nested/deeper/file.bin");
        fs::write(&from, b"x").expect("seed file");

        let storage = FsFileStorage::new();
        storage
            .move_file(&from, &to)
            .expect("move should auto-create parents");

        assert!(to.exists());
    }

    #[test]
    fn test_move_file_noop_when_paths_equal() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("same.bin");
        fs::write(&path, b"keep").expect("seed file");

        let storage = FsFileStorage::new();
        storage
            .move_file(&path, &path)
            .expect("self-move must be a noop");

        assert_eq!(fs::read(&path).expect("read"), b"keep");
    }

    #[test]
    fn test_move_file_refuses_to_overwrite_existing_destination() {
        let dir = tempfile::tempdir().expect("tempdir");
        let from = dir.path().join("source.bin");
        let to = dir.path().join("victim.bin");
        fs::write(&from, b"new").expect("seed source");
        fs::write(&to, b"existing").expect("seed dest");

        let storage = FsFileStorage::new();
        let result = storage.move_file(&from, &to);
        assert!(result.is_err(), "must not silently clobber");
        // Source kept intact so the user can retry against another folder.
        assert_eq!(fs::read(&from).expect("source still here"), b"new");
        assert_eq!(fs::read(&to).expect("victim untouched"), b"existing");
    }

    #[test]
    fn test_move_file_propagates_missing_source_error() {
        let dir = tempfile::tempdir().expect("tempdir");
        let from = dir.path().join("ghost.bin");
        let to = dir.path().join("dest.bin");

        let storage = FsFileStorage::new();
        let result = storage.move_file(&from, &to);
        assert!(matches!(result, Err(DomainError::StorageError(_))));
    }

    #[test]
    fn test_move_meta_relocates_sidecar_when_present() {
        let dir = tempfile::tempdir().expect("tempdir");
        let from = dir.path().join("file.bin");
        let to = dir.path().join("moved").join("file.bin");
        let storage = FsFileStorage::new();

        storage.write_meta(&from, &make_meta()).expect("seed meta");
        // Body file isn't required for move_meta; it operates on the sidecar
        // alone, which is exactly the contract the change_directory handler
        // relies on after the body has already been moved.
        storage
            .move_meta(&from, &to)
            .expect("move_meta should succeed");

        assert!(!meta_path(&from).exists(), "old sidecar must be gone");
        assert!(meta_path(&to).exists(), "new sidecar must exist");
    }

    #[test]
    fn test_move_meta_is_noop_when_sidecar_absent() {
        let dir = tempfile::tempdir().expect("tempdir");
        let from = dir.path().join("file.bin");
        let to = dir.path().join("moved").join("file.bin");

        let storage = FsFileStorage::new();
        storage
            .move_meta(&from, &to)
            .expect("missing sidecar must succeed silently");
        assert!(!meta_path(&to).exists(), "no sidecar should appear");
    }

    #[test]
    fn test_move_meta_returns_ok_when_destination_exists_and_source_missing() {
        // Race shape: the destination sidecar already exists (e.g. left
        // over from an earlier failed move) AND the source sidecar is
        // absent. The sidecar contract says missing source = no-op, so we
        // MUST NOT surface "destination already exists" — it would roll
        // back the change_directory handler's body move for nothing.
        let dir = tempfile::tempdir().expect("tempdir");
        let from = dir.path().join("file.bin");
        let dest_dir = dir.path().join("dest");
        std::fs::create_dir(&dest_dir).expect("seed dest dir");
        let to = dest_dir.join("file.bin");

        let storage = FsFileStorage::new();
        storage.write_meta(&to, &make_meta()).expect("seed dest meta");
        assert!(meta_path(&to).exists(), "dest sidecar must pre-exist");
        assert!(!meta_path(&from).exists(), "source sidecar must be absent");

        storage
            .move_meta(&from, &to)
            .expect("missing source must trump dest-exists for sidecars");

        // Pre-existing dest sidecar must NOT be touched: we returned a
        // no-op without reserving anything.
        assert!(meta_path(&to).exists(), "dest sidecar must remain intact");
    }

    #[test]
    fn test_move_meta_surfaces_destination_exists_when_source_present() {
        // Symmetric guard: if the source IS present and the destination
        // is occupied by some other sidecar, we must refuse the move so
        // we don't silently clobber unrelated metadata.
        let dir = tempfile::tempdir().expect("tempdir");
        let from = dir.path().join("file.bin");
        let dest_dir = dir.path().join("dest");
        std::fs::create_dir(&dest_dir).expect("seed dest dir");
        let to = dest_dir.join("file.bin");

        let storage = FsFileStorage::new();
        storage.write_meta(&from, &make_meta()).expect("seed source meta");
        storage.write_meta(&to, &make_meta()).expect("seed dest meta");

        let err = storage
            .move_meta(&from, &to)
            .expect_err("present source + occupied dest must error");
        assert!(matches!(err, DomainError::StorageError(_)));
        // Both sidecars stay intact so the user can intervene.
        assert!(meta_path(&from).exists());
        assert!(meta_path(&to).exists());
    }

    #[test]
    fn test_move_meta_swallows_source_notfound_without_orphan_placeholder() {
        // Race shape: a concurrent process deletes the sidecar between the
        // start of move_meta and the rename call. The old probe-then-move
        // version would propagate that as an error, rolling back an
        // already-completed body move in the change_directory handler.
        // The new contract: missing source is a no-op AND no orphan
        // placeholder is left behind at the destination.
        let dir = tempfile::tempdir().expect("tempdir");
        let from = dir.path().join("file.bin");
        let to = dir.path().join("dest").join("file.bin");

        let storage = FsFileStorage::new();
        storage
            .move_meta(&from, &to)
            .expect("missing sidecar must succeed silently");
        assert!(
            !meta_path(&to).exists(),
            "the reserved placeholder must be cleaned up when source is missing"
        );
    }

    #[test]
    fn test_copy_then_delete_round_trip() {
        // Direct test of the cross-FS fallback: covers byte preservation and
        // source removal even though we can't realistically straddle two
        // filesystems inside a unit test.
        let dir = tempfile::tempdir().expect("tempdir");
        let from = dir.path().join("payload.bin");
        let to = dir.path().join("dest.bin");
        fs::write(&from, b"copy-me").expect("seed");

        super::copy_then_delete(&from, &to).expect("copy+delete should succeed");
        assert!(!from.exists());
        assert_eq!(fs::read(&to).expect("read dest"), b"copy-me");
    }

    #[test]
    fn test_copy_then_delete_cleans_up_destination_on_copy_failure() {
        // The destination is a directory, so `fs::copy` fails after no bytes
        // have been written. We expect an error and no orphan file at `to`.
        let dir = tempfile::tempdir().expect("tempdir");
        let from = dir.path().join("source.bin");
        let to = dir.path().join("dest_dir");
        fs::write(&from, b"x").expect("seed");
        fs::create_dir(&to).expect("seed dest dir");

        let result = super::copy_then_delete(&from, &to);
        assert!(result.is_err());
        assert!(from.exists(), "source must remain after rollback");
    }

    #[test]
    fn test_move_file_atomically_reserves_destination_against_clobber() {
        // The destination is created between the start of move_file and the
        // rename; without the create_new reservation step a racing process
        // could squeeze a different file in there and we would silently
        // overwrite it. With the reservation the second move sees the
        // pre-existing reserved name and refuses with "destination exists".
        let dir = tempfile::tempdir().expect("tempdir");
        let from = dir.path().join("source.bin");
        let to = dir.path().join("dest.bin");
        fs::write(&from, b"new").expect("seed source");
        // Simulate the race outcome: someone else's file is already at `to`
        // before we even start. Without the reservation step, fs::rename
        // would clobber it on Unix; with the reservation it is rejected.
        fs::write(&to, b"victim").expect("seed competing dest");

        let storage = FsFileStorage::new();
        let result = storage.move_file(&from, &to);
        assert!(result.is_err(), "must refuse to clobber an existing file");
        assert_eq!(fs::read(&from).expect("source still here"), b"new");
        assert_eq!(fs::read(&to).expect("victim untouched"), b"victim");
    }

    #[test]
    fn test_is_cross_device_recognises_exdev_codes() {
        // Synthetic os errors — covers the raw_os_error fallback path even
        // when the host kernel doesn't surface ErrorKind::CrossesDevices.
        let exdev = std::io::Error::from_raw_os_error(18);
        assert!(super::is_cross_device(&exdev), "EXDEV must be detected");

        let other = std::io::Error::from_raw_os_error(2); // ENOENT
        assert!(
            !super::is_cross_device(&other),
            "ENOENT is not cross-device"
        );
    }
}
