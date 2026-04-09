use std::fmt;

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
}

impl PluginManifest {
    pub fn new(info: PluginInfo) -> Self {
        Self {
            info,
            capabilities: Vec::new(),
            min_vortex_version: None,
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
