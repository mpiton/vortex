//! Bridges `SettingsUpdated` events to live engine knobs.
//!
//! The download engine caches `dynamic_split_*` parameters in atomic
//! fields so settings changes from the UI take effect on already-running
//! and newly-started downloads without restart. Mirrors the pattern used
//! by [`super::queue_config_bridge`] for `max_concurrent_downloads`.

use std::sync::Arc;

use crate::adapters::driven::network::SegmentedDownloadEngine;
use crate::domain::event::DomainEvent;
use crate::domain::ports::driven::{ConfigStore, EventBus};

/// Subscribe the engine to configuration updates.
///
/// On every [`DomainEvent::SettingsUpdated`], reads the current
/// `dynamic_split_*` values and forwards them to
/// [`SegmentedDownloadEngine::set_dynamic_split`]. Read errors are
/// logged and swallowed so one bad read does not poison the
/// subscription.
pub fn subscribe_engine_to_config(
    event_bus: &dyn EventBus,
    config_store: Arc<dyn ConfigStore>,
    engine: Arc<SegmentedDownloadEngine>,
) {
    event_bus.subscribe(Box::new(move |event| {
        if !matches!(event, DomainEvent::SettingsUpdated) {
            return;
        }
        match config_store.get_config() {
            Ok(config) => {
                engine.set_dynamic_split(
                    config.dynamic_split_enabled,
                    config.dynamic_split_min_remaining_mb,
                );
            }
            Err(err) => {
                tracing::error!(%err, "engine_config_bridge: failed to read config");
            }
        }
    }));
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::Mutex;

    use super::*;
    use crate::domain::error::DomainError;
    use crate::domain::model::config::{AppConfig, ConfigPatch, apply_patch};
    use crate::domain::model::download::DownloadId;
    use crate::domain::model::meta::DownloadMeta;
    use crate::domain::ports::driven::FileStorage;

    struct StubConfigStore {
        config: Mutex<AppConfig>,
    }

    impl ConfigStore for StubConfigStore {
        fn get_config(&self) -> Result<AppConfig, DomainError> {
            Ok(self.config.lock().unwrap().clone())
        }

        fn update_config(&self, patch: ConfigPatch) -> Result<AppConfig, DomainError> {
            let mut cfg = self.config.lock().unwrap();
            apply_patch(&mut cfg, &patch);
            Ok(cfg.clone())
        }
    }

    type Handler = Box<dyn Fn(&DomainEvent) + Send + Sync + 'static>;

    struct SyncEventBus {
        handlers: Mutex<Vec<Handler>>,
    }

    impl SyncEventBus {
        fn new() -> Self {
            Self {
                handlers: Mutex::new(Vec::new()),
            }
        }
    }

    impl EventBus for SyncEventBus {
        fn publish(&self, event: DomainEvent) {
            let handlers = self.handlers.lock().unwrap();
            for handler in handlers.iter() {
                handler(&event);
            }
        }

        fn subscribe(&self, handler: Handler) {
            self.handlers.lock().unwrap().push(handler);
        }
    }

    struct NoopStorage;
    impl FileStorage for NoopStorage {
        fn create_file(&self, _path: &Path, _size: u64) -> Result<(), DomainError> {
            Ok(())
        }
        fn write_segment(
            &self,
            _path: &Path,
            _offset: u64,
            _data: &[u8],
        ) -> Result<(), DomainError> {
            Ok(())
        }
        fn read_meta(&self, _path: &Path) -> Result<Option<DownloadMeta>, DomainError> {
            Ok(None)
        }
        fn write_meta(&self, _path: &Path, _meta: &DownloadMeta) -> Result<(), DomainError> {
            Ok(())
        }
        fn delete_meta(&self, _path: &Path) -> Result<(), DomainError> {
            Ok(())
        }
    }

    fn make_engine() -> Arc<SegmentedDownloadEngine> {
        Arc::new(SegmentedDownloadEngine::new(
            reqwest::Client::new(),
            Arc::new(NoopStorage),
            Arc::new(SyncEventBus::new()),
            4,
        ))
    }

    const MIB: u64 = 1024 * 1024;

    #[tokio::test]
    async fn test_settings_updated_propagates_dynamic_split_changes() {
        let cfg = AppConfig {
            dynamic_split_enabled: false,
            dynamic_split_min_remaining_mb: 16,
            ..AppConfig::default()
        };
        let config_store: Arc<dyn ConfigStore> = Arc::new(StubConfigStore {
            config: Mutex::new(cfg),
        });
        let bus = SyncEventBus::new();
        let engine = make_engine();

        // Seed the engine with values that differ from the persisted config so
        // a successful bridge call has something to flip.
        engine.set_dynamic_split(true, 4);
        assert_eq!(engine.dynamic_split_state(), (true, 4 * MIB));

        subscribe_engine_to_config(&bus, Arc::clone(&config_store), Arc::clone(&engine));

        // Publishing must propagate the persisted (false, 16 MiB) into the engine.
        bus.publish(DomainEvent::SettingsUpdated);
        assert_eq!(engine.dynamic_split_state(), (false, 16 * MIB));

        // A subsequent patch + publish must flip both knobs again.
        config_store
            .update_config(ConfigPatch {
                dynamic_split_enabled: Some(true),
                dynamic_split_min_remaining_mb: Some(8),
                ..Default::default()
            })
            .unwrap();
        bus.publish(DomainEvent::SettingsUpdated);
        assert_eq!(engine.dynamic_split_state(), (true, 8 * MIB));
    }

    #[tokio::test]
    async fn test_non_settings_events_are_ignored() {
        // Persisted config differs from the engine state so a stray bridge
        // call would be observable.
        let cfg = AppConfig {
            dynamic_split_enabled: false,
            dynamic_split_min_remaining_mb: 32,
            ..AppConfig::default()
        };
        let config_store: Arc<dyn ConfigStore> = Arc::new(StubConfigStore {
            config: Mutex::new(cfg),
        });
        let bus = SyncEventBus::new();
        let engine = make_engine();
        engine.set_dynamic_split(true, 4);
        let before = engine.dynamic_split_state();
        assert_eq!(before, (true, 4 * MIB));

        subscribe_engine_to_config(&bus, Arc::clone(&config_store), Arc::clone(&engine));

        // Non-Settings events must NOT touch the engine.
        bus.publish(DomainEvent::DownloadStarted { id: DownloadId(1) });
        assert_eq!(engine.dynamic_split_state(), before);
    }
}
