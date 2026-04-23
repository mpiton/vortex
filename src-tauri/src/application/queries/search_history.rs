//! Full-text history search handler.

use crate::application::error::AppError;
use crate::application::query_bus::QueryBus;
use crate::domain::model::views::HistoryEntry;

impl QueryBus {
    pub async fn handle_search_history(
        &self,
        query: super::SearchHistoryQuery,
    ) -> Result<Vec<HistoryEntry>, AppError> {
        let entries = self.history_repo().search(&query.query)?;
        Ok(entries)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::application::queries::SearchHistoryQuery;
    use crate::application::test_support::{InMemoryHistoryRepo, make_history_query_bus};
    use crate::domain::model::download::DownloadId;
    use crate::domain::model::views::HistoryEntry;
    use crate::domain::ports::driven::HistoryRepository;

    #[tokio::test]
    async fn search_history_matches_by_file_name() {
        let repo = Arc::new(InMemoryHistoryRepo::new());
        repo.record(&HistoryEntry {
            id: 0,
            download_id: DownloadId(1),
            file_name: "alpha-video.mp4".to_string(),
            url: "https://x.test/a".to_string(),
            total_bytes: 10,
            completed_at: 1_000,
            duration_seconds: 1,
            avg_speed: 10,
            destination_path: "/tmp/alpha-video.mp4".to_string(),
        })
        .unwrap();
        repo.record(&HistoryEntry {
            id: 0,
            download_id: DownloadId(2),
            file_name: "beta.zip".to_string(),
            url: "https://x.test/b".to_string(),
            total_bytes: 10,
            completed_at: 2_000,
            duration_seconds: 1,
            avg_speed: 10,
            destination_path: "/tmp/beta.zip".to_string(),
        })
        .unwrap();

        let bus = make_history_query_bus(repo);
        let result = bus
            .handle_search_history(SearchHistoryQuery {
                query: "alpha".to_string(),
            })
            .await
            .unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].file_name, "alpha-video.mp4");
    }
}
