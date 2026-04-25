use std::collections::HashMap;
use std::fmt;
use std::str::FromStr;

use crate::domain::error::DomainError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginCategory {
    Crawler,
    Hoster,
    Debrid,
    Container,
    Captcha,
    Extractor,
    Notifier,
    Utility,
}

impl fmt::Display for PluginCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            PluginCategory::Crawler => "Crawler",
            PluginCategory::Hoster => "Hoster",
            PluginCategory::Debrid => "Debrid",
            PluginCategory::Container => "Container",
            PluginCategory::Captcha => "Captcha",
            PluginCategory::Extractor => "Extractor",
            PluginCategory::Notifier => "Notifier",
            PluginCategory::Utility => "Utility",
        };
        write!(f, "{name}")
    }
}

impl FromStr for PluginCategory {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "crawler" => Ok(PluginCategory::Crawler),
            "hoster" => Ok(PluginCategory::Hoster),
            "debrid" => Ok(PluginCategory::Debrid),
            "container" => Ok(PluginCategory::Container),
            "captcha" => Ok(PluginCategory::Captcha),
            "extractor" => Ok(PluginCategory::Extractor),
            "notifier" => Ok(PluginCategory::Notifier),
            "utility" => Ok(PluginCategory::Utility),
            other => Err(format!("unknown plugin category: '{other}'")),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PluginInfo {
    name: String,
    version: String,
    description: String,
    author: String,
    category: PluginCategory,
    enabled: bool,
}

impl PluginInfo {
    pub fn new(
        name: String,
        version: String,
        description: String,
        author: String,
        category: PluginCategory,
    ) -> Self {
        Self {
            name,
            version,
            description,
            author,
            category,
            enabled: true,
        }
    }

    pub fn enable(&mut self) {
        self.enabled = true;
    }

    pub fn disable(&mut self) {
        self.enabled = false;
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn version(&self) -> &str {
        &self.version
    }

    pub fn description(&self) -> &str {
        &self.description
    }

    pub fn author(&self) -> &str {
        &self.author
    }

    pub fn category(&self) -> PluginCategory {
        self.category
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PluginManifest {
    info: PluginInfo,
    capabilities: Vec<String>,
    min_vortex_version: Option<String>,
    config_defaults: HashMap<String, String>,
    config_schema: PluginConfigSchema,
}

impl PluginManifest {
    pub fn new(info: PluginInfo) -> Self {
        Self {
            info,
            capabilities: Vec::new(),
            min_vortex_version: None,
            config_defaults: HashMap::new(),
            config_schema: PluginConfigSchema::new(),
        }
    }

    pub fn with_capabilities(mut self, caps: Vec<String>) -> Self {
        self.capabilities = caps;
        self
    }

    pub fn with_min_version(mut self, v: String) -> Self {
        self.min_vortex_version = Some(v);
        self
    }

    pub fn with_config_defaults(mut self, defaults: HashMap<String, String>) -> Self {
        self.config_defaults = defaults;
        self
    }

    pub fn with_config_schema(mut self, schema: PluginConfigSchema) -> Self {
        self.config_schema = schema;
        self
    }

    pub fn info(&self) -> &PluginInfo {
        &self.info
    }

    pub fn capabilities(&self) -> &[String] {
        &self.capabilities
    }

    pub fn has_capability(&self, cap: &str) -> bool {
        self.capabilities.iter().any(|c| c == cap)
    }

    pub fn min_vortex_version(&self) -> Option<&str> {
        self.min_vortex_version.as_deref()
    }

    pub fn config_defaults(&self) -> &HashMap<String, String> {
        &self.config_defaults
    }

    pub fn config_schema(&self) -> &PluginConfigSchema {
        &self.config_schema
    }
}

/// Type tag of a single configuration field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigFieldType {
    String,
    Boolean,
    Integer,
    Float,
    Url,
    Enum,
    Array,
}

impl fmt::Display for ConfigFieldType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            ConfigFieldType::String => "string",
            ConfigFieldType::Boolean => "boolean",
            ConfigFieldType::Integer => "integer",
            ConfigFieldType::Float => "float",
            ConfigFieldType::Url => "url",
            ConfigFieldType::Enum => "enum",
            ConfigFieldType::Array => "array",
        };
        write!(f, "{name}")
    }
}

impl FromStr for ConfigFieldType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "string" => Ok(ConfigFieldType::String),
            "boolean" | "bool" => Ok(ConfigFieldType::Boolean),
            "integer" | "int" => Ok(ConfigFieldType::Integer),
            "float" | "number" => Ok(ConfigFieldType::Float),
            "url" => Ok(ConfigFieldType::Url),
            "enum" => Ok(ConfigFieldType::Enum),
            "array" => Ok(ConfigFieldType::Array),
            other => Err(format!("unknown config field type: '{other}'")),
        }
    }
}

/// One configuration field declared by a plugin's `[config]` table.
///
/// Values are encoded as strings on the wire (matching the host's
/// `plugin_configs` storage). [`ConfigField::validate`] is the single
/// source of truth — UI hints are derived from the field metadata but
/// the backend re-validates before persisting.
#[derive(Debug, Clone, PartialEq)]
pub struct ConfigField {
    field_type: ConfigFieldType,
    default: Option<String>,
    description: Option<String>,
    options: Vec<String>,
    min: Option<f64>,
    max: Option<f64>,
    regex: Option<String>,
}

impl ConfigField {
    pub fn new(field_type: ConfigFieldType) -> Self {
        Self {
            field_type,
            default: None,
            description: None,
            options: Vec::new(),
            min: None,
            max: None,
            regex: None,
        }
    }

    pub fn with_default(mut self, default: impl Into<String>) -> Self {
        self.default = Some(default.into());
        self
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn with_options(mut self, options: Vec<String>) -> Self {
        self.options = options;
        self
    }

    pub fn with_min(mut self, min: f64) -> Self {
        self.min = Some(min);
        self
    }

    pub fn with_max(mut self, max: f64) -> Self {
        self.max = Some(max);
        self
    }

    pub fn with_regex(mut self, regex: impl Into<String>) -> Self {
        self.regex = Some(regex.into());
        self
    }

    pub fn field_type(&self) -> ConfigFieldType {
        self.field_type
    }

    pub fn default_value(&self) -> Option<&str> {
        self.default.as_deref()
    }

    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    pub fn options(&self) -> &[String] {
        &self.options
    }

    pub fn min(&self) -> Option<f64> {
        self.min
    }

    pub fn max(&self) -> Option<f64> {
        self.max
    }

    pub fn regex(&self) -> Option<&str> {
        self.regex.as_deref()
    }

    pub fn validate(&self, value: &str) -> Result<(), DomainError> {
        match self.field_type {
            ConfigFieldType::Boolean => {
                if value != "true" && value != "false" {
                    return Err(DomainError::ValidationError(format!(
                        "expected 'true' or 'false', got '{value}'"
                    )));
                }
            }
            ConfigFieldType::Integer => {
                let parsed: i64 = value.parse().map_err(|_| {
                    DomainError::ValidationError(format!("expected integer, got '{value}'"))
                })?;
                self.check_numeric_bounds(parsed as f64)?;
            }
            ConfigFieldType::Float => {
                let parsed: f64 = value.parse().map_err(|_| {
                    DomainError::ValidationError(format!("expected float, got '{value}'"))
                })?;
                self.check_numeric_bounds(parsed)?;
            }
            ConfigFieldType::Url => {
                if !value.starts_with("http://") && !value.starts_with("https://") {
                    return Err(DomainError::ValidationError(format!(
                        "expected http(s) URL, got '{value}'"
                    )));
                }
            }
            ConfigFieldType::Enum => {
                if !self.options.iter().any(|o| o == value) {
                    return Err(DomainError::ValidationError(format!(
                        "value '{value}' not in allowed options"
                    )));
                }
            }
            ConfigFieldType::String => {
                if !self.options.is_empty() && !self.options.iter().any(|o| o == value) {
                    return Err(DomainError::ValidationError(format!(
                        "value '{value}' not in allowed options"
                    )));
                }
            }
            ConfigFieldType::Array => {
                let trimmed = value.trim();
                if !trimmed.starts_with('[') || !trimmed.ends_with(']') {
                    return Err(DomainError::ValidationError(format!(
                        "expected JSON array, got '{value}'"
                    )));
                }
            }
        }

        if let Some(pattern) = &self.regex
            && !match_regex(pattern, value)
        {
            return Err(DomainError::ValidationError(format!(
                "value '{value}' does not match regex"
            )));
        }

        Ok(())
    }

    fn check_numeric_bounds(&self, n: f64) -> Result<(), DomainError> {
        if let Some(min) = self.min
            && n < min
        {
            return Err(DomainError::ValidationError(format!(
                "value {n} below minimum {min}"
            )));
        }
        if let Some(max) = self.max
            && n > max
        {
            return Err(DomainError::ValidationError(format!(
                "value {n} above maximum {max}"
            )));
        }
        Ok(())
    }
}

/// Schema describing every configurable field of a plugin.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PluginConfigSchema {
    fields: HashMap<String, ConfigField>,
}

impl PluginConfigSchema {
    pub fn new() -> Self {
        Self {
            fields: HashMap::new(),
        }
    }

    pub fn insert(&mut self, key: impl Into<String>, field: ConfigField) {
        self.fields.insert(key.into(), field);
    }

    pub fn get(&self, key: &str) -> Option<&ConfigField> {
        self.fields.get(key)
    }

    pub fn fields(&self) -> &HashMap<String, ConfigField> {
        &self.fields
    }

    pub fn is_empty(&self) -> bool {
        self.fields.is_empty()
    }

    pub fn len(&self) -> usize {
        self.fields.len()
    }

    pub fn validate(&self, key: &str, value: &str) -> Result<(), DomainError> {
        let field = self.fields.get(key).ok_or_else(|| {
            DomainError::NotFound(format!("config key '{key}' not declared by plugin"))
        })?;
        field.validate(value)
    }
}

/// Minimal POSIX-like regex matcher built with std only.
///
/// Domain layer constraint: no external crate. Supports anchors (`^`, `$`),
/// wildcard (`.`), char classes (`[a-z]`, `[^abc]`), greedy quantifiers
/// (`*`, `+`, `?`) and escapes (`\d`, `\w`, `\s`, `\.`). Sufficient for the
/// validation patterns declared by community plugins. Returns `false` on
/// malformed patterns rather than panicking.
fn match_regex(pattern: &str, value: &str) -> bool {
    let pat: Vec<char> = pattern.chars().collect();
    let val: Vec<char> = value.chars().collect();

    let (anchor_start, body) = if pat.first() == Some(&'^') {
        (true, &pat[1..])
    } else {
        (false, &pat[..])
    };
    let (anchor_end, body) = if body.last() == Some(&'$') {
        (true, &body[..body.len() - 1])
    } else {
        (false, body)
    };

    if anchor_start {
        regex_match_from(body, &val, 0, anchor_end)
    } else {
        for start in 0..=val.len() {
            if regex_match_from(body, &val, start, anchor_end) {
                return true;
            }
        }
        false
    }
}

fn regex_match_from(pat: &[char], val: &[char], start: usize, anchor_end: bool) -> bool {
    let mut pi = 0;
    let mut vi = start;

    while pi < pat.len() {
        let (atom_pat, atom_len) = parse_atom(&pat[pi..]);
        let next_pi = pi + atom_len;
        let quantifier = pat.get(next_pi).copied();

        match quantifier {
            Some('*') => {
                let mut matches = vi;
                while matches < val.len() && atom_match(&atom_pat, val[matches]) {
                    matches += 1;
                }
                loop {
                    if regex_match_from(&pat[next_pi + 1..], val, matches, anchor_end) {
                        return true;
                    }
                    if matches == vi {
                        return false;
                    }
                    matches -= 1;
                }
            }
            Some('+') => {
                if vi >= val.len() || !atom_match(&atom_pat, val[vi]) {
                    return false;
                }
                let mut matches = vi + 1;
                while matches < val.len() && atom_match(&atom_pat, val[matches]) {
                    matches += 1;
                }
                while matches > vi {
                    if regex_match_from(&pat[next_pi + 1..], val, matches, anchor_end) {
                        return true;
                    }
                    matches -= 1;
                }
                return false;
            }
            Some('?') => {
                if vi < val.len()
                    && atom_match(&atom_pat, val[vi])
                    && regex_match_from(&pat[next_pi + 1..], val, vi + 1, anchor_end)
                {
                    return true;
                }
                return regex_match_from(&pat[next_pi + 1..], val, vi, anchor_end);
            }
            _ => {
                if vi >= val.len() || !atom_match(&atom_pat, val[vi]) {
                    return false;
                }
                vi += 1;
                pi = next_pi;
            }
        }
    }

    if anchor_end { vi == val.len() } else { true }
}

#[derive(Debug, Clone)]
enum Atom {
    Any,
    Literal(char),
    Class(Vec<(char, char)>, bool),
    Digit,
    Word,
    Space,
}

fn parse_atom(pat: &[char]) -> (Atom, usize) {
    if pat.is_empty() {
        return (Atom::Any, 0);
    }
    match pat[0] {
        '.' => (Atom::Any, 1),
        '\\' => {
            if pat.len() < 2 {
                return (Atom::Literal('\\'), 1);
            }
            let atom = match pat[1] {
                'd' => Atom::Digit,
                'w' => Atom::Word,
                's' => Atom::Space,
                c => Atom::Literal(c),
            };
            (atom, 2)
        }
        '[' => {
            let mut i = 1;
            let mut ranges = Vec::new();
            let negate = pat.get(1) == Some(&'^');
            if negate {
                i = 2;
            }
            while i < pat.len() && pat[i] != ']' {
                let start = pat[i];
                if i + 2 < pat.len() && pat[i + 1] == '-' && pat[i + 2] != ']' {
                    ranges.push((start, pat[i + 2]));
                    i += 3;
                } else {
                    ranges.push((start, start));
                    i += 1;
                }
            }
            (Atom::Class(ranges, negate), i + 1)
        }
        c => (Atom::Literal(c), 1),
    }
}

fn atom_match(atom: &Atom, c: char) -> bool {
    match atom {
        Atom::Any => true,
        Atom::Literal(l) => *l == c,
        Atom::Class(ranges, negate) => {
            let inside = ranges.iter().any(|(a, b)| c >= *a && c <= *b);
            inside != *negate
        }
        Atom::Digit => c.is_ascii_digit(),
        Atom::Word => c.is_ascii_alphanumeric() || c == '_',
        Atom::Space => c.is_whitespace(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_info() -> PluginInfo {
        PluginInfo::new(
            "test-plugin".to_string(),
            "1.0.0".to_string(),
            "A test plugin".to_string(),
            "author".to_string(),
            PluginCategory::Hoster,
        )
    }

    #[test]
    fn test_plugin_info_new() {
        let info = make_info();
        assert_eq!(info.name(), "test-plugin");
        assert_eq!(info.version(), "1.0.0");
        assert_eq!(info.description(), "A test plugin");
        assert_eq!(info.author(), "author");
        assert_eq!(info.category(), PluginCategory::Hoster);
        assert!(info.is_enabled());
    }

    #[test]
    fn test_plugin_info_enable_disable() {
        let mut info = make_info();
        assert!(info.is_enabled());
        info.disable();
        assert!(!info.is_enabled());
        info.enable();
        assert!(info.is_enabled());
    }

    #[test]
    fn test_plugin_manifest_capabilities() {
        let info = make_info();
        let manifest = PluginManifest::new(info)
            .with_capabilities(vec!["http".to_string(), "extract".to_string()])
            .with_min_version("0.5.0".to_string());

        assert_eq!(manifest.capabilities(), &["http", "extract"]);
        assert_eq!(manifest.min_vortex_version(), Some("0.5.0"));
    }

    #[test]
    fn test_plugin_manifest_has_capability() {
        let info = make_info();
        let manifest = PluginManifest::new(info)
            .with_capabilities(vec!["http".to_string(), "extract".to_string()]);

        assert!(manifest.has_capability("http"));
        assert!(manifest.has_capability("extract"));
        assert!(!manifest.has_capability("ftp"));
    }

    #[test]
    fn test_plugin_manifest_config_defaults() {
        let info = make_info();
        let defaults = HashMap::from([
            ("default_quality".to_string(), "720p".to_string()),
            ("extract_audio_only".to_string(), "false".to_string()),
        ]);
        let manifest = PluginManifest::new(info).with_config_defaults(defaults.clone());

        assert_eq!(manifest.config_defaults(), &defaults);
    }

    #[test]
    fn test_plugin_category_display() {
        assert_eq!(PluginCategory::Crawler.to_string(), "Crawler");
        assert_eq!(PluginCategory::Hoster.to_string(), "Hoster");
        assert_eq!(PluginCategory::Debrid.to_string(), "Debrid");
        assert_eq!(PluginCategory::Container.to_string(), "Container");
        assert_eq!(PluginCategory::Captcha.to_string(), "Captcha");
        assert_eq!(PluginCategory::Extractor.to_string(), "Extractor");
        assert_eq!(PluginCategory::Notifier.to_string(), "Notifier");
        assert_eq!(PluginCategory::Utility.to_string(), "Utility");
    }

    #[test]
    fn test_config_field_type_from_str_known() {
        assert_eq!(
            "string".parse::<ConfigFieldType>().unwrap(),
            ConfigFieldType::String
        );
        assert_eq!(
            "boolean".parse::<ConfigFieldType>().unwrap(),
            ConfigFieldType::Boolean
        );
        assert_eq!(
            "integer".parse::<ConfigFieldType>().unwrap(),
            ConfigFieldType::Integer
        );
        assert_eq!(
            "float".parse::<ConfigFieldType>().unwrap(),
            ConfigFieldType::Float
        );
        assert_eq!(
            "url".parse::<ConfigFieldType>().unwrap(),
            ConfigFieldType::Url
        );
        assert_eq!(
            "enum".parse::<ConfigFieldType>().unwrap(),
            ConfigFieldType::Enum
        );
    }

    #[test]
    fn test_config_field_type_from_str_unknown_returns_err() {
        assert!("unknown".parse::<ConfigFieldType>().is_err());
    }

    #[test]
    fn test_config_field_type_display_lowercase() {
        assert_eq!(ConfigFieldType::String.to_string(), "string");
        assert_eq!(ConfigFieldType::Boolean.to_string(), "boolean");
        assert_eq!(ConfigFieldType::Integer.to_string(), "integer");
    }

    #[test]
    fn test_config_field_validate_boolean_accepts_true_false() {
        let f = ConfigField::new(ConfigFieldType::Boolean);
        assert!(f.validate("true").is_ok());
        assert!(f.validate("false").is_ok());
    }

    #[test]
    fn test_config_field_validate_boolean_rejects_other() {
        let f = ConfigField::new(ConfigFieldType::Boolean);
        let err = f.validate("yes").unwrap_err();
        assert!(matches!(err, DomainError::ValidationError(_)));
    }

    #[test]
    fn test_config_field_validate_integer_parses_and_checks_bounds() {
        let f = ConfigField::new(ConfigFieldType::Integer)
            .with_min(1.0)
            .with_max(10.0);
        assert!(f.validate("5").is_ok());
        assert!(matches!(
            f.validate("abc").unwrap_err(),
            DomainError::ValidationError(_)
        ));
        assert!(matches!(
            f.validate("0").unwrap_err(),
            DomainError::ValidationError(_)
        ));
        assert!(matches!(
            f.validate("11").unwrap_err(),
            DomainError::ValidationError(_)
        ));
    }

    #[test]
    fn test_config_field_validate_float_with_bounds() {
        let f = ConfigField::new(ConfigFieldType::Float)
            .with_min(0.0)
            .with_max(1.0);
        assert!(f.validate("0.5").is_ok());
        assert!(f.validate("1.5").is_err());
        assert!(f.validate("-0.5").is_err());
    }

    #[test]
    fn test_config_field_validate_url_requires_http_scheme() {
        let f = ConfigField::new(ConfigFieldType::Url);
        assert!(f.validate("https://example.com").is_ok());
        assert!(f.validate("http://example.com").is_ok());
        assert!(f.validate("ftp://example.com").is_err());
        assert!(f.validate("not-a-url").is_err());
    }

    #[test]
    fn test_config_field_validate_enum_checks_options() {
        let f = ConfigField::new(ConfigFieldType::Enum).with_options(vec![
            "360p".to_string(),
            "720p".to_string(),
            "1080p".to_string(),
        ]);
        assert!(f.validate("720p").is_ok());
        assert!(f.validate("4K").is_err());
    }

    #[test]
    fn test_config_field_validate_string_with_options_acts_as_enum() {
        let f = ConfigField::new(ConfigFieldType::String)
            .with_options(vec!["fast".to_string(), "slow".to_string()]);
        assert!(f.validate("fast").is_ok());
        assert!(f.validate("medium").is_err());
    }

    #[test]
    fn test_config_field_validate_string_without_options_accepts_anything() {
        let f = ConfigField::new(ConfigFieldType::String);
        assert!(f.validate("anything goes").is_ok());
        assert!(f.validate("").is_ok());
    }

    #[test]
    fn test_config_field_validate_regex_constrains_string() {
        let f = ConfigField::new(ConfigFieldType::String).with_regex(r"^[a-z]+$");
        assert!(f.validate("hello").is_ok());
        assert!(f.validate("Hello").is_err());
        assert!(f.validate("hello123").is_err());
    }

    #[test]
    fn test_config_field_default_value_optional() {
        let f = ConfigField::new(ConfigFieldType::String).with_default("hi");
        assert_eq!(f.default_value(), Some("hi"));
        let g = ConfigField::new(ConfigFieldType::Integer);
        assert!(g.default_value().is_none());
    }

    #[test]
    fn test_plugin_config_schema_insert_and_get() {
        let mut schema = PluginConfigSchema::new();
        assert!(schema.is_empty());
        schema.insert(
            "quality",
            ConfigField::new(ConfigFieldType::Enum)
                .with_options(vec!["360p".into(), "720p".into()])
                .with_default("720p"),
        );
        assert!(!schema.is_empty());
        assert_eq!(schema.len(), 1);
        let field = schema.get("quality").unwrap();
        assert_eq!(field.field_type(), ConfigFieldType::Enum);
        assert_eq!(field.default_value(), Some("720p"));
    }

    #[test]
    fn test_plugin_config_schema_validate_unknown_key_returns_not_found() {
        let schema = PluginConfigSchema::new();
        let err = schema.validate("ghost", "v").unwrap_err();
        assert!(matches!(err, DomainError::NotFound(_)));
    }

    #[test]
    fn test_plugin_config_schema_validate_delegates_to_field() {
        let mut schema = PluginConfigSchema::new();
        schema.insert("audio", ConfigField::new(ConfigFieldType::Boolean));
        assert!(schema.validate("audio", "true").is_ok());
        assert!(matches!(
            schema.validate("audio", "yes").unwrap_err(),
            DomainError::ValidationError(_)
        ));
    }

    #[test]
    fn test_plugin_manifest_with_config_schema() {
        let info = make_info();
        let mut schema = PluginConfigSchema::new();
        schema.insert("foo", ConfigField::new(ConfigFieldType::String));
        let manifest = PluginManifest::new(info).with_config_schema(schema);
        assert_eq!(manifest.config_schema().len(), 1);
        assert!(manifest.config_schema().get("foo").is_some());
    }

    #[test]
    fn test_plugin_manifest_default_config_schema_empty() {
        let manifest = PluginManifest::new(make_info());
        assert!(manifest.config_schema().is_empty());
    }
}
