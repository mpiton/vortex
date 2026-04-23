//! Paginated history listing.

use crate::application::error::AppError;
use crate::application::query_bus::QueryBus;
use crate::domain::model::views::HistoryEntry;

impl QueryBus {
    pub async fn handle_list_history(
        &self,
        query: super::ListHistoryQuery,
    ) -> Result<Vec<HistoryEntry>, AppError> {
        let entries =
            self.history_repo()
                .list(query.filter, query.sort, query.limit, query.offset)?;
        Ok(entries)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::application::queries::ListHistoryQuery;
    use crate::application::test_support::{InMemoryHistoryRepo, make_history_query_bus};
    use crate::domain::model::download::DownloadId;
    use crate::domain::model::views::{HistoryEntry, HistoryFilter};
    use crate::domain::ports::driven::HistoryRepository;

    fn seed(repo: &InMemoryHistoryRepo) {
        for i in 1..=5u64 {
            repo.record(&HistoryEntry {
                id: 0,
                download_id: DownloadId(i),
                file_name: format!("f{i}.bin"),
                url: format!("https://ex.com/{i}"),
                total_bytes: 100,
                completed_at: i * 1_000,
                duration_seconds: 10,
                avg_speed: 10,
                destination_path: format!("/tmp/f{i}"),
            })
            .unwrap();
        }
    }

    #[tokio::test]
    async fn list_history_returns_all_sorted_desc_by_completed_at() {
        let repo = Arc::new(InMemoryHistoryRepo::new());
        seed(&repo);
        let bus = make_history_query_bus(repo);
        let result = bus
            .handle_list_history(ListHistoryQuery {
                filter: None,
                sort: None,
                limit: None,
                offset: None,
            })
            .await
            .unwrap();
        assert_eq!(result.len(), 5);
        assert_eq!(result[0].completed_at, 5_000);
        assert_eq!(result[4].completed_at, 1_000);
    }

    #[tokio::test]
    async fn list_history_paginates() {
        let repo = Arc::new(InMemoryHistoryRepo::new());
        seed(&repo);
        let bus = make_history_query_bus(repo);
        let page = bus
            .handle_list_history(ListHistoryQuery {
                filter: None,
                sort: None,
                limit: Some(2),
                offset: Some(2),
            })
            .await
            .unwrap();
        assert_eq!(page.len(), 2);
        assert_eq!(page[0].completed_at, 3_000);
    }

    #[tokio::test]
    async fn list_history_applies_date_filter() {
        let repo = Arc::new(InMemoryHistoryRepo::new());
        seed(&repo);
        let bus = make_history_query_bus(repo);
        let result = bus
            .handle_list_history(ListHistoryQuery {
                filter: Some(HistoryFilter {
                    date_from: Some(1_500),
                    date_to: Some(3_500),
                    hostname: None,
                }),
                sort: None,
                limit: None,
                offset: None,
            })
            .await
            .unwrap();
        assert_eq!(result.len(), 2);
        assert!(
            result
                .iter()
                .all(|e| e.completed_at >= 1_500 && e.completed_at <= 3_500)
        );
    }
}
