//! CQRS query bus — dispatches queries to their handlers.
//!
//! Holds references to driven ports needed by query handlers.
//! Actual handler implementations will be added in tasks 11-12.

use std::sync::Arc;

use crate::domain::ports::driven::{
    AccountRepository, ArchiveExtractor, DownloadReadRepository, HistoryRepository,
    PluginConfigStore, PluginLoader, PluginReadRepository, StatsRepository,
};

/// Central dispatcher for CQRS queries.
///
/// Each driven port is injected via the constructor as `Arc<dyn Trait>`.
/// Query handler `impl` blocks will be added in later tasks.
pub struct QueryBus {
    download_read_repo: Arc<dyn DownloadReadRepository>,
    history_repo: Arc<dyn HistoryRepository>,
    stats_repo: Arc<dyn StatsRepository>,
    plugin_read_repo: Arc<dyn PluginReadRepository>,
    archive_extractor: Arc<dyn ArchiveExtractor>,
    plugin_loader: Option<Arc<dyn PluginLoader>>,
    plugin_config_store: Option<Arc<dyn PluginConfigStore>>,
    account_repo: Option<Arc<dyn AccountRepository>>,
}

impl QueryBus {
    pub fn new(
        download_read_repo: Arc<dyn DownloadReadRepository>,
        history_repo: Arc<dyn HistoryRepository>,
        stats_repo: Arc<dyn StatsRepository>,
        plugin_read_repo: Arc<dyn PluginReadRepository>,
        archive_extractor: Arc<dyn ArchiveExtractor>,
    ) -> Self {
        Self {
            download_read_repo,
            history_repo,
            stats_repo,
            plugin_read_repo,
            archive_extractor,
            plugin_loader: None,
            plugin_config_store: None,
            account_repo: None,
        }
    }

    /// Builder-style setter for the plugin loader. Optional so test
    /// fixtures that never query plugin manifests don't have to provide
    /// one.
    pub fn with_plugin_loader(mut self, loader: Arc<dyn PluginLoader>) -> Self {
        self.plugin_loader = Some(loader);
        self
    }

    /// Builder-style setter for the plugin config persistence port.
    pub fn with_plugin_config_store(mut self, store: Arc<dyn PluginConfigStore>) -> Self {
        self.plugin_config_store = Some(store);
        self
    }

    /// Builder-style setter for the account repository. Optional so
    /// existing fixtures that never query accounts don't have to
    /// provide a mock.
    pub fn with_account_repo(mut self, repo: Arc<dyn AccountRepository>) -> Self {
        self.account_repo = Some(repo);
        self
    }

    pub fn account_repo(&self) -> Option<&dyn AccountRepository> {
        self.account_repo.as_deref()
    }

    pub fn download_read_repo(&self) -> &dyn DownloadReadRepository {
        self.download_read_repo.as_ref()
    }

    pub fn history_repo(&self) -> &dyn HistoryRepository {
        self.history_repo.as_ref()
    }

    pub fn stats_repo(&self) -> &dyn StatsRepository {
        self.stats_repo.as_ref()
    }

    pub fn plugin_read_repo(&self) -> &dyn PluginReadRepository {
        self.plugin_read_repo.as_ref()
    }

    pub fn plugin_loader(&self) -> Option<&dyn PluginLoader> {
        self.plugin_loader.as_deref()
    }

    pub fn plugin_config_store(&self) -> Option<&dyn PluginConfigStore> {
        self.plugin_config_store.as_deref()
    }

    pub(crate) fn archive_extractor_arc(&self) -> Arc<dyn ArchiveExtractor> {
        Arc::clone(&self.archive_extractor)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use super::QueryBus;
    use crate::domain::error::DomainError;
    use crate::domain::model::archive::ArchiveFormat;
    use crate::domain::model::download::DownloadId;
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

        fn delete_older_than(&self, _before_timestamp: u64) -> Result<u64, DomainError> {
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

    #[test]
    fn test_query_bus_new_compiles() {
        let _bus = QueryBus::new(
            Arc::new(MockDownloadReadRepo),
            Arc::new(MockHistoryRepo),
            Arc::new(MockStatsRepo),
            Arc::new(MockPluginReadRepo),
            Arc::new(FakeArchiveExtractor),
        );
    }

    #[test]
    fn test_query_bus_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<QueryBus>();
    }
}
