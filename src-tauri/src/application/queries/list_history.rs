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
    use crate::application::query_bus::QueryBus;
    use crate::application::test_support::InMemoryHistoryRepo;
    use crate::domain::error::DomainError;
    use crate::domain::model::archive::ArchiveFormat;
    use crate::domain::model::download::{DownloadId, DownloadState};
    use crate::domain::model::plugin::PluginInfo;
    use crate::domain::model::views::{
        DownloadDetailView, DownloadFilter, DownloadView, HistoryFilter, SortOrder, StateCountMap,
        StatsView,
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

    fn seed(repo: &InMemoryHistoryRepo) {
        for i in 1..=5u64 {
            repo.record(&crate::domain::model::views::HistoryEntry {
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
        // Avoid unused DownloadState warning in older toolchains
        let _ = DownloadState::Completed;
    }

    #[tokio::test]
    async fn list_history_returns_all_sorted_desc_by_completed_at() {
        let repo = Arc::new(InMemoryHistoryRepo::new());
        seed(&repo);
        let bus = make_bus(repo);
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
        let bus = make_bus(repo);
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
        let bus = make_bus(repo);
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
