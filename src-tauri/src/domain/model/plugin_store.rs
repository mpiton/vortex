//! Domain types for the plugin store catalogue.

use crate::domain::model::plugin::PluginCategory;

/// A plugin entry as declared in the central registry.
#[derive(Debug, Clone, PartialEq)]
pub struct PluginStoreEntry {
    pub name: String,
    pub description: String,
    pub author: String,
    pub version: String,
    pub category: PluginCategory,
    pub repository: String,
    pub checksum_sha256: String,
    pub checksum_sha256_toml: Option<String>,
    pub official: bool,
    pub min_vortex_version: Option<String>,
    pub status: PluginStoreStatus,
    pub installed_version: Option<String>,
}

/// Installation status of a store entry compared to locally installed plugins.
#[derive(Debug, Clone, PartialEq)]
pub enum PluginStoreStatus {
    NotInstalled,
    /// Installed at the exact version listed in the registry.
    Installed,
    /// Registry has a newer version than what's installed.
    UpdateAvailable,
    /// Installed version is ahead of the registry version (e.g. local dev build).
    Downgrade,
}

/// Parse a `major.minor.patch` version string into a comparable tuple.
fn parse_semver(s: &str) -> Option<(u64, u64, u64)> {
    let mut parts = s.splitn(3, '.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next().unwrap_or("0").parse().ok()?;
    Some((major, minor, patch))
}

impl PluginStoreEntry {
    /// Derive status by comparing installed version against registry version.
    ///
    /// Parses both versions as `major.minor.patch` tuples using pure std — no
    /// external semver crate is needed in the domain layer.
    pub fn with_status(mut self, installed_version: Option<&str>) -> Self {
        self.installed_version = installed_version.map(str::to_string);
        self.status = match installed_version {
            None => PluginStoreStatus::NotInstalled,
            Some(v) if v == self.version => PluginStoreStatus::Installed,
            Some(v) => match (parse_semver(v), parse_semver(&self.version)) {
                (Some(inst), Some(reg)) if inst > reg => PluginStoreStatus::Downgrade,
                _ => PluginStoreStatus::UpdateAvailable,
            },
        };
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::model::plugin::PluginCategory;

    fn entry(version: &str) -> PluginStoreEntry {
        PluginStoreEntry {
            name: "test-plugin".into(),
            description: "A test plugin".into(),
            author: "author".into(),
            version: version.into(),
            category: PluginCategory::Utility,
            repository: "https://github.com/author/test-plugin".into(),
            checksum_sha256: "abc123".into(),
            checksum_sha256_toml: None,
            official: false,
            min_vortex_version: None,
            status: PluginStoreStatus::NotInstalled,
            installed_version: None,
        }
    }

    #[test]
    fn test_with_status_not_installed_when_none() {
        let e = entry("1.0.0").with_status(None);
        assert_eq!(e.status, PluginStoreStatus::NotInstalled);
        assert_eq!(e.installed_version, None);
    }

    #[test]
    fn test_with_status_installed_when_same_version() {
        let e = entry("1.0.0").with_status(Some("1.0.0"));
        assert_eq!(e.status, PluginStoreStatus::Installed);
        assert_eq!(e.installed_version, Some("1.0.0".into()));
    }

    #[test]
    fn test_with_status_update_available_when_different_version() {
        let e = entry("1.1.0").with_status(Some("1.0.0"));
        assert_eq!(e.status, PluginStoreStatus::UpdateAvailable);
        assert_eq!(e.installed_version, Some("1.0.0".into()));
    }

    #[test]
    fn test_with_status_downgrade_when_installed_ahead_of_registry() {
        // installed=2.0.0, registry=1.0.0 → Downgrade
        let e = entry("1.0.0").with_status(Some("2.0.0"));
        assert_eq!(e.status, PluginStoreStatus::Downgrade);
        assert_eq!(e.installed_version, Some("2.0.0".into()));
    }
}
