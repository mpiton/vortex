//! Streaming SHA-256 / MD5 file digest implementation.
//!
//! Pure adapter — implements the [`ChecksumComputer`] port. Reads files in
//! 8 MB chunks so multi-gigabyte downloads can be hashed without buffering the
//! entire file in memory.

use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

use md5::{Digest as Md5Digest, Md5};
use sha2::Sha256;

use crate::domain::error::DomainError;
use crate::domain::model::checksum::ChecksumAlgorithm;
use crate::domain::ports::driven::checksum_computer::ChecksumComputer;

/// Read in 8 MB chunks per task requirement (PRD-v2 §3 P0.6).
const CHUNK_SIZE: usize = 8 * 1024 * 1024;

/// Compute the SHA-256 / MD5 digest of `path`. Pure helper — no port dep.
pub fn compute_file_checksum(
    path: &Path,
    algorithm: ChecksumAlgorithm,
) -> Result<String, DomainError> {
    let file = File::open(path).map_err(|e| {
        DomainError::StorageError(format!(
            "failed to open file for checksum at {}: {e}",
            path.display()
        ))
    })?;
    let mut reader = BufReader::with_capacity(CHUNK_SIZE, file);
    let mut buffer = vec![0u8; CHUNK_SIZE];

    match algorithm {
        ChecksumAlgorithm::Sha256 => {
            let mut hasher = Sha256::new();
            stream_into_hasher(&mut reader, &mut buffer, &mut hasher)?;
            Ok(hex::encode(hasher.finalize()))
        }
        ChecksumAlgorithm::Md5 => {
            let mut hasher = Md5::new();
            stream_into_hasher(&mut reader, &mut buffer, &mut hasher)?;
            Ok(hex::encode(hasher.finalize()))
        }
    }
}

fn stream_into_hasher<R, D>(
    reader: &mut R,
    buffer: &mut [u8],
    hasher: &mut D,
) -> Result<(), DomainError>
where
    R: Read,
    D: digest::Update,
{
    loop {
        let n = reader
            .read(buffer)
            .map_err(|e| DomainError::StorageError(format!("checksum read failure: {e}")))?;
        if n == 0 {
            return Ok(());
        }
        hasher.update(&buffer[..n]);
    }
}

pub struct StreamingChecksumComputer;

impl StreamingChecksumComputer {
    pub fn new() -> Self {
        Self
    }
}

impl Default for StreamingChecksumComputer {
    fn default() -> Self {
        Self::new()
    }
}

impl ChecksumComputer for StreamingChecksumComputer {
    fn compute(&self, path: &Path, algorithm: ChecksumAlgorithm) -> Result<String, DomainError> {
        compute_file_checksum(path, algorithm)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn write_temp(bytes: &[u8]) -> NamedTempFile {
        let mut f = NamedTempFile::new().expect("temp file");
        f.write_all(bytes).expect("write");
        f.flush().expect("flush");
        f
    }

    #[test]
    fn test_compute_sha256_empty_file_matches_known_value() {
        // SHA-256 of empty input
        let f = write_temp(b"");
        let digest = compute_file_checksum(f.path(), ChecksumAlgorithm::Sha256).expect("compute");
        assert_eq!(
            digest,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn test_compute_md5_empty_file_matches_known_value() {
        // MD5 of empty input
        let f = write_temp(b"");
        let digest = compute_file_checksum(f.path(), ChecksumAlgorithm::Md5).expect("compute");
        assert_eq!(digest, "d41d8cd98f00b204e9800998ecf8427e");
    }

    #[test]
    fn test_compute_sha256_well_known_value_for_abc() {
        let f = write_temp(b"abc");
        let digest = compute_file_checksum(f.path(), ChecksumAlgorithm::Sha256).expect("compute");
        assert_eq!(
            digest,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn test_compute_md5_well_known_value_for_abc() {
        let f = write_temp(b"abc");
        let digest = compute_file_checksum(f.path(), ChecksumAlgorithm::Md5).expect("compute");
        assert_eq!(digest, "900150983cd24fb0d6963f7d28e17f72");
    }

    #[test]
    fn test_compute_handles_payload_larger_than_chunk() {
        // > CHUNK_SIZE forces multiple read iterations through the streaming loop.
        let payload = vec![0xABu8; CHUNK_SIZE + 4096];
        let f = write_temp(&payload);
        let sha256 = compute_file_checksum(f.path(), ChecksumAlgorithm::Sha256).expect("compute");
        assert_eq!(sha256.len(), 64);
        // Sanity: same bytes hashed twice produce the same digest.
        let again = compute_file_checksum(f.path(), ChecksumAlgorithm::Sha256).expect("compute");
        assert_eq!(sha256, again);
    }

    #[test]
    fn test_compute_returns_storage_error_for_missing_file() {
        let path = std::path::Path::new("/nonexistent/path/to/file.bin");
        let err = compute_file_checksum(path, ChecksumAlgorithm::Sha256).unwrap_err();
        assert!(matches!(err, DomainError::StorageError(_)));
    }

    #[test]
    fn test_streaming_checksum_computer_implements_port() {
        let f = write_temp(b"hello");
        let computer = StreamingChecksumComputer::new();
        let result = computer
            .compute(f.path(), ChecksumAlgorithm::Sha256)
            .unwrap();
        // SHA-256("hello")
        assert_eq!(
            result,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }
}
