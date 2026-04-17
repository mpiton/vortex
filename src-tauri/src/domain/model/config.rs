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
    // ── General ──────────────────────────────────────────────────────
    /// Download directory path. `None` means the adapter must resolve
    /// a platform-specific default (e.g., `~/Downloads`) before use.
    pub download_dir: Option<String>,
    pub start_minimized: bool,
    pub notifications_enabled: bool,
    pub auto_extract: bool,
    pub clipboard_monitoring: bool,
    pub sound_enabled: bool,
    pub confirm_delete: bool,
    pub subfolder_per_package: bool,

    // ── Downloads ────────────────────────────────────────────────────
    pub max_concurrent_downloads: u32,
    pub max_segments_per_download: u32,
    /// `None` means unlimited.
    pub speed_limit_bytes_per_sec: Option<u64>,
    pub max_retries: u32,
    pub retry_delay_seconds: u32,
    pub verify_checksums: bool,
    pub pre_allocate_space: bool,

    // ── Network ──────────────────────────────────────────────────────
    /// `"none"`, `"http"`, or `"socks5"`.
    pub proxy_type: String,
    pub proxy_url: Option<String>,
    pub user_agent: String,
    pub dns_over_https: bool,
    pub connection_timeout_seconds: u32,

    // ── Remote Access ────────────────────────────────────────────────
    /// Whether to spawn the embedded web/REST server on startup.
    /// When `false` (default), no socket is bound regardless of the
    /// `rest_api_enabled` / `websocket_enabled` preferences below.
    pub web_interface_enabled: bool,
    pub web_interface_port: u16,
    /// User-facing preference: inert while `web_interface_enabled` is
    /// `false`. Callers that spawn the server MUST also enforce
    /// `api_key` validity before serving requests.
    pub rest_api_enabled: bool,
    pub api_key: String,
    /// User-facing preference: inert while `web_interface_enabled` is
    /// `false`. See `rest_api_enabled`.
    pub websocket_enabled: bool,

    // ── Browser Integration ──────────────────────────────────────────
    /// Minimum file size in megabytes. `0.0` means capture all.
    pub min_file_size_mb: f64,
    pub excluded_domains: Vec<String>,
    pub excluded_extensions: Vec<String>,

    // ── Appearance ───────────────────────────────────────────────────
    /// `"light"`, `"dark"`, or `"auto"`.
    pub theme: String,
    /// Hex color string (e.g. `"#4F46E5"`).
    pub accent_color: String,
    pub compact_mode: bool,
    pub locale: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            // General
            download_dir: None,
            start_minimized: false,
            notifications_enabled: true,
            auto_extract: true,
            clipboard_monitoring: true,
            sound_enabled: false,
            confirm_delete: true,
            subfolder_per_package: false,

            // Downloads
            max_concurrent_downloads: 4,
            max_segments_per_download: 8,
            speed_limit_bytes_per_sec: None,
            max_retries: 5,
            retry_delay_seconds: 10,
            verify_checksums: true,
            pre_allocate_space: true,

            // Network
            proxy_type: "none".to_string(),
            proxy_url: None,
            user_agent: "Vortex/1.0".to_string(),
            dns_over_https: false,
            connection_timeout_seconds: 30,

            // Remote Access
            web_interface_enabled: false,
            web_interface_port: 9876,
            rest_api_enabled: true,
            api_key: String::new(),
            websocket_enabled: true,

            // Browser Integration
            min_file_size_mb: 1.0,
            excluded_domains: Vec::new(),
            excluded_extensions: Vec::new(),

            // Appearance
            theme: "auto".to_string(),
            accent_color: "#4F46E5".to_string(),
            compact_mode: false,
            locale: "en".to_string(),
        }
    }
}

/// Partial configuration update.
///
/// Only the fields set to `Some(...)` will be applied.
/// This avoids overwriting unchanged settings.
///
/// For nullable fields (`download_dir`, `speed_limit_bytes_per_sec`,
/// `proxy_url`): `None` = no change, `Some(None)` = reset to default,
/// `Some(Some(value))` = set.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ConfigPatch {
    // General
    pub download_dir: Option<Option<String>>,
    pub start_minimized: Option<bool>,
    pub notifications_enabled: Option<bool>,
    pub auto_extract: Option<bool>,
    pub clipboard_monitoring: Option<bool>,
    pub sound_enabled: Option<bool>,
    pub confirm_delete: Option<bool>,
    pub subfolder_per_package: Option<bool>,

    // Downloads
    pub max_concurrent_downloads: Option<u32>,
    pub max_segments_per_download: Option<u32>,
    pub speed_limit_bytes_per_sec: Option<Option<u64>>,
    pub max_retries: Option<u32>,
    pub retry_delay_seconds: Option<u32>,
    pub verify_checksums: Option<bool>,
    pub pre_allocate_space: Option<bool>,

    // Network
    pub proxy_type: Option<String>,
    pub proxy_url: Option<Option<String>>,
    pub user_agent: Option<String>,
    pub dns_over_https: Option<bool>,
    pub connection_timeout_seconds: Option<u32>,

    // Remote Access
    pub web_interface_enabled: Option<bool>,
    pub web_interface_port: Option<u16>,
    pub rest_api_enabled: Option<bool>,
    pub api_key: Option<String>,
    pub websocket_enabled: Option<bool>,

    // Browser Integration
    pub min_file_size_mb: Option<f64>,
    pub excluded_domains: Option<Vec<String>>,
    pub excluded_extensions: Option<Vec<String>>,

    // Appearance
    pub theme: Option<String>,
    pub accent_color: Option<String>,
    pub compact_mode: Option<bool>,
    pub locale: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_app_config_matches_prd_6_10() {
        let config = AppConfig::default();

        // Domain stays pure: adapter layer hydrates download_dir.
        assert_eq!(config.download_dir, None);

        // General
        assert!(
            config.auto_extract,
            "PRD §6.10: auto_extract defaults to ON"
        );

        // Downloads
        assert_eq!(config.max_concurrent_downloads, 4);
        assert_eq!(config.max_retries, 5);
        assert_eq!(config.retry_delay_seconds, 10);
        assert!(config.verify_checksums);

        // Browser integration
        assert_eq!(config.min_file_size_mb, 1.0);

        // Remote access
        assert_eq!(config.web_interface_port, 9876);
        assert!(config.rest_api_enabled);
        assert!(config.websocket_enabled);
    }
}

/// Apply a `ConfigPatch` to an `AppConfig`, updating only fields
/// that are `Some(...)`. Pure function — no I/O.
pub fn apply_patch(config: &mut AppConfig, patch: &ConfigPatch) {
    // General
    if let Some(ref dir) = patch.download_dir {
        config.download_dir = dir.clone();
    }
    if let Some(v) = patch.start_minimized {
        config.start_minimized = v;
    }
    if let Some(v) = patch.notifications_enabled {
        config.notifications_enabled = v;
    }
    if let Some(v) = patch.auto_extract {
        config.auto_extract = v;
    }
    if let Some(v) = patch.clipboard_monitoring {
        config.clipboard_monitoring = v;
    }
    if let Some(v) = patch.sound_enabled {
        config.sound_enabled = v;
    }
    if let Some(v) = patch.confirm_delete {
        config.confirm_delete = v;
    }
    if let Some(v) = patch.subfolder_per_package {
        config.subfolder_per_package = v;
    }

    // Downloads
    if let Some(v) = patch.max_concurrent_downloads {
        config.max_concurrent_downloads = v;
    }
    if let Some(v) = patch.max_segments_per_download {
        config.max_segments_per_download = v;
    }
    if let Some(ref limit) = patch.speed_limit_bytes_per_sec {
        config.speed_limit_bytes_per_sec = *limit;
    }
    if let Some(v) = patch.max_retries {
        config.max_retries = v;
    }
    if let Some(v) = patch.retry_delay_seconds {
        config.retry_delay_seconds = v;
    }
    if let Some(v) = patch.verify_checksums {
        config.verify_checksums = v;
    }
    if let Some(v) = patch.pre_allocate_space {
        config.pre_allocate_space = v;
    }

    // Network
    if let Some(ref v) = patch.proxy_type {
        config.proxy_type = v.clone();
    }
    if let Some(ref v) = patch.proxy_url {
        config.proxy_url = v.clone();
    }
    if let Some(ref v) = patch.user_agent {
        config.user_agent = v.clone();
    }
    if let Some(v) = patch.dns_over_https {
        config.dns_over_https = v;
    }
    if let Some(v) = patch.connection_timeout_seconds {
        config.connection_timeout_seconds = v;
    }

    // Remote Access
    if let Some(v) = patch.web_interface_enabled {
        config.web_interface_enabled = v;
    }
    if let Some(v) = patch.web_interface_port {
        config.web_interface_port = v;
    }
    if let Some(v) = patch.rest_api_enabled {
        config.rest_api_enabled = v;
    }
    if let Some(ref v) = patch.api_key {
        config.api_key = v.clone();
    }
    if let Some(v) = patch.websocket_enabled {
        config.websocket_enabled = v;
    }

    // Browser Integration
    if let Some(v) = patch.min_file_size_mb {
        config.min_file_size_mb = v;
    }
    if let Some(ref v) = patch.excluded_domains {
        config.excluded_domains = v.clone();
    }
    if let Some(ref v) = patch.excluded_extensions {
        config.excluded_extensions = v.clone();
    }

    // Appearance
    if let Some(ref v) = patch.theme {
        config.theme = v.clone();
    }
    if let Some(ref v) = patch.accent_color {
        config.accent_color = v.clone();
    }
    if let Some(v) = patch.compact_mode {
        config.compact_mode = v;
    }
    if let Some(ref v) = patch.locale {
        config.locale = v.clone();
    }
}
