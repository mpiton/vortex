//! Serializable plugin store entry DTO for the frontend.

use serde::{Deserialize, Serialize};

use crate::domain::model::plugin_store::{PluginStoreEntry, PluginStoreStatus};

/// Task 8 (store_list handler) will use this DTO.
#[derive(Debug, Clone, Serialize, Deserialize)]
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
    /// "not_installed" | "installed" | "update_available" | "downgrade"
    pub status: String,
    pub repository: String,
    pub checksum_sha256: String,
    pub checksum_sha256_toml: Option<String>,
    pub min_vortex_version: Option<String>,
}

impl From<PluginStoreEntry> for PluginStoreEntryDto {
    fn from(e: PluginStoreEntry) -> Self {
        let status = match e.status {
            PluginStoreStatus::NotInstalled => "not_installed",
            PluginStoreStatus::Installed => "installed",
            PluginStoreStatus::UpdateAvailable => "update_available",
            PluginStoreStatus::Downgrade => "downgrade",
        };
        Self {
            name: e.name,
            description: e.description,
            author: e.author,
            version: e.version,
            installed_version: e.installed_version,
            category: e.category.to_string().to_lowercase(),
            official: e.official,
            status: status.to_string(),
            repository: e.repository,
            checksum_sha256: e.checksum_sha256,
            checksum_sha256_toml: e.checksum_sha256_toml,
            min_vortex_version: e.min_vortex_version,
        }
    }
}

impl PluginStoreEntryDto {
    /// Override `installed_version` and re-derive `status` against the
    /// current registry version. Used by `get_plugin_store` to keep the
    /// status in sync with the live loader state without having to rewrite
    /// the on-disk cache after every install/uninstall.
    pub fn enrich_with_installed(&mut self, installed: Option<String>) {
        self.status = derive_status_str(&self.version, installed.as_deref()).to_string();
        self.installed_version = installed;
    }
}

fn derive_status_str(registry_version: &str, installed: Option<&str>) -> &'static str {
    fn parse_semver(s: &str) -> Option<(u64, u64, u64)> {
        // Strip SemVer pre-release (`-foo`) and build-metadata (`+foo`)
        // suffixes before parsing so `2.0.0-dev` and `1.2.0+build.1` still
        // compare on the `MAJOR.MINOR.PATCH` core.
        let core = s.split(['-', '+']).next().unwrap_or(s);
        let mut parts = core.splitn(3, '.');
        let major = parts.next()?.parse().ok()?;
        let minor = parts.next()?.parse().ok()?;
        let patch = parts.next().unwrap_or("0").parse().ok()?;
        Some((major, minor, patch))
    }
    match installed {
        None => "not_installed",
        Some(v) if v == registry_version => "installed",
        Some(v) => match (parse_semver(v), parse_semver(registry_version)) {
            // Two versions that differ only in their pre-release / build
            // suffix (e.g. `1.0.0+build.1` vs `1.0.0`) collapse to the
            // same normalised core — treat them as installed, not as
            // update_available.
            (Some(inst), Some(reg)) if inst == reg => "installed",
            (Some(inst), Some(reg)) if inst > reg => "downgrade",
            _ => "update_available",
        },
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
            checksum_sha256_toml: None,
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
        assert_eq!(dto.category, "hoster");
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
    fn test_dto_from_downgrade() {
        let dto =
            PluginStoreEntryDto::from(make_entry(PluginStoreStatus::Downgrade, Some("2.0.0")));
        assert_eq!(dto.status, "downgrade");
        assert_eq!(dto.installed_version, Some("2.0.0".into()));
    }

    #[test]
    fn test_dto_serializes_camel_case() {
        let dto = PluginStoreEntryDto::from(make_entry(PluginStoreStatus::NotInstalled, None));
        let v = serde_json::to_value(&dto).unwrap();
        assert!(v.get("installedVersion").is_some());
        assert!(v.get("installed_version").is_none());
    }

    #[test]
    fn test_enrich_with_installed_marks_installed_at_same_version() {
        // A cached "not_installed" entry becomes "installed" once the loader
        // reports the plugin at the registry version.
        let mut dto = PluginStoreEntryDto::from(make_entry(PluginStoreStatus::NotInstalled, None));
        dto.enrich_with_installed(Some("1.0.0".into()));
        assert_eq!(dto.status, "installed");
        assert_eq!(dto.installed_version, Some("1.0.0".into()));
    }

    #[test]
    fn test_enrich_with_installed_flags_update_available() {
        // Cached entry version=1.0.0, loader reports 0.9.0 → update_available.
        let mut dto = PluginStoreEntryDto::from(make_entry(PluginStoreStatus::NotInstalled, None));
        dto.enrich_with_installed(Some("0.9.0".into()));
        assert_eq!(dto.status, "update_available");
    }

    #[test]
    fn test_enrich_with_installed_flags_downgrade() {
        // Cached entry version=1.0.0, loader reports 2.0.0 → downgrade
        // (e.g. local dev build ahead of the registry).
        let mut dto = PluginStoreEntryDto::from(make_entry(PluginStoreStatus::NotInstalled, None));
        dto.enrich_with_installed(Some("2.0.0".into()));
        assert_eq!(dto.status, "downgrade");
    }

    #[test]
    fn test_enrich_with_installed_reverts_to_not_installed_when_none() {
        // Loader no longer reports the plugin → status reverts to not_installed
        // even if the cache previously recorded it as installed.
        let mut dto =
            PluginStoreEntryDto::from(make_entry(PluginStoreStatus::Installed, Some("1.0.0")));
        dto.enrich_with_installed(None);
        assert_eq!(dto.status, "not_installed");
        assert_eq!(dto.installed_version, None);
    }

    #[test]
    fn test_enrich_with_installed_handles_prerelease_suffix_as_downgrade() {
        // A dev build tagged `2.0.0-dev` against a registry `1.0.0` must
        // classify as "downgrade" — the pre-release suffix on the patch
        // segment previously broke `parse_semver` and collapsed to
        // "update_available".
        let mut dto = PluginStoreEntryDto::from(make_entry(PluginStoreStatus::NotInstalled, None));
        dto.enrich_with_installed(Some("2.0.0-dev".into()));
        assert_eq!(dto.status, "downgrade");
    }

    #[test]
    fn test_enrich_with_installed_handles_build_metadata_suffix() {
        // Build metadata suffixes (`+build.N`) must also not trip the parser.
        let mut dto = PluginStoreEntryDto::from(make_entry(PluginStoreStatus::NotInstalled, None));
        dto.enrich_with_installed(Some("0.9.0+build.1".into()));
        assert_eq!(dto.status, "update_available");
    }

    #[test]
    fn test_enrich_with_installed_treats_build_metadata_as_installed() {
        // `1.0.0+build.1` and `1.0.0` share the same (major, minor, patch)
        // core — they must be considered installed, not `update_available`,
        // since the build metadata is not part of semantic ordering.
        let mut dto = PluginStoreEntryDto::from(make_entry(PluginStoreStatus::NotInstalled, None));
        dto.enrich_with_installed(Some("1.0.0+build.1".into()));
        assert_eq!(dto.status, "installed");
    }
}
