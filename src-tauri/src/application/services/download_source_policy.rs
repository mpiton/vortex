//! Classifies persisted download modules before network access.

use crate::domain::error::DomainError;
use crate::domain::model::plugin::PluginCategory;
use crate::domain::ports::driven::PluginLoader;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PersistedSourceKind {
    Direct,
    Protected,
}

impl PersistedSourceKind {
    pub(crate) fn is_protected(self) -> bool {
        self == Self::Protected
    }
}

pub(crate) fn classify_download_module(
    plugins: &dyn PluginLoader,
    module_name: Option<&str>,
) -> Result<PersistedSourceKind, DomainError> {
    let Some(module_name) = module_name else {
        return Ok(PersistedSourceKind::Direct);
    };
    let loaded = plugins
        .list_loaded()?
        .into_iter()
        .find(|info| info.name() == module_name);
    let info = match loaded {
        Some(info) => Some(info),
        None => plugins.find_installed_manifest(module_name)?,
    };
    let Some(info) = info else {
        return if matches!(
            module_name,
            "builtin-http" | "core-http" | "http" | "magnet"
        ) {
            Ok(PersistedSourceKind::Direct)
        } else {
            Err(DomainError::NotFound(format!(
                "download plugin '{module_name}' is unavailable"
            )))
        };
    };
    Ok(if is_protected_plugin_category(info.category()) {
        PersistedSourceKind::Protected
    } else {
        PersistedSourceKind::Direct
    })
}

pub(crate) fn is_protected_plugin_category(category: PluginCategory) -> bool {
    matches!(category, PluginCategory::Hoster | PluginCategory::Debrid)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::model::plugin::{PluginInfo, PluginManifest};

    struct NamedPluginLoader(PluginInfo);

    impl PluginLoader for NamedPluginLoader {
        fn load(&self, _: &PluginManifest) -> Result<(), DomainError> {
            Ok(())
        }

        fn unload(&self, _: &str) -> Result<(), DomainError> {
            Ok(())
        }

        fn resolve_url(&self, _: &str) -> Result<Option<PluginInfo>, DomainError> {
            Ok(None)
        }

        fn list_loaded(&self) -> Result<Vec<PluginInfo>, DomainError> {
            Ok(vec![self.0.clone()])
        }

        fn set_enabled(&self, _: &str, _: bool) -> Result<(), DomainError> {
            Ok(())
        }
    }

    #[test]
    fn only_hoster_and_debrid_categories_are_protected() {
        assert!(is_protected_plugin_category(PluginCategory::Hoster));
        assert!(is_protected_plugin_category(PluginCategory::Debrid));
        assert!(!is_protected_plugin_category(PluginCategory::Crawler));
    }

    #[test]
    fn loaded_hoster_named_like_a_legacy_alias_remains_protected() {
        for name in ["http", "magnet"] {
            let loader = NamedPluginLoader(PluginInfo::new(
                name.into(),
                "1.0.0".into(),
                name.into(),
                "vortex".into(),
                PluginCategory::Hoster,
            ));

            assert_eq!(
                classify_download_module(&loader, Some(name)).unwrap(),
                PersistedSourceKind::Protected,
                "{name}"
            );
        }
    }
}
