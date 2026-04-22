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
    use crate::application::query_bus::QueryBus;
    use crate::application::test_support::InMemoryHistoryRepo;
    use crate::domain::error::DomainError;
    use crate::domain::model::archive::ArchiveFormat;
    use crate::domain::model::download::DownloadId;
    use crate::domain::model::plugin::PluginInfo;
    use crate::domain::model::views::{
        DownloadDetailView, DownloadFilter, DownloadView, SortOrder, StateCountMap, StatsView,
    };
    use crate::domain::ports::driven::{
        ArchiveExtractor, DownloadReadRepository, HistoryRepository, PluginReadRepository,
        StatsRepository,
    };
    use std::collections::HashMap;

    struct NoopExtractor;
    impl ArchiveExtractor for NoopExtractor {
        fn detect_format(&self, _: &std::path::Path) -> Result<Option<ArchiveFormat>, DomainError> {
            Ok(None)
        }
        fn can_extract(&self, _: &std::path::Path) -> Result<bool, DomainError> {
            Ok(false)
        }
        fn extract(
            &self,
            _: &std::path::Path,
            _: &std::path::Path,
            _: Option<&str>,
        ) -> Result<crate::domain::model::archive::ExtractSummary, DomainError> {
            Ok(crate::domain::model::archive::ExtractSummary {
                extracted_files: 0,
                extracted_bytes: 0,
                duration_ms: 0,
                warnings: vec![],
            })
        }
        fn list_contents(
            &self,
            _: &std::path::Path,
            _: Option<&str>,
        ) -> Result<Vec<crate::domain::model::archive::ArchiveEntry>, DomainError> {
            Ok(vec![])
        }
        fn detect_segments(
            &self,
            _: &std::path::Path,
        ) -> Result<Option<Vec<std::path::PathBuf>>, DomainError> {
            Ok(None)
        }
    }

    struct NoopDownloadRead;
    impl DownloadReadRepository for NoopDownloadRead {
        fn find_downloads(
            &self,
            _: Option<DownloadFilter>,
            _: Option<SortOrder>,
            _: Option<usize>,
            _: Option<usize>,
        ) -> Result<Vec<DownloadView>, DomainError> {
            Ok(vec![])
        }
        fn find_download_detail(
            &self,
            _: DownloadId,
        ) -> Result<Option<DownloadDetailView>, DomainError> {
            Ok(None)
        }
        fn count_by_state(&self) -> Result<StateCountMap, DomainError> {
            Ok(HashMap::new())
        }
    }

    struct NoopStats;
    impl StatsRepository for NoopStats {
        fn record_completed(&self, _: u64, _: u64) -> Result<(), DomainError> {
            Ok(())
        }
        fn get_stats(&self) -> Result<StatsView, DomainError> {
            Ok(StatsView {
                total_downloaded_bytes: 0,
                total_files: 0,
                avg_speed: 0,
                peak_speed: 0,
                success_rate: 0.0,
                daily_volumes: vec![],
                top_hosts: vec![],
            })
        }
    }

    struct NoopPluginRead;
    impl PluginReadRepository for NoopPluginRead {
        fn list_loaded(&self) -> Result<Vec<PluginInfo>, DomainError> {
            Ok(vec![])
        }
    }

    fn make_bus(history: Arc<dyn HistoryRepository>) -> QueryBus {
        QueryBus::new(
            Arc::new(NoopDownloadRead),
            history,
            Arc::new(NoopStats),
            Arc::new(NoopPluginRead),
            Arc::new(NoopExtractor),
        )
    }

    #[tokio::test]
    async fn get_history_entry_returns_entry_when_found() {
        let repo = Arc::new(InMemoryHistoryRepo::new());
        repo.record(&crate::domain::model::views::HistoryEntry {
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

        let bus = make_bus(repo);
        let result = bus
            .handle_get_history_entry(GetHistoryEntryQuery { id: stored_id })
            .await
            .unwrap();
        assert_eq!(result.download_id, DownloadId(42));
    }

    #[tokio::test]
    async fn get_history_entry_returns_not_found_when_missing() {
        let repo = Arc::new(InMemoryHistoryRepo::new());
        let bus = make_bus(repo);
        let err = bus
            .handle_get_history_entry(GetHistoryEntryQuery { id: 777 })
            .await
            .unwrap_err();
        assert!(matches!(err, AppError::Domain(DomainError::NotFound(_))));
    }
}
