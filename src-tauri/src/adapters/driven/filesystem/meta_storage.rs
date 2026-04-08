//! Bincode serialization layer for download metadata.
//!
//! Provides adapter-level structs (`StoredDownloadMeta`, `StoredSegmentMeta`)
//! that derive `bincode::Encode`/`Decode`. The domain `DownloadMeta` is
//! converted to/from these structs at the serialization boundary so the
//! domain remains free of external dependencies.

use crate::domain::error::DomainError;
use crate::domain::model::download::DownloadId;
use crate::domain::model::meta::{DownloadMeta, SegmentMeta};

/// Serializable mirror of [`DownloadMeta`] for `.vortex-meta` files.
#[derive(bincode::Encode, bincode::Decode)]
struct StoredDownloadMeta {
    download_id: u64,
    url: String,
    file_name: String,
    total_bytes: Option<u64>,
    segments: Vec<StoredSegmentMeta>,
    checksum_expected: Option<String>,
    created_at: u64,
    updated_at: u64,
}

/// Serializable mirror of [`SegmentMeta`].
#[derive(bincode::Encode, bincode::Decode)]
struct StoredSegmentMeta {
    id: u32,
    start_byte: u64,
    end_byte: u64,
    downloaded_bytes: u64,
    completed: bool,
}

impl From<&DownloadMeta> for StoredDownloadMeta {
    fn from(meta: &DownloadMeta) -> Self {
        Self {
            download_id: meta.download_id.0,
            url: meta.url.clone(),
            file_name: meta.file_name.clone(),
            total_bytes: meta.total_bytes,
            segments: meta.segments.iter().map(StoredSegmentMeta::from).collect(),
            checksum_expected: meta.checksum_expected.clone(),
            created_at: meta.created_at,
            updated_at: meta.updated_at,
        }
    }
}

impl From<&SegmentMeta> for StoredSegmentMeta {
    fn from(seg: &SegmentMeta) -> Self {
        Self {
            id: seg.id,
            start_byte: seg.start_byte,
            end_byte: seg.end_byte,
            downloaded_bytes: seg.downloaded_bytes,
            completed: seg.completed,
        }
    }
}

impl From<StoredDownloadMeta> for DownloadMeta {
    fn from(stored: StoredDownloadMeta) -> Self {
        Self {
            download_id: DownloadId(stored.download_id),
            url: stored.url,
            file_name: stored.file_name,
            total_bytes: stored.total_bytes,
            segments: stored.segments.into_iter().map(SegmentMeta::from).collect(),
            checksum_expected: stored.checksum_expected,
            created_at: stored.created_at,
            updated_at: stored.updated_at,
        }
    }
}

impl From<StoredSegmentMeta> for SegmentMeta {
    fn from(stored: StoredSegmentMeta) -> Self {
        Self {
            id: stored.id,
            start_byte: stored.start_byte,
            end_byte: stored.end_byte,
            downloaded_bytes: stored.downloaded_bytes,
            completed: stored.completed,
        }
    }
}

/// Serialize a [`DownloadMeta`] to bytes for `.vortex-meta` persistence.
pub fn serialize_meta(meta: &DownloadMeta) -> Result<Vec<u8>, DomainError> {
    let stored = StoredDownloadMeta::from(meta);
    bincode::encode_to_vec(&stored, bincode::config::standard())
        .map_err(|e| DomainError::StorageError(format!("failed to serialize download meta: {e}")))
}

/// Deserialize bytes from a `.vortex-meta` file back into [`DownloadMeta`].
///
/// Limits allocation to 1 MiB to prevent OOM from corrupted files
/// with inflated collection lengths.
pub fn deserialize_meta(data: &[u8]) -> Result<DownloadMeta, DomainError> {
    const MAX_META_SIZE: usize = 1 << 20; // 1 MiB
    let config = bincode::config::standard().with_limit::<MAX_META_SIZE>();
    let (stored, _): (StoredDownloadMeta, _) =
        bincode::decode_from_slice(data, config).map_err(|e| {
            DomainError::StorageError(format!("failed to deserialize download meta: {e}"))
        })?;
    Ok(DownloadMeta::from(stored))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_meta() -> DownloadMeta {
        DownloadMeta {
            download_id: DownloadId(42),
            url: "https://example.com/file.zip".to_string(),
            file_name: "file.zip".to_string(),
            total_bytes: Some(1_000_000),
            segments: vec![
                SegmentMeta {
                    id: 0,
                    start_byte: 0,
                    end_byte: 499_999,
                    downloaded_bytes: 250_000,
                    completed: false,
                },
                SegmentMeta {
                    id: 1,
                    start_byte: 500_000,
                    end_byte: 999_999,
                    downloaded_bytes: 500_000,
                    completed: true,
                },
            ],
            checksum_expected: Some("abc123".to_string()),
            created_at: 1_700_000_000,
            updated_at: 1_700_001_000,
        }
    }

    #[test]
    fn test_serialize_deserialize_roundtrip() {
        let original = make_meta();
        let bytes = serialize_meta(&original).expect("serialize should succeed");
        let restored = deserialize_meta(&bytes).expect("deserialize should succeed");
        assert_eq!(original, restored);
    }

    #[test]
    fn test_deserialize_corrupted_data_returns_error() {
        let corrupted = vec![0xFF, 0xFE, 0xFD, 0xFC];
        let result = deserialize_meta(&corrupted);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("deserialize"),
            "error should mention deserialization: {err}"
        );
    }

    #[test]
    fn test_empty_segments_roundtrip() {
        let meta = DownloadMeta {
            download_id: DownloadId(1),
            url: "https://example.com/tiny".to_string(),
            file_name: "tiny".to_string(),
            total_bytes: None,
            segments: vec![],
            checksum_expected: None,
            created_at: 0,
            updated_at: 0,
        };
        let bytes = serialize_meta(&meta).expect("serialize should succeed");
        let restored = deserialize_meta(&bytes).expect("deserialize should succeed");
        assert_eq!(meta, restored);
    }
}
