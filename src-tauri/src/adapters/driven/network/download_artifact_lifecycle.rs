use std::path::Path;
use std::sync::Arc;

use crate::domain::error::DomainError;
use crate::domain::model::download::DownloadId;
use crate::domain::model::meta::DownloadMeta;
use crate::domain::ports::driven::FileStorage;

pub(super) struct AttemptFailure {
    pub(super) message: String,
    pub(super) owns_artifacts: bool,
    pub(super) retryable_with_mirror: bool,
}

pub(super) enum AttemptOutcome {
    Completed,
    Cancelled,
    Failed(AttemptFailure),
}

impl AttemptFailure {
    pub(super) fn retryable(message: String, owns_artifacts: bool) -> Self {
        Self {
            message,
            owns_artifacts,
            retryable_with_mirror: true,
        }
    }

    pub(super) fn terminal(message: String, owns_artifacts: bool) -> Self {
        Self {
            message,
            owns_artifacts,
            retryable_with_mirror: false,
        }
    }
}

pub(super) fn resume_metadata_matches(
    metadata: &DownloadMeta,
    download_id: DownloadId,
    stable_url: &str,
    destination: &Path,
    total_size: u64,
) -> bool {
    let filename_matches = destination
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|filename| metadata.file_name == filename);
    let size_matches = if total_size > 0 {
        metadata.total_bytes == Some(total_size)
    } else {
        matches!(metadata.total_bytes, None | Some(0))
    };
    metadata.download_id == download_id
        && metadata.url == stable_url
        && filename_matches
        && size_matches
}

pub(super) fn ownership_metadata(
    download_id: DownloadId,
    stable_url: String,
    destination: &Path,
    total_size: u64,
) -> DownloadMeta {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    DownloadMeta {
        download_id,
        url: stable_url,
        file_name: destination
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_string(),
        total_bytes: (total_size > 0).then_some(total_size),
        segments: Vec::new(),
        checksum_expected: None,
        created_at: now,
        updated_at: now,
    }
}

pub(super) async fn cleanup_download_artifacts(
    file_storage: &Arc<dyn FileStorage>,
    dest_path: &Path,
) -> Result<(), DomainError> {
    let storage = file_storage.clone();
    let path = dest_path.to_path_buf();
    tokio::task::spawn_blocking(move || storage.delete_download_artifacts(&path))
        .await
        .map_err(|_| DomainError::StorageError("download artifact cleanup stopped".into()))?
}
