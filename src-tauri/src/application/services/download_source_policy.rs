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
    if matches!(
        module_name,
        "builtin-http" | "core-http" | "http" | "magnet"
    ) {
        return Ok(PersistedSourceKind::Direct);
    }
    let loaded = plugins
        .list_loaded()?
        .into_iter()
        .find(|info| info.name() == module_name);
    let info = match loaded {
        Some(info) => Some(info),
        None => plugins.find_installed_manifest(module_name)?,
    };
    let info = info.ok_or_else(|| {
        DomainError::NotFound(format!("download plugin '{module_name}' is unavailable"))
    })?;
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

    #[test]
    fn only_hoster_and_debrid_categories_are_protected() {
        assert!(is_protected_plugin_category(PluginCategory::Hoster));
        assert!(is_protected_plugin_category(PluginCategory::Debrid));
        assert!(!is_protected_plugin_category(PluginCategory::Crawler));
    }
}
