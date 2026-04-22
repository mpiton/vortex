//! Handler for `UpdateConfigCommand`.

use crate::application::command_bus::CommandBus;
use crate::application::error::AppError;
use crate::domain::event::DomainEvent;
use crate::domain::model::config::AppConfig;

/// Validate the merged configuration after applying a patch.
/// Returns `Err(AppError::Validation)` on invalid combined state.
fn validate_config(config: &AppConfig) -> Result<(), AppError> {
    if !matches!(config.proxy_type.as_str(), "none" | "http" | "socks5") {
        return Err(AppError::Validation(format!(
            "proxy_type must be none, http, or socks5, got '{}'",
            config.proxy_type
        )));
    }
    if !matches!(config.theme.as_str(), "light" | "dark" | "auto") {
        return Err(AppError::Validation(format!(
            "theme must be light, dark, or auto, got '{}'",
            config.theme
        )));
    }
    if !matches!(config.locale.as_str(), "en" | "fr") {
        return Err(AppError::Validation(format!(
            "locale must be one of en, fr, got '{}'",
            config.locale
        )));
    }
    if config.web_interface_port < 1024 {
        return Err(AppError::Validation(format!(
            "web_interface_port must be >= 1024, got {}",
            config.web_interface_port
        )));
    }
    if !(1..=20).contains(&config.max_concurrent_downloads) {
        return Err(AppError::Validation(format!(
            "max_concurrent_downloads must be 1-20, got {}",
            config.max_concurrent_downloads
        )));
    }
    if !(1..=32).contains(&config.max_segments_per_download) {
        return Err(AppError::Validation(format!(
            "max_segments_per_download must be 1-32, got {}",
            config.max_segments_per_download
        )));
    }
    if !(5..=300).contains(&config.connection_timeout_seconds) {
        return Err(AppError::Validation(format!(
            "connection_timeout_seconds must be 5-300, got {}",
            config.connection_timeout_seconds
        )));
    }
    // Cross-field: proxy URL required when proxy is enabled
    if config.proxy_type != "none" && config.proxy_url.as_ref().is_none_or(|u| u.is_empty()) {
        return Err(AppError::Validation(
            "proxy_url is required when proxy_type is not 'none'".to_string(),
        ));
    }
    Ok(())
}

impl CommandBus {
    pub fn handle_update_config(
        &self,
        cmd: super::UpdateConfigCommand,
    ) -> Result<AppConfig, AppError> {
        // Merge patch with current config and validate the result
        let current = self.config_store().get_config()?;
        let mut merged = current;
        crate::domain::model::config::apply_patch(&mut merged, &cmd.patch);
        validate_config(&merged)?;

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

        struct FakeArchiveExtractor;
        impl crate::domain::ports::driven::ArchiveExtractor for FakeArchiveExtractor {
            fn detect_format(
                &self,
                _file_path: &std::path::Path,
            ) -> Result<Option<crate::domain::model::archive::ArchiveFormat>, DomainError>
            {
                Ok(None)
            }
            fn can_extract(&self, _file_path: &std::path::Path) -> Result<bool, DomainError> {
                Ok(false)
            }
            fn extract(
                &self,
                _file_path: &std::path::Path,
                _dest_dir: &std::path::Path,
                _password: Option<&str>,
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
                _file_path: &std::path::Path,
                _password: Option<&str>,
            ) -> Result<Vec<crate::domain::model::archive::ArchiveEntry>, DomainError> {
                Ok(vec![])
            }
            fn detect_segments(
                &self,
                _file_path: &std::path::Path,
            ) -> Result<Option<Vec<std::path::PathBuf>>, DomainError> {
                Ok(None)
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
            Arc::new(FakeArchiveExtractor),
            Arc::new(crate::application::test_support::NoopHistoryRepo),
            None,
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

    #[test]
    fn test_handle_update_config_rejects_concurrent_above_20() {
        let bus = make_command_bus();
        let cmd = UpdateConfigCommand {
            patch: ConfigPatch {
                max_concurrent_downloads: Some(21),
                ..Default::default()
            },
        };

        let result = bus.handle_update_config(cmd);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("max_concurrent_downloads")
        );
    }

    #[test]
    fn test_handle_update_config_accepts_max_concurrent_20() {
        let bus = make_command_bus();
        let cmd = UpdateConfigCommand {
            patch: ConfigPatch {
                max_concurrent_downloads: Some(20),
                ..Default::default()
            },
        };

        let result = bus.handle_update_config(cmd);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().max_concurrent_downloads, 20);
    }

    #[test]
    fn test_handle_update_config_rejects_proxy_without_url() {
        let bus = make_command_bus();
        let cmd = UpdateConfigCommand {
            patch: ConfigPatch {
                proxy_type: Some("http".to_string()),
                // proxy_url not set → merged config has None
                ..Default::default()
            },
        };

        let result = bus.handle_update_config(cmd);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("proxy_url"));
    }

    #[test]
    fn test_handle_update_config_accepts_proxy_with_url() {
        let bus = make_command_bus();
        let cmd = UpdateConfigCommand {
            patch: ConfigPatch {
                proxy_type: Some("http".to_string()),
                proxy_url: Some(Some("http://proxy:8080".to_string())),
                ..Default::default()
            },
        };

        let result = bus.handle_update_config(cmd).unwrap();
        assert_eq!(result.proxy_type, "http");
        assert_eq!(result.proxy_url, Some("http://proxy:8080".to_string()));
    }

    #[test]
    fn test_handle_update_config_rejects_invalid_locale() {
        let bus = make_command_bus();
        let cmd = UpdateConfigCommand {
            patch: ConfigPatch {
                locale: Some("xx".to_string()),
                ..Default::default()
            },
        };

        let result = bus.handle_update_config(cmd);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("locale"));
    }

    #[test]
    fn test_handle_update_config_accepts_valid_locale() {
        let bus = make_command_bus();
        for locale in ["en", "fr"] {
            let cmd = UpdateConfigCommand {
                patch: ConfigPatch {
                    locale: Some(locale.to_string()),
                    ..Default::default()
                },
            };
            let result = bus.handle_update_config(cmd).unwrap();
            assert_eq!(result.locale, locale);
        }
    }
}
