//! TOML-backed implementation of the `ConfigStore` port.
//!
//! Reads and writes `config.toml` with atomic file operations
//! (write to `.tmp` then rename) and automatic parent directory creation.

use std::path::PathBuf;
use std::sync::Mutex;

use crate::domain::error::DomainError;
use crate::domain::model::config::{AppConfig, ConfigPatch, apply_patch};
use crate::domain::ports::driven::ConfigStore;

/// Persists application configuration as a TOML file.
pub struct TomlConfigStore {
    path: PathBuf,
    lock: Mutex<()>,
}

impl TomlConfigStore {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            lock: Mutex::new(()),
        }
    }

    fn read_or_default(&self) -> Result<AppConfig, DomainError> {
        if !self.path.exists() {
            return Ok(AppConfig::default());
        }
        let content = std::fs::read_to_string(&self.path)
            .map_err(|e| DomainError::StorageError(format!("failed to read config: {e}")))?;
        let dto: ConfigDto = toml::from_str(&content)
            .map_err(|e| DomainError::StorageError(format!("failed to parse config: {e}")))?;
        Ok(AppConfig::from(dto))
    }

    fn write_config(&self, config: &AppConfig) -> Result<(), DomainError> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                DomainError::StorageError(format!("failed to create config directory: {e}"))
            })?;
        }
        let dto = ConfigDto::from(config.clone());
        let content = toml::to_string_pretty(&dto)
            .map_err(|e| DomainError::StorageError(format!("failed to serialize config: {e}")))?;

        let tmp_path = self.path.with_extension("tmp");
        {
            use std::io::Write;
            let mut file = std::fs::OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(&tmp_path)
                .map_err(|e| {
                    DomainError::StorageError(format!("failed to create config tmp file: {e}"))
                })?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                file.set_permissions(std::fs::Permissions::from_mode(0o600))
                    .map_err(|e| {
                        DomainError::StorageError(format!(
                            "failed to set config file permissions: {e}"
                        ))
                    })?;
            }
            file.write_all(content.as_bytes()).map_err(|e| {
                DomainError::StorageError(format!("failed to write config tmp file: {e}"))
            })?;
            file.sync_all().map_err(|e| {
                DomainError::StorageError(format!("failed to sync config tmp file: {e}"))
            })?;
        }
        std::fs::rename(&tmp_path, &self.path)
            .map_err(|e| DomainError::StorageError(format!("failed to rename config file: {e}")))?;
        Ok(())
    }
}

impl ConfigStore for TomlConfigStore {
    fn get_config(&self) -> Result<AppConfig, DomainError> {
        let _guard = self
            .lock
            .lock()
            .map_err(|e| DomainError::StorageError(format!("config lock poisoned: {e}")))?;
        let config = self.read_or_default()?;
        if !self.path.exists() {
            self.write_config(&config)?;
        }
        Ok(config)
    }

    fn update_config(&self, patch: ConfigPatch) -> Result<AppConfig, DomainError> {
        let _guard = self
            .lock
            .lock()
            .map_err(|e| DomainError::StorageError(format!("config lock poisoned: {e}")))?;
        let mut config = self.read_or_default()?;
        apply_patch(&mut config, &patch);
        self.write_config(&config)?;
        Ok(config)
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(default, rename_all = "snake_case")]
struct ConfigDto {
    // General
    download_dir: Option<String>,
    start_minimized: bool,
    notifications_enabled: bool,
    auto_extract: bool,
    clipboard_monitoring: bool,
    sound_enabled: bool,
    confirm_delete: bool,
    subfolder_per_package: bool,

    // Downloads
    max_concurrent_downloads: u32,
    max_segments_per_download: u32,
    speed_limit_bytes_per_sec: Option<u64>,
    max_retries: u32,
    retry_delay_seconds: u32,
    verify_checksums: bool,
    pre_allocate_space: bool,

    // Network
    proxy_type: String,
    proxy_url: Option<String>,
    user_agent: String,
    dns_over_https: bool,
    connection_timeout_seconds: u32,

    // Remote Access
    web_interface_enabled: bool,
    web_interface_port: u16,
    rest_api_enabled: bool,
    api_key: String,
    websocket_enabled: bool,

    // Browser Integration
    min_file_size_mb: f64,
    excluded_domains: Vec<String>,
    excluded_extensions: Vec<String>,

    // Appearance
    theme: String,
    accent_color: String,
    compact_mode: bool,
    locale: String,
}

impl Default for ConfigDto {
    fn default() -> Self {
        let defaults = AppConfig::default();
        Self::from(defaults)
    }
}

impl From<AppConfig> for ConfigDto {
    fn from(c: AppConfig) -> Self {
        Self {
            download_dir: c.download_dir,
            start_minimized: c.start_minimized,
            notifications_enabled: c.notifications_enabled,
            auto_extract: c.auto_extract,
            clipboard_monitoring: c.clipboard_monitoring,
            sound_enabled: c.sound_enabled,
            confirm_delete: c.confirm_delete,
            subfolder_per_package: c.subfolder_per_package,
            max_concurrent_downloads: c.max_concurrent_downloads,
            max_segments_per_download: c.max_segments_per_download,
            speed_limit_bytes_per_sec: c.speed_limit_bytes_per_sec,
            max_retries: c.max_retries,
            retry_delay_seconds: c.retry_delay_seconds,
            verify_checksums: c.verify_checksums,
            pre_allocate_space: c.pre_allocate_space,
            proxy_type: c.proxy_type,
            proxy_url: c.proxy_url,
            user_agent: c.user_agent,
            dns_over_https: c.dns_over_https,
            connection_timeout_seconds: c.connection_timeout_seconds,
            web_interface_enabled: c.web_interface_enabled,
            web_interface_port: c.web_interface_port,
            rest_api_enabled: c.rest_api_enabled,
            api_key: c.api_key,
            websocket_enabled: c.websocket_enabled,
            min_file_size_mb: c.min_file_size_mb,
            excluded_domains: c.excluded_domains,
            excluded_extensions: c.excluded_extensions,
            theme: c.theme,
            accent_color: c.accent_color,
            compact_mode: c.compact_mode,
            locale: c.locale,
        }
    }
}

impl From<ConfigDto> for AppConfig {
    fn from(d: ConfigDto) -> Self {
        Self {
            download_dir: d.download_dir,
            start_minimized: d.start_minimized,
            notifications_enabled: d.notifications_enabled,
            auto_extract: d.auto_extract,
            clipboard_monitoring: d.clipboard_monitoring,
            sound_enabled: d.sound_enabled,
            confirm_delete: d.confirm_delete,
            subfolder_per_package: d.subfolder_per_package,
            max_concurrent_downloads: d.max_concurrent_downloads,
            max_segments_per_download: d.max_segments_per_download,
            speed_limit_bytes_per_sec: d.speed_limit_bytes_per_sec,
            max_retries: d.max_retries,
            retry_delay_seconds: d.retry_delay_seconds,
            verify_checksums: d.verify_checksums,
            pre_allocate_space: d.pre_allocate_space,
            proxy_type: d.proxy_type,
            proxy_url: d.proxy_url,
            user_agent: d.user_agent,
            dns_over_https: d.dns_over_https,
            connection_timeout_seconds: d.connection_timeout_seconds,
            web_interface_enabled: d.web_interface_enabled,
            web_interface_port: d.web_interface_port,
            rest_api_enabled: d.rest_api_enabled,
            api_key: d.api_key,
            websocket_enabled: d.websocket_enabled,
            min_file_size_mb: d.min_file_size_mb,
            excluded_domains: d.excluded_domains,
            excluded_extensions: d.excluded_extensions,
            theme: d.theme,
            accent_color: d.accent_color,
            compact_mode: d.compact_mode,
            locale: d.locale,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_config_returns_defaults_when_file_missing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let store = TomlConfigStore::new(path.clone());

        let config = store.get_config().unwrap();
        assert_eq!(config, AppConfig::default());
        // File should be created with defaults
        assert!(path.exists());
    }

    #[test]
    fn test_save_load_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let store = TomlConfigStore::new(path);

        let patch = ConfigPatch {
            max_concurrent_downloads: Some(10),
            theme: Some("dark".to_string()),
            locale: Some("fr".to_string()),
            ..Default::default()
        };

        let updated = store.update_config(patch).unwrap();
        assert_eq!(updated.max_concurrent_downloads, 10);
        assert_eq!(updated.theme, "dark");
        assert_eq!(updated.locale, "fr");

        // Reload from file
        let reloaded = store.get_config().unwrap();
        assert_eq!(reloaded, updated);
    }

    #[test]
    fn test_partial_patch_preserves_other_fields() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let store = TomlConfigStore::new(path);

        // Set some values
        store
            .update_config(ConfigPatch {
                max_retries: Some(5),
                sound_enabled: Some(true),
                ..Default::default()
            })
            .unwrap();

        // Patch only one field
        let updated = store
            .update_config(ConfigPatch {
                max_retries: Some(10),
                ..Default::default()
            })
            .unwrap();

        assert_eq!(updated.max_retries, 10);
        assert!(updated.sound_enabled); // preserved
    }

    #[test]
    fn test_creates_parent_directories() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("deep").join("config.toml");
        let store = TomlConfigStore::new(path.clone());

        let config = store.get_config().unwrap();
        assert_eq!(config, AppConfig::default());
        assert!(path.exists());
    }

    #[test]
    fn test_missing_fields_in_toml_use_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        // Write a partial TOML file
        std::fs::write(&path, "theme = \"dark\"\n").unwrap();

        let store = TomlConfigStore::new(path);
        let config = store.get_config().unwrap();

        assert_eq!(config.theme, "dark");
        // All other fields should be defaults
        assert_eq!(config.max_concurrent_downloads, 3);
        assert!(config.notifications_enabled);
    }

    #[test]
    fn test_nullable_fields_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let store = TomlConfigStore::new(path);

        // Set a nullable field
        let updated = store
            .update_config(ConfigPatch {
                speed_limit_bytes_per_sec: Some(Some(1024)),
                proxy_url: Some(Some("http://proxy:8080".to_string())),
                ..Default::default()
            })
            .unwrap();

        assert_eq!(updated.speed_limit_bytes_per_sec, Some(1024));
        assert_eq!(updated.proxy_url, Some("http://proxy:8080".to_string()));

        // Clear the nullable field
        let cleared = store
            .update_config(ConfigPatch {
                speed_limit_bytes_per_sec: Some(None),
                ..Default::default()
            })
            .unwrap();

        assert_eq!(cleared.speed_limit_bytes_per_sec, None);
        // proxy_url should still be set
        assert_eq!(cleared.proxy_url, Some("http://proxy:8080".to_string()));
    }
}
