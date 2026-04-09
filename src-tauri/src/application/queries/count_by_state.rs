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
    use crate::domain::model::download::{DownloadId, DownloadState};
    use crate::domain::model::plugin::PluginInfo;
    use crate::domain::model::views::{
        DownloadDetailView, DownloadFilter, DownloadView, HistoryEntry, SortOrder, StateCountMap,
        StatsView,
    };
    use crate::domain::ports::driven::{
        DownloadReadRepository, HistoryRepository, PluginReadRepository, StatsRepository,
    };

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
        fn delete_older_than(&self, _before: u64) -> Result<u64, DomainError> {
            Ok(0)
        }
    }

    struct MockStatsRepo;
    impl StatsRepository for MockStatsRepo {
        fn record_completed(&self, _bytes: u64, _avg_speed: u64) -> Result<(), DomainError> {
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
        );

        let result = bus
            .handle_count_by_state(CountDownloadsByStateQuery)
            .await
            .unwrap();
        assert!(result.is_empty());
    }
}
