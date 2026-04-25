//! Serializable download view DTO for the frontend.

use serde::Serialize;

use crate::domain::model::views::DownloadView;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadViewDto {
    pub id: String,
    pub file_name: String,
    pub url: String,
    pub source_hostname: String,
    pub state: String,
    pub progress_percent: f64,
    pub speed_bytes_per_sec: u64,
    pub downloaded_bytes: u64,
    pub total_bytes: Option<u64>,
    pub eta_seconds: Option<u64>,
    pub segments_active: u32,
    pub segments_total: u32,
    pub module_name: Option<String>,
    pub account_name: Option<String>,
    pub error_message: Option<String>,
    pub priority: u8,
    pub queue_position: i64,
    pub created_at: u64,
}

impl From<DownloadView> for DownloadViewDto {
    fn from(v: DownloadView) -> Self {
        Self {
            id: v.id.0.to_string(),
            file_name: v.file_name,
            url: v.url,
            source_hostname: v.source_hostname,
            state: v.state.to_string(),
            progress_percent: v.progress_percent,
            speed_bytes_per_sec: v.speed_bytes_per_sec,
            downloaded_bytes: v.downloaded_bytes,
            total_bytes: v.total_bytes,
            eta_seconds: v.eta_seconds,
            segments_active: v.segments_active,
            segments_total: v.segments_total,
            module_name: v.module_name,
            account_name: v.account_name,
            error_message: v.error_message,
            priority: v.priority,
            queue_position: v.queue_position,
            created_at: v.created_at,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::model::download::{DownloadId, DownloadState};
    use crate::domain::model::views::DownloadView;

    fn make_dto() -> DownloadViewDto {
        DownloadViewDto {
            id: "1".to_string(),
            file_name: "test.zip".to_string(),
            url: "https://example.com/test.zip".to_string(),
            source_hostname: "example.com".to_string(),
            state: "Downloading".to_string(),
            progress_percent: 50.0,
            speed_bytes_per_sec: 1024,
            downloaded_bytes: 512,
            total_bytes: Some(1024),
            eta_seconds: Some(10),
            segments_active: 2,
            segments_total: 4,
            module_name: None,
            account_name: None,
            error_message: Some("network error".to_string()),
            priority: 5,
            queue_position: 0,
            created_at: 1700000000,
        }
    }

    #[test]
    fn test_download_view_dto_serializes_to_camel_case() {
        let dto = make_dto();
        let value = serde_json::to_value(&dto).unwrap();
        assert!(value.get("fileName").is_some());
        assert!(value.get("speedBytesPerSec").is_some());
        assert!(value.get("progressPercent").is_some());
        assert!(value.get("downloadedBytes").is_some());
        assert!(value.get("totalBytes").is_some());
        assert!(value.get("etaSeconds").is_some());
        assert!(value.get("segmentsActive").is_some());
        assert!(value.get("segmentsTotal").is_some());
        assert!(value.get("moduleName").is_some());
        assert!(value.get("accountName").is_some());
        assert!(value.get("errorMessage").is_some());
        assert!(value.get("priority").is_some());
        assert!(value.get("queuePosition").is_some());
        assert!(value.get("createdAt").is_some());
    }

    #[test]
    fn test_download_view_dto_from_domain() {
        let view = DownloadView {
            id: DownloadId(42),
            file_name: "test.zip".to_string(),
            url: "https://example.com/test.zip".to_string(),
            source_hostname: "example.com".to_string(),
            state: DownloadState::Downloading,
            progress_percent: 50.0,
            speed_bytes_per_sec: 1024,
            downloaded_bytes: 512,
            total_bytes: Some(1024),
            eta_seconds: Some(10),
            segments_active: 2,
            segments_total: 4,
            module_name: None,
            account_name: None,
            error_message: Some("network error".to_string()),
            priority: 5,
            queue_position: 7,
            created_at: 1700000000,
        };
        let dto = DownloadViewDto::from(view);
        assert_eq!(dto.id, "42");
        assert_eq!(dto.state, "Downloading");
        assert_eq!(dto.error_message.as_deref(), Some("network error"));
        assert_eq!(dto.priority, 5);
        assert_eq!(dto.queue_position, 7);
    }
}
