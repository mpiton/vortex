//! Serializable history entry DTO for the frontend.

use serde::Serialize;

use crate::domain::model::views::HistoryEntry;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryViewDto {
    pub download_id: String,
    pub file_name: String,
    pub url: String,
    pub total_bytes: u64,
    pub completed_at: u64,
    pub duration_seconds: u64,
    pub avg_speed: u64,
    pub destination_path: String,
}

impl From<HistoryEntry> for HistoryViewDto {
    fn from(e: HistoryEntry) -> Self {
        Self {
            download_id: e.download_id.0.to_string(),
            file_name: e.file_name,
            url: e.url,
            total_bytes: e.total_bytes,
            completed_at: e.completed_at,
            duration_seconds: e.duration_seconds,
            avg_speed: e.avg_speed,
            destination_path: e.destination_path,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::model::download::DownloadId;
    use crate::domain::model::views::HistoryEntry;

    #[test]
    fn test_history_view_dto_from_domain() {
        let entry = HistoryEntry {
            download_id: DownloadId(99),
            file_name: "movie.mkv".to_string(),
            url: "https://example.com/movie.mkv".to_string(),
            total_bytes: 2_000_000,
            completed_at: 1700001000,
            duration_seconds: 120,
            avg_speed: 16666,
            destination_path: "/home/user/Downloads/movie.mkv".to_string(),
        };
        let dto = HistoryViewDto::from(entry);
        assert_eq!(dto.download_id, "99");
        assert_eq!(dto.file_name, "movie.mkv");
        assert_eq!(dto.total_bytes, 2_000_000);
    }

    #[test]
    fn test_history_view_dto_serializes_to_camel_case() {
        let dto = HistoryViewDto {
            download_id: "1".to_string(),
            file_name: "file.zip".to_string(),
            url: "https://example.com".to_string(),
            total_bytes: 1024,
            completed_at: 0,
            duration_seconds: 10,
            avg_speed: 100,
            destination_path: "/tmp/file.zip".to_string(),
        };
        let value = serde_json::to_value(&dto).unwrap();
        assert!(value.get("downloadId").is_some());
        assert!(value.get("fileName").is_some());
        assert!(value.get("totalBytes").is_some());
        assert!(value.get("completedAt").is_some());
        assert!(value.get("durationSeconds").is_some());
        assert!(value.get("avgSpeed").is_some());
        assert!(value.get("destinationPath").is_some());
    }
}
