//! Handler for `RegisterLocalFileCommand`.
//!
//! Registers an already-downloaded local file as a Completed download.
//! Used after `download_to_file` produces a merged file via yt-dlp.

use crate::application::command_bus::CommandBus;
use crate::application::error::AppError;
use crate::domain::event::DomainEvent;
use crate::domain::model::download::{Download, DownloadId, Url};

impl CommandBus {
    pub async fn handle_register_local_file(
        &self,
        cmd: super::RegisterLocalFileCommand,
    ) -> Result<DownloadId, AppError> {
        let url = Url::new(&cmd.source_url)?;
        let id = super::start_download::next_download_id();
        let dest = cmd.destination_path.to_string_lossy().to_string();

        let mut download = Download::new(id, url, cmd.filename, dest);

        if let Some(hostname) = cmd.source_hostname {
            download = download.with_source_hostname(hostname);
        }
        if cmd.file_size > 0 {
            download.set_file_size(cmd.file_size);
        }

        // Advance state machine: Queued → Downloading → Completed.
        // DownloadStarted event is intentionally dropped — the file was already
        // downloaded by yt-dlp, so emitting DownloadStarted would be misleading.
        download.start().map_err(AppError::Domain)?;
        let completed_event = download.complete().map_err(AppError::Domain)?;

        self.download_repo().save(&download)?;
        self.event_bus().publish(DomainEvent::DownloadCreated { id });
        self.event_bus().publish(completed_event);

        Ok(id)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Mutex;

    use crate::application::command_bus::CommandBus;
    use crate::application::commands::RegisterLocalFileCommand;
    use crate::domain::error::DomainError;
    use crate::domain::event::DomainEvent;
    use crate::domain::model::config::{AppConfig, ConfigPatch};
    use crate::domain::model::credential::Credential;
    use crate::domain::model::download::{Download, DownloadId, DownloadState};
    use crate::domain::model::http::HttpResponse;
    use crate::domain::model::meta::DownloadMeta;
    use crate::domain::model::plugin::{PluginInfo, PluginManifest};
    use crate::domain::ports::driven::{
        ClipboardObserver, ConfigStore, CredentialStore, DownloadEngine, DownloadRepository,
        EventBus, FileStorage, HttpClient, PluginLoader,
    };
    use std::sync::Arc;

    struct MockRepo(Mutex<HashMap<u64, Download>>);
    impl MockRepo { fn new() -> Self { Self(Mutex::new(HashMap::new())) } }
    impl DownloadRepository for MockRepo {
        fn find_by_id(&self, id: DownloadId) -> Result<Option<Download>, DomainError> {
            Ok(self.0.lock().unwrap().get(&id.0).cloned())
        }
        fn save(&self, d: &Download) -> Result<(), DomainError> {
            self.0.lock().unwrap().insert(d.id().0, d.clone()); Ok(())
        }
        fn delete(&self, id: DownloadId) -> Result<(), DomainError> {
            self.0.lock().unwrap().remove(&id.0); Ok(())
        }
        fn find_by_state(&self, s: DownloadState) -> Result<Vec<Download>, DomainError> {
            Ok(self.0.lock().unwrap().values().filter(|d| d.state() == s).cloned().collect())
        }
    }
    struct MockEngine;
    impl DownloadEngine for MockEngine {
        fn start(&self, _: &Download) -> Result<(), DomainError> { Ok(()) }
        fn pause(&self, _: DownloadId) -> Result<(), DomainError> { Ok(()) }
        fn resume(&self, _: DownloadId) -> Result<(), DomainError> { Ok(()) }
        fn cancel(&self, _: DownloadId) -> Result<(), DomainError> { Ok(()) }
    }
    struct MockBus(Mutex<Vec<DomainEvent>>);
    impl MockBus { fn new() -> Self { Self(Mutex::new(vec![])) } }
    impl EventBus for MockBus {
        fn publish(&self, e: DomainEvent) { self.0.lock().unwrap().push(e); }
        fn subscribe(&self, _: Box<dyn Fn(&DomainEvent) + Send + Sync>) {}
    }
    struct MockHttp;
    impl HttpClient for MockHttp {
        fn head(&self, _: &str) -> Result<HttpResponse, DomainError> { Err(DomainError::NetworkError("no".into())) }
        fn get_range(&self, _: &str, s: u64, e: u64) -> Result<Vec<u8>, DomainError> { Ok(vec![0u8; (e - s + 1) as usize]) }
        fn supports_range(&self, _: &str) -> Result<bool, DomainError> { Ok(false) }
    }
    struct MockFs;
    impl FileStorage for MockFs {
        fn create_file(&self, _: &std::path::Path, _: u64) -> Result<(), DomainError> { Ok(()) }
        fn write_segment(&self, _: &std::path::Path, _: u64, _: &[u8]) -> Result<(), DomainError> { Ok(()) }
        fn read_meta(&self, _: &std::path::Path) -> Result<Option<DownloadMeta>, DomainError> { Ok(None) }
        fn write_meta(&self, _: &std::path::Path, _: &DownloadMeta) -> Result<(), DomainError> { Ok(()) }
        fn delete_meta(&self, _: &std::path::Path) -> Result<(), DomainError> { Ok(()) }
    }
    struct MockPlugin;
    impl PluginLoader for MockPlugin {
        fn load(&self, _: &PluginManifest) -> Result<(), DomainError> { Ok(()) }
        fn unload(&self, _: &str) -> Result<(), DomainError> { Ok(()) }
        fn resolve_url(&self, _: &str) -> Result<Option<PluginInfo>, DomainError> { Ok(None) }
        fn list_loaded(&self) -> Result<Vec<PluginInfo>, DomainError> { Ok(vec![]) }
        fn set_enabled(&self, _: &str, _: bool) -> Result<(), DomainError> { Ok(()) }
    }
    struct MockCfg;
    impl ConfigStore for MockCfg {
        fn get_config(&self) -> Result<AppConfig, DomainError> { Ok(AppConfig::default()) }
        fn update_config(&self, _: ConfigPatch) -> Result<AppConfig, DomainError> { Ok(AppConfig::default()) }
    }
    struct MockCred;
    impl CredentialStore for MockCred {
        fn get(&self, _: &str) -> Result<Option<Credential>, DomainError> { Ok(None) }
        fn store(&self, _: &str, _: &Credential) -> Result<(), DomainError> { Ok(()) }
        fn delete(&self, _: &str) -> Result<(), DomainError> { Ok(()) }
    }
    struct MockClip;
    impl ClipboardObserver for MockClip {
        fn start(&self) -> Result<(), DomainError> { Ok(()) }
        fn stop(&self) -> Result<(), DomainError> { Ok(()) }
        fn get_urls(&self) -> Result<Vec<String>, DomainError> { Ok(vec![]) }
    }
    struct FakeArchive;
    impl crate::domain::ports::driven::ArchiveExtractor for FakeArchive {
        fn detect_format(&self, _: &std::path::Path) -> Result<Option<crate::domain::model::archive::ArchiveFormat>, DomainError> { Ok(None) }
        fn can_extract(&self, _: &std::path::Path) -> Result<bool, DomainError> { Ok(false) }
        fn extract(&self, _: &std::path::Path, _: &std::path::Path, _: Option<&str>) -> Result<crate::domain::model::archive::ExtractSummary, DomainError> {
            Ok(crate::domain::model::archive::ExtractSummary { extracted_files: 0, extracted_bytes: 0, duration_ms: 0, warnings: vec![] })
        }
        fn list_contents(&self, _: &std::path::Path, _: Option<&str>) -> Result<Vec<crate::domain::model::archive::ArchiveEntry>, DomainError> { Ok(vec![]) }
        fn detect_segments(&self, _: &std::path::Path) -> Result<Option<Vec<std::path::PathBuf>>, DomainError> { Ok(None) }
    }

    fn make_bus() -> (CommandBus, Arc<MockRepo>, Arc<MockBus>) {
        let repo = Arc::new(MockRepo::new());
        let events = Arc::new(MockBus::new());
        let bus = CommandBus::new(
            repo.clone(), Arc::new(MockEngine), events.clone(),
            Arc::new(MockFs), Arc::new(MockHttp), Arc::new(MockPlugin),
            Arc::new(MockCfg), Arc::new(MockCred), Arc::new(MockClip),
            Arc::new(FakeArchive), None,
        );
        (bus, repo, events)
    }

    #[tokio::test]
    async fn test_register_local_file_creates_completed_download() {
        let (bus, repo, _) = make_bus();
        let cmd = RegisterLocalFileCommand {
            source_url: "https://www.youtube.com/watch?v=dQw4w9WgXcQ".to_string(),
            destination_path: PathBuf::from("/tmp/downloads/video.mp4"),
            filename: "Rick Astley - Never Gonna Give You Up.mp4".to_string(),
            source_hostname: Some("www.youtube.com".to_string()),
            file_size: 52_428_800,
        };
        let id = bus.handle_register_local_file(cmd).await.unwrap();
        let saved = repo.0.lock().unwrap().get(&id.0).cloned().unwrap();
        assert_eq!(saved.state(), DownloadState::Completed);
        assert_eq!(saved.file_name(), "Rick Astley - Never Gonna Give You Up.mp4");
        assert_eq!(saved.source_hostname(), "www.youtube.com");
    }

    #[tokio::test]
    async fn test_register_local_file_emits_created_and_completed_events() {
        let (bus, _, events) = make_bus();
        let cmd = RegisterLocalFileCommand {
            source_url: "https://www.youtube.com/watch?v=dQw4w9WgXcQ".to_string(),
            destination_path: PathBuf::from("/tmp/downloads/video.mp4"),
            filename: "video.mp4".to_string(),
            source_hostname: None,
            file_size: 0,
        };
        let id = bus.handle_register_local_file(cmd).await.unwrap();
        let evs = events.0.lock().unwrap();
        assert!(evs.iter().any(|e| *e == DomainEvent::DownloadCreated { id }), "must emit DownloadCreated");
        assert!(evs.iter().any(|e| *e == DomainEvent::DownloadCompleted { id }), "must emit DownloadCompleted");
    }
}
