//! Port for application configuration persistence.
//!
//! Reads and writes `config.toml` (or equivalent) through a
//! typed interface. The adapter handles file I/O and parsing.

use crate::domain::error::DomainError;
use crate::domain::model::config::{AppConfig, ConfigPatch};

/// Reads and updates application configuration.
///
/// The adapter loads from `~/.config/vortex/config.toml`,
/// applies patches, and writes back atomically.
pub trait ConfigStore: Send + Sync {
    /// Get the current application configuration.
    fn get_config(&self) -> Result<AppConfig, DomainError>;

    /// Apply a partial update to the configuration.
    ///
    /// Returns the full updated configuration after applying the patch.
    fn update_config(&self, patch: ConfigPatch) -> Result<AppConfig, DomainError>;
}
