//! `plugin_report_broken` command handler.
//!
//! Builds a pre-filled GitHub "new issue" URL for the named plugin and
//! hands it to the [`UrlOpener`](crate::domain::ports::driven::UrlOpener)
//! port. The plugin must declare a `repository_url` in its manifest;
//! otherwise the caller gets a [`AppError::Validation`] explaining what
//! is missing.
//!
//! Diagnostic context (versions, OS, recent logs, URL under test) is
//! supplied by the driving adapter on every call. The handler stays free
//! of any reference to the host process so it remains trivially mockable
//! and platform-agnostic.

use crate::application::command_bus::CommandBus;
use crate::application::error::AppError;
use crate::domain::model::plugin::build_report_broken_url;

impl CommandBus {
    pub async fn handle_report_broken_plugin(
        &self,
        cmd: super::ReportBrokenPluginCommand,
    ) -> Result<String, AppError> {
        let opener = self
            .url_opener_arc()
            .ok_or_else(|| AppError::Plugin("url opener port not configured".to_string()))?;

        // A "broken plugin" is precisely the case where load failed, so
        // `list_loaded` won't see it. Fall back to the on-disk manifest
        // before declaring the plugin unknown.
        let loader = self.plugin_loader();
        let manifest = match loader
            .list_loaded()?
            .into_iter()
            .find(|info| info.name() == cmd.plugin_name)
        {
            Some(info) => info,
            None => loader
                .find_installed_manifest(&cmd.plugin_name)?
                .ok_or_else(|| {
                    AppError::NotFound(format!("plugin '{}' is not installed", cmd.plugin_name))
                })?,
        };

        let repo_url = manifest.repository_url().ok_or_else(|| {
            AppError::Validation(format!(
                "plugin '{}' has no repository_url in its manifest",
                cmd.plugin_name
            ))
        })?;

        let issue_url = build_report_broken_url(
            repo_url,
            manifest.name(),
            manifest.version(),
            &cmd.vortex_version,
            &cmd.os,
            &cmd.log_lines,
            cmd.tested_url.as_deref(),
        )?;

        // Launcher failure must not lose the URL: the frontend uses the
        // returned value as a clipboard fallback when the OS browser is
        // unavailable (no graphical session, broken `xdg-open`, etc.).
        let url_for_browser = issue_url.clone();
        let plugin_name = cmd.plugin_name.clone();
        let join = tokio::task::spawn_blocking(move || opener.open_url(&url_for_browser)).await;
        match join {
            Ok(Ok(())) => {}
            Ok(Err(e)) => tracing::warn!(
                error = %e,
                plugin = %plugin_name,
                "report_broken_plugin: url opener failed; returning URL for clipboard fallback"
            ),
            Err(e) => tracing::warn!(
                error = %e,
                plugin = %plugin_name,
                "report_broken_plugin: url opener task panicked; returning URL for clipboard fallback"
            ),
        }

        Ok(issue_url)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::Path;
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::application::commands::ReportBrokenPluginCommand;
    use crate::domain::error::DomainError;
    use crate::domain::event::DomainEvent;
    use crate::domain::model::archive::{ArchiveEntry, ArchiveFormat, ExtractSummary};
    use crate::domain::model::config::{AppConfig, ConfigPatch};
    use crate::domain::model::credential::Credential;
    use crate::domain::model::download::{Download, DownloadId, DownloadState};
    use crate::domain::model::http::HttpResponse;
    use crate::domain::model::meta::DownloadMeta;
    use crate::domain::model::plugin::{PluginCategory, PluginInfo, PluginManifest};
    use crate::domain::ports::driven::{
        ArchiveExtractor, ClipboardObserver, ConfigStore, CredentialStore, DownloadEngine,
        DownloadRepository, EventBus, FileStorage, HttpClient, PluginLoader, UrlOpener,
    };

    struct Repo;
    impl DownloadRepository for Repo {
        fn find_by_id(&self, _: DownloadId) -> Result<Option<Download>, DomainError> {
            Ok(None)
        }
        fn save(&self, _: &Download) -> Result<(), DomainError> {
            Ok(())
        }
        fn delete(&self, _: DownloadId) -> Result<(), DomainError> {
            Ok(())
        }
        fn find_by_state(&self, _: DownloadState) -> Result<Vec<Download>, DomainError> {
            Ok(vec![])
        }
    }

    struct Engine;
    impl DownloadEngine for Engine {
        fn start(&self, _: &Download) -> Result<(), DomainError> {
            Ok(())
        }
        fn pause(&self, _: DownloadId) -> Result<(), DomainError> {
            Ok(())
        }
        fn resume(&self, _: DownloadId) -> Result<(), DomainError> {
            Ok(())
        }
        fn cancel(&self, _: DownloadId) -> Result<(), DomainError> {
            Ok(())
        }
    }

    struct Bus;
    impl EventBus for Bus {
        fn publish(&self, _: DomainEvent) {}
        fn subscribe(&self, _: Box<dyn Fn(&DomainEvent) + Send + Sync>) {}
    }

    struct FS;
    impl FileStorage for FS {
        fn create_file(&self, _: &Path, _: u64) -> Result<(), DomainError> {
            Ok(())
        }
        fn write_segment(&self, _: &Path, _: u64, _: &[u8]) -> Result<(), DomainError> {
            Ok(())
        }
        fn read_meta(&self, _: &Path) -> Result<Option<DownloadMeta>, DomainError> {
            Ok(None)
        }
        fn write_meta(&self, _: &Path, _: &DownloadMeta) -> Result<(), DomainError> {
            Ok(())
        }
        fn delete_meta(&self, _: &Path) -> Result<(), DomainError> {
            Ok(())
        }
    }

    struct Http;
    impl HttpClient for Http {
        fn head(&self, _: &str) -> Result<HttpResponse, DomainError> {
            Ok(HttpResponse {
                status_code: 200,
                headers: HashMap::new(),
                body: vec![],
            })
        }
        fn get_range(&self, _: &str, _: u64, _: u64) -> Result<Vec<u8>, DomainError> {
            Ok(vec![])
        }
        fn supports_range(&self, _: &str) -> Result<bool, DomainError> {
            Ok(true)
        }
    }

    struct StaticLoader {
        infos: Vec<PluginInfo>,
        installed: Vec<PluginInfo>,
    }
    impl PluginLoader for StaticLoader {
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
            Ok(self.infos.clone())
        }
        fn find_installed_manifest(&self, name: &str) -> Result<Option<PluginInfo>, DomainError> {
            Ok(self.installed.iter().find(|i| i.name() == name).cloned())
        }
        fn set_enabled(&self, _: &str, _: bool) -> Result<(), DomainError> {
            Ok(())
        }
    }

    struct Cfg;
    impl ConfigStore for Cfg {
        fn get_config(&self) -> Result<AppConfig, DomainError> {
            Ok(AppConfig::default())
        }
        fn update_config(&self, _: ConfigPatch) -> Result<AppConfig, DomainError> {
            Ok(AppConfig::default())
        }
    }

    struct Creds;
    impl CredentialStore for Creds {
        fn get(&self, _: &str) -> Result<Option<Credential>, DomainError> {
            Ok(None)
        }
        fn store(&self, _: &str, _: &Credential) -> Result<(), DomainError> {
            Ok(())
        }
        fn delete(&self, _: &str) -> Result<(), DomainError> {
            Ok(())
        }
    }

    struct Clip;
    impl ClipboardObserver for Clip {
        fn start(&self) -> Result<(), DomainError> {
            Ok(())
        }
        fn stop(&self) -> Result<(), DomainError> {
            Ok(())
        }
        fn get_urls(&self) -> Result<Vec<String>, DomainError> {
            Ok(vec![])
        }
    }

    struct Arch;
    impl ArchiveExtractor for Arch {
        fn detect_format(&self, _: &Path) -> Result<Option<ArchiveFormat>, DomainError> {
            Ok(None)
        }
        fn can_extract(&self, _: &Path) -> Result<bool, DomainError> {
            Ok(false)
        }
        fn extract(
            &self,
            _: &Path,
            _: &Path,
            _: Option<&str>,
        ) -> Result<ExtractSummary, DomainError> {
            Ok(ExtractSummary {
                extracted_files: 0,
                extracted_bytes: 0,
                duration_ms: 0,
                warnings: vec![],
            })
        }
        fn list_contents(
            &self,
            _: &Path,
            _: Option<&str>,
        ) -> Result<Vec<ArchiveEntry>, DomainError> {
            Ok(vec![])
        }
        fn detect_segments(
            &self,
            _: &Path,
        ) -> Result<Option<Vec<std::path::PathBuf>>, DomainError> {
            Ok(None)
        }
    }

    struct RecordingUrlOpener {
        opened: Mutex<Vec<String>>,
        result: Mutex<Result<(), DomainError>>,
    }

    impl RecordingUrlOpener {
        fn ok() -> Arc<Self> {
            Arc::new(Self {
                opened: Mutex::new(Vec::new()),
                result: Mutex::new(Ok(())),
            })
        }

        fn failing(err: DomainError) -> Arc<Self> {
            Arc::new(Self {
                opened: Mutex::new(Vec::new()),
                result: Mutex::new(Err(err)),
            })
        }
    }

    impl UrlOpener for RecordingUrlOpener {
        fn open_url(&self, url: &str) -> Result<(), DomainError> {
            self.opened.lock().unwrap().push(url.to_string());
            self.result.lock().unwrap().clone()
        }
    }

    fn build_bus(loader: Arc<StaticLoader>, opener: Option<Arc<dyn UrlOpener>>) -> CommandBus {
        let bus = CommandBus::new(
            Arc::new(Repo),
            Arc::new(Engine),
            Arc::new(Bus),
            Arc::new(FS),
            Arc::new(Http),
            loader,
            Arc::new(Cfg),
            Arc::new(Creds),
            Arc::new(Clip),
            Arc::new(Arch),
            Arc::new(crate::application::test_support::NoopHistoryRepo),
            None,
        );
        match opener {
            Some(o) => bus.with_url_opener(o),
            None => bus,
        }
    }

    fn info_with_repo(name: &str, version: &str, repo: Option<&str>) -> PluginInfo {
        let info = PluginInfo::new(
            name.to_string(),
            version.to_string(),
            "desc".to_string(),
            "author".to_string(),
            PluginCategory::Hoster,
        );
        match repo {
            Some(r) => info.with_repository_url(r),
            None => info,
        }
    }

    fn cmd(plugin_name: &str) -> ReportBrokenPluginCommand {
        ReportBrokenPluginCommand {
            plugin_name: plugin_name.to_string(),
            log_lines: vec!["ERROR: boom".to_string()],
            tested_url: Some("https://example.com/x".to_string()),
            vortex_version: "0.2.0".to_string(),
            os: "linux".to_string(),
        }
    }

    #[tokio::test]
    async fn handle_report_broken_plugin_opens_prefilled_url() {
        let loader = Arc::new(StaticLoader {
            installed: vec![],
            infos: vec![info_with_repo(
                "vortex-mod-youtube",
                "1.2.3",
                Some("https://github.com/mpiton/vortex-mod-youtube"),
            )],
        });
        let opener = RecordingUrlOpener::ok();
        let bus = build_bus(loader, Some(opener.clone() as Arc<dyn UrlOpener>));

        let url = bus
            .handle_report_broken_plugin(cmd("vortex-mod-youtube"))
            .await
            .unwrap();

        let opened = opener.opened.lock().unwrap();
        assert_eq!(opened.len(), 1);
        assert_eq!(opened[0], url);
        assert!(
            url.starts_with("https://github.com/mpiton/vortex-mod-youtube/issues/new?"),
            "unexpected URL: {url}"
        );
        assert!(url.contains("Vortex%3A%200.2.0"));
        assert!(url.contains("OS%3A%20linux"));
        assert!(url.contains("Tested%20URL%3A"));
    }

    #[tokio::test]
    async fn handle_report_broken_plugin_returns_not_found_when_plugin_unknown() {
        let loader = Arc::new(StaticLoader {
            infos: vec![],
            installed: vec![],
        });
        let opener = RecordingUrlOpener::ok();
        let bus = build_bus(loader, Some(opener.clone() as Arc<dyn UrlOpener>));

        let err = bus
            .handle_report_broken_plugin(cmd("vortex-mod-missing"))
            .await
            .unwrap_err();
        assert!(matches!(err, AppError::NotFound(_)), "{err:?}");
        assert!(opener.opened.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn handle_report_broken_plugin_falls_back_to_installed_manifest_when_not_loaded() {
        // The whole point of "broken plugin" is that the plugin failed to
        // load. The handler must still find its manifest on disk.
        let loader = Arc::new(StaticLoader {
            infos: vec![],
            installed: vec![info_with_repo(
                "vortex-mod-broken",
                "0.1.0",
                Some("https://github.com/o/vortex-mod-broken"),
            )],
        });
        let opener = RecordingUrlOpener::ok();
        let bus = build_bus(loader, Some(opener.clone() as Arc<dyn UrlOpener>));

        let url = bus
            .handle_report_broken_plugin(cmd("vortex-mod-broken"))
            .await
            .unwrap();

        assert!(
            url.starts_with("https://github.com/o/vortex-mod-broken/issues/new?"),
            "unexpected URL: {url}"
        );
        let opened = opener.opened.lock().unwrap();
        assert_eq!(opened.len(), 1);
        assert_eq!(opened[0], url);
    }

    #[tokio::test]
    async fn handle_report_broken_plugin_validation_error_when_repo_missing() {
        let loader = Arc::new(StaticLoader {
            installed: vec![],
            infos: vec![info_with_repo("vortex-mod-foo", "1.0.0", None)],
        });
        let opener = RecordingUrlOpener::ok();
        let bus = build_bus(loader, Some(opener.clone() as Arc<dyn UrlOpener>));

        let err = bus
            .handle_report_broken_plugin(cmd("vortex-mod-foo"))
            .await
            .unwrap_err();
        assert!(matches!(err, AppError::Validation(_)), "{err:?}");
        assert!(opener.opened.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn handle_report_broken_plugin_errors_when_opener_port_missing() {
        let loader = Arc::new(StaticLoader {
            installed: vec![],
            infos: vec![info_with_repo(
                "vortex-mod-x",
                "1",
                Some("https://github.com/o/r"),
            )],
        });
        let bus = build_bus(loader, None);

        let err = bus
            .handle_report_broken_plugin(cmd("vortex-mod-x"))
            .await
            .unwrap_err();
        assert!(matches!(err, AppError::Plugin(_)), "{err:?}");
    }

    #[tokio::test]
    async fn handle_report_broken_plugin_returns_url_even_when_launcher_fails() {
        // When the OS launcher fails (no graphical session, broken
        // `xdg-open`, …), the handler must still hand back the issue URL
        // so the frontend can offer a clipboard fallback.
        let loader = Arc::new(StaticLoader {
            installed: vec![],
            infos: vec![info_with_repo(
                "vortex-mod-x",
                "1",
                Some("https://github.com/o/r"),
            )],
        });
        let opener =
            RecordingUrlOpener::failing(DomainError::StorageError("xdg-open failed".into()));
        let bus = build_bus(loader, Some(opener.clone() as Arc<dyn UrlOpener>));

        let url = bus
            .handle_report_broken_plugin(cmd("vortex-mod-x"))
            .await
            .unwrap();

        assert!(
            url.starts_with("https://github.com/o/r/issues/new?"),
            "unexpected URL: {url}"
        );
        let opened = opener.opened.lock().unwrap();
        assert_eq!(opened.len(), 1, "opener should still be invoked once");
        assert_eq!(opened[0], url);
    }

    #[tokio::test]
    async fn handle_report_broken_plugin_rejects_non_github_repo() {
        let loader = Arc::new(StaticLoader {
            installed: vec![],
            infos: vec![info_with_repo(
                "vortex-mod-x",
                "1",
                Some("https://gitlab.com/o/r"),
            )],
        });
        let opener = RecordingUrlOpener::ok();
        let bus = build_bus(loader, Some(opener.clone() as Arc<dyn UrlOpener>));

        let err = bus
            .handle_report_broken_plugin(cmd("vortex-mod-x"))
            .await
            .unwrap_err();
        assert!(
            matches!(err, AppError::Validation(_) | AppError::Domain(_)),
            "{err:?}"
        );
        assert!(opener.opened.lock().unwrap().is_empty());
    }
}
