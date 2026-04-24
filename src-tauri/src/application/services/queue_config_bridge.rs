//! Bridges `SettingsUpdated` events to [`QueueManager::set_max_concurrent`].
//!
//! Reads `max_concurrent_downloads` from [`ConfigStore`] on every
//! `SettingsUpdated` event and propagates the new limit to the queue
//! manager so the running scheduler reflects UI changes without restart.

use std::sync::Arc;

use crate::application::services::QueueManager;
use crate::domain::event::DomainEvent;
use crate::domain::model::config::normalize_max_concurrent;
use crate::domain::ports::driven::{ConfigStore, EventBus};

/// Subscribe the queue manager to configuration updates.
///
/// On every [`DomainEvent::SettingsUpdated`], reads the current
/// `max_concurrent_downloads` value and forwards it to
/// [`QueueManager::set_max_concurrent`]. Read errors are logged and
/// swallowed so one bad read does not poison the subscription.
pub fn subscribe_queue_to_config(
    event_bus: &dyn EventBus,
    config_store: Arc<dyn ConfigStore>,
    queue_manager: Arc<QueueManager>,
) {
    event_bus.subscribe(Box::new(move |event| {
        if !matches!(event, DomainEvent::SettingsUpdated) {
            return;
        }
        match config_store.get_config() {
            Ok(config) => {
                queue_manager.set_max_concurrent(normalize_max_concurrent(
                    config.max_concurrent_downloads,
                ));
            }
            Err(err) => {
                tracing::error!(%err, "queue_config_bridge: failed to read config");
            }
        }
    }));
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;
    use crate::domain::error::DomainError;
    use crate::domain::model::config::{AppConfig, ConfigPatch, apply_patch};
    use crate::domain::model::download::{Download, DownloadId, DownloadState};

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

    struct NoopRepo;
    impl crate::domain::ports::driven::DownloadRepository for NoopRepo {
        fn find_by_id(&self, _id: DownloadId) -> Result<Option<Download>, DomainError> {
            Ok(None)
        }
        fn save(&self, _d: &Download) -> Result<(), DomainError> {
            Ok(())
        }
        fn delete(&self, _id: DownloadId) -> Result<(), DomainError> {
            Ok(())
        }
        fn find_by_state(&self, _state: DownloadState) -> Result<Vec<Download>, DomainError> {
            Ok(Vec::new())
        }
    }

    struct NoopEngine;
    impl crate::domain::ports::driven::DownloadEngine for NoopEngine {
        fn start(&self, _d: &Download) -> Result<(), DomainError> {
            Ok(())
        }
        fn pause(&self, _id: DownloadId) -> Result<(), DomainError> {
            Ok(())
        }
        fn resume(&self, _id: DownloadId) -> Result<(), DomainError> {
            Ok(())
        }
        fn cancel(&self, _id: DownloadId) -> Result<(), DomainError> {
            Ok(())
        }
    }

    fn make_manager(initial_max: usize) -> Arc<QueueManager> {
        Arc::new(QueueManager::new(
            Arc::new(NoopRepo),
            Arc::new(NoopEngine),
            Arc::new(SyncEventBus::new()),
            initial_max,
        ))
    }

    #[tokio::test]
    async fn test_settings_updated_propagates_new_max_concurrent() {
        let cfg = AppConfig {
            max_concurrent_downloads: 4,
            ..AppConfig::default()
        };
        let config_store: Arc<dyn ConfigStore> = Arc::new(StubConfigStore {
            config: Mutex::new(cfg),
        });
        let bus = SyncEventBus::new();
        let qm = make_manager(4);

        subscribe_queue_to_config(&bus, Arc::clone(&config_store), Arc::clone(&qm));

        // Simulate user raising the limit via settings.
        let patch = ConfigPatch {
            max_concurrent_downloads: Some(10),
            ..Default::default()
        };
        config_store.update_config(patch).unwrap();
        bus.publish(DomainEvent::SettingsUpdated);

        // Set is synchronous (AtomicUsize store happens before the spawn).
        assert_eq!(qm.max_concurrent(), 10);
    }

    #[tokio::test]
    async fn test_non_settings_events_are_ignored() {
        let cfg = AppConfig::default();
        let config_store: Arc<dyn ConfigStore> = Arc::new(StubConfigStore {
            config: Mutex::new(cfg),
        });
        let bus = SyncEventBus::new();
        let qm = make_manager(4);

        subscribe_queue_to_config(&bus, Arc::clone(&config_store), Arc::clone(&qm));

        bus.publish(DomainEvent::DownloadStarted { id: DownloadId(1) });

        assert_eq!(qm.max_concurrent(), 4);
    }

    #[tokio::test]
    async fn test_settings_updated_reads_config_each_time() {
        let cfg = AppConfig {
            max_concurrent_downloads: 2,
            ..AppConfig::default()
        };
        let config_store: Arc<dyn ConfigStore> = Arc::new(StubConfigStore {
            config: Mutex::new(cfg),
        });
        let bus = SyncEventBus::new();
        let qm = make_manager(2);

        subscribe_queue_to_config(&bus, Arc::clone(&config_store), Arc::clone(&qm));

        config_store
            .update_config(ConfigPatch {
                max_concurrent_downloads: Some(5),
                ..Default::default()
            })
            .unwrap();
        bus.publish(DomainEvent::SettingsUpdated);
        assert_eq!(qm.max_concurrent(), 5);

        config_store
            .update_config(ConfigPatch {
                max_concurrent_downloads: Some(12),
                ..Default::default()
            })
            .unwrap();
        bus.publish(DomainEvent::SettingsUpdated);
        assert_eq!(qm.max_concurrent(), 12);
    }

    #[tokio::test]
    async fn test_settings_updated_clamps_zero_to_minimum() {
        // A corrupted config (e.g. hand-edited `config.toml`) with
        // `max_concurrent_downloads = 0` would stall the scheduler if
        // forwarded verbatim. The bridge must clamp to the domain minimum.
        let cfg = AppConfig {
            max_concurrent_downloads: 0,
            ..AppConfig::default()
        };
        let config_store: Arc<dyn ConfigStore> = Arc::new(StubConfigStore {
            config: Mutex::new(cfg),
        });
        let bus = SyncEventBus::new();
        let qm = make_manager(4);

        subscribe_queue_to_config(&bus, Arc::clone(&config_store), Arc::clone(&qm));

        bus.publish(DomainEvent::SettingsUpdated);

        assert_eq!(qm.max_concurrent(), 1);
    }

    #[tokio::test]
    async fn test_settings_updated_clamps_above_range_to_maximum() {
        let cfg = AppConfig {
            max_concurrent_downloads: 9999,
            ..AppConfig::default()
        };
        let config_store: Arc<dyn ConfigStore> = Arc::new(StubConfigStore {
            config: Mutex::new(cfg),
        });
        let bus = SyncEventBus::new();
        let qm = make_manager(4);

        subscribe_queue_to_config(&bus, Arc::clone(&config_store), Arc::clone(&qm));

        bus.publish(DomainEvent::SettingsUpdated);

        assert_eq!(qm.max_concurrent(), 20);
    }
}
