//! Fetch a single history entry by its primary key.

use crate::application::error::AppError;
use crate::application::query_bus::QueryBus;
use crate::domain::error::DomainError;
use crate::domain::model::views::HistoryEntry;

impl QueryBus {
    pub async fn handle_get_history_entry(
        &self,
        query: super::GetHistoryEntryQuery,
    ) -> Result<HistoryEntry, AppError> {
        match self.history_repo().find_by_id(query.id)? {
            Some(entry) => Ok(entry),
            None => Err(AppError::Domain(DomainError::NotFound(format!(
                "history entry {}",
                query.id
            )))),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::application::queries::GetHistoryEntryQuery;
    use crate::application::test_support::{InMemoryHistoryRepo, make_history_query_bus};
    use crate::domain::error::DomainError;
    use crate::domain::model::download::DownloadId;
    use crate::domain::ports::driven::HistoryRepository;

    #[tokio::test]
    async fn get_history_entry_returns_entry_when_found() {
        let repo = Arc::new(InMemoryHistoryRepo::new());
        repo.record(&HistoryEntry {
            id: 0,
            download_id: DownloadId(42),
            file_name: "f.bin".to_string(),
            url: "https://ex.com/f".to_string(),
            total_bytes: 42,
            completed_at: 1_000,
            duration_seconds: 1,
            avg_speed: 1,
            destination_path: "/tmp/f".to_string(),
        })
        .unwrap();
        let stored_id = repo.snapshot()[0].id;

        let bus = make_history_query_bus(repo);
        let result = bus
            .handle_get_history_entry(GetHistoryEntryQuery { id: stored_id })
            .await
            .unwrap();
        assert_eq!(result.download_id, DownloadId(42));
    }

    #[tokio::test]
    async fn get_history_entry_returns_not_found_when_missing() {
        let repo = Arc::new(InMemoryHistoryRepo::new());
        let bus = make_history_query_bus(repo);
        let err = bus
            .handle_get_history_entry(GetHistoryEntryQuery { id: 777 })
            .await
            .unwrap_err();
        assert!(matches!(err, AppError::Domain(DomainError::NotFound(_))));
    }
}
