use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering as AtomicOrdering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::adapters::driven::filesystem::FsFileStorage;
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
}

struct RotatingHosterSourceResolver {
    calls: AtomicUsize,
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
        Ok(ResolvedDownloadSource::protected(
            "https://127.0.0.1/secret-token".into(),
        ))
    }
}

impl DownloadSourceResolver for RotatingHosterSourceResolver {
    fn requires_resolution(&self, _: &Download) -> Result<bool, DomainError> {
        Ok(true)
    }

    fn resolve(&self, _: &Download) -> Result<ResolvedDownloadSource, DomainError> {
        let call = self.calls.fetch_add(1, AtomicOrdering::SeqCst) + 1;
        Ok(
            ResolvedDownloadSource::protected(format!("https://1.1.1.1/file?capability={call}"))
                .with_request_headers(vec![(
                    "Authorization".into(),
                    format!("Bearer token-{call}"),
                )]),
        )
    }
}

#[tokio::test]
async fn premium_source_is_resolved_jit_and_blocked_before_private_network_access() {
    let storage = Arc::new(MockFileStorage::new());
    let bus = Arc::new(CollectingEventBus::new());
    let resolver = Arc::new(LoopbackSourceResolver {
        calls: AtomicUsize::new(0),
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
        let resolver = Arc::new(RotatingHosterSourceResolver {
            calls: AtomicUsize::new(0),
        });
        let download = make_download(200 + index as u64, "https://hoster.example/page")
            .with_module_name(service.into())
            .with_remote_metadata(Some(42), Some(true));

        let first = prepare_sources(
            download.clone(),
            Some(resolver.clone()),
            reqwest::Client::new(),
            CancellationToken::new(),
            ResolutionCancellation::default(),
        )
        .await
        .expect("first start resolves");
        let retry = prepare_sources(
            download,
            Some(resolver.clone()),
            reqwest::Client::new(),
            CancellationToken::new(),
            ResolutionCancellation::default(),
        )
        .await
        .expect("retry resolves again");

        assert_eq!(resolver.calls.load(AtomicOrdering::SeqCst), 2, "{service}");
        assert_ne!(first.urls, retry.urls, "{service}");
        assert!(first.policy.is_protected());
        assert!(retry.policy.is_protected());
        assert_eq!(first.size_hint, Some(42));
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
        file_storage: Arc::new(MockFileStorage::new()),
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
    })
    .await;

    assert!(matches!(outcome, AttemptOutcome::Failed(_)));
    assert!(!destination.exists());
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
