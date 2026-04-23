use crate::application::error::AppError;
use crate::application::query_bus::QueryBus;
use crate::domain::model::views::StateCountMap;

impl QueryBus {
    pub async fn handle_count_by_state(
        &self,
        _query: super::CountDownloadsByStateQuery,
    ) -> Result<StateCountMap, AppError> {
        let counts = self.download_read_repo().count_by_state()?;
        Ok(counts)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use crate::application::queries::CountDownloadsByStateQuery;
    use crate::application::query_bus::QueryBus;
    use crate::domain::error::DomainError;
    use crate::domain::model::archive::ArchiveFormat;
    use crate::domain::model::download::{DownloadId, DownloadState};
    use crate::domain::model::plugin::PluginInfo;
    use crate::domain::model::views::{
        DownloadDetailView, DownloadFilter, DownloadView, HistoryEntry, HistoryFilter, HistorySort,
        ModuleStats, SortOrder, StateCountMap, StatsPeriod, StatsView,
    };
    use crate::domain::ports::driven::{
        ArchiveExtractor, DownloadReadRepository, HistoryRepository, PluginReadRepository,
        StatsRepository,
    };

    struct FakeArchiveExtractor;
    impl ArchiveExtractor for FakeArchiveExtractor {
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

    struct MockDownloadReadRepo {
        counts: StateCountMap,
    }

    impl DownloadReadRepository for MockDownloadReadRepo {
        fn find_downloads(
            &self,
            _filter: Option<DownloadFilter>,
            _sort: Option<SortOrder>,
            _limit: Option<usize>,
            _offset: Option<usize>,
        ) -> Result<Vec<DownloadView>, DomainError> {
            Ok(vec![])
        }

        fn find_download_detail(
            &self,
            _id: DownloadId,
        ) -> Result<Option<DownloadDetailView>, DomainError> {
            Ok(None)
        }

        fn count_by_state(&self) -> Result<StateCountMap, DomainError> {
            Ok(self.counts.clone())
        }
    }

    struct MockHistoryRepo;
    impl HistoryRepository for MockHistoryRepo {
        fn record(&self, _entry: &HistoryEntry) -> Result<(), DomainError> {
            Ok(())
        }
        fn find_recent(&self, _limit: usize) -> Result<Vec<HistoryEntry>, DomainError> {
            Ok(vec![])
        }
        fn find_by_download(&self, _id: DownloadId) -> Result<Vec<HistoryEntry>, DomainError> {
            Ok(vec![])
        }
        fn list(
            &self,
            _filter: Option<HistoryFilter>,
            _sort: Option<HistorySort>,
            _limit: Option<usize>,
            _offset: Option<usize>,
        ) -> Result<Vec<HistoryEntry>, DomainError> {
            Ok(vec![])
        }
        fn search(&self, _query: &str) -> Result<Vec<HistoryEntry>, DomainError> {
            Ok(vec![])
        }
        fn find_by_id(&self, _id: u64) -> Result<Option<HistoryEntry>, DomainError> {
            Ok(None)
        }
        fn delete_by_id(&self, _id: u64) -> Result<bool, DomainError> {
            Ok(false)
        }
        fn delete_all(&self) -> Result<u64, DomainError> {
            Ok(0)
        }
        fn delete_older_than(&self, _before: u64) -> Result<u64, DomainError> {
            Ok(0)
        }
    }

    struct MockStatsRepo;
    impl StatsRepository for MockStatsRepo {
        fn record_completed(&self, _bytes: u64, _avg_speed: u64) -> Result<(), DomainError> {
            Ok(())
        }
        fn get_stats(&self, _: StatsPeriod) -> Result<StatsView, DomainError> {
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
        fn top_modules(&self, _: u32) -> Result<Vec<ModuleStats>, DomainError> {
            Ok(vec![])
        }
    }

    struct MockPluginReadRepo;
    impl PluginReadRepository for MockPluginReadRepo {
        fn list_loaded(&self) -> Result<Vec<PluginInfo>, DomainError> {
            Ok(vec![])
        }
    }

    #[tokio::test]
    async fn test_count_by_state_returns_correct_counts() {
        let mut counts = HashMap::new();
        counts.insert(DownloadState::Downloading, 3);
        counts.insert(DownloadState::Completed, 10);
        counts.insert(DownloadState::Error, 1);

        let bus = QueryBus::new(
            Arc::new(MockDownloadReadRepo { counts }),
            Arc::new(MockHistoryRepo),
            Arc::new(MockStatsRepo),
            Arc::new(MockPluginReadRepo),
            Arc::new(FakeArchiveExtractor),
        );

        let result = bus
            .handle_count_by_state(CountDownloadsByStateQuery)
            .await
            .unwrap();
        assert_eq!(result.get(&DownloadState::Downloading), Some(&3));
        assert_eq!(result.get(&DownloadState::Completed), Some(&10));
        assert_eq!(result.get(&DownloadState::Error), Some(&1));
        assert_eq!(result.get(&DownloadState::Paused), None);
    }

    #[tokio::test]
    async fn test_count_by_state_empty() {
        let bus = QueryBus::new(
            Arc::new(MockDownloadReadRepo {
                counts: HashMap::new(),
            }),
            Arc::new(MockHistoryRepo),
            Arc::new(MockStatsRepo),
            Arc::new(MockPluginReadRepo),
            Arc::new(FakeArchiveExtractor),
        );

        let result = bus
            .handle_count_by_state(CountDownloadsByStateQuery)
            .await
            .unwrap();
        assert!(result.is_empty());
    }
}
