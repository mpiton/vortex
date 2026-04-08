//! Read-only port for querying loaded plugins (CQRS read side).
//!
//! Separated from `PluginLoader` to enforce the CQRS boundary:
//! the query bus should not have access to write operations
//! like `load` or `unload`.

use crate::domain::error::DomainError;
use crate::domain::model::plugin::PluginInfo;

/// Read-only view of loaded plugins.
///
/// Used by query handlers to list plugins without exposing
/// mutation capabilities (`load`, `unload`, `resolve_url`).
pub trait PluginReadRepository: Send + Sync {
    /// List all currently loaded plugins.
    fn list_loaded(&self) -> Result<Vec<PluginInfo>, DomainError>;
}
