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
use crate::domain::model::account::{AccountId, AccountStatus};
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

        // Use the pre-computed filename when available (e.g. set by media plugins
        // that already know the video title). Otherwise probe via HEAD or fall back
        // to extracting the last URL path segment.
        //
        // Reject path-bearing overrides: since `dest_dir.join(&file_name)` is
        // used below, an absolute path or one containing `..` would escape the
        // configured download directory.
        let file_name = if let Some(name) = cmd.filename.as_deref().filter(|s| !s.is_empty()) {
            let candidate = std::path::Path::new(name);
            if candidate.is_absolute() || candidate.components().count() != 1 {
                return Err(AppError::Domain(
                    crate::domain::error::DomainError::ValidationError(format!(
                        "invalid filename override (must be a single file component): {name}"
                    )),
                ));
            }
            name.to_string()
        } else {
            // file_size and resume_supported are discovered by the engine at download time.
            // The HEAD probe here is used only for filename resolution.
            match self.http_client().head(url.as_str()) {
                Ok(resp) => extract_filename(&resp, &url),
                Err(_) => filename_from_url(&url),
            }
        };

        let dest_dir = cmd.destination.unwrap_or_else(|| {
            // Prefer user-configured download dir; fall back to ~/Downloads/
            self.config_store()
                .get_config()
                .ok()
                .and_then(|c| c.download_dir)
                .map(PathBuf::from)
                .or_else(dirs::download_dir)
                .unwrap_or_else(|| PathBuf::from("."))
        });
        let dest = dest_dir.join(&file_name);

        let id = next_download_id();
        let account_lock = cmd
            .account_id
            .as_ref()
            .map(|id| self.account_operation_lock(id))
            .transpose()?;
        let _account_guard = match account_lock {
            Some(lock) => Some(lock.lock_owned().await),
            None => None,
        };
        self.validate_download_account(cmd.module_name.as_deref(), cmd.account_id.as_ref())?;
        // Append to the back of the queue so a freshly added download
        // does not jump in front of items the user has explicitly
        // reordered (default queue_position 0 would sort before 1..N).
        // Hold the queue-position lock so the read+write is atomic vs.
        // concurrent move_to_top/move_to_bottom/start_download calls.
        let _guard = self.lock_queue_positions().await;
        let queue_position = super::move_queue::next_queue_position(self.download_repo())?;

        let mut download = Download::new(id, url, file_name, dest.to_string_lossy().to_string())
            .with_queue_position(queue_position);

        if let Some(hostname) = cmd.source_hostname_override {
            download = download.with_source_hostname(hostname);
        }
        if let Some(module_name) = cmd.module_name {
            download = download.with_module_name(module_name);
        }
        if let Some(account_id) = cmd.account_id {
            download = download.with_account_id(account_id);
        }

        self.download_repo().save(&download)?;
        self.event_bus()
            .publish(DomainEvent::DownloadCreated { id });

        Ok(id)
    }

    pub(super) fn validate_download_account(
        &self,
        module_name: Option<&str>,
        account_id: Option<&AccountId>,
    ) -> Result<(), AppError> {
        let (Some(module_name), Some(account_id)) = (module_name, account_id) else {
            if account_id.is_some() {
                return Err(AppError::Validation(
                    "account association requires a plugin name".into(),
                ));
            }
            return Ok(());
        };
        let repo = self
            .account_repo()
            .ok_or_else(|| AppError::Validation("account repository not configured".into()))?;
        let store = self.account_credential_store().ok_or_else(|| {
            AppError::Validation("account credential store not configured".into())
        })?;
        let mut account = repo.find_by_id(account_id)?.ok_or_else(|| {
            AppError::NotFound(format!("account {} not found", account_id.as_str()))
        })?;
        if account.service_name() != module_name {
            return Err(AppError::Validation(format!(
                "account {} is not compatible with plugin {module_name}",
                account_id.as_str()
            )));
        }
        if store.get_password(account_id)?.is_none() {
            account.set_status(AccountStatus::MissingCredential);
            self.save_account_availability(repo, &account)?;
            self.event_bus()
                .publish(DomainEvent::AccountValidationFailed {
                    id: account_id.clone(),
                    error: "Account credential is unavailable".into(),
                });
            return Err(AppError::NotFound(format!(
                "credential for account {} not found",
                account_id.as_str()
            )));
        }
        if !account.is_selectable(self.account_now_ms()?) {
            return Err(AppError::Validation(format!(
                "account {} is not available",
                account_id.as_str()
            )));
        }
        Ok(())
    }
}

/// Generate a restart-safe, collision-resistant download ID that fits
/// within JavaScript's `Number.MAX_SAFE_INTEGER` (2^53).
///
/// Layout: millisecond timestamp in high 41 bits, monotonic counter in
/// low 12 bits. Disjoint bit ranges prevent the `(T, seq)` vs
/// `(T+seq, 0)` collision class. 12-bit counter allows 4096 downloads
/// per millisecond.
pub(super) fn next_download_id() -> DownloadId {
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
    use crate::application::commands::{DeleteAccountCommand, StartDownloadCommand};
    use crate::application::error::AppError;
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

    use crate::application::commands::tests_support::{
        FakeAccountCredentialStore, InMemoryAccountRepo,
    };
    use crate::domain::model::account::{Account, AccountId, AccountStatus, AccountType};
    use crate::domain::ports::driven::{AccountCredentialStore, AccountRepository, Clock};

    struct FixedAccountClock;

    impl Clock for FixedAccountClock {
        fn now_unix_secs(&self) -> u64 {
            1
        }
    }

    struct SignallingCredentialStore {
        password_read: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
    }

    impl AccountCredentialStore for SignallingCredentialStore {
        fn store_password(&self, _: &AccountId, _: &str) -> Result<(), DomainError> {
            Ok(())
        }

        fn get_password(&self, _: &AccountId) -> Result<Option<String>, DomainError> {
            if let Some(sender) = self.password_read.lock().unwrap().take() {
                let _ = sender.send(());
            }
            Ok(Some("api-key".into()))
        }

        fn delete_password(&self, _: &AccountId) -> Result<(), DomainError> {
            Ok(())
        }
    }

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

    struct FakeArchiveExtractor;
    impl crate::domain::ports::driven::ArchiveExtractor for FakeArchiveExtractor {
        fn detect_format(
            &self,
            _file_path: &std::path::Path,
        ) -> Result<Option<crate::domain::model::archive::ArchiveFormat>, DomainError> {
            Ok(None)
        }
        fn can_extract(&self, _file_path: &std::path::Path) -> Result<bool, DomainError> {
            Ok(false)
        }
        fn extract(
            &self,
            _file_path: &std::path::Path,
            _dest_dir: &std::path::Path,
            _password: Option<&str>,
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
            _file_path: &std::path::Path,
            _password: Option<&str>,
        ) -> Result<Vec<crate::domain::model::archive::ArchiveEntry>, DomainError> {
            Ok(vec![])
        }
        fn detect_segments(
            &self,
            _file_path: &std::path::Path,
        ) -> Result<Option<Vec<std::path::PathBuf>>, DomainError> {
            Ok(None)
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
            Arc::new(FakeArchiveExtractor),
            Arc::new(crate::application::test_support::NoopHistoryRepo),
            None,
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
            filename: None,
            source_hostname_override: None,
            module_name: None,
            account_id: None,
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
    async fn test_start_download_persists_validated_account_association() {
        let (bus, repo, _) = make_command_bus(Arc::new(MockHttpClient::failing()));
        let account_repo = Arc::new(InMemoryAccountRepo::new());
        let credentials = Arc::new(FakeAccountCredentialStore::new());
        let account_id = AccountId::new("account-1");
        let account = Account::reconstruct_with_status(
            account_id.clone(),
            "vortex-mod-1fichier".into(),
            "alice".into(),
            AccountType::Premium,
            true,
            None,
            None,
            Some(u64::MAX),
            Some(1),
            0,
            AccountStatus::Valid,
            None,
        );
        account_repo.save(&account).unwrap();
        credentials.store_password(&account_id, "api-key").unwrap();
        let bus = bus
            .with_account_repo(account_repo)
            .with_account_credential_store(credentials)
            .with_account_clock(Arc::new(FixedAccountClock));

        let id = bus
            .handle_start_download(StartDownloadCommand {
                url: "https://download.1fichier.com/token/file.zip".into(),
                destination: Some(PathBuf::from("/tmp")),
                filename: Some("file.zip".into()),
                source_hostname_override: Some("1fichier.com".into()),
                module_name: Some("vortex-mod-1fichier".into()),
                account_id: Some(account_id.clone()),
            })
            .await
            .expect("valid account association");

        let stored = repo.store.lock().unwrap().get(&id.0).cloned().unwrap();
        assert_eq!(stored.module_name(), Some("vortex-mod-1fichier"));
        assert_eq!(stored.account_id(), Some(&account_id));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn account_delete_waits_until_validated_download_is_persisted() {
        let (bus, _, _) = make_command_bus(Arc::new(MockHttpClient::failing()));
        let account_repo = Arc::new(InMemoryAccountRepo::new());
        let account_id = AccountId::new("account-1");
        let mut account = Account::new(
            account_id.clone(),
            "vortex-mod-1fichier".into(),
            "alice".into(),
            AccountType::Premium,
            0,
        );
        account.set_status(AccountStatus::Valid);
        account_repo.save(&account).unwrap();
        let (password_read_tx, password_read_rx) = tokio::sync::oneshot::channel();
        let credentials = Arc::new(SignallingCredentialStore {
            password_read: Mutex::new(Some(password_read_tx)),
        });
        let bus = Arc::new(
            bus.with_account_repo(account_repo)
                .with_account_credential_store(credentials)
                .with_account_clock(Arc::new(FixedAccountClock)),
        );
        let queue_guard = bus.lock_queue_positions().await;
        let start_bus = Arc::clone(&bus);
        let start_account = account_id.clone();
        let start = tokio::spawn(async move {
            start_bus
                .handle_start_download(StartDownloadCommand {
                    url: "https://1fichier.com/?abc123".into(),
                    destination: Some(PathBuf::from("/tmp")),
                    filename: Some("file.zip".into()),
                    source_hostname_override: None,
                    module_name: Some("vortex-mod-1fichier".into()),
                    account_id: Some(start_account),
                })
                .await
        });
        password_read_rx.await.expect("account validation reached");

        let delete_bus = Arc::clone(&bus);
        let mut deletion = tokio::spawn(async move {
            delete_bus
                .handle_delete_account(DeleteAccountCommand { id: account_id })
                .await
        });
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(100), &mut deletion)
                .await
                .is_err(),
            "deletion must serialize with validation and persistence"
        );

        drop(queue_guard);
        start.await.unwrap().expect("download persisted first");
        let error = deletion
            .await
            .unwrap()
            .expect_err("a referenced account cannot be deleted");
        assert!(matches!(error, AppError::Validation(_)));
    }

    #[tokio::test]
    async fn test_start_download_rejects_account_with_missing_credential() {
        let (bus, _, events) = make_command_bus(Arc::new(MockHttpClient::failing()));
        let account_repo = Arc::new(InMemoryAccountRepo::new());
        let credentials = Arc::new(FakeAccountCredentialStore::new());
        let account_id = AccountId::new("account-1");
        let account = Account::reconstruct_with_status(
            account_id.clone(),
            "vortex-mod-1fichier".into(),
            "alice".into(),
            AccountType::Premium,
            true,
            None,
            None,
            Some(u64::MAX),
            Some(1),
            0,
            AccountStatus::Valid,
            None,
        );
        account_repo.save(&account).unwrap();
        let bus = bus
            .with_account_repo(account_repo)
            .with_account_credential_store(credentials);

        let error = bus
            .handle_start_download(StartDownloadCommand {
                url: "https://download.1fichier.com/token/file.zip".into(),
                destination: Some(PathBuf::from("/tmp")),
                filename: Some("file.zip".into()),
                source_hostname_override: Some("1fichier.com".into()),
                module_name: Some("vortex-mod-1fichier".into()),
                account_id: Some(account_id.clone()),
            })
            .await
            .expect_err("missing credential must reject association");

        assert!(matches!(error, AppError::NotFound(_)));
        assert!(events.events.lock().unwrap().iter().any(|event| matches!(
            event,
            DomainEvent::AccountValidationFailed { id, .. } if id == &account_id
        )));
    }

    #[tokio::test]
    async fn test_start_download_uses_injected_clock_for_account_cooldown() {
        let (bus, _, _) = make_command_bus(Arc::new(MockHttpClient::failing()));
        let account_repo = Arc::new(InMemoryAccountRepo::new());
        let credentials = Arc::new(FakeAccountCredentialStore::new());
        let account_id = AccountId::new("account-1");
        let account = Account::reconstruct_with_status(
            account_id.clone(),
            "vortex-mod-1fichier".into(),
            "alice".into(),
            AccountType::Premium,
            true,
            None,
            None,
            Some(u64::MAX),
            Some(1),
            0,
            AccountStatus::Cooldown,
            Some(2_000),
        );
        account_repo.save(&account).unwrap();
        credentials.store_password(&account_id, "api-key").unwrap();
        let bus = bus
            .with_account_repo(account_repo)
            .with_account_credential_store(credentials)
            .with_account_clock(Arc::new(FixedAccountClock));

        let error = bus
            .handle_start_download(StartDownloadCommand {
                url: "https://download.1fichier.com/token/file.zip".into(),
                destination: Some(PathBuf::from("/tmp")),
                filename: Some("file.zip".into()),
                source_hostname_override: Some("1fichier.com".into()),
                module_name: Some("vortex-mod-1fichier".into()),
                account_id: Some(account_id),
            })
            .await
            .expect_err("injected clock keeps cooldown active");

        assert!(matches!(error, AppError::Validation(_)));
    }

    #[tokio::test]
    async fn test_start_download_invalid_url_returns_error() {
        let (bus, _, _) = make_command_bus(Arc::new(MockHttpClient::failing()));

        let cmd = StartDownloadCommand {
            url: "not-a-valid-url".to_string(),
            destination: None,
            filename: None,
            source_hostname_override: None,
            module_name: None,
            account_id: None,
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
            filename: None,
            source_hostname_override: None,
            module_name: None,
            account_id: None,
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

    #[tokio::test]
    async fn test_filename_override_skips_head_probe() {
        // Regression for YouTube downloads: when a filename override is provided
        // (e.g. "Rick Astley - Never Gonna Give You Up.mp4") the HEAD probe must
        // be skipped and the override used directly.
        let (bus, repo, _) = make_command_bus(Arc::new(MockHttpClient::failing()));

        let cmd = StartDownloadCommand {
            url: "https://rr1---sn-n4g-cvq6.googlevideo.com/videoplayback?expire=123".to_string(),
            destination: Some(PathBuf::from("/tmp")),
            filename: Some("Rick Astley - Never Gonna Give You Up.mp4".to_string()),
            source_hostname_override: None,
            module_name: None,
            account_id: None,
        };

        let id = bus.handle_start_download(cmd).await.unwrap();

        let saved = repo.store.lock().unwrap().get(&id.0).cloned().unwrap();
        assert_eq!(
            saved.file_name(),
            "Rick Astley - Never Gonna Give You Up.mp4",
            "filename override must be used, not the CDN URL path segment"
        );
    }

    #[tokio::test]
    async fn test_start_download_appends_to_back_of_existing_queue() {
        // Regression: a freshly created download must not jump in front of
        // items the user has already reordered. With a max queue_position of 5
        // among reorderable items, the new download must land at 5 + stride.
        let (bus, repo, _) = make_command_bus(Arc::new(MockHttpClient::failing()));

        let pre_existing = Download::new(
            DownloadId(1),
            crate::domain::model::download::Url::new("https://example.com/a.zip").unwrap(),
            "a.zip".to_string(),
            "/tmp/a.zip".to_string(),
        )
        .with_queue_position(5);
        repo.save(&pre_existing).unwrap();

        let cmd = StartDownloadCommand {
            url: "https://example.com/b.zip".to_string(),
            destination: Some(PathBuf::from("/tmp")),
            filename: Some("b.zip".to_string()),
            source_hostname_override: None,
            module_name: None,
            account_id: None,
        };

        let id = bus.handle_start_download(cmd).await.unwrap();
        let saved = repo.store.lock().unwrap().get(&id.0).cloned().unwrap();
        assert_eq!(
            saved.queue_position(),
            5 + 1024,
            "new download must append after the highest existing reorderable position"
        );
    }

    #[tokio::test]
    async fn test_source_hostname_override_replaces_cdn_hostname() {
        // Regression for YouTube downloads: the download must store "youtube.com"
        // (the origin) rather than "rr1---sn-n4g-cvq6.googlevideo.com" (the CDN).
        let (bus, repo, _) = make_command_bus(Arc::new(MockHttpClient::failing()));

        let cmd = StartDownloadCommand {
            url: "https://rr1---sn-n4g-cvq6.googlevideo.com/videoplayback?expire=123".to_string(),
            destination: Some(PathBuf::from("/tmp")),
            filename: Some("video.mp4".to_string()),
            source_hostname_override: Some("www.youtube.com".to_string()),
            module_name: None,
            account_id: None,
        };

        let id = bus.handle_start_download(cmd).await.unwrap();

        let saved = repo.store.lock().unwrap().get(&id.0).cloned().unwrap();
        assert_eq!(
            saved.source_hostname(),
            "www.youtube.com",
            "source_hostname must reflect the origin, not the CDN"
        );
    }

    #[tokio::test]
    async fn hoster_metadata_is_preserved_when_the_stable_source_is_created() {
        let (bus, repo, _) = make_command_bus(Arc::new(MockHttpClient::failing()));

        let id = bus
            .handle_start_download(StartDownloadCommand {
                url: "https://gofile.io/d/folder/file-a".into(),
                destination: Some(PathBuf::from("/tmp")),
                filename: Some("archive.zip".into()),
                size_bytes: Some(42),
                resume_supported: Some(true),
                source_hostname_override: None,
                module_name: Some("vortex-mod-gofile".into()),
                account_id: None,
            })
            .await
            .expect("stable hoster download is created");

        let saved = repo.store.lock().unwrap().get(&id.0).cloned().unwrap();
        assert_eq!(saved.url().as_str(), "https://gofile.io/d/folder/file-a");
        assert_eq!(saved.file_name(), "archive.zip");
        assert_eq!(saved.file_size().map(|size| size.0), Some(42));
        assert!(saved.resume_supported());
        assert_eq!(saved.module_name(), Some("vortex-mod-gofile"));
    }
}
