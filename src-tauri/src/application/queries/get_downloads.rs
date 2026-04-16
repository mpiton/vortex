use crate::application::error::AppError;
use crate::application::query_bus::QueryBus;
use crate::domain::model::views::DownloadView;

impl QueryBus {
    pub async fn handle_get_downloads(
        &self,
        query: super::GetDownloadsQuery,
    ) -> Result<Vec<DownloadView>, AppError> {
        let views = self.download_read_repo().find_downloads(
            query.filter,
            query.sort,
            query.limit,
            query.offset,
        )?;
        Ok(views)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use crate::application::queries::GetDownloadsQuery;
    use crate::application::query_bus::QueryBus;
    use crate::domain::error::DomainError;
    use crate::domain::model::archive::ArchiveFormat;
    use crate::domain::model::download::{DownloadId, DownloadState};
    use crate::domain::model::plugin::PluginInfo;
    use crate::domain::model::views::{
        DownloadDetailView, DownloadFilter, DownloadView, HistoryEntry, SortOrder, StateCountMap,
        StatsView,
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
        views: Vec<DownloadView>,
    }

    impl MockDownloadReadRepo {
        fn with_views(views: Vec<DownloadView>) -> Self {
            Self { views }
        }
    }

    impl DownloadReadRepository for MockDownloadReadRepo {
        fn find_downloads(
            &self,
            filter: Option<DownloadFilter>,
            _sort: Option<SortOrder>,
            limit: Option<usize>,
            _offset: Option<usize>,
        ) -> Result<Vec<DownloadView>, DomainError> {
            let mut result = self.views.clone();
            if let Some(f) = filter {
                if let Some(state) = f.state {
                    result.retain(|v| v.state == state);
                }
                if let Some(ref search) = f.search {
                    let s = search.to_lowercase();
                    result.retain(|v| v.file_name.to_lowercase().contains(&s));
                }
            }
            if let Some(n) = limit {
                result.truncate(n);
            }
            Ok(result)
        }

        fn find_download_detail(
            &self,
            _id: DownloadId,
        ) -> Result<Option<DownloadDetailView>, DomainError> {
            Ok(None)
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

    fn make_query_bus(repo: MockDownloadReadRepo) -> QueryBus {
        QueryBus::new(
            Arc::new(repo),
            Arc::new(MockHistoryRepo),
            Arc::new(MockStatsRepo),
            Arc::new(MockPluginReadRepo),
            Arc::new(FakeArchiveExtractor),
        )
    }

    fn make_view(id: u64, name: &str, state: DownloadState) -> DownloadView {
        DownloadView {
            id: DownloadId(id),
            file_name: name.to_string(),
            url: format!("http://example.com/{name}"),
            state,
            progress_percent: 0.0,
            speed_bytes_per_sec: 0,
            downloaded_bytes: 0,
            total_bytes: None,
            eta_seconds: None,
            segments_active: 0,
            segments_total: 1,
            module_name: None,
            account_name: None,
            error_message: None,
            created_at: 0,
        }
    }

    #[tokio::test]
    async fn test_get_downloads_returns_all_when_no_filter() {
        let views = vec![
            make_view(1, "a.zip", DownloadState::Downloading),
            make_view(2, "b.zip", DownloadState::Completed),
        ];
        let bus = make_query_bus(MockDownloadReadRepo::with_views(views));
        let result = bus
            .handle_get_downloads(GetDownloadsQuery {
                filter: None,
                sort: None,
                limit: None,
                offset: None,
            })
            .await
            .unwrap();
        assert_eq!(result.len(), 2);
    }

    #[tokio::test]
    async fn test_get_downloads_filters_by_state() {
        let views = vec![
            make_view(1, "a.zip", DownloadState::Downloading),
            make_view(2, "b.zip", DownloadState::Completed),
            make_view(3, "c.zip", DownloadState::Downloading),
        ];
        let bus = make_query_bus(MockDownloadReadRepo::with_views(views));
        let result = bus
            .handle_get_downloads(GetDownloadsQuery {
                filter: Some(DownloadFilter {
                    state: Some(DownloadState::Downloading),
                    search: None,
                    host: None,
                }),
                sort: None,
                limit: None,
                offset: None,
            })
            .await
            .unwrap();
        assert_eq!(result.len(), 2);
        assert!(result.iter().all(|v| v.state == DownloadState::Downloading));
    }

    #[tokio::test]
    async fn test_get_downloads_search_by_filename() {
        let views = vec![
            make_view(1, "movie.mp4", DownloadState::Completed),
            make_view(2, "photo.jpg", DownloadState::Completed),
        ];
        let bus = make_query_bus(MockDownloadReadRepo::with_views(views));
        let result = bus
            .handle_get_downloads(GetDownloadsQuery {
                filter: Some(DownloadFilter {
                    state: None,
                    search: Some("movie".to_string()),
                    host: None,
                }),
                sort: None,
                limit: None,
                offset: None,
            })
            .await
            .unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].file_name, "movie.mp4");
    }

    #[tokio::test]
    async fn test_get_downloads_empty_returns_empty_vec() {
        let bus = make_query_bus(MockDownloadReadRepo::with_views(vec![]));
        let result = bus
            .handle_get_downloads(GetDownloadsQuery {
                filter: None,
                sort: None,
                limit: None,
                offset: None,
            })
            .await
            .unwrap();
        assert!(result.is_empty());
    }
}
