//! Port for streaming-hash file checksum computation.
//!
//! Implementations stream-read the target file in chunks and return the
//! hex-encoded digest for the requested algorithm. Implementations MUST be
//! safe to invoke from any tokio worker thread (the queue manager calls
//! `compute` synchronously inside an async handler).

use std::path::Path;

use crate::domain::error::DomainError;
use crate::domain::model::checksum::ChecksumAlgorithm;

pub trait ChecksumComputer: Send + Sync {
    /// Compute the digest of `path` using `algorithm`. Returns the lowercase
    /// hex string. File I/O failures bubble up as `DomainError::StorageError`.
    fn compute(&self, path: &Path, algorithm: ChecksumAlgorithm) -> Result<String, DomainError>;
}
