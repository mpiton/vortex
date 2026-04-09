//! Handler for `StartDownloadCommand`.
//!
//! Validates the URL, probes the remote server for metadata,
//! creates the `Download` aggregate, persists it, and emits
//! `DownloadCreated` so the queue manager can schedule it.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::application::command_bus::CommandBus;
use crate::application::error::AppError;
use crate::domain::event::DomainEvent;
use crate::domain::model::download::{Download, DownloadId, Url};
use crate::domain::model::http::HttpResponse;

/// Monotonic counter combined with nanosecond timestamp for restart-safe
/// ID generation. The counter prevents collisions within a process; the
/// timestamp prevents collisions across restarts.
static NEXT_DOWNLOAD_SEQ: AtomicU64 = AtomicU64::new(0);

impl CommandBus {
    pub async fn handle_start_download(
        &self,
        cmd: super::StartDownloadCommand,
    ) -> Result<DownloadId, AppError> {
        let url = Url::new(&cmd.url)?;

        // file_size and resume_supported are discovered by the engine at download time.
        // The HEAD probe here is used only for filename resolution.
        let file_name = match self.http_client().head(url.as_str()) {
            Ok(resp) => extract_filename(&resp, &url),
            Err(_) => filename_from_url(&url),
        };

        let dest = cmd
            .destination
            .unwrap_or_else(|| PathBuf::from("."))
            .join(&file_name);

        let id = next_download_id();

        let download = Download::new(id, url, file_name, dest.to_string_lossy().to_string());

        self.download_repo().save(&download)?;
        self.event_bus()
            .publish(DomainEvent::DownloadCreated { id });

        Ok(id)
    }
}

/// Generate a restart-safe, collision-resistant download ID that fits
/// within JavaScript's `Number.MAX_SAFE_INTEGER` (2^53).
///
/// Layout: millisecond timestamp in high 41 bits, monotonic counter in
/// low 12 bits. Disjoint bit ranges prevent the `(T, seq)` vs
/// `(T+seq, 0)` collision class. 12-bit counter allows 4096 downloads
/// per millisecond.
fn next_download_id() -> DownloadId {
    let seq = NEXT_DOWNLOAD_SEQ.fetch_add(1, Ordering::Relaxed) & 0xFFF;
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    DownloadId((ts << 12) | seq)
}

fn extract_filename(resp: &HttpResponse, url: &Url) -> String {
    if let Some(cd) = resp.header("content-disposition")
        && let Some(name) = parse_content_disposition(cd)
    {
        return name;
    }
    filename_from_url(url)
}

fn filename_from_url(url: &Url) -> String {
    url.as_str()
        .rsplit('/')
        .next()
        .and_then(|s| s.split('?').next())
        .filter(|s| !s.is_empty())
        .unwrap_or("download")
        .to_string()
}

fn parse_content_disposition(value: &str) -> Option<String> {
    value.split(';').find_map(|part| {
        let part = part.trim();
        if part.starts_with("filename=") {
            Some(
                part.trim_start_matches("filename=")
                    .trim_matches('"')
                    .to_string(),
            )
        } else {
            None
        }
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::Path;
    use std::sync::Mutex;

    use crate::application::command_bus::CommandBus;
    use crate::application::commands::StartDownloadCommand;
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

    struct MockDownloadEngine;

    impl DownloadEngine for MockDownloadEngine {
        fn start(&self, _download: &Download) -> Result<(), DomainError> {
            Ok(())
        }
        fn pause(&self, _id: DownloadId) -> Result<(), DomainError> {
            Ok(())
        }
        fn resume(&self, _id: DownloadId) -> Result<(), DomainError> {
            Ok(())
        }
        fn cancel(&self, _id: DownloadId) -> Result<(), DomainError> {
            Ok(())
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

    struct MockHttpClient {
        response: Mutex<Option<HttpResponse>>,
    }

    impl MockHttpClient {
        fn with_response(resp: HttpResponse) -> Self {
            Self {
                response: Mutex::new(Some(resp)),
            }
        }

        fn failing() -> Self {
            Self {
                response: Mutex::new(None),
            }
        }
    }

    impl HttpClient for MockHttpClient {
        fn head(&self, _url: &str) -> Result<HttpResponse, DomainError> {
            self.response
                .lock()
                .unwrap()
                .clone()
                .ok_or_else(|| DomainError::NetworkError("connection refused".to_string()))
        }
        fn get_range(&self, _url: &str, start: u64, end: u64) -> Result<Vec<u8>, DomainError> {
            Ok(vec![
                0u8;
                end.saturating_sub(start).saturating_add(1) as usize
            ])
        }
        fn supports_range(&self, _url: &str) -> Result<bool, DomainError> {
            Ok(true)
        }
    }

    struct MockFileStorage;
    impl FileStorage for MockFileStorage {
        fn create_file(&self, _path: &Path, _size: u64) -> Result<(), DomainError> {
            Ok(())
        }
        fn write_segment(
            &self,
            _path: &Path,
            _offset: u64,
            _data: &[u8],
        ) -> Result<(), DomainError> {
            Ok(())
        }
        fn read_meta(&self, _path: &Path) -> Result<Option<DownloadMeta>, DomainError> {
            Ok(None)
        }
        fn write_meta(&self, _path: &Path, _meta: &DownloadMeta) -> Result<(), DomainError> {
            Ok(())
        }
        fn delete_meta(&self, _path: &Path) -> Result<(), DomainError> {
            Ok(())
        }
    }

    struct MockPluginLoader;
    impl PluginLoader for MockPluginLoader {
        fn load(&self, _manifest: &PluginManifest) -> Result<(), DomainError> {
            Ok(())
        }
        fn unload(&self, _name: &str) -> Result<(), DomainError> {
            Ok(())
        }
        fn resolve_url(&self, _url: &str) -> Result<Option<PluginInfo>, DomainError> {
            Ok(None)
        }
        fn list_loaded(&self) -> Result<Vec<PluginInfo>, DomainError> {
            Ok(vec![])
        }
        fn set_enabled(&self, _name: &str, _enabled: bool) -> Result<(), DomainError> {
            Ok(())
        }
    }

    struct MockConfigStore;
    impl ConfigStore for MockConfigStore {
        fn get_config(&self) -> Result<AppConfig, DomainError> {
            Ok(AppConfig::default())
        }
        fn update_config(&self, _patch: ConfigPatch) -> Result<AppConfig, DomainError> {
            Ok(AppConfig::default())
        }
    }

    struct MockCredentialStore;
    impl CredentialStore for MockCredentialStore {
        fn get(&self, _service: &str) -> Result<Option<Credential>, DomainError> {
            Ok(None)
        }
        fn store(&self, _service: &str, _credential: &Credential) -> Result<(), DomainError> {
            Ok(())
        }
        fn delete(&self, _service: &str) -> Result<(), DomainError> {
            Ok(())
        }
    }

    struct MockClipboardObserver;
    impl ClipboardObserver for MockClipboardObserver {
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

    fn make_command_bus(
        http_client: Arc<dyn HttpClient>,
    ) -> (CommandBus, Arc<MockDownloadRepo>, Arc<MockEventBus>) {
        let repo = Arc::new(MockDownloadRepo::new());
        let event_bus = Arc::new(MockEventBus::new());
        let bus = CommandBus::new(
            repo.clone(),
            Arc::new(MockDownloadEngine),
            event_bus.clone(),
            Arc::new(MockFileStorage),
            http_client,
            Arc::new(MockPluginLoader),
            Arc::new(MockConfigStore),
            Arc::new(MockCredentialStore),
            Arc::new(MockClipboardObserver),
        );
        (bus, repo, event_bus)
    }

    #[tokio::test]
    async fn test_start_download_persists_and_emits_event() {
        let mut headers = HashMap::new();
        headers.insert("content-length".to_string(), vec!["1024".to_string()]);
        headers.insert(
            "content-disposition".to_string(),
            vec!["attachment; filename=\"report.pdf\"".to_string()],
        );
        let resp = HttpResponse {
            status_code: 200,
            headers,
            body: vec![],
        };

        let (bus, repo, event_bus) =
            make_command_bus(Arc::new(MockHttpClient::with_response(resp)));

        let cmd = StartDownloadCommand {
            url: "https://example.com/files/report.pdf".to_string(),
            destination: Some(PathBuf::from("/tmp/downloads")),
        };

        let id = bus.handle_start_download(cmd).await.unwrap();

        let saved = repo.store.lock().unwrap().get(&id.0).cloned();
        assert!(saved.is_some());
        let dl = saved.unwrap();
        assert_eq!(dl.state(), DownloadState::Queued);
        assert_eq!(dl.file_name(), "report.pdf");

        let events = event_bus.events.lock().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0], DomainEvent::DownloadCreated { id });
    }

    #[tokio::test]
    async fn test_start_download_invalid_url_returns_error() {
        let (bus, _, _) = make_command_bus(Arc::new(MockHttpClient::failing()));

        let cmd = StartDownloadCommand {
            url: "not-a-valid-url".to_string(),
            destination: None,
        };

        let result = bus.handle_start_download(cmd).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_start_download_head_failure_uses_url_fallback() {
        let (bus, repo, _) = make_command_bus(Arc::new(MockHttpClient::failing()));

        let cmd = StartDownloadCommand {
            url: "https://example.com/path/archive.tar.gz".to_string(),
            destination: Some(PathBuf::from("/tmp")),
        };

        let id = bus.handle_start_download(cmd).await.unwrap();

        let saved = repo.store.lock().unwrap().get(&id.0).cloned().unwrap();
        assert_eq!(saved.file_name(), "archive.tar.gz");
    }

    use std::path::PathBuf;

    #[test]
    fn test_parse_content_disposition_extracts_filename() {
        let name = super::parse_content_disposition("attachment; filename=\"hello.zip\"");
        assert_eq!(name, Some("hello.zip".to_string()));
    }

    #[test]
    fn test_parse_content_disposition_returns_none_without_filename() {
        let name = super::parse_content_disposition("inline");
        assert_eq!(name, None);
    }

    #[test]
    fn test_filename_from_url_extracts_last_segment() {
        let url =
            crate::domain::model::download::Url::new("https://example.com/path/file.bin").unwrap();
        assert_eq!(super::filename_from_url(&url), "file.bin");
    }

    #[test]
    fn test_filename_from_url_strips_query_string() {
        let url =
            crate::domain::model::download::Url::new("https://example.com/file.bin?token=abc")
                .unwrap();
        assert_eq!(super::filename_from_url(&url), "file.bin");
    }
}
