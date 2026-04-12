//! Handler for `ListPluginsQuery`.
//!
//! Returns all loaded plugins as `PluginViewDto` read models.

use crate::application::error::AppError;
use crate::application::query_bus::QueryBus;
use crate::application::read_models::plugin_view::PluginViewDto;

impl QueryBus {
    pub async fn handle_list_plugins(
        &self,
        _query: super::ListPluginsQuery,
    ) -> Result<Vec<PluginViewDto>, AppError> {
        let plugins = self.plugin_read_repo().list_loaded()?;
        Ok(plugins.into_iter().map(PluginViewDto::from).collect())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::application::queries::ListPluginsQuery;
    use crate::application::query_bus::QueryBus;
    use crate::domain::error::DomainError;
    use crate::domain::model::archive::ArchiveFormat;
    use crate::domain::model::download::DownloadId;
    use crate::domain::model::plugin::{PluginCategory, PluginInfo};
    use crate::domain::model::views::{
        DownloadDetailView, DownloadFilter, DownloadView, HistoryEntry, SortOrder, StateCountMap,
        StatsView,
    };
    use crate::domain::ports::driven::{
        ArchiveExtractor, DownloadReadRepository, HistoryRepository, PluginReadRepository,
        StatsRepository,
    };
    use std::collections::HashMap;

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

    struct MockDownloadReadRepo;
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

    struct MockPluginReadRepo {
        plugins: Vec<PluginInfo>,
    }

    impl MockPluginReadRepo {
        fn empty() -> Self {
            Self { plugins: vec![] }
        }

        fn with_plugins(plugins: Vec<PluginInfo>) -> Self {
            Self { plugins }
        }
    }

    impl PluginReadRepository for MockPluginReadRepo {
        fn list_loaded(&self) -> Result<Vec<PluginInfo>, DomainError> {
            Ok(self.plugins.clone())
        }
    }

    fn make_query_bus(plugin_repo: MockPluginReadRepo) -> QueryBus {
        QueryBus::new(
            Arc::new(MockDownloadReadRepo),
            Arc::new(MockHistoryRepo),
            Arc::new(MockStatsRepo),
            Arc::new(plugin_repo),
            Arc::new(FakeArchiveExtractor),
        )
    }

    fn make_plugin(name: &str, version: &str) -> PluginInfo {
        PluginInfo::new(
            name.to_string(),
            version.to_string(),
            "A plugin".to_string(),
            "author".to_string(),
            PluginCategory::Hoster,
        )
    }

    #[tokio::test]
    async fn test_list_plugins_empty_returns_empty_vec() {
        let bus = make_query_bus(MockPluginReadRepo::empty());
        let result = bus.handle_list_plugins(ListPluginsQuery).await.unwrap();
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn test_list_plugins_maps_plugin_info_to_dto() {
        let plugins = vec![
            make_plugin("plugin-a", "1.0.0"),
            make_plugin("plugin-b", "2.0.0"),
        ];
        let bus = make_query_bus(MockPluginReadRepo::with_plugins(plugins));
        let result = bus.handle_list_plugins(ListPluginsQuery).await.unwrap();
        assert_eq!(result.len(), 2);
        let names: Vec<&str> = result.iter().map(|p| p.name.as_str()).collect();
        assert!(names.contains(&"plugin-a"));
        assert!(names.contains(&"plugin-b"));
    }

    #[tokio::test]
    async fn test_list_plugins_dto_fields_match_plugin_info() {
        let bus = make_query_bus(MockPluginReadRepo::with_plugins(vec![make_plugin(
            "my-plugin",
            "3.1.0",
        )]));
        let result = bus.handle_list_plugins(ListPluginsQuery).await.unwrap();
        assert_eq!(result.len(), 1);
        let dto = &result[0];
        assert_eq!(dto.name, "my-plugin");
        assert_eq!(dto.version, "3.1.0");
        assert_eq!(dto.category, "Hoster");
        assert!(dto.enabled);
    }
}
