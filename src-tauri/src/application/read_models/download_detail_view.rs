//! Serializable download detail view DTO for the frontend.

use serde::Serialize;

use crate::domain::model::views::{DownloadDetailView, SegmentView};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)] // Returned by query handlers (tasks 11-12)
pub struct SegmentViewDto {
    pub id: u32,
    pub start_byte: u64,
    pub end_byte: u64,
    pub downloaded_bytes: u64,
    pub state: String,
}

impl From<SegmentView> for SegmentViewDto {
    fn from(s: SegmentView) -> Self {
        Self {
            id: s.id,
            start_byte: s.start_byte,
            end_byte: s.end_byte,
            downloaded_bytes: s.downloaded_bytes,
            state: s.state.to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)] // Returned by query handlers (tasks 11-12)
pub struct DownloadDetailViewDto {
    pub id: String,
    pub file_name: String,
    pub url: String,
    pub state: String,
    pub progress_percent: f64,
    pub speed_bytes_per_sec: u64,
    pub downloaded_bytes: u64,
    pub total_bytes: Option<u64>,
    pub eta_seconds: Option<u64>,
    pub segments: Vec<SegmentViewDto>,
    pub checksum_expected: Option<String>,
    pub destination_path: String,
    pub module_name: Option<String>,
    pub account_name: Option<String>,
    pub resume_supported: bool,
    pub retry_count: u32,
    pub max_retries: u32,
    pub created_at: u64,
    pub updated_at: u64,
}

impl From<DownloadDetailView> for DownloadDetailViewDto {
    fn from(v: DownloadDetailView) -> Self {
        Self {
            id: v.id.0.to_string(),
            file_name: v.file_name,
            url: v.url,
            state: v.state.to_string(),
            progress_percent: v.progress_percent,
            speed_bytes_per_sec: v.speed_bytes_per_sec,
            downloaded_bytes: v.downloaded_bytes,
            total_bytes: v.total_bytes,
            eta_seconds: v.eta_seconds,
            segments: v.segments.into_iter().map(SegmentViewDto::from).collect(),
            checksum_expected: v.checksum_expected,
            destination_path: v.destination_path,
            module_name: v.module_name,
            account_name: v.account_name,
            resume_supported: v.resume_supported,
            retry_count: v.retry_count,
            max_retries: v.max_retries,
            created_at: v.created_at,
            updated_at: v.updated_at,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::model::download::{DownloadId, DownloadState};
    use crate::domain::model::segment::SegmentState;
    use crate::domain::model::views::{DownloadDetailView, SegmentView};

    #[test]
    fn test_segment_view_dto_from_domain() {
        let seg = SegmentView {
            id: 1,
            start_byte: 0,
            end_byte: 512,
            downloaded_bytes: 256,
            state: SegmentState::Downloading,
        };
        let dto = SegmentViewDto::from(seg);
        assert_eq!(dto.id, 1);
        assert_eq!(dto.state, "Downloading");
    }

    #[test]
    fn test_download_detail_view_dto_from_domain() {
        let view = DownloadDetailView {
            id: DownloadId(7),
            file_name: "archive.zip".to_string(),
            url: "https://example.com/archive.zip".to_string(),
            state: DownloadState::Paused,
            progress_percent: 25.0,
            speed_bytes_per_sec: 512,
            downloaded_bytes: 256,
            total_bytes: None,
            eta_seconds: None,
            segments: vec![],
            checksum_expected: None,
            destination_path: "/tmp/archive.zip".to_string(),
            module_name: None,
            account_name: None,
            resume_supported: true,
            retry_count: 1,
            max_retries: 5,
            created_at: 1700000000,
            updated_at: 1700000100,
        };
        let dto = DownloadDetailViewDto::from(view);
        assert_eq!(dto.id, "7");
        assert_eq!(dto.state, "Paused");
        assert!(dto.segments.is_empty());
    }

    #[test]
    fn test_download_detail_view_dto_serializes_to_camel_case() {
        let dto = DownloadDetailViewDto {
            id: "1".to_string(),
            file_name: "test.zip".to_string(),
            url: "https://example.com".to_string(),
            state: "Queued".to_string(),
            progress_percent: 0.0,
            speed_bytes_per_sec: 0,
            downloaded_bytes: 0,
            total_bytes: None,
            eta_seconds: None,
            segments: vec![],
            checksum_expected: None,
            destination_path: "/tmp".to_string(),
            module_name: None,
            account_name: None,
            resume_supported: false,
            retry_count: 0,
            max_retries: 5,
            created_at: 0,
            updated_at: 0,
        };
        let value = serde_json::to_value(&dto).unwrap();
        assert!(value.get("fileName").is_some());
        assert!(value.get("progressPercent").is_some());
        assert!(value.get("speedBytesPerSec").is_some());
        assert!(value.get("downloadedBytes").is_some());
        assert!(value.get("checksumExpected").is_some());
        assert!(value.get("destinationPath").is_some());
        assert!(value.get("resumeSupported").is_some());
        assert!(value.get("retryCount").is_some());
        assert!(value.get("maxRetries").is_some());
        assert!(value.get("createdAt").is_some());
        assert!(value.get("updatedAt").is_some());
    }
}
