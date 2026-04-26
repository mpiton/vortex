//! Persistent storage for plugin configuration values.
//!
//! Each plugin owns a flat (key, value) map; values are encoded as
//! UTF-8 strings (matching the in-memory `plugin_configs` map used by
//! the host functions). The schema lives on the manifest, not here.

use std::collections::HashMap;

use crate::domain::error::DomainError;

/// Persists plugin configuration values across restarts.
///
/// Adapters typically back this with a SQLite table. Implementations
/// must be thread-safe so command handlers can mutate values from
/// concurrent IPC calls.
pub trait PluginConfigStore: Send + Sync {
    /// Read every (key, value) pair recorded for `plugin_name`.
    /// Returns an empty map when the plugin has no persisted overrides.
    fn get_values(&self, plugin_name: &str) -> Result<HashMap<String, String>, DomainError>;

    /// Persist a single (key, value) pair, replacing the previous value
    /// when one exists.
    fn set_value(&self, plugin_name: &str, key: &str, value: &str) -> Result<(), DomainError>;

    /// Read every (plugin_name → key → value) tuple persisted by the
    /// store. Used at startup to repopulate the in-memory `plugin_configs`
    /// map after defaults from the manifest are applied.
    fn list_all(&self) -> Result<HashMap<String, HashMap<String, String>>, DomainError>;

    /// Remove every persisted value for `plugin_name`. Called when a
    /// plugin is uninstalled so the configs do not linger as ghost rows.
    fn delete_all(&self, plugin_name: &str) -> Result<(), DomainError>;
}
