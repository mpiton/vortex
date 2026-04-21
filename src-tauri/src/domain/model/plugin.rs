use std::collections::HashMap;
use std::fmt;
use std::str::FromStr;

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
}

impl PluginManifest {
    pub fn new(info: PluginInfo) -> Self {
        Self {
            info,
            capabilities: Vec::new(),
            min_vortex_version: None,
            config_defaults: HashMap::new(),
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
}
