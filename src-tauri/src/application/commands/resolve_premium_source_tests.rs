use std::sync::atomic::Ordering;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use super::*;
use crate::application::commands::tests_support::{
    CapturingEventBus, FakeAccountCredentialStore, InMemoryAccountRepo, InMemoryDownloadRepo,
};
use crate::domain::event::DomainEvent;
use crate::domain::model::account::AccountStatus;
use crate::domain::model::download::{Download, DownloadId, Url};
use crate::domain::ports::driven::{
    AccountCredentialStore, AccountRepository, DownloadRepository, DownloadSourceResolver,
};

#[path = "resolve_premium_source_test_support.rs"]
mod support;
use support::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn resolves_the_direct_url_only_when_the_engine_requests_it() {
    let repo = Arc::new(InMemoryAccountRepo::new());
    let credentials = Arc::new(FakeAccountCredentialStore::new());
    let plugin = Arc::new(DirectUrlPlugin::new());
    let events = Arc::new(CapturingEventBus::new());
    let account = valid_account("account-1");
    repo.save(&account).unwrap();
    credentials.store_password(account.id(), "api-key").unwrap();
    let download = download(account.id().clone());
    let downloads = Arc::new(InMemoryDownloadRepo::new());
    downloads.seed(download.clone());
    let resolver = handler_with_downloads(
        repo.clone(),
        credentials,
        plugin.clone(),
        events.clone(),
        downloads,
    );

    let source = tokio::task::spawn_blocking(move || resolver.resolve(&download))
        .await
        .unwrap()
        .unwrap();

    assert_eq!(source.request_url(), "https://1.1.1.1/short-lived-token");
    assert!(source.is_protected());
    assert_eq!(source.filename(), Some("file.zip"));
    assert_eq!(source.size_bytes(), Some(42));
    assert_eq!(source.resumable(), Some(true));
    assert_eq!(plugin.calls.lock().unwrap()[0].2, "api-key");
    assert_eq!(
        repo.find_by_id(account.id())
            .unwrap()
            .unwrap()
            .traffic_left(),
        Some(90)
    );
    assert!(
        events
            .snapshot()
            .iter()
            .any(|event| matches!(event, DomainEvent::AccountUpdated { id } if id == account.id()))
    );
}

#[test]
fn free_hoster_download_is_resolved_jit_with_backend_only_headers() {
    let plugin = Arc::new(DirectUrlPlugin::new());
    let resolver = handler(
        Arc::new(InMemoryAccountRepo::new()),
        Arc::new(FakeAccountCredentialStore::new()),
        plugin.clone(),
        Arc::new(CapturingEventBus::new()),
    );
    let download = Download::new(
        DownloadId(2),
        Url::new("https://1fichier.com/?free").unwrap(),
        "file.zip".into(),
        "/tmp/file.zip".into(),
    )
    .with_module_name("vortex-mod-1fichier".into());

    assert!(resolver.requires_resolution(&download).unwrap());
    let source = resolver.resolve(&download).expect("free hoster resolves");

    assert_eq!(source.request_url(), "https://1.1.1.1/short-lived-token");
    assert!(source.is_protected());
    assert_eq!(
        source.request_headers(),
        &[("Referer".into(), "https://1fichier.com/".into())]
    );
    assert_eq!(source.filename(), Some("file.zip"));
    assert_eq!(source.size_bytes(), Some(42));
    assert_eq!(source.resumable(), Some(true));
    assert_eq!(plugin.calls.lock().unwrap()[0].2, "");
}

#[test]
fn builtin_http_download_does_not_require_plugin_resolution() {
    let resolver = handler(
        Arc::new(InMemoryAccountRepo::new()),
        Arc::new(FakeAccountCredentialStore::new()),
        Arc::new(DirectUrlPlugin::new()),
        Arc::new(CapturingEventBus::new()),
    );
    let download = Download::new(
        DownloadId(3),
        Url::new("https://example.com/file.zip").unwrap(),
        "file.zip".into(),
        "/tmp/file.zip".into(),
    )
    .with_module_name("builtin-http".into());

    assert!(!resolver.requires_resolution(&download).unwrap());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rotates_jit_failures_and_persists_the_selected_backup() {
    for (password, expected_status, expected_calls) in [
        (
            Some("quota-key"),
            AccountStatus::QuotaExhausted,
            vec!["quota-key", "working-key"],
        ),
        (
            Some("expired-key"),
            AccountStatus::Expired,
            vec!["expired-key", "working-key"],
        ),
        (
            Some("zero-traffic-key"),
            AccountStatus::QuotaExhausted,
            vec!["zero-traffic-key", "working-key"],
        ),
        (None, AccountStatus::MissingCredential, vec!["working-key"]),
    ] {
        assert_jit_rotation(password, expected_status, &expected_calls).await;
    }
}

async fn assert_jit_rotation(
    primary_password: Option<&str>,
    expected_status: AccountStatus,
    expected_calls: &[&str],
) {
    let repo = Arc::new(InMemoryAccountRepo::new());
    let credentials = Arc::new(FakeAccountCredentialStore::new());
    let plugin = Arc::new(DirectUrlPlugin::new());
    let primary = valid_account("primary");
    let backup = valid_account("backup");
    repo.save(&primary).unwrap();
    repo.save(&backup).unwrap();
    if let Some(password) = primary_password {
        credentials.store_password(primary.id(), password).unwrap();
    }
    credentials
        .store_password(backup.id(), "working-key")
        .unwrap();
    let downloads = Arc::new(InMemoryDownloadRepo::new());
    let queued = download(primary.id().clone());
    let queued_id = queued.id();
    downloads.seed(queued.clone());
    let resolver = handler_with_downloads(
        repo.clone(),
        credentials,
        plugin.clone(),
        Arc::new(CapturingEventBus::new()),
        downloads.clone(),
    );

    let source = tokio::task::spawn_blocking(move || resolver.resolve(&queued))
        .await
        .unwrap()
        .expect("backup account resolves the source");

    assert_eq!(source.request_url(), "https://1.1.1.1/short-lived-token");
    let calls = plugin.calls.lock().unwrap();
    assert_eq!(
        calls.iter().map(|call| call.2.as_str()).collect::<Vec<_>>(),
        expected_calls
    );
    assert_eq!(
        repo.find_by_id(primary.id()).unwrap().unwrap().status(),
        expected_status
    );
    assert_eq!(
        downloads
            .find_by_id(queued_id)
            .unwrap()
            .unwrap()
            .account_id(),
        Some(backup.id())
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rotates_across_two_failed_accounts_before_committing_the_third() {
    let repo = Arc::new(InMemoryAccountRepo::new());
    let credentials = Arc::new(FakeAccountCredentialStore::new());
    let plugin = Arc::new(DirectUrlPlugin::new());
    let primary = valid_account("a-primary");
    let secondary = valid_account("b-secondary");
    let tertiary = valid_account("c-tertiary");
    for account in [&primary, &secondary, &tertiary] {
        repo.save(account).unwrap();
    }
    credentials
        .store_password(primary.id(), "quota-key")
        .unwrap();
    credentials
        .store_password(secondary.id(), "quota-key")
        .unwrap();
    credentials
        .store_password(tertiary.id(), "working-key")
        .unwrap();
    let downloads = Arc::new(InMemoryDownloadRepo::new());
    let queued = download(primary.id().clone());
    let download_id = queued.id();
    downloads.seed(queued.clone());
    let resolver = handler_with_downloads(
        repo,
        credentials,
        plugin.clone(),
        Arc::new(CapturingEventBus::new()),
        downloads.clone(),
    );

    let source = tokio::task::spawn_blocking(move || resolver.resolve(&queued))
        .await
        .unwrap()
        .expect("third account resolves the source");

    assert_eq!(source.request_url(), "https://1.1.1.1/short-lived-token");
    assert_eq!(
        plugin
            .calls
            .lock()
            .unwrap()
            .iter()
            .map(|call| call.2.as_str())
            .collect::<Vec<_>>(),
        ["quota-key", "quota-key", "working-key"]
    );
    assert_eq!(
        downloads
            .find_by_id(download_id)
            .unwrap()
            .unwrap()
            .account_id(),
        Some(tertiary.id())
    );
}

struct CasBarrierDownloadRepo {
    inner: InMemoryDownloadRepo,
    entered: Mutex<Option<std::sync::mpsc::Sender<()>>>,
    release: Arc<(Mutex<bool>, Condvar)>,
}

impl DownloadRepository for CasBarrierDownloadRepo {
    fn find_by_id(
        &self,
        id: crate::domain::model::download::DownloadId,
    ) -> Result<Option<crate::domain::model::download::Download>, DomainError> {
        self.inner.find_by_id(id)
    }

    fn save(&self, download: &crate::domain::model::download::Download) -> Result<(), DomainError> {
        self.inner.save(download)
    }

    fn delete(&self, id: crate::domain::model::download::DownloadId) -> Result<(), DomainError> {
        self.inner.delete(id)
    }

    fn compare_and_set_account_reference(
        &self,
        id: crate::domain::model::download::DownloadId,
        expected: &crate::domain::model::account::AccountId,
        replacement: &crate::domain::model::account::AccountId,
    ) -> Result<bool, DomainError> {
        if let Some(entered) = self.entered.lock().unwrap().take() {
            entered.send(()).unwrap();
        }
        let (released, condition) = &*self.release;
        let mut released = released.lock().unwrap();
        while !*released {
            released = condition.wait(released).unwrap();
        }
        self.inner
            .compare_and_set_account_reference(id, expected, replacement)
    }

    fn find_by_state(
        &self,
        state: crate::domain::model::download::DownloadState,
    ) -> Result<Vec<crate::domain::model::download::Download>, DomainError> {
        self.inner.find_by_state(state)
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_jit_rotation_does_not_recreate_a_concurrently_deleted_download() {
    let repo = Arc::new(InMemoryAccountRepo::new());
    let credentials = Arc::new(FakeAccountCredentialStore::new());
    let plugin = Arc::new(DirectUrlPlugin::new());
    let primary = valid_account("primary");
    let backup = valid_account("backup");
    repo.save(&primary).unwrap();
    repo.save(&backup).unwrap();
    credentials
        .store_password(primary.id(), "quota-key")
        .unwrap();
    credentials
        .store_password(backup.id(), "working-key")
        .unwrap();
    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let release = Arc::new((Mutex::new(false), Condvar::new()));
    let downloads = Arc::new(CasBarrierDownloadRepo {
        inner: InMemoryDownloadRepo::new(),
        entered: Mutex::new(Some(entered_tx)),
        release: release.clone(),
    });
    let stale_snapshot = download(primary.id().clone());
    let download_id = stale_snapshot.id();
    downloads.save(&stale_snapshot).unwrap();
    let resolver = handler_with_downloads(
        repo,
        credentials,
        plugin.clone(),
        Arc::new(CapturingEventBus::new()),
        downloads.clone(),
    );

    let resolving = tokio::task::spawn_blocking(move || resolver.resolve(&stale_snapshot));
    tokio::task::spawn_blocking(move || entered_rx.recv_timeout(Duration::from_secs(1)))
        .await
        .unwrap()
        .expect("rotation reached account CAS");
    downloads.delete(download_id).unwrap();
    let (released, condition) = &*release;
    *released.lock().unwrap() = true;
    condition.notify_all();
    let result = resolving.await.unwrap();

    assert!(matches!(result, Err(DomainError::NotFound(_))));
    assert!(downloads.find_by_id(download_id).unwrap().is_none());
    assert_eq!(plugin.calls.lock().unwrap().len(), 2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn initial_success_rejects_a_concurrent_account_reassociation_without_rotating() {
    let repo = Arc::new(InMemoryAccountRepo::new());
    let credentials = Arc::new(FakeAccountCredentialStore::new());
    let plugin = Arc::new(DirectUrlPlugin::new());
    let primary = valid_account("primary");
    let replacement = valid_account("replacement");
    repo.save(&primary).unwrap();
    repo.save(&replacement).unwrap();
    credentials
        .store_password(primary.id(), "working-key")
        .unwrap();
    credentials
        .store_password(replacement.id(), "working-key")
        .unwrap();
    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let release = Arc::new((Mutex::new(false), Condvar::new()));
    let downloads = Arc::new(CasBarrierDownloadRepo {
        inner: InMemoryDownloadRepo::new(),
        entered: Mutex::new(Some(entered_tx)),
        release: release.clone(),
    });
    let snapshot = download(primary.id().clone());
    let download_id = snapshot.id();
    downloads.save(&snapshot).unwrap();
    let resolver = handler_with_downloads(
        repo,
        credentials,
        plugin.clone(),
        Arc::new(CapturingEventBus::new()),
        downloads.clone(),
    );

    let resolving = tokio::task::spawn_blocking(move || resolver.resolve(&snapshot));
    tokio::task::spawn_blocking(move || entered_rx.recv_timeout(Duration::from_secs(1)))
        .await
        .unwrap()
        .expect("resolution reached account CAS");
    assert!(
        downloads
            .inner
            .compare_and_set_account_reference(download_id, primary.id(), replacement.id())
            .unwrap()
    );
    let (released, condition) = &*release;
    *released.lock().unwrap() = true;
    condition.notify_all();

    let error = resolving.await.unwrap().expect_err("association conflict");
    assert!(matches!(error, DomainError::ValidationError(_)));
    assert_eq!(plugin.calls.lock().unwrap().len(), 1);
    assert_eq!(
        downloads
            .find_by_id(download_id)
            .unwrap()
            .unwrap()
            .account_id(),
        Some(replacement.id())
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_jit_rotation_updates_only_the_account_reference() {
    let repo = Arc::new(InMemoryAccountRepo::new());
    let credentials = Arc::new(FakeAccountCredentialStore::new());
    let plugin = Arc::new(DirectUrlPlugin::new());
    let primary = valid_account("primary");
    let backup = valid_account("backup");
    repo.save(&primary).unwrap();
    repo.save(&backup).unwrap();
    credentials
        .store_password(primary.id(), "quota-key")
        .unwrap();
    credentials
        .store_password(backup.id(), "working-key")
        .unwrap();
    let downloads = Arc::new(InMemoryDownloadRepo::new());
    let mut stale_snapshot = download(primary.id().clone());
    stale_snapshot.start().unwrap();
    let mut paused = stale_snapshot.clone();
    paused.pause().unwrap();
    downloads.seed(paused);
    let download_id = stale_snapshot.id();
    let resolver = handler_with_downloads(
        repo,
        credentials,
        plugin,
        Arc::new(CapturingEventBus::new()),
        downloads.clone(),
    );

    tokio::task::spawn_blocking(move || resolver.resolve(&stale_snapshot))
        .await
        .unwrap()
        .expect("backup account resolves the source");

    let persisted = downloads.find_by_id(download_id).unwrap().unwrap();
    assert_eq!(
        persisted.state(),
        crate::domain::model::download::DownloadState::Paused
    );
    assert_eq!(persisted.account_id(), Some(backup.id()));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_jit_resolution_preserves_cooldown_when_no_backup_exists() {
    let repo = Arc::new(InMemoryAccountRepo::new());
    let credentials = Arc::new(FakeAccountCredentialStore::new());
    let account = valid_account("primary");
    repo.save(&account).unwrap();
    credentials
        .store_password(account.id(), "cooldown-key")
        .unwrap();
    let resolver = handler(
        repo,
        credentials,
        Arc::new(DirectUrlPlugin::new()),
        Arc::new(CapturingEventBus::new()),
    );
    let download = download(account.id().clone());

    let error = tokio::task::spawn_blocking(move || resolver.resolve(&download))
        .await
        .unwrap()
        .expect_err("the typed cooldown must reach the engine");

    assert!(matches!(error, DomainError::AccountCooldown));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn missing_premium_direct_url_is_a_typed_hoster_no_file_error() {
    let repo = Arc::new(InMemoryAccountRepo::new());
    let credentials = Arc::new(FakeAccountCredentialStore::new());
    let account = valid_account("primary");
    repo.save(&account).unwrap();
    credentials
        .store_password(account.id(), "missing-url")
        .unwrap();
    let resolver = handler(
        repo,
        credentials,
        Arc::new(DirectUrlPlugin::new()),
        Arc::new(CapturingEventBus::new()),
    );
    let download = download(account.id().clone());

    let error = tokio::task::spawn_blocking(move || resolver.resolve(&download))
        .await
        .unwrap()
        .expect_err("missing direct URL must remain a typed hoster failure");

    assert_eq!(error, DomainError::HosterNoFile);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn missing_credential_persistence_failure_is_not_suppressed() {
    let repo = Arc::new(SaveFailingRepo::new());
    let account = valid_account("account-1");
    repo.save(&account).unwrap();
    repo.fail_saves.store(true, Ordering::SeqCst);
    let resolver = handler(
        repo,
        Arc::new(FakeAccountCredentialStore::new()),
        Arc::new(DirectUrlPlugin::new()),
        Arc::new(CapturingEventBus::new()),
    );
    let download = download(account.id().clone());

    let error = tokio::task::spawn_blocking(move || resolver.resolve(&download))
        .await
        .unwrap()
        .expect_err("storage failure must win over missing credential");

    assert!(matches!(error, DomainError::StorageError(_)));
}

#[tokio::test]
async fn missing_credential_state_is_persisted_before_frontend_event() {
    let repo = Arc::new(InMemoryAccountRepo::new());
    let account = valid_account("account-1");
    repo.save(&account).unwrap();
    let events = Arc::new(CapturingEventBus::new());
    let resolver = handler(
        repo.clone(),
        Arc::new(FakeAccountCredentialStore::new()),
        Arc::new(DirectUrlPlugin::new()),
        events.clone(),
    );

    let error = resolver
        .handle(ResolvePremiumSourceCommand::new(
            account.id().clone(),
            account.service_name().to_string(),
            "https://1fichier.com/?abc123".into(),
        ))
        .await
        .expect_err("missing credential");

    assert!(matches!(error, DomainError::NotFound(_)));
    assert_eq!(
        repo.find_by_id(account.id()).unwrap().unwrap().status(),
        AccountStatus::MissingCredential
    );
    assert!(events.snapshot().iter().any(|event| matches!(
        event,
        DomainEvent::AccountValidationFailed { id, .. } if id == account.id()
    )));
}
