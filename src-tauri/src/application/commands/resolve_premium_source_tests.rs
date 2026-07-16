use std::sync::Arc;
use std::sync::atomic::Ordering;

use super::*;
use crate::application::commands::tests_support::{
    CapturingEventBus, FakeAccountCredentialStore, InMemoryAccountRepo, InMemoryDownloadRepo,
};
use crate::domain::event::DomainEvent;
use crate::domain::model::account::AccountStatus;
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
    let resolver = handler(repo.clone(), credentials, plugin.clone(), events.clone());
    let download = download(account.id().clone());

    let source = tokio::task::spawn_blocking(move || resolver.resolve(&download))
        .await
        .unwrap()
        .unwrap();

    assert_eq!(source.request_url(), "https://1.1.1.1/short-lived-token");
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
