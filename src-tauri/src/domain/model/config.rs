//! Application configuration types.
//!
//! Used by `ConfigStore` port for reading and updating settings.
//! These types live in the domain because the port traits reference them.

use crate::domain::model::account::AccountSelectionStrategy;

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
    /// Enable runtime re-split of slow segments when a faster segment
    /// finishes. PRD §7.1 (répartition dynamique).
    pub dynamic_split_enabled: bool,
    /// Minimum remaining bytes (in MiB) required before a segment is
    /// eligible for re-split. Below this threshold, the parallelism gain
    /// is dwarfed by HTTP request and rebalance overhead.
    pub dynamic_split_min_remaining_mb: u64,

    // ── History ──────────────────────────────────────────────────────
    /// Number of days history entries are retained before automatic
    /// hard-delete. `0` disables retention (entries are kept forever).
    /// PRD §6.8 / §7.9 — hard delete decision (privacy by default).
    pub history_retention_days: i64,

    // ── Accounts ─────────────────────────────────────────────────────
    /// Strategy used by `AccountSelector` when several accounts exist
    /// for the same service. PRD §6.4 — "Auto-select du meilleur
    /// compte disponible".
    pub account_selection_strategy: AccountSelectionStrategy,

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
            dynamic_split_enabled: true,
            dynamic_split_min_remaining_mb: 4,

            // History
            history_retention_days: 30,

            // Accounts
            account_selection_strategy: AccountSelectionStrategy::DEFAULT,

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
    pub dynamic_split_enabled: Option<bool>,
    pub dynamic_split_min_remaining_mb: Option<u64>,

    // History
    pub history_retention_days: Option<i64>,

    // Accounts
    pub account_selection_strategy: Option<AccountSelectionStrategy>,

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

/// Lower bound for `max_concurrent_downloads` accepted by the scheduler.
pub const MIN_MAX_CONCURRENT_DOWNLOADS: u32 = 1;

/// Upper bound for `max_concurrent_downloads` accepted by the scheduler.
pub const MAX_MAX_CONCURRENT_DOWNLOADS: u32 = 20;

/// Allowed retention values exposed in the Settings UI dropdown
/// (PRD §6.8: 7j / 30j / 90j / 1an / illimité).
/// `0` means "never purge".
pub const HISTORY_RETENTION_PRESETS_DAYS: [i64; 5] = [0, 7, 30, 90, 365];

/// Sanitize a persisted `history_retention_days` value.
///
/// Negative values (corrupted/hand-edited) collapse to `0` (= unlimited),
/// matching the privacy-safe default of "no spurious deletes".
pub fn normalize_history_retention_days(raw: i64) -> i64 {
    raw.max(0)
}

/// Clamp a persisted `max_concurrent_downloads` to the queue scheduler's
/// valid range and convert to `usize`.
///
/// Guards against corrupt/manually-edited config values (e.g. `0`, which
/// would stall the queue) or out-of-range values. Used on both the
/// startup path and the runtime `SettingsUpdated` bridge so both honour
/// the same policy.
pub fn normalize_max_concurrent(raw: u32) -> usize {
    raw.clamp(MIN_MAX_CONCURRENT_DOWNLOADS, MAX_MAX_CONCURRENT_DOWNLOADS) as usize
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
    if let Some(v) = patch.dynamic_split_enabled {
        config.dynamic_split_enabled = v;
    }
    if let Some(v) = patch.dynamic_split_min_remaining_mb {
        config.dynamic_split_min_remaining_mb = v;
    }

    // History
    if let Some(v) = patch.history_retention_days {
        config.history_retention_days = normalize_history_retention_days(v);
    }

    // Accounts
    if let Some(v) = patch.account_selection_strategy {
        config.account_selection_strategy = v;
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

        // Remote access — protocols enabled by PRD, but the gatekeeper
        // (`web_interface_enabled`) stays off and `api_key` empty in the
        // domain. The adapter layer is responsible for hydrating a generated
        // key on first launch; we lock the bare-domain defaults here so a
        // future change cannot accidentally expose the server with no auth.
        assert!(!config.web_interface_enabled);
        assert_eq!(config.web_interface_port, 9876);
        assert!(config.rest_api_enabled);
        assert!(config.websocket_enabled);
        assert!(config.api_key.is_empty());
    }

    #[test]
    fn test_default_dynamic_split_enabled_and_min_remaining() {
        let c = AppConfig::default();
        assert!(
            c.dynamic_split_enabled,
            "PRD §7.1: dynamic split on by default"
        );
        assert_eq!(c.dynamic_split_min_remaining_mb, 4);
    }

    #[test]
    fn test_apply_patch_updates_dynamic_split_fields() {
        let mut config = AppConfig::default();
        let patch = ConfigPatch {
            dynamic_split_enabled: Some(false),
            dynamic_split_min_remaining_mb: Some(16),
            ..Default::default()
        };
        apply_patch(&mut config, &patch);
        assert!(!config.dynamic_split_enabled);
        assert_eq!(config.dynamic_split_min_remaining_mb, 16);
    }

    #[test]
    fn test_normalize_max_concurrent_clamps_zero_to_min() {
        assert_eq!(
            normalize_max_concurrent(0),
            MIN_MAX_CONCURRENT_DOWNLOADS as usize
        );
    }

    #[test]
    fn test_normalize_max_concurrent_preserves_in_range_values() {
        assert_eq!(normalize_max_concurrent(1), 1);
        assert_eq!(normalize_max_concurrent(4), 4);
        assert_eq!(normalize_max_concurrent(20), 20);
    }

    #[test]
    fn test_normalize_max_concurrent_clamps_above_range_to_max() {
        assert_eq!(
            normalize_max_concurrent(21),
            MAX_MAX_CONCURRENT_DOWNLOADS as usize
        );
        assert_eq!(
            normalize_max_concurrent(u32::MAX),
            MAX_MAX_CONCURRENT_DOWNLOADS as usize
        );
    }

    #[test]
    fn test_default_history_retention_is_thirty_days() {
        // PRD §6.8 lists 7/30/90/365/illimité.  30j is a privacy-safe
        // middle ground and matches existing defaults seen in similar
        // download managers.
        assert_eq!(AppConfig::default().history_retention_days, 30);
    }

    #[test]
    fn test_apply_patch_updates_history_retention_days() {
        let mut config = AppConfig::default();
        let patch = ConfigPatch {
            history_retention_days: Some(90),
            ..Default::default()
        };
        apply_patch(&mut config, &patch);
        assert_eq!(config.history_retention_days, 90);
    }

    #[test]
    fn test_apply_patch_can_set_history_retention_to_zero_unlimited() {
        let mut config = AppConfig {
            history_retention_days: 30,
            ..AppConfig::default()
        };
        let patch = ConfigPatch {
            history_retention_days: Some(0),
            ..Default::default()
        };
        apply_patch(&mut config, &patch);
        assert_eq!(config.history_retention_days, 0);
    }

    #[test]
    fn test_apply_patch_clamps_negative_history_retention_to_zero() {
        // A crafted IPC/REST payload could send a negative value;
        // `apply_patch` must normalize it instead of trusting the input.
        let mut config = AppConfig::default();
        let patch = ConfigPatch {
            history_retention_days: Some(-99),
            ..Default::default()
        };
        apply_patch(&mut config, &patch);
        assert_eq!(config.history_retention_days, 0);
    }

    #[test]
    fn test_normalize_history_retention_days_clamps_negatives_to_zero() {
        assert_eq!(normalize_history_retention_days(-1), 0);
        assert_eq!(normalize_history_retention_days(i64::MIN), 0);
    }

    #[test]
    fn test_default_account_selection_strategy_is_best_traffic() {
        assert_eq!(
            AppConfig::default().account_selection_strategy,
            AccountSelectionStrategy::BestTraffic
        );
    }

    #[test]
    fn test_apply_patch_updates_account_selection_strategy() {
        let mut config = AppConfig::default();
        let patch = ConfigPatch {
            account_selection_strategy: Some(AccountSelectionStrategy::RoundRobin),
            ..Default::default()
        };
        apply_patch(&mut config, &patch);
        assert_eq!(
            config.account_selection_strategy,
            AccountSelectionStrategy::RoundRobin
        );
    }

    #[test]
    fn test_normalize_history_retention_days_passes_through_non_negative() {
        for &v in &HISTORY_RETENTION_PRESETS_DAYS {
            assert_eq!(normalize_history_retention_days(v), v);
        }
        assert_eq!(normalize_history_retention_days(7), 7);
        assert_eq!(normalize_history_retention_days(i64::MAX), i64::MAX);
    }
}
