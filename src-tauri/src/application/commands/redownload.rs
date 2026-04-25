//! Handler for `RedownloadCommand`.
//!
//! Creates a brand-new [`Download`](crate::domain::model::download::Download)
//! aggregate seeded from a previous entry (either a completed download or a
//! history record) and emits `DownloadCreated` so the queue manager can
//! schedule it. Always produces a new [`DownloadId`] — never mutates the
//! source entry.

use crate::application::command_bus::CommandBus;
use crate::application::commands::RedownloadSource;
use crate::application::error::AppError;
use crate::domain::event::DomainEvent;
use crate::domain::model::download::{Download, DownloadId, DownloadState, Url};

impl CommandBus {
    pub async fn handle_redownload(
        &self,
        cmd: super::RedownloadCommand,
    ) -> Result<DownloadId, AppError> {
        let template = self.load_template(&cmd.source)?;

        let url = Url::new(&template.url)?;
        let dest = cmd
            .destination_override
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| template.destination_path.clone());

        let id = super::start_download::next_download_id();
        // Lock against concurrent queue-position allocation; held until
        // after `save` so the read+write is atomic.
        let _guard = self.lock_queue_positions().await;
        let queue_position = super::move_queue::next_queue_position(self.download_repo())?;
        let mut download = Download::new(id, url, template.file_name.clone(), dest)
            .with_queue_position(queue_position);

        if let Some(hostname) = template.source_hostname.clone() {
            download = download.with_source_hostname(hostname);
        }
        if let Some(priority) = template.priority {
            download = download.with_priority(priority);
        }
        if let Some(segments) = template.segments_count {
            download = download.with_segments_count(segments);
        }
        if let Some(module) = template.module_name.clone() {
            download = download.with_module_name(module);
        }
        if let Some(account) = template.account_id {
            download = download.with_account_id(account);
        }

        self.download_repo().save(&download)?;
        self.event_bus()
            .publish(DomainEvent::DownloadCreated { id });

        Ok(id)
    }

    fn load_template(&self, source: &RedownloadSource) -> Result<RedownloadTemplate, AppError> {
        match source {
            RedownloadSource::Download(id) => {
                let download = self
                    .download_repo()
                    .find_by_id(*id)?
                    .ok_or_else(|| AppError::NotFound(format!("Download {} not found", id.0)))?;
                if download.state() != DownloadState::Completed {
                    return Err(AppError::Validation(format!(
                        "download is not completed (current state: {})",
                        download.state()
                    )));
                }
                Ok(RedownloadTemplate {
                    url: download.url().as_str().to_string(),
                    file_name: download.file_name().to_string(),
                    destination_path: download.destination_path().to_string(),
                    source_hostname: Some(download.source_hostname().to_string()),
                    priority: Some(*download.priority()),
                    segments_count: Some(download.segments_count()),
                    module_name: download.module_name().map(str::to_string),
                    account_id: download.account_id(),
                })
            }
            RedownloadSource::History(history_id) => {
                let entry = self
                    .history_repo()
                    .find_by_id(*history_id)?
                    .ok_or_else(|| {
                        AppError::NotFound(format!("History entry {history_id} not found"))
                    })?;
                Ok(RedownloadTemplate {
                    url: entry.url,
                    file_name: entry.file_name,
                    destination_path: entry.destination_path,
                    source_hostname: None,
                    priority: None,
                    segments_count: None,
                    module_name: None,
                    account_id: None,
                })
            }
        }
    }
}

struct RedownloadTemplate {
    url: String,
    file_name: String,
    destination_path: String,
    source_hostname: Option<String>,
    priority: Option<crate::domain::model::Priority>,
    segments_count: Option<u32>,
    module_name: Option<String>,
    account_id: Option<u64>,
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex};

    use crate::application::command_bus::CommandBus;
    use crate::application::commands::{RedownloadCommand, RedownloadSource};
    use crate::application::error::AppError;
    use crate::application::test_support::InMemoryHistoryRepo;
    use crate::domain::error::DomainError;
    use crate::domain::event::DomainEvent;
    use crate::domain::model::Priority;
    use crate::domain::model::config::{AppConfig, ConfigPatch};
    use crate::domain::model::credential::Credential;
    use crate::domain::model::download::{Download, DownloadId, DownloadState, Url};
    use crate::domain::model::http::HttpResponse;
    use crate::domain::model::meta::DownloadMeta;
    use crate::domain::model::plugin::{PluginInfo, PluginManifest};
    use crate::domain::model::views::HistoryEntry;
    use crate::domain::ports::driven::{
        ArchiveExtractor, ClipboardObserver, ConfigStore, CredentialStore, DownloadEngine,
        DownloadRepository, EventBus, FileStorage, HistoryRepository, HttpClient, PluginLoader,
    };

    struct MockDownloadRepo {
        store: Mutex<HashMap<u64, Download>>,
    }

    impl MockDownloadRepo {
        fn new() -> Self {
            Self {
                store: Mutex::new(HashMap::new()),
            }
        }
    }

    impl DownloadRepository for MockDownloadRepo {
        fn find_by_id(&self, id: DownloadId) -> Result<Option<Download>, DomainError> {
            Ok(self.store.lock().unwrap().get(&id.0).cloned())
        }
        fn save(&self, d: &Download) -> Result<(), DomainError> {
            self.store.lock().unwrap().insert(d.id().0, d.clone());
            Ok(())
        }
        fn delete(&self, id: DownloadId) -> Result<(), DomainError> {
            self.store.lock().unwrap().remove(&id.0);
            Ok(())
        }
        fn find_by_state(&self, s: DownloadState) -> Result<Vec<Download>, DomainError> {
            Ok(self
                .store
                .lock()
                .unwrap()
                .values()
                .filter(|d| d.state() == s)
                .cloned()
                .collect())
        }
    }

    struct MockEventBus {
        events: Mutex<Vec<DomainEvent>>,
    }

    impl MockEventBus {
        fn new() -> Self {
            Self {
                events: Mutex::new(Vec::new()),
            }
        }
    }

    impl EventBus for MockEventBus {
        fn publish(&self, event: DomainEvent) {
            self.events.lock().unwrap().push(event);
        }
        fn subscribe(&self, _handler: Box<dyn Fn(&DomainEvent) + Send + Sync>) {}
    }

    struct StubEngine;
    impl DownloadEngine for StubEngine {
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

    struct StubFileStorage;
    impl FileStorage for StubFileStorage {
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

    struct StubHttp;
    impl HttpClient for StubHttp {
        fn head(&self, _: &str) -> Result<HttpResponse, DomainError> {
            Ok(HttpResponse {
                status_code: 200,
                headers: HashMap::new(),
                body: vec![],
            })
        }
        fn get_range(&self, _: &str, start: u64, end: u64) -> Result<Vec<u8>, DomainError> {
            Ok(vec![
                0u8;
                end.saturating_sub(start).saturating_add(1) as usize
            ])
        }
        fn supports_range(&self, _: &str) -> Result<bool, DomainError> {
            Ok(true)
        }
    }

    struct StubLoader;
    impl PluginLoader for StubLoader {
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
            Ok(vec![])
        }
        fn set_enabled(&self, _: &str, _: bool) -> Result<(), DomainError> {
            Ok(())
        }
    }

    struct StubConfig;
    impl ConfigStore for StubConfig {
        fn get_config(&self) -> Result<AppConfig, DomainError> {
            Ok(AppConfig::default())
        }
        fn update_config(&self, _: ConfigPatch) -> Result<AppConfig, DomainError> {
            Ok(AppConfig::default())
        }
    }

    struct StubCreds;
    impl CredentialStore for StubCreds {
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

    struct StubClip;
    impl ClipboardObserver for StubClip {
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

    struct StubArchive;
    impl ArchiveExtractor for StubArchive {
        fn detect_format(
            &self,
            _: &Path,
        ) -> Result<Option<crate::domain::model::archive::ArchiveFormat>, DomainError> {
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
        ) -> Result<crate::domain::model::archive::ExtractSummary, DomainError> {
            Ok(crate::domain::model::archive::ExtractSummary {
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
        ) -> Result<Vec<crate::domain::model::archive::ArchiveEntry>, DomainError> {
            Ok(vec![])
        }
        fn detect_segments(&self, _: &Path) -> Result<Option<Vec<PathBuf>>, DomainError> {
            Ok(None)
        }
    }

    fn make_bus(
        repo: Arc<MockDownloadRepo>,
        events: Arc<MockEventBus>,
        history: Arc<dyn HistoryRepository>,
    ) -> CommandBus {
        CommandBus::new(
            repo,
            Arc::new(StubEngine),
            events,
            Arc::new(StubFileStorage),
            Arc::new(StubHttp),
            Arc::new(StubLoader),
            Arc::new(StubConfig),
            Arc::new(StubCreds),
            Arc::new(StubClip),
            Arc::new(StubArchive),
            history,
            None,
        )
    }

    fn completed_download(id: u64) -> Download {
        let mut d = Download::new(
            DownloadId(id),
            Url::new("https://example.com/files/report.pdf").unwrap(),
            "report.pdf".to_string(),
            "/downloads/report.pdf".to_string(),
        )
        .with_segments_count(4)
        .with_priority(Priority::new(9).unwrap())
        .with_module_name("vortex-mod-example".to_string())
        .with_account_id(7);
        d.start().unwrap();
        d.complete().unwrap();
        d
    }

    #[tokio::test]
    async fn redownload_from_completed_copies_url_and_options() {
        let repo = Arc::new(MockDownloadRepo::new());
        let events = Arc::new(MockEventBus::new());
        let history: Arc<dyn HistoryRepository> = Arc::new(InMemoryHistoryRepo::new());
        let original = completed_download(1);
        repo.save(&original).unwrap();
        let bus = make_bus(repo.clone(), events.clone(), history);

        let new_id = bus
            .handle_redownload(RedownloadCommand {
                source: RedownloadSource::Download(DownloadId(1)),
                destination_override: None,
            })
            .await
            .unwrap();

        assert_ne!(new_id, DownloadId(1), "re-download must create a new id");
        let created = repo.find_by_id(new_id).unwrap().unwrap();
        assert_eq!(
            created.url().as_str(),
            "https://example.com/files/report.pdf"
        );
        assert_eq!(created.file_name(), "report.pdf");
        assert_eq!(created.destination_path(), "/downloads/report.pdf");
        assert_eq!(created.segments_count(), 4);
        assert_eq!(*created.priority(), Priority::new(9).unwrap());
        assert_eq!(created.module_name(), Some("vortex-mod-example"));
        assert_eq!(created.account_id(), Some(7));
        assert_eq!(
            created.state(),
            DownloadState::Queued,
            "re-downloaded entries must start fresh in Queued",
        );
    }

    #[tokio::test]
    async fn redownload_emits_download_created_event_with_new_id() {
        let repo = Arc::new(MockDownloadRepo::new());
        let events = Arc::new(MockEventBus::new());
        let history: Arc<dyn HistoryRepository> = Arc::new(InMemoryHistoryRepo::new());
        repo.save(&completed_download(2)).unwrap();
        let bus = make_bus(repo, events.clone(), history);

        let new_id = bus
            .handle_redownload(RedownloadCommand {
                source: RedownloadSource::Download(DownloadId(2)),
                destination_override: None,
            })
            .await
            .unwrap();

        let emitted = events.events.lock().unwrap();
        assert_eq!(emitted.len(), 1);
        assert_eq!(emitted[0], DomainEvent::DownloadCreated { id: new_id });
    }

    #[tokio::test]
    async fn redownload_honours_destination_override() {
        let repo = Arc::new(MockDownloadRepo::new());
        let events = Arc::new(MockEventBus::new());
        let history: Arc<dyn HistoryRepository> = Arc::new(InMemoryHistoryRepo::new());
        repo.save(&completed_download(3)).unwrap();
        let bus = make_bus(repo.clone(), events, history);

        let new_id = bus
            .handle_redownload(RedownloadCommand {
                source: RedownloadSource::Download(DownloadId(3)),
                destination_override: Some(PathBuf::from("/tmp/report (1).pdf")),
            })
            .await
            .unwrap();

        let created = repo.find_by_id(new_id).unwrap().unwrap();
        assert_eq!(created.destination_path(), "/tmp/report (1).pdf");
    }

    #[tokio::test]
    async fn redownload_from_history_builds_download_with_defaults() {
        let repo = Arc::new(MockDownloadRepo::new());
        let events = Arc::new(MockEventBus::new());
        let history = Arc::new(InMemoryHistoryRepo::new());
        history
            .record(&HistoryEntry {
                id: 0,
                download_id: DownloadId(99),
                file_name: "clip.mp4".to_string(),
                url: "https://cdn.example.com/clip.mp4".to_string(),
                total_bytes: 1024,
                completed_at: 100,
                duration_seconds: 10,
                avg_speed: 100,
                destination_path: "/downloads/clip.mp4".to_string(),
            })
            .unwrap();

        let history_dyn: Arc<dyn HistoryRepository> = history.clone();
        let bus = make_bus(repo.clone(), events, history_dyn);

        let new_id = bus
            .handle_redownload(RedownloadCommand {
                source: RedownloadSource::History(1),
                destination_override: None,
            })
            .await
            .unwrap();

        let created = repo.find_by_id(new_id).unwrap().unwrap();
        assert_eq!(created.url().as_str(), "https://cdn.example.com/clip.mp4");
        assert_eq!(created.file_name(), "clip.mp4");
        assert_eq!(created.destination_path(), "/downloads/clip.mp4");
        assert_eq!(created.segments_count(), 1, "history lacks segments");
        assert_eq!(created.module_name(), None);
    }

    #[tokio::test]
    async fn redownload_rejects_non_completed_download() {
        let repo = Arc::new(MockDownloadRepo::new());
        let events = Arc::new(MockEventBus::new());
        let history: Arc<dyn HistoryRepository> = Arc::new(InMemoryHistoryRepo::new());
        let mut queued = Download::new(
            DownloadId(42),
            Url::new("https://example.com/x.zip").unwrap(),
            "x.zip".to_string(),
            "/downloads/x.zip".to_string(),
        );
        queued.start().unwrap();
        repo.save(&queued).unwrap();
        let bus = make_bus(repo, events, history);

        let err = bus
            .handle_redownload(RedownloadCommand {
                source: RedownloadSource::Download(DownloadId(42)),
                destination_override: None,
            })
            .await
            .unwrap_err();

        assert!(matches!(err, AppError::Validation(_)), "{err:?}");
    }

    #[tokio::test]
    async fn redownload_missing_download_returns_not_found() {
        let repo = Arc::new(MockDownloadRepo::new());
        let events = Arc::new(MockEventBus::new());
        let history: Arc<dyn HistoryRepository> = Arc::new(InMemoryHistoryRepo::new());
        let bus = make_bus(repo, events, history);

        let err = bus
            .handle_redownload(RedownloadCommand {
                source: RedownloadSource::Download(DownloadId(404)),
                destination_override: None,
            })
            .await
            .unwrap_err();

        assert!(matches!(err, AppError::NotFound(_)), "{err:?}");
    }

    #[tokio::test]
    async fn redownload_missing_history_entry_returns_not_found() {
        let repo = Arc::new(MockDownloadRepo::new());
        let events = Arc::new(MockEventBus::new());
        let history: Arc<dyn HistoryRepository> = Arc::new(InMemoryHistoryRepo::new());
        let bus = make_bus(repo, events, history);

        let err = bus
            .handle_redownload(RedownloadCommand {
                source: RedownloadSource::History(404),
                destination_override: None,
            })
            .await
            .unwrap_err();

        assert!(matches!(err, AppError::NotFound(_)), "{err:?}");
    }
}
