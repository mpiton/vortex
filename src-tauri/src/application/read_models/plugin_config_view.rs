//! Serializable plugin-configuration view DTO for the frontend.
//!
//! Bundles the schema (so the UI can render typed fields) and the
//! current values (so the UI can populate them) in a single payload.

use std::collections::HashMap;

use serde::Serialize;

use crate::domain::model::plugin::{ConfigField, PluginConfigSchema};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigFieldDto {
    pub key: String,
    pub field_type: String,
    pub default: Option<String>,
    pub description: Option<String>,
    pub options: Vec<String>,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub regex: Option<String>,
}

impl ConfigFieldDto {
    pub fn from_field(key: &str, field: &ConfigField) -> Self {
        Self {
            key: key.to_string(),
            field_type: field.field_type().to_string(),
            default: field.default_value().map(|s| s.to_string()),
            description: field.description().map(|s| s.to_string()),
            options: field.options().to_vec(),
            min: field.min(),
            max: field.max(),
            regex: field.regex().map(|s| s.to_string()),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginConfigView {
    pub fields: Vec<ConfigFieldDto>,
    pub values: HashMap<String, String>,
}

impl PluginConfigView {
    pub fn new(schema: &PluginConfigSchema, values: HashMap<String, String>) -> Self {
        let mut fields: Vec<ConfigFieldDto> = schema
            .fields()
            .iter()
            .map(|(k, f)| ConfigFieldDto::from_field(k, f))
            .collect();
        fields.sort_by(|a, b| a.key.cmp(&b.key));
        Self { fields, values }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::model::plugin::{ConfigField, ConfigFieldType, PluginConfigSchema};

    #[test]
    fn test_plugin_config_view_serializes_camel_case() {
        let mut schema = PluginConfigSchema::new();
        schema.insert(
            "default_quality",
            ConfigField::new(ConfigFieldType::Enum)
                .with_options(vec!["360p".into(), "720p".into()])
                .with_default("720p")
                .with_description("Quality tier"),
        );
        let mut values = HashMap::new();
        values.insert("default_quality".to_string(), "720p".to_string());

        let view = PluginConfigView::new(&schema, values);
        let json = serde_json::to_value(&view).unwrap();
        assert!(json.get("fields").is_some());
        assert!(json.get("values").is_some());
        let field0 = &json.get("fields").unwrap().as_array().unwrap()[0];
        assert_eq!(field0.get("key").unwrap(), "default_quality");
        assert_eq!(field0.get("fieldType").unwrap(), "enum");
        assert_eq!(field0.get("default").unwrap(), "720p");
    }

    #[test]
    fn test_plugin_config_view_fields_sorted_by_key() {
        let mut schema = PluginConfigSchema::new();
        schema.insert("zeta", ConfigField::new(ConfigFieldType::String));
        schema.insert("alpha", ConfigField::new(ConfigFieldType::String));
        schema.insert("mu", ConfigField::new(ConfigFieldType::String));

        let view = PluginConfigView::new(&schema, HashMap::new());
        let keys: Vec<&str> = view.fields.iter().map(|f| f.key.as_str()).collect();
        assert_eq!(keys, vec!["alpha", "mu", "zeta"]);
    }
}
