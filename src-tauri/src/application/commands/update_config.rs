//! Handler for `UpdateConfigCommand`.

use crate::application::command_bus::CommandBus;
use crate::application::error::AppError;
use crate::domain::event::DomainEvent;
use crate::domain::model::config::{AppConfig, ConfigPatch};

/// Validate a `ConfigPatch` before applying.
/// Returns `Err(AppError::Validation)` on invalid values.
fn validate_patch(patch: &ConfigPatch) -> Result<(), AppError> {
    if let Some(ref pt) = patch.proxy_type
        && !matches!(pt.as_str(), "none" | "http" | "socks5")
    {
        return Err(AppError::Validation(format!(
            "proxy_type must be none, http, or socks5, got '{pt}'"
        )));
    }
    if let Some(ref t) = patch.theme
        && !matches!(t.as_str(), "light" | "dark" | "auto")
    {
        return Err(AppError::Validation(format!(
            "theme must be light, dark, or auto, got '{t}'"
        )));
    }
    if let Some(port) = patch.web_interface_port
        && port < 1024
    {
        return Err(AppError::Validation(format!(
            "web_interface_port must be >= 1024, got {port}"
        )));
    }
    if let Some(max) = patch.max_concurrent_downloads
        && !(1..=100).contains(&max)
    {
        return Err(AppError::Validation(format!(
            "max_concurrent_downloads must be 1-100, got {max}"
        )));
    }
    if let Some(seg) = patch.max_segments_per_download
        && !(1..=32).contains(&seg)
    {
        return Err(AppError::Validation(format!(
            "max_segments_per_download must be 1-32, got {seg}"
        )));
    }
    if let Some(t) = patch.connection_timeout_seconds
        && !(5..=300).contains(&t)
    {
        return Err(AppError::Validation(format!(
            "connection_timeout_seconds must be 5-300, got {t}"
        )));
    }
    Ok(())
}

impl CommandBus {
    pub fn handle_update_config(
        &self,
        cmd: super::UpdateConfigCommand,
    ) -> Result<AppConfig, AppError> {
        validate_patch(&cmd.patch)?;
        let updated = self.config_store().update_config(cmd.patch)?;
        self.event_bus().publish(DomainEvent::SettingsUpdated);
        Ok(updated)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::commands::UpdateConfigCommand;
    use crate::domain::model::config::ConfigPatch;

    // Re-use the test helper from command_bus tests
    fn make_command_bus() -> CommandBus {
        use std::collections::HashMap;
        use std::sync::{Arc, Mutex};

        use crate::domain::error::DomainError;
        use crate::domain::model::config::apply_patch;
        use crate::domain::model::credential::Credential;
        use crate::domain::model::download::{Download, DownloadId, DownloadState};
        use crate::domain::model::http::HttpResponse;
        use crate::domain::model::meta::DownloadMeta;
        use crate::domain::model::plugin::{PluginInfo, PluginManifest};
        use crate::domain::ports::driven::{
            ClipboardObserver, ConfigStore, CredentialStore, DownloadEngine, DownloadRepository,
            EventBus, FileStorage, HttpClient, PluginLoader,
        };

        struct StubRepo;
        impl DownloadRepository for StubRepo {
            fn find_by_id(&self, _: DownloadId) -> Result<Option<Download>, DomainError> {
                Ok(None)
            }
            fn save(&self, _: &Download) -> Result<(), DomainError> {
                Ok(())
            }
            fn delete(&self, _: DownloadId) -> Result<(), DomainError> {
                Ok(())
            }
            fn find_by_state(&self, _: DownloadState) -> Result<Vec<Download>, DomainError> {
                Ok(vec![])
            }
        }

        struct StubEngine;
        impl DownloadEngine for StubEngine {
            fn start(&self, _: &Download) -> Result<(), DomainError> {
                Ok(())
            }
            fn pause(&self, _: DownloadId) -> Result<(), DomainError> {
                Ok(())
            }
            fn resume(&self, _: DownloadId) -> Result<(), DomainError> {
                Ok(())
            }
            fn cancel(&self, _: DownloadId) -> Result<(), DomainError> {
                Ok(())
            }
        }

        struct CollectingEventBus {
            events: Mutex<Vec<DomainEvent>>,
        }
        impl EventBus for CollectingEventBus {
            fn publish(&self, event: DomainEvent) {
                self.events.lock().unwrap().push(event);
            }
            fn subscribe(&self, _: Box<dyn Fn(&DomainEvent) + Send + Sync>) {}
        }

        struct StubFs;
        impl FileStorage for StubFs {
            fn create_file(&self, _: &std::path::Path, _: u64) -> Result<(), DomainError> {
                Ok(())
            }
            fn write_segment(
                &self,
                _: &std::path::Path,
                _: u64,
                _: &[u8],
            ) -> Result<(), DomainError> {
                Ok(())
            }
            fn read_meta(&self, _: &std::path::Path) -> Result<Option<DownloadMeta>, DomainError> {
                Ok(None)
            }
            fn write_meta(&self, _: &std::path::Path, _: &DownloadMeta) -> Result<(), DomainError> {
                Ok(())
            }
            fn delete_meta(&self, _: &std::path::Path) -> Result<(), DomainError> {
                Ok(())
            }
        }

        struct StubHttp;
        impl HttpClient for StubHttp {
            fn head(&self, _: &str) -> Result<HttpResponse, DomainError> {
                Ok(HttpResponse {
                    status_code: 200,
                    headers: HashMap::new(),
                    body: vec![],
                })
            }
            fn get_range(&self, _: &str, _: u64, _: u64) -> Result<Vec<u8>, DomainError> {
                Ok(vec![])
            }
            fn supports_range(&self, _: &str) -> Result<bool, DomainError> {
                Ok(true)
            }
        }

        struct StubPluginLoader;
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
        }

        struct InMemoryConfigStore {
            config: Mutex<AppConfig>,
        }
        impl ConfigStore for InMemoryConfigStore {
            fn get_config(&self) -> Result<AppConfig, DomainError> {
                Ok(self.config.lock().unwrap().clone())
            }
            fn update_config(&self, patch: ConfigPatch) -> Result<AppConfig, DomainError> {
                let mut config = self.config.lock().unwrap();
                apply_patch(&mut config, &patch);
                Ok(config.clone())
            }
        }

        struct StubCredStore;
        impl CredentialStore for StubCredStore {
            fn get(&self, _: &str) -> Result<Option<Credential>, DomainError> {
                Ok(None)
            }
            fn store(&self, _: &str, _: &Credential) -> Result<(), DomainError> {
                Ok(())
            }
            fn delete(&self, _: &str) -> Result<(), DomainError> {
                Ok(())
            }
        }

        struct StubClipboard;
        impl ClipboardObserver for StubClipboard {
            fn start(&self) -> Result<(), DomainError> {
                Ok(())
            }
            fn stop(&self) -> Result<(), DomainError> {
                Ok(())
            }
            fn get_urls(&self) -> Result<Vec<String>, DomainError> {
                Ok(vec![])
            }
        }

        CommandBus::new(
            Arc::new(StubRepo),
            Arc::new(StubEngine),
            Arc::new(CollectingEventBus {
                events: Mutex::new(Vec::new()),
            }),
            Arc::new(StubFs),
            Arc::new(StubHttp),
            Arc::new(StubPluginLoader),
            Arc::new(InMemoryConfigStore {
                config: Mutex::new(AppConfig::default()),
            }),
            Arc::new(StubCredStore),
            Arc::new(StubClipboard),
        )
    }

    #[test]
    fn test_handle_update_config_returns_updated_config() {
        let bus = make_command_bus();
        let cmd = UpdateConfigCommand {
            patch: ConfigPatch {
                theme: Some("dark".to_string()),
                max_concurrent_downloads: Some(5),
                ..Default::default()
            },
        };

        let result = bus.handle_update_config(cmd).unwrap();
        assert_eq!(result.theme, "dark");
        assert_eq!(result.max_concurrent_downloads, 5);
        // Other fields should remain at defaults
        assert_eq!(result.locale, "en");
    }

    #[test]
    fn test_handle_update_config_empty_patch_returns_defaults() {
        let bus = make_command_bus();
        let cmd = UpdateConfigCommand {
            patch: ConfigPatch::default(),
        };

        let result = bus.handle_update_config(cmd).unwrap();
        assert_eq!(result, AppConfig::default());
    }

    #[test]
    fn test_handle_update_config_rejects_invalid_proxy_type() {
        let bus = make_command_bus();
        let cmd = UpdateConfigCommand {
            patch: ConfigPatch {
                proxy_type: Some("invalid".to_string()),
                ..Default::default()
            },
        };

        let result = bus.handle_update_config(cmd);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("proxy_type"));
    }

    #[test]
    fn test_handle_update_config_rejects_invalid_theme() {
        let bus = make_command_bus();
        let cmd = UpdateConfigCommand {
            patch: ConfigPatch {
                theme: Some("neon".to_string()),
                ..Default::default()
            },
        };

        let result = bus.handle_update_config(cmd);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("theme"));
    }

    #[test]
    fn test_handle_update_config_rejects_low_port() {
        let bus = make_command_bus();
        let cmd = UpdateConfigCommand {
            patch: ConfigPatch {
                web_interface_port: Some(80),
                ..Default::default()
            },
        };

        let result = bus.handle_update_config(cmd);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("web_interface_port")
        );
    }

    #[test]
    fn test_handle_update_config_rejects_zero_concurrent() {
        let bus = make_command_bus();
        let cmd = UpdateConfigCommand {
            patch: ConfigPatch {
                max_concurrent_downloads: Some(0),
                ..Default::default()
            },
        };

        let result = bus.handle_update_config(cmd);
        assert!(result.is_err());
    }
}
