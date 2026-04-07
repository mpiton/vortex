//! Application configuration types.
//!
//! Used by `ConfigStore` port for reading and updating settings.
//! These types live in the domain because the port traits reference them.

/// Application-wide configuration.
///
/// Represents the full config as stored in `config.toml`.
/// Adapters serialize/deserialize this to the actual file format.
#[derive(Debug, Clone, PartialEq)]
pub struct AppConfig {
    pub download_dir: String,
    pub max_concurrent_downloads: u32,
    pub max_segments_per_download: u32,
    pub speed_limit_bytes_per_sec: Option<u64>,
    pub auto_extract: bool,
    pub theme: String,
    pub locale: String,
    pub clipboard_monitoring: bool,
    pub minimize_to_tray: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            download_dir: String::new(),
            max_concurrent_downloads: 3,
            max_segments_per_download: 8,
            speed_limit_bytes_per_sec: None,
            auto_extract: false,
            theme: "system".to_string(),
            locale: "en".to_string(),
            clipboard_monitoring: true,
            minimize_to_tray: true,
        }
    }
}

/// Partial configuration update.
///
/// Only the fields set to `Some(...)` will be applied.
/// This avoids overwriting unchanged settings.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ConfigPatch {
    pub download_dir: Option<String>,
    pub max_concurrent_downloads: Option<u32>,
    pub max_segments_per_download: Option<u32>,
    pub speed_limit_bytes_per_sec: Option<Option<u64>>,
    pub auto_extract: Option<bool>,
    pub theme: Option<String>,
    pub locale: Option<String>,
    pub clipboard_monitoring: Option<bool>,
    pub minimize_to_tray: Option<bool>,
}
