use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering as AtomicOrdering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::adapters::driven::filesystem::FsFileStorage;
use crate::adapters::driven::network::restricted_download_client;
use crate::domain::model::account::AccountId;
use crate::domain::model::download::{Download, DownloadId};
use crate::domain::model::meta::DownloadMeta;
use crate::domain::ports::driven::{
    DownloadSourceResolver, EventBus, FileStorage, ResolvedDownloadSource,
};

use super::test_support::*;
use super::*;

struct LoopbackSourceResolver {
    calls: AtomicUsize,
    url: String,
}

struct RotatingHosterSourceResolver {
    calls: AtomicUsize,
    base_url: String,
}

struct DirectSourceResolver;

struct HeaderedDirectSourceResolver {
    url: String,
}

fn loopback_source_client(
    _: &reqwest::Url,
    request_headers: &[(String, String)],
) -> Result<reqwest::Client, DomainError> {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .default_headers(
            crate::adapters::driven::network::safe_url::validated_plugin_headers(request_headers)?,
        )
        .build()
        .map_err(|_| DomainError::NetworkError("test HTTP client creation failed".into()))
}

struct BlockingSourceResolver {
    entered: Mutex<Option<std::sync::mpsc::Sender<()>>>,
    release: Arc<(Mutex<bool>, Condvar)>,
    committed: AtomicBool,
    finished: AtomicBool,
}

impl DownloadSourceResolver for BlockingSourceResolver {
    fn resolve(&self, _: &Download) -> Result<ResolvedDownloadSource, DomainError> {
        Err(DomainError::PluginError(
            "cancellable path was not used".into(),
        ))
    }

    fn resolve_cancellable(
        &self,
        _: &Download,
        cancellation: &ResolutionCancellation,
    ) -> Result<ResolvedDownloadSource, DomainError> {
        if let Some(entered) = self.entered.lock().unwrap().take() {
            entered.send(()).unwrap();
        }
        let (released, condition) = &*self.release;
        let mut released = released.lock().unwrap();
        while !*released {
            released = condition.wait(released).unwrap();
        }
        let result = cancellation.run_if_active(|| {
            self.committed.store(true, AtomicOrdering::SeqCst);
            Ok(ResolvedDownloadSource::protected(
                "https://1.1.1.1/late-token".into(),
            ))
        });
        self.finished.store(true, AtomicOrdering::SeqCst);
        result
    }
}

impl DownloadSourceResolver for LoopbackSourceResolver {
    fn resolve(&self, _: &Download) -> Result<ResolvedDownloadSource, DomainError> {
        self.calls.fetch_add(1, AtomicOrdering::SeqCst);
        Ok(ResolvedDownloadSource::protected(self.url.clone()))
    }
}

impl DownloadSourceResolver for RotatingHosterSourceResolver {
    fn requires_resolution(&self, _: &Download) -> Result<bool, DomainError> {
        Ok(true)
    }

    fn resolve(&self, _: &Download) -> Result<ResolvedDownloadSource, DomainError> {
        let call = self.calls.fetch_add(1, AtomicOrdering::SeqCst) + 1;
        Ok(ResolvedDownloadSource::protected(format!(
            "{}/{}",
            self.base_url,
            if call == 1 { "expired" } else { "fresh" }
        ))
        .with_request_headers(vec![(
            "Authorization".into(),
            format!("Bearer token-{call}"),
        )]))
    }
}

impl DownloadSourceResolver for DirectSourceResolver {
    fn requires_resolution(&self, _: &Download) -> Result<bool, DomainError> {
        Ok(true)
    }

    fn resolve(&self, _: &Download) -> Result<ResolvedDownloadSource, DomainError> {
        Ok(ResolvedDownloadSource::direct(
            "https://example.com/file.bin".into(),
        ))
    }
}

impl DownloadSourceResolver for HeaderedDirectSourceResolver {
    fn requires_resolution(&self, _: &Download) -> Result<bool, DomainError> {
        Ok(true)
    }

    fn resolve(&self, _: &Download) -> Result<ResolvedDownloadSource, DomainError> {
        Ok(ResolvedDownloadSource::direct(self.url.clone())
            .with_request_headers(vec![("Authorization".into(), "Bearer secret".into())]))
    }
}

#[tokio::test]
async fn resolved_direct_source_keeps_direct_policy() {
    let prepared = prepare_sources(
        make_download(197, "https://example.com/file.bin"),
        Some(Arc::new(DirectSourceResolver)),
        reqwest::Client::new(),
        CancellationToken::new(),
        ResolutionCancellation::default(),
        restricted_download_client,
    )
    .await
    .expect("direct source prepares");

    assert_eq!(prepared.policy, SourcePolicy::Direct);
}

#[tokio::test]
async fn credential_bearing_direct_source_uses_restricted_network_policy() {
    let error = match prepare_sources(
        make_download(198, "https://example.com/file.bin"),
        Some(Arc::new(HeaderedDirectSourceResolver {
            url: "http://127.0.0.1/private".into(),
        })),
        reqwest::Client::new(),
        CancellationToken::new(),
        ResolutionCancellation::default(),
        restricted_download_client,
    )
    .await
    {
        Ok(_) => panic!("credential-bearing direct source must reject private cleartext URLs"),
        Err(error) => error,
    };

    assert!(matches!(error, DomainError::NetworkError(_)));
}

#[tokio::test]
async fn premium_source_is_resolved_jit_and_blocked_before_private_network_access() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let private_url = format!("https://{}/secret-token", listener.local_addr().unwrap());
    let storage = Arc::new(MockFileStorage::new());
    let bus = Arc::new(CollectingEventBus::new());
    let resolver = Arc::new(LoopbackSourceResolver {
        calls: AtomicUsize::new(0),
        url: private_url,
    });
    let engine = make_engine(storage, bus.clone()).with_source_resolver(resolver.clone());
    let download = make_download(99, "https://1fichier.com/?abc123")
        .with_module_name("vortex-mod-1fichier".into())
        .with_account_id(AccountId::new("account-1"));

    engine.start(&download).expect("spawn download");

    assert!(
        bus.wait_for_event_async(
            |event| matches!(event, DomainEvent::DownloadFailed { id, .. } if id.0 == 99),
            Duration::from_secs(2),
        )
        .await
    );
    assert_eq!(resolver.calls.load(AtomicOrdering::SeqCst), 1);
    assert!(
        tokio::time::timeout(Duration::from_millis(100), listener.accept())
            .await
            .is_err(),
        "private listener must not receive a connection"
    );
    let serialized = format!("{:?}", bus.collected());
    assert!(!serialized.contains("secret-token"));
    assert_eq!(download.url().as_str(), "https://1fichier.com/?abc123");
}

#[tokio::test]
async fn every_hoster_engine_start_resolves_a_fresh_download_capability() {
    for (index, service) in [
        "vortex-mod-mediafire",
        "vortex-mod-pixeldrain",
        "vortex-mod-gofile",
    ]
    .into_iter()
    .enumerate()
    {
        let server = MockServer::start().await;
        Mock::given(method("HEAD"))
            .and(path("/expired"))
            .respond_with(ResponseTemplate::new(410))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/expired"))
            .respond_with(ResponseTemplate::new(410))
            .mount(&server)
            .await;
        Mock::given(method("HEAD"))
            .and(path("/fresh"))
            .and(header("authorization", "Bearer token-2"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "application/octet-stream")
                    .insert_header("content-length", "4"),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/fresh"))
            .and(header("authorization", "Bearer token-2"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"data"))
            .mount(&server)
            .await;
        let resolver = Arc::new(RotatingHosterSourceResolver {
            calls: AtomicUsize::new(0),
            base_url: server.uri(),
        });
        let temp = tempfile::tempdir().unwrap();
        let destination = temp.path().join(format!("hoster-{index}.bin"));
        let bus = Arc::new(CollectingEventBus::new());
        let engine = make_engine(Arc::new(FsFileStorage::new()), bus.clone())
            .with_source_resolver(resolver.clone())
            .with_resolved_client_factory_for_testing(loopback_source_client);
        let download = Download::new(
            DownloadId(200 + index as u64),
            crate::domain::model::download::Url::new("https://hoster.example/page").unwrap(),
            format!("hoster-{index}.bin"),
            destination.to_string_lossy().into_owned(),
        )
        .with_module_name(service.into());

        engine.start(&download).expect("engine start");
        assert!(
            bus.wait_for_event_async(
                |event| matches!(event, DomainEvent::DownloadCompleted { id } if id == &download.id()),
                Duration::from_secs(3),
            )
            .await,
            "{service} did not complete"
        );
        assert_eq!(resolver.calls.load(AtomicOrdering::SeqCst), 2, "{service}");
        assert_eq!(std::fs::read(destination).unwrap(), b"data", "{service}");
    }
}

#[tokio::test]
async fn protected_hoster_attempt_rejects_an_unexpected_html_page() {
    let server = MockServer::start().await;
    Mock::given(method("HEAD"))
        .and(path("/download"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/html; charset=utf-8")
                .insert_header("content-length", "18"),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/download"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/html; charset=utf-8")
                .set_body_string("<html>expired</html>"),
        )
        .mount(&server)
        .await;
    let storage: Arc<dyn FileStorage> = Arc::new(MockFileStorage::new());
    let events: Arc<dyn EventBus> = Arc::new(CollectingEventBus::new());

    let outcome = run_mirror_attempt(MirrorAttemptParams {
        url: format!("{}/download", server.uri()),
        download_id: DownloadId(299),
        segments_count: 1,
        client: reqwest::Client::new(),
        file_storage: storage,
        event_bus: events,
        dest_path: PathBuf::from("/tmp/vortex-hoster-html-test.bin"),
        pause_rx: watch::channel(false).1,
        user_cancel_token: CancellationToken::new(),
        attempt_token: CancellationToken::new(),
        min_segment_bytes: 1,
        dynamic_split_enabled: Arc::new(AtomicBool::new(false)),
        dynamic_split_min_remaining_bytes: Arc::new(AtomicU64::new(1)),
        resume_url: "https://hoster.example/page".into(),
        source_policy: SourcePolicy::Protected { allow_html: false },
        size_hint: None,
        resume_supported: None,
    })
    .await;

    match outcome {
        AttemptOutcome::Failed(failure) => {
            assert!(failure.message.contains("HTML"), "{}", failure.message)
        }
        _ => panic!("hoster HTML must be rejected"),
    }
}

#[tokio::test]
async fn protected_hoster_attempt_rejects_html_disguised_as_binary() {
    let server = MockServer::start().await;
    let body = "<!doctype html><html>expired</html>";
    Mock::given(method("HEAD"))
        .and(path("/download"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "application/octet-stream")
                .insert_header("content-length", body.len().to_string()),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/download"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "application/octet-stream")
                .set_body_string(body),
        )
        .mount(&server)
        .await;

    let temp = tempfile::tempdir().unwrap();
    let destination = temp.path().join("disguised-html.bin");
    let outcome = run_mirror_attempt(MirrorAttemptParams {
        url: format!("{}/download", server.uri()),
        download_id: DownloadId(300),
        segments_count: 1,
        client: reqwest::Client::new(),
        file_storage: Arc::new(FsFileStorage::new()),
        event_bus: Arc::new(CollectingEventBus::new()),
        dest_path: destination.clone(),
        pause_rx: watch::channel(false).1,
        user_cancel_token: CancellationToken::new(),
        attempt_token: CancellationToken::new(),
        min_segment_bytes: 1,
        dynamic_split_enabled: Arc::new(AtomicBool::new(false)),
        dynamic_split_min_remaining_bytes: Arc::new(AtomicU64::new(1)),
        resume_url: "https://hoster.example/page".into(),
        source_policy: SourcePolicy::Protected { allow_html: false },
        size_hint: None,
        resume_supported: None,
    })
    .await;

    match outcome {
        AttemptOutcome::Failed(failure) => {
            assert!(failure.message.contains("HTML"), "{}", failure.message)
        }
        _ => panic!("disguised hoster HTML must be rejected"),
    }
    assert!(!destination.exists());
}

#[tokio::test]
async fn protected_hoster_attempt_grows_an_atomically_reserved_unknown_length_file() {
    let server = MockServer::start().await;
    Mock::given(method("HEAD"))
        .and(path("/download"))
        .respond_with(
            ResponseTemplate::new(200).insert_header("content-type", "application/octet-stream"),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/download"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"data"))
        .mount(&server)
        .await;
    let temp = tempfile::tempdir().unwrap();
    let destination = temp.path().join("unknown-length.bin");

    let outcome = run_mirror_attempt(MirrorAttemptParams {
        url: format!("{}/download", server.uri()),
        download_id: DownloadId(302),
        segments_count: 1,
        client: reqwest::Client::new(),
        file_storage: Arc::new(FsFileStorage::new()),
        event_bus: Arc::new(CollectingEventBus::new()),
        dest_path: destination.clone(),
        pause_rx: watch::channel(false).1,
        user_cancel_token: CancellationToken::new(),
        attempt_token: CancellationToken::new(),
        min_segment_bytes: 1,
        dynamic_split_enabled: Arc::new(AtomicBool::new(false)),
        dynamic_split_min_remaining_bytes: Arc::new(AtomicU64::new(1)),
        resume_url: "https://hoster.example/page".into(),
        source_policy: SourcePolicy::Protected { allow_html: false },
        size_hint: None,
        resume_supported: None,
    })
    .await;

    assert!(matches!(outcome, AttemptOutcome::Completed));
    assert_eq!(std::fs::read(destination).unwrap(), b"data");
}

#[tokio::test]
async fn protected_hoster_attempt_never_deletes_an_unowned_destination() {
    let server = MockServer::start().await;
    Mock::given(method("HEAD"))
        .and(path("/download"))
        .respond_with(
            ResponseTemplate::new(200).insert_header("content-type", "application/octet-stream"),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/download"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"data"))
        .mount(&server)
        .await;
    let temp = tempfile::tempdir().unwrap();
    let destination = temp.path().join("existing.bin");
    std::fs::write(&destination, b"user-owned").unwrap();

    let outcome = run_mirror_attempt(MirrorAttemptParams {
        url: format!("{}/download", server.uri()),
        download_id: DownloadId(301),
        segments_count: 1,
        client: reqwest::Client::new(),
        file_storage: Arc::new(FsFileStorage::new()),
        event_bus: Arc::new(CollectingEventBus::new()),
        dest_path: destination.clone(),
        pause_rx: watch::channel(false).1,
        user_cancel_token: CancellationToken::new(),
        attempt_token: CancellationToken::new(),
        min_segment_bytes: 1,
        dynamic_split_enabled: Arc::new(AtomicBool::new(false)),
        dynamic_split_min_remaining_bytes: Arc::new(AtomicU64::new(1)),
        resume_url: "https://hoster.example/page".into(),
        source_policy: SourcePolicy::Protected { allow_html: false },
        size_hint: None,
        resume_supported: None,
    })
    .await;

    assert!(matches!(outcome, AttemptOutcome::Failed(_)));
    assert_eq!(std::fs::read(destination).unwrap(), b"user-owned");
}

#[tokio::test]
async fn direct_attempt_never_deletes_an_unowned_destination() {
    let server = MockServer::start().await;
    Mock::given(method("HEAD"))
        .and(path("/download"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "application/octet-stream")
                .insert_header("content-length", "4"),
        )
        .mount(&server)
        .await;
    let temp = tempfile::tempdir().unwrap();
    let destination = temp.path().join("existing-direct.bin");
    std::fs::write(&destination, b"user-owned").unwrap();

    let outcome = run_mirror_attempt(MirrorAttemptParams {
        url: format!("{}/download", server.uri()),
        download_id: DownloadId(306),
        segments_count: 1,
        client: reqwest::Client::new(),
        file_storage: Arc::new(FsFileStorage::new()),
        event_bus: Arc::new(CollectingEventBus::new()),
        dest_path: destination.clone(),
        pause_rx: watch::channel(false).1,
        user_cancel_token: CancellationToken::new(),
        attempt_token: CancellationToken::new(),
        min_segment_bytes: 1,
        dynamic_split_enabled: Arc::new(AtomicBool::new(false)),
        dynamic_split_min_remaining_bytes: Arc::new(AtomicU64::new(1)),
        resume_url: "https://example.com/file.bin".into(),
        source_policy: SourcePolicy::Direct,
        size_hint: None,
        resume_supported: None,
    })
    .await;

    assert!(matches!(outcome, AttemptOutcome::Failed(_)));
    assert_eq!(std::fs::read(destination).unwrap(), b"user-owned");
}

#[tokio::test]
async fn protected_hoster_attempt_rejects_resume_metadata_owned_by_another_download() {
    let server = MockServer::start().await;
    Mock::given(method("HEAD"))
        .and(path("/download"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "application/octet-stream")
                .insert_header("content-length", "4"),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/download"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"data"))
        .mount(&server)
        .await;
    let temp = tempfile::tempdir().unwrap();
    let destination = temp.path().join("collision.bin");
    std::fs::write(&destination, b"mine").unwrap();
    let storage = Arc::new(FsFileStorage::new());
    storage
        .write_meta(
            &destination,
            &DownloadMeta {
                download_id: DownloadId(999),
                url: "https://hoster.example/page".into(),
                file_name: "collision.bin".into(),
                total_bytes: Some(4),
                segments: Vec::new(),
                checksum_expected: None,
                created_at: 0,
                updated_at: 0,
            },
        )
        .unwrap();

    let outcome = run_mirror_attempt(MirrorAttemptParams {
        url: format!("{}/download", server.uri()),
        download_id: DownloadId(303),
        segments_count: 1,
        client: reqwest::Client::new(),
        file_storage: storage,
        event_bus: Arc::new(CollectingEventBus::new()),
        dest_path: destination.clone(),
        pause_rx: watch::channel(false).1,
        user_cancel_token: CancellationToken::new(),
        attempt_token: CancellationToken::new(),
        min_segment_bytes: 1,
        dynamic_split_enabled: Arc::new(AtomicBool::new(false)),
        dynamic_split_min_remaining_bytes: Arc::new(AtomicU64::new(1)),
        resume_url: "https://hoster.example/page".into(),
        source_policy: SourcePolicy::Protected { allow_html: false },
        size_hint: None,
        resume_supported: None,
    })
    .await;

    assert!(matches!(outcome, AttemptOutcome::Failed(_)));
    assert_eq!(std::fs::read(destination).unwrap(), b"mine");
}

#[tokio::test]
async fn protected_hoster_attempt_rejects_a_body_shorter_than_the_plugin_size_hint() {
    let server = MockServer::start().await;
    Mock::given(method("HEAD"))
        .and(path("/download"))
        .respond_with(
            ResponseTemplate::new(200).insert_header("content-type", "application/octet-stream"),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/download"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"data"))
        .mount(&server)
        .await;
    let temp = tempfile::tempdir().unwrap();
    let destination = temp.path().join("truncated.bin");

    let outcome = run_mirror_attempt(MirrorAttemptParams {
        url: format!("{}/download", server.uri()),
        download_id: DownloadId(304),
        segments_count: 1,
        client: reqwest::Client::new(),
        file_storage: Arc::new(FsFileStorage::new()),
        event_bus: Arc::new(CollectingEventBus::new()),
        dest_path: destination.clone(),
        pause_rx: watch::channel(false).1,
        user_cancel_token: CancellationToken::new(),
        attempt_token: CancellationToken::new(),
        min_segment_bytes: 1,
        dynamic_split_enabled: Arc::new(AtomicBool::new(false)),
        dynamic_split_min_remaining_bytes: Arc::new(AtomicU64::new(1)),
        resume_url: "https://hoster.example/page".into(),
        source_policy: SourcePolicy::Protected { allow_html: false },
        size_hint: Some(8),
        resume_supported: None,
    })
    .await;

    assert!(matches!(outcome, AttemptOutcome::Failed(_)));
    assert!(!destination.exists());
}

#[tokio::test]
async fn protected_hoster_attempt_rejects_content_length_conflicting_with_size_hint() {
    let server = MockServer::start().await;
    Mock::given(method("HEAD"))
        .and(path("/download"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "application/octet-stream")
                .insert_header("content-length", "4"),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/download"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"data"))
        .mount(&server)
        .await;
    let temp = tempfile::tempdir().unwrap();
    let destination = temp.path().join("size-conflict.bin");

    let outcome = run_mirror_attempt(MirrorAttemptParams {
        url: format!("{}/download", server.uri()),
        download_id: DownloadId(307),
        segments_count: 1,
        client: reqwest::Client::new(),
        file_storage: Arc::new(FsFileStorage::new()),
        event_bus: Arc::new(CollectingEventBus::new()),
        dest_path: destination.clone(),
        pause_rx: watch::channel(false).1,
        user_cancel_token: CancellationToken::new(),
        attempt_token: CancellationToken::new(),
        min_segment_bytes: 1,
        dynamic_split_enabled: Arc::new(AtomicBool::new(false)),
        dynamic_split_min_remaining_bytes: Arc::new(AtomicU64::new(1)),
        resume_url: "https://hoster.example/page".into(),
        source_policy: SourcePolicy::Protected { allow_html: false },
        size_hint: Some(8),
        resume_supported: None,
    })
    .await;

    assert!(matches!(outcome, AttemptOutcome::Failed(_)));
    assert!(!destination.exists());
}

#[tokio::test]
async fn unknown_remote_size_replaces_a_known_size_owned_artifact() {
    let server = MockServer::start().await;
    Mock::given(method("HEAD"))
        .and(path("/download"))
        .respond_with(
            ResponseTemplate::new(200).insert_header("content-type", "application/octet-stream"),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/download"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"data"))
        .mount(&server)
        .await;
    let temp = tempfile::tempdir().unwrap();
    let destination = temp.path().join("owned.bin");
    std::fs::write(&destination, b"stale-data").unwrap();
    let storage = Arc::new(FsFileStorage::new());
    storage
        .write_meta(
            &destination,
            &DownloadMeta {
                download_id: DownloadId(308),
                url: "https://hoster.example/page".into(),
                file_name: "owned.bin".into(),
                total_bytes: Some(10),
                segments: Vec::new(),
                checksum_expected: None,
                created_at: 0,
                updated_at: 0,
            },
        )
        .unwrap();

    let outcome = run_mirror_attempt(MirrorAttemptParams {
        url: format!("{}/download", server.uri()),
        download_id: DownloadId(308),
        segments_count: 1,
        client: reqwest::Client::new(),
        file_storage: storage,
        event_bus: Arc::new(CollectingEventBus::new()),
        dest_path: destination.clone(),
        pause_rx: watch::channel(false).1,
        user_cancel_token: CancellationToken::new(),
        attempt_token: CancellationToken::new(),
        min_segment_bytes: 1,
        dynamic_split_enabled: Arc::new(AtomicBool::new(false)),
        dynamic_split_min_remaining_bytes: Arc::new(AtomicU64::new(1)),
        resume_url: "https://hoster.example/page".into(),
        source_policy: SourcePolicy::Protected { allow_html: false },
        size_hint: None,
        resume_supported: None,
    })
    .await;

    assert!(matches!(outcome, AttemptOutcome::Completed));
    assert_eq!(std::fs::read(destination).unwrap(), b"data");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancellation_wins_against_blocked_jit_resolution_without_late_commit() {
    let storage = Arc::new(MockFileStorage::new());
    let bus = Arc::new(CollectingEventBus::new());
    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let release = Arc::new((Mutex::new(false), Condvar::new()));
    let resolver = Arc::new(BlockingSourceResolver {
        entered: Mutex::new(Some(entered_tx)),
        release: release.clone(),
        committed: AtomicBool::new(false),
        finished: AtomicBool::new(false),
    });
    let engine = make_engine(storage, bus.clone()).with_source_resolver(resolver.clone());
    let download = make_download(98, "https://1fichier.com/?abc123")
        .with_module_name("vortex-mod-1fichier".into())
        .with_account_id(AccountId::new("account-1"));
    engine.start(&download).expect("spawn download");
    tokio::task::spawn_blocking(move || entered_rx.recv_timeout(Duration::from_secs(1)))
        .await
        .unwrap()
        .expect("resolver entered");

    engine
        .cancel(download.id())
        .expect("cancel active download");
    assert!(
        bus.wait_for_event_async(
            |event| matches!(event, DomainEvent::DownloadCancelled { id } if id.0 == 98),
            Duration::from_millis(300),
        )
        .await,
        "cancellation must not wait for the blocking resolver"
    );

    let (released, condition) = &*release;
    *released.lock().unwrap() = true;
    condition.notify_all();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(1);
    while !resolver.finished.load(AtomicOrdering::SeqCst) && tokio::time::Instant::now() < deadline
    {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(resolver.finished.load(AtomicOrdering::SeqCst));
    assert!(!resolver.committed.load(AtomicOrdering::SeqCst));
}

// --- Tests ---
