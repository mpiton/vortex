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
    async fn search_history_matches_by_file_name() {
        let repo = Arc::new(InMemoryHistoryRepo::new());
        repo.record(&crate::domain::model::views::HistoryEntry {
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
        repo.record(&crate::domain::model::views::HistoryEntry {
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

        let bus = make_bus(repo);
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
