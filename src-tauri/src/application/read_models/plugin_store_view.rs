//! Serializable plugin store entry DTO for the frontend.

use serde::Serialize;

use crate::domain::model::plugin_store::{PluginStoreEntry, PluginStoreStatus};

// Task 8 (store_list handler) will use this DTO — remove once wired.
#[expect(dead_code)]
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginStoreEntryDto {
    pub name: String,
    pub description: String,
    pub author: String,
    /// Version declared in the registry (latest available).
    pub version: String,
    /// Currently installed version, if any.
    pub installed_version: Option<String>,
    pub category: String,
    pub official: bool,
    /// "not_installed" | "installed" | "update_available"
    pub status: String,
}

impl From<PluginStoreEntry> for PluginStoreEntryDto {
    fn from(e: PluginStoreEntry) -> Self {
        let status = match e.status {
            PluginStoreStatus::NotInstalled => "not_installed",
            PluginStoreStatus::Installed => "installed",
            PluginStoreStatus::UpdateAvailable => "update_available",
        };
        Self {
            name: e.name,
            description: e.description,
            author: e.author,
            version: e.version,
            installed_version: e.installed_version,
            category: e.category.to_string(),
            official: e.official,
            status: status.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::model::plugin::PluginCategory;
    use crate::domain::model::plugin_store::PluginStoreStatus;

    fn make_entry(status: PluginStoreStatus, installed: Option<&str>) -> PluginStoreEntry {
        PluginStoreEntry {
            name: "vortex-mod-gallery".into(),
            description: "Gallery".into(),
            author: "johndoe".into(),
            version: "1.0.0".into(),
            category: PluginCategory::Hoster,
            repository: "https://github.com/johndoe/vortex-mod-gallery".into(),
            checksum_sha256: "abc".into(),
            official: false,
            min_vortex_version: None,
            status,
            installed_version: installed.map(str::to_string),
        }
    }

    #[test]
    fn test_dto_from_not_installed() {
        let dto = PluginStoreEntryDto::from(make_entry(PluginStoreStatus::NotInstalled, None));
        assert_eq!(dto.status, "not_installed");
        assert_eq!(dto.installed_version, None);
        assert_eq!(dto.category, "Hoster");
    }

    #[test]
    fn test_dto_from_installed() {
        let dto =
            PluginStoreEntryDto::from(make_entry(PluginStoreStatus::Installed, Some("1.0.0")));
        assert_eq!(dto.status, "installed");
        assert_eq!(dto.installed_version, Some("1.0.0".into()));
    }

    #[test]
    fn test_dto_from_update_available() {
        let dto = PluginStoreEntryDto::from(make_entry(
            PluginStoreStatus::UpdateAvailable,
            Some("0.9.0"),
        ));
        assert_eq!(dto.status, "update_available");
        assert_eq!(dto.installed_version, Some("0.9.0".into()));
    }

    #[test]
    fn test_dto_serializes_camel_case() {
        let dto = PluginStoreEntryDto::from(make_entry(PluginStoreStatus::NotInstalled, None));
        let v = serde_json::to_value(&dto).unwrap();
        assert!(v.get("installedVersion").is_some());
        assert!(v.get("installed_version").is_none());
    }
}
