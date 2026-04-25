//! Handler for `GetPluginConfigQuery`.
//!
//! Returns the schema declared by the plugin's manifest joined with the
//! current persisted values (or the manifest defaults when nothing has
//! been persisted yet). The frontend uses the schema to render typed
//! form fields and the values to populate them.

use std::collections::HashMap;

use crate::application::error::AppError;
use crate::application::query_bus::QueryBus;
use crate::application::read_models::plugin_config_view::PluginConfigView;

impl QueryBus {
    pub async fn handle_get_plugin_config(
        &self,
        query: super::GetPluginConfigQuery,
    ) -> Result<PluginConfigView, AppError> {
        let loader = self
            .plugin_loader()
            .ok_or_else(|| AppError::Plugin("plugin loader not configured".into()))?;
        let manifest = loader.get_manifest(&query.plugin_name)?.ok_or_else(|| {
            AppError::Plugin(format!("plugin '{}' is not loaded", query.plugin_name))
        })?;

        let store = self
            .plugin_config_store()
            .ok_or_else(|| AppError::Plugin("plugin config store not configured".into()))?;
        let mut values = store
            .get_values(&query.plugin_name)
            .map_err(AppError::Domain)?;

        // Drop persisted values that no longer match the current schema
        // (e.g. after a plugin update tightens a regex, removes an enum
        // option, or renames a key) so the UI never surfaces a value
        // the backend would reject on save.
        let schema = manifest.config_schema();
        values.retain(|key, value| schema.validate(key, value).is_ok());

        // Fill missing keys with their manifest defaults so the UI never
        // renders an empty input for a field that has a declared default.
        for (key, field) in schema.fields() {
            if !values.contains_key(key)
                && let Some(default) = field.default_value()
            {
                values.insert(key.clone(), default.to_string());
            }
        }

        Ok(PluginConfigView::new(
            manifest.config_schema(),
            values_to_view(values),
        ))
    }
}

fn values_to_view(values: HashMap<String, String>) -> HashMap<String, String> {
    values
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use crate::application::queries::GetPluginConfigQuery;
    use crate::application::query_bus::QueryBus;
    use crate::domain::error::DomainError;
    use crate::domain::model::download::DownloadId;
    use crate::domain::model::plugin::{
        ConfigField, ConfigFieldType, PluginCategory, PluginConfigSchema, PluginInfo,
        PluginManifest,
    };
    use crate::domain::model::views::{
        DownloadDetailView, DownloadFilter, DownloadView, HistoryEntry, HistoryFilter, HistorySort,
        ModuleStats, SortOrder, StateCountMap, StatsPeriod, StatsView,
    };
    use crate::domain::ports::driven::{
        ArchiveExtractor, DownloadReadRepository, HistoryRepository, PluginConfigStore,
        PluginLoader, PluginReadRepository, StatsRepository,
    };

    struct FakeArchiveExtractor;
    impl ArchiveExtractor for FakeArchiveExtractor {
        fn detect_format(
            &self,
            _: &std::path::Path,
        ) -> Result<Option<crate::domain::model::archive::ArchiveFormat>, DomainError> {
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

    struct MockReadRepo;
    impl DownloadReadRepository for MockReadRepo {
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

    struct MockHistoryRepo;
    impl HistoryRepository for MockHistoryRepo {
        fn record(&self, _: &HistoryEntry) -> Result<(), DomainError> {
            Ok(())
        }
        fn find_recent(&self, _: usize) -> Result<Vec<HistoryEntry>, DomainError> {
            Ok(vec![])
        }
        fn find_by_download(&self, _: DownloadId) -> Result<Vec<HistoryEntry>, DomainError> {
            Ok(vec![])
        }
        fn list(
            &self,
            _: Option<HistoryFilter>,
            _: Option<HistorySort>,
            _: Option<usize>,
            _: Option<usize>,
        ) -> Result<Vec<HistoryEntry>, DomainError> {
            Ok(vec![])
        }
        fn search(&self, _: &str) -> Result<Vec<HistoryEntry>, DomainError> {
            Ok(vec![])
        }
        fn find_by_id(&self, _: u64) -> Result<Option<HistoryEntry>, DomainError> {
            Ok(None)
        }
        fn delete_by_id(&self, _: u64) -> Result<bool, DomainError> {
            Ok(false)
        }
        fn delete_all(&self) -> Result<u64, DomainError> {
            Ok(0)
        }
        fn delete_older_than(&self, _: u64) -> Result<u64, DomainError> {
            Ok(0)
        }
    }

    struct MockStatsRepo;
    impl StatsRepository for MockStatsRepo {
        fn record_completed(&self, _: u64, _: u64) -> Result<(), DomainError> {
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

    struct EmptyPluginRepo;
    impl PluginReadRepository for EmptyPluginRepo {
        fn list_loaded(&self) -> Result<Vec<PluginInfo>, DomainError> {
            Ok(vec![])
        }
    }

    struct StubPluginLoader {
        manifest: Option<PluginManifest>,
    }
    impl PluginLoader for StubPluginLoader {
        fn load(&self, _: &PluginManifest) -> Result<(), DomainError> {
            Ok(())
        }
        fn unload(&self, _: &str) -> Result<(), DomainError> {
            Ok(())
        }
        fn resolve_url(&self, _: &str) -> Result<Option<PluginInfo>, DomainError> {
            Ok(None)
        }
        fn list_loaded(&self) -> Result<Vec<PluginInfo>, DomainError> {
            Ok(vec![])
        }
        fn set_enabled(&self, _: &str, _: bool) -> Result<(), DomainError> {
            Ok(())
        }
        fn get_manifest(&self, _: &str) -> Result<Option<PluginManifest>, DomainError> {
            Ok(self.manifest.clone())
        }
    }

    struct InMemoryStore {
        values: HashMap<(String, String), String>,
    }
    impl InMemoryStore {
        fn new() -> Self {
            Self {
                values: HashMap::new(),
            }
        }
    }
    impl PluginConfigStore for InMemoryStore {
        fn get_values(&self, plugin_name: &str) -> Result<HashMap<String, String>, DomainError> {
            Ok(self
                .values
                .iter()
                .filter(|((p, _), _)| p == plugin_name)
                .map(|((_, k), v)| (k.clone(), v.clone()))
                .collect())
        }
        fn set_value(&self, _: &str, _: &str, _: &str) -> Result<(), DomainError> {
            Ok(())
        }
        fn list_all(&self) -> Result<HashMap<String, HashMap<String, String>>, DomainError> {
            Ok(HashMap::new())
        }
        fn delete_all(&self, _: &str) -> Result<(), DomainError> {
            Ok(())
        }
    }

    fn make_manifest() -> PluginManifest {
        let info = PluginInfo::new(
            "yt".to_string(),
            "1.0.0".to_string(),
            "yt".to_string(),
            "x".to_string(),
            PluginCategory::Crawler,
        );
        let mut schema = PluginConfigSchema::new();
        schema.insert(
            "default_quality",
            ConfigField::new(ConfigFieldType::Enum)
                .with_options(vec!["360p".into(), "720p".into(), "1080p".into()])
                .with_default("720p"),
        );
        schema.insert(
            "audio_only",
            ConfigField::new(ConfigFieldType::Boolean).with_default("false"),
        );
        PluginManifest::new(info).with_config_schema(schema)
    }

    fn make_query_bus(loader: StubPluginLoader, store: InMemoryStore) -> QueryBus {
        QueryBus::new(
            Arc::new(MockReadRepo),
            Arc::new(MockHistoryRepo),
            Arc::new(MockStatsRepo),
            Arc::new(EmptyPluginRepo),
            Arc::new(FakeArchiveExtractor),
        )
        .with_plugin_loader(Arc::new(loader))
        .with_plugin_config_store(Arc::new(store))
    }

    #[tokio::test]
    async fn test_get_plugin_config_returns_schema_and_defaults() {
        let bus = make_query_bus(
            StubPluginLoader {
                manifest: Some(make_manifest()),
            },
            InMemoryStore::new(),
        );
        let view = bus
            .handle_get_plugin_config(GetPluginConfigQuery {
                plugin_name: "yt".into(),
            })
            .await
            .unwrap();
        assert_eq!(view.fields.len(), 2);
        assert_eq!(
            view.values.get("default_quality"),
            Some(&"720p".to_string())
        );
        assert_eq!(view.values.get("audio_only"), Some(&"false".to_string()));
    }

    #[tokio::test]
    async fn test_get_plugin_config_persisted_overrides_default() {
        let mut store = InMemoryStore::new();
        store
            .values
            .insert(("yt".into(), "default_quality".into()), "1080p".into());
        let bus = make_query_bus(
            StubPluginLoader {
                manifest: Some(make_manifest()),
            },
            store,
        );
        let view = bus
            .handle_get_plugin_config(GetPluginConfigQuery {
                plugin_name: "yt".into(),
            })
            .await
            .unwrap();
        assert_eq!(
            view.values.get("default_quality"),
            Some(&"1080p".to_string())
        );
    }

    #[tokio::test]
    async fn test_get_plugin_config_unknown_plugin_returns_err() {
        let bus = make_query_bus(StubPluginLoader { manifest: None }, InMemoryStore::new());
        let err = bus
            .handle_get_plugin_config(GetPluginConfigQuery {
                plugin_name: "ghost".into(),
            })
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            crate::application::error::AppError::Plugin(_)
        ));
    }
}
