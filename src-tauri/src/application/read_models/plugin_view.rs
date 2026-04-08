//! Serializable plugin view DTO for the frontend.

use serde::Serialize;

use crate::domain::model::plugin::PluginInfo;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginViewDto {
    pub name: String,
    pub version: String,
    pub description: String,
    pub author: String,
    pub category: String,
    pub enabled: bool,
}

impl From<PluginInfo> for PluginViewDto {
    fn from(p: PluginInfo) -> Self {
        Self {
            name: p.name().to_string(),
            version: p.version().to_string(),
            description: p.description().to_string(),
            author: p.author().to_string(),
            category: p.category().to_string(),
            enabled: p.is_enabled(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::model::plugin::{PluginCategory, PluginInfo};

    fn make_plugin_info() -> PluginInfo {
        PluginInfo::new(
            "my-plugin".to_string(),
            "2.0.0".to_string(),
            "A sample plugin".to_string(),
            "dev-author".to_string(),
            PluginCategory::Hoster,
        )
    }

    #[test]
    fn test_plugin_view_dto_from_domain() {
        let info = make_plugin_info();
        let dto = PluginViewDto::from(info);
        assert_eq!(dto.name, "my-plugin");
        assert_eq!(dto.version, "2.0.0");
        assert_eq!(dto.description, "A sample plugin");
        assert_eq!(dto.author, "dev-author");
        assert_eq!(dto.category, "Hoster");
        assert!(dto.enabled);
    }

    #[test]
    fn test_plugin_view_dto_serializes_to_camel_case() {
        let dto = PluginViewDto {
            name: "plugin".to_string(),
            version: "1.0.0".to_string(),
            description: "desc".to_string(),
            author: "author".to_string(),
            category: "Utility".to_string(),
            enabled: true,
        };
        let value = serde_json::to_value(&dto).unwrap();
        assert!(value.get("name").is_some());
        assert!(value.get("version").is_some());
        assert!(value.get("description").is_some());
        assert!(value.get("author").is_some());
        assert!(value.get("category").is_some());
        assert!(value.get("enabled").is_some());
    }
}
