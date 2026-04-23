use crate::application::error::AppError;
use crate::application::query_bus::QueryBus;
use crate::domain::model::views::DownloadDetailView;

impl QueryBus {
    pub async fn handle_get_download_detail(
        &self,
        query: super::GetDownloadDetailQuery,
    ) -> Result<DownloadDetailView, AppError> {
        self.download_read_repo()
            .find_download_detail(query.id)?
            .ok_or_else(|| AppError::NotFound(format!("Download {} not found", query.id.0)))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use crate::application::queries::GetDownloadDetailQuery;
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
        detail: Option<DownloadDetailView>,
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
            Ok(self.detail.clone())
        }

        fn count_by_state(&self) -> Result<StateCountMap, DomainError> {
            Ok(HashMap::new())
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

    fn make_detail() -> DownloadDetailView {
        DownloadDetailView {
            id: DownloadId(42),
            file_name: "test.zip".to_string(),
            url: "http://example.com/test.zip".to_string(),
            source_hostname: "example.com".to_string(),
            state: DownloadState::Downloading,
            progress_percent: 50.0,
            speed_bytes_per_sec: 1024,
            downloaded_bytes: 512,
            total_bytes: Some(1024),
            eta_seconds: Some(10),
            segments: vec![],
            checksum_expected: None,
            destination_path: "/tmp/test.zip".to_string(),
            module_name: None,
            account_name: None,
            resume_supported: true,
            retry_count: 0,
            max_retries: 5,
            created_at: 0,
            updated_at: 0,
        }
    }

    #[tokio::test]
    async fn test_get_download_detail_returns_detail() {
        let bus = QueryBus::new(
            Arc::new(MockDownloadReadRepo {
                detail: Some(make_detail()),
            }),
            Arc::new(MockHistoryRepo),
            Arc::new(MockStatsRepo),
            Arc::new(MockPluginReadRepo),
            Arc::new(FakeArchiveExtractor),
        );
        let result = bus
            .handle_get_download_detail(GetDownloadDetailQuery { id: DownloadId(42) })
            .await
            .unwrap();
        assert_eq!(result.id, DownloadId(42));
        assert_eq!(result.file_name, "test.zip");
    }

    #[tokio::test]
    async fn test_get_download_detail_not_found() {
        let bus = QueryBus::new(
            Arc::new(MockDownloadReadRepo { detail: None }),
            Arc::new(MockHistoryRepo),
            Arc::new(MockStatsRepo),
            Arc::new(MockPluginReadRepo),
            Arc::new(FakeArchiveExtractor),
        );
        let result = bus
            .handle_get_download_detail(GetDownloadDetailQuery {
                id: DownloadId(999),
            })
            .await;
        assert!(matches!(
            result,
            Err(crate::application::error::AppError::NotFound(_))
        ));
    }
}
