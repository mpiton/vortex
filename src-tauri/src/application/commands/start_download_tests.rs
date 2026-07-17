use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::application::commands::{DeleteAccountCommand, StartDownloadCommand};
use crate::application::error::AppError;
use crate::domain::error::DomainError;
use crate::domain::event::DomainEvent;
use crate::domain::model::download::{Download, DownloadId, DownloadState};
use crate::domain::model::http::HttpResponse;
use crate::domain::model::plugin::{PluginCategory, PluginInfo, PluginManifest};
use crate::domain::ports::driven::{DownloadRepository, HttpClient, PluginLoader};

use crate::application::commands::tests_support::{
    FakeAccountCredentialStore, InMemoryAccountRepo, build_download_bus,
    build_download_bus_with_plugin_loader,
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

struct HosterPluginLoader;
impl PluginLoader for HosterPluginLoader {
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
        Ok(vec![PluginInfo::new(
            "vortex-mod-hoster".into(),
            "1.0.0".into(),
            "test hoster".into(),
            "vortex".into(),
            PluginCategory::Hoster,
        )])
    }
    fn set_enabled(&self, _name: &str, _enabled: bool) -> Result<(), DomainError> {
        Ok(())
    }
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

    let (bus, repo, event_bus) = build_download_bus(Arc::new(MockHttpClient::with_response(resp)));

    let cmd = StartDownloadCommand {
        url: "https://example.com/files/report.pdf".to_string(),
        destination: Some(PathBuf::from("/tmp/downloads")),
        filename: None,
        size_bytes: None,
        resume_supported: None,
        source_hostname_override: None,
        module_name: None,
        account_id: None,
    };

    let id = bus.handle_start_download(cmd).await.unwrap();

    let saved = repo.find_by_id(id).unwrap();
    assert!(saved.is_some());
    let dl = saved.unwrap();
    assert_eq!(dl.state(), DownloadState::Queued);
    assert_eq!(dl.file_name(), "report.pdf");

    let events = event_bus.snapshot();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0], DomainEvent::DownloadCreated { id });
}

#[tokio::test]
async fn test_start_download_persists_validated_account_association() {
    let (bus, repo, _) = build_download_bus(Arc::new(MockHttpClient::failing()));
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
            size_bytes: None,
            resume_supported: None,
            source_hostname_override: Some("1fichier.com".into()),
            module_name: Some("vortex-mod-1fichier".into()),
            account_id: Some(account_id.clone()),
        })
        .await
        .expect("valid account association");

    let stored = repo.find_by_id(id).unwrap().unwrap();
    assert_eq!(stored.module_name(), Some("vortex-mod-1fichier"));
    assert_eq!(stored.account_id(), Some(&account_id));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn account_delete_waits_until_validated_download_is_persisted() {
    let (bus, _, _) = build_download_bus(Arc::new(MockHttpClient::failing()));
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
                size_bytes: None,
                resume_supported: None,
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
    let (bus, _, events) = build_download_bus(Arc::new(MockHttpClient::failing()));
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
            size_bytes: None,
            resume_supported: None,
            source_hostname_override: Some("1fichier.com".into()),
            module_name: Some("vortex-mod-1fichier".into()),
            account_id: Some(account_id.clone()),
        })
        .await
        .expect_err("missing credential must reject association");

    assert!(matches!(error, AppError::NotFound(_)));
    assert!(events.snapshot().iter().any(|event| matches!(
        event,
        DomainEvent::AccountValidationFailed { id, .. } if id == &account_id
    )));
}

#[tokio::test]
async fn test_start_download_uses_injected_clock_for_account_cooldown() {
    let (bus, _, _) = build_download_bus(Arc::new(MockHttpClient::failing()));
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
            size_bytes: None,
            resume_supported: None,
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
    let (bus, _, _) = build_download_bus(Arc::new(MockHttpClient::failing()));

    let cmd = StartDownloadCommand {
        url: "not-a-valid-url".to_string(),
        destination: None,
        filename: None,
        size_bytes: None,
        resume_supported: None,
        source_hostname_override: None,
        module_name: None,
        account_id: None,
    };

    let result = bus.handle_start_download(cmd).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_start_download_head_failure_uses_url_fallback() {
    let (bus, repo, _) = build_download_bus(Arc::new(MockHttpClient::failing()));

    let cmd = StartDownloadCommand {
        url: "https://example.com/path/archive.tar.gz".to_string(),
        destination: Some(PathBuf::from("/tmp")),
        filename: None,
        size_bytes: None,
        resume_supported: None,
        source_hostname_override: None,
        module_name: None,
        account_id: None,
    };

    let id = bus.handle_start_download(cmd).await.unwrap();

    let saved = repo.find_by_id(id).unwrap().unwrap();
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
        crate::domain::model::download::Url::new("https://example.com/file.bin?token=abc").unwrap();
    assert_eq!(super::filename_from_url(&url), "file.bin");
}

#[tokio::test]
async fn test_filename_override_skips_head_probe() {
    // Regression for YouTube downloads: when a filename override is provided
    // (e.g. "Rick Astley - Never Gonna Give You Up.mp4") the HEAD probe must
    // be skipped and the override used directly.
    let (bus, repo, _) = build_download_bus(Arc::new(MockHttpClient::failing()));

    let cmd = StartDownloadCommand {
        url: "https://rr1---sn-n4g-cvq6.googlevideo.com/videoplayback?expire=123".to_string(),
        destination: Some(PathBuf::from("/tmp")),
        filename: Some("Rick Astley - Never Gonna Give You Up.mp4".to_string()),
        size_bytes: None,
        resume_supported: None,
        source_hostname_override: None,
        module_name: None,
        account_id: None,
    };

    let id = bus.handle_start_download(cmd).await.unwrap();

    let saved = repo.find_by_id(id).unwrap().unwrap();
    assert_eq!(
        saved.file_name(),
        "Rick Astley - Never Gonna Give You Up.mp4",
        "filename override must be used, not the CDN URL path segment"
    );
}

#[tokio::test]
async fn hoster_without_filename_never_uses_the_generic_head_probe() {
    let mut headers = HashMap::new();
    headers.insert(
        "content-disposition".to_string(),
        vec!["attachment; filename=from-unsafe-probe.bin".to_string()],
    );
    let response = HttpResponse {
        status_code: 200,
        headers,
        body: vec![],
    };
    let (bus, repo, _) = build_download_bus_with_plugin_loader(
        Arc::new(MockHttpClient::with_response(response)),
        Arc::new(HosterPluginLoader),
    );

    let id = bus
        .handle_start_download(StartDownloadCommand {
            url: "https://hoster.example/stable-id".into(),
            destination: Some(PathBuf::from("/tmp")),
            filename: None,
            size_bytes: None,
            resume_supported: None,
            source_hostname_override: None,
            module_name: Some("vortex-mod-hoster".into()),
            account_id: None,
        })
        .await
        .expect("protected hoster source is queued");

    let saved = repo.find_by_id(id).unwrap().unwrap();
    assert_eq!(saved.file_name(), "stable-id");
}

#[tokio::test]
async fn builtin_http_without_filename_does_not_require_a_plugin_manifest() {
    let (bus, repo, _) = build_download_bus(Arc::new(MockHttpClient::failing()));

    let id = bus
        .handle_start_download(StartDownloadCommand {
            url: "https://example.com/".into(),
            destination: Some(PathBuf::from("/tmp")),
            filename: None,
            size_bytes: None,
            resume_supported: None,
            source_hostname_override: None,
            module_name: Some("builtin-http".into()),
            account_id: None,
        })
        .await
        .expect("built-in HTTP remains a direct source");

    let saved = repo.find_by_id(id).unwrap().unwrap();
    assert_eq!(saved.file_name(), "download");
}

#[tokio::test]
async fn test_start_download_appends_to_back_of_existing_queue() {
    // Regression: a freshly created download must not jump in front of
    // items the user has already reordered. With a max queue_position of 5
    // among reorderable items, the new download must land at 5 + stride.
    let (bus, repo, _) = build_download_bus(Arc::new(MockHttpClient::failing()));

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
        size_bytes: None,
        resume_supported: None,
        source_hostname_override: None,
        module_name: None,
        account_id: None,
    };

    let id = bus.handle_start_download(cmd).await.unwrap();
    let saved = repo.find_by_id(id).unwrap().unwrap();
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
    let (bus, repo, _) = build_download_bus(Arc::new(MockHttpClient::failing()));

    let cmd = StartDownloadCommand {
        url: "https://rr1---sn-n4g-cvq6.googlevideo.com/videoplayback?expire=123".to_string(),
        destination: Some(PathBuf::from("/tmp")),
        filename: Some("video.mp4".to_string()),
        size_bytes: None,
        resume_supported: None,
        source_hostname_override: Some("www.youtube.com".to_string()),
        module_name: None,
        account_id: None,
    };

    let id = bus.handle_start_download(cmd).await.unwrap();

    let saved = repo.find_by_id(id).unwrap().unwrap();
    assert_eq!(
        saved.source_hostname(),
        "www.youtube.com",
        "source_hostname must reflect the origin, not the CDN"
    );
}

#[tokio::test]
async fn hoster_metadata_is_preserved_when_the_stable_source_is_created() {
    let (bus, repo, _) = build_download_bus(Arc::new(MockHttpClient::failing()));

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

    let saved = repo.find_by_id(id).unwrap().unwrap();
    assert_eq!(saved.url().as_str(), "https://gofile.io/d/folder/file-a");
    assert_eq!(saved.file_name(), "archive.zip");
    assert_eq!(saved.file_size().map(|size| size.0), Some(42));
    assert!(saved.resume_supported());
    assert_eq!(saved.module_name(), Some("vortex-mod-gofile"));
}
