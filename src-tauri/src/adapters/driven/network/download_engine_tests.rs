use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::time::Duration;

use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::adapters::driven::filesystem::FsFileStorage;
use crate::domain::model::download::{Download, DownloadId, Url};
use crate::domain::model::meta::DownloadMeta;
use crate::domain::ports::driven::FileStorage;

use super::test_support::*;
use super::*;

#[tokio::test]
async fn test_start_spawns_download_and_completes() {
    let server = MockServer::start().await;
    let body = vec![b'a'; 1024];

    Mock::given(method("HEAD"))
        .and(path("/file"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-length", "1024")
                .insert_header("accept-ranges", "bytes"),
        )
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/file"))
        .respond_with(ResponseTemplate::new(206).set_body_bytes(body))
        .mount(&server)
        .await;

    let storage = Arc::new(MockFileStorage::new());
    let bus = Arc::new(CollectingEventBus::new());
    let engine = make_engine(storage, bus.clone());

    let url = format!("{}/file", server.uri());
    let download = make_download(1, &url);

    engine.start(&download).unwrap();

    let found = bus
        .wait_for_event_async(
            |e| matches!(e, DomainEvent::DownloadCompleted { id } if id.0 == 1),
            Duration::from_secs(5),
        )
        .await;

    assert!(found, "DownloadCompleted not received");

    let events = bus.collected();
    assert!(
        events
            .iter()
            .any(|e| matches!(e, DomainEvent::DownloadStarted { id } if id.0 == 1)),
        "DownloadStarted not published"
    );
}

#[tokio::test]
async fn test_start_fallback_single_segment_no_range() {
    let server = MockServer::start().await;
    let body = vec![b'b'; 512];

    Mock::given(method("HEAD"))
        .and(path("/norange"))
        .respond_with(
            ResponseTemplate::new(200).insert_header("content-length", "512"),
            // No accept-ranges header → single segment fallback
        )
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/norange"))
        .respond_with(ResponseTemplate::new(206).set_body_bytes(body))
        .mount(&server)
        .await;

    let storage = Arc::new(MockFileStorage::new());
    let bus = Arc::new(CollectingEventBus::new());
    let engine = make_engine(storage, bus.clone());

    let url = format!("{}/norange", server.uri());
    let download = make_download(2, &url);

    engine.start(&download).unwrap();

    let found = bus
        .wait_for_event_async(
            |e| {
                matches!(
                    e,
                    DomainEvent::DownloadCompleted { id } | DomainEvent::DownloadFailed { id, .. }
                    if id.0 == 2
                )
            },
            Duration::from_secs(5),
        )
        .await;

    assert!(found, "download did not finish");

    let events = bus.collected();
    assert!(
        events
            .iter()
            .any(|e| matches!(e, DomainEvent::DownloadCompleted { id } if id.0 == 2)),
        "expected DownloadCompleted, events: {events:?}"
    );
}

#[tokio::test]
async fn test_start_falls_back_to_get_when_head_returns_non_success() {
    let server = MockServer::start().await;
    let body = vec![b'g'; 256];

    Mock::given(method("HEAD"))
        .and(path("/head-blocked"))
        .respond_with(ResponseTemplate::new(405))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/head-blocked"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-length", "256")
                .set_body_bytes(body),
        )
        .mount(&server)
        .await;

    let storage = Arc::new(MockFileStorage::new());
    let bus = Arc::new(CollectingEventBus::new());
    let engine = make_engine(storage, bus.clone());

    let url = format!("{}/head-blocked", server.uri());
    let download = make_download(20, &url);

    engine.start(&download).unwrap();

    let found = bus
        .wait_for_event_async(
            |e| matches!(e, DomainEvent::DownloadCompleted { id } if id.0 == 20),
            Duration::from_secs(5),
        )
        .await;

    assert!(found, "download should complete via GET fallback");
}

#[tokio::test]
async fn test_pause_sends_signal() {
    let server = MockServer::start().await;
    // Slow server to keep download active
    let body = vec![b'p'; 64 * 1024];

    Mock::given(method("HEAD"))
        .and(path("/slow"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-length", &(64 * 1024u64).to_string())
                .insert_header("accept-ranges", "bytes"),
        )
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/slow"))
        .respond_with(
            ResponseTemplate::new(206)
                .set_body_bytes(body)
                .set_delay(Duration::from_secs(10)),
        )
        .mount(&server)
        .await;

    let storage = Arc::new(MockFileStorage::new());
    let bus = Arc::new(CollectingEventBus::new());
    let engine = make_engine(storage, bus.clone());

    let url = format!("{}/slow", server.uri());
    let download = make_download(3, &url);

    engine.start(&download).unwrap();

    // Wait for DownloadStarted before pausing
    bus.wait_for_event_async(
        |e| matches!(e, DomainEvent::DownloadStarted { id } if id.0 == 3),
        Duration::from_secs(3),
    )
    .await;

    let pause_result = engine.pause(DownloadId(3));
    assert!(pause_result.is_ok(), "pause should succeed");

    let events = bus.collected();
    assert!(
        events
            .iter()
            .any(|e| matches!(e, DomainEvent::DownloadPaused { id } if id.0 == 3)),
        "DownloadPaused not published"
    );

    // Clean up
    let _ = engine.cancel(DownloadId(3));
}

#[tokio::test]
async fn test_cancel_stops_download() {
    let server = MockServer::start().await;
    let body = vec![b'c'; 64 * 1024];

    Mock::given(method("HEAD"))
        .and(path("/cancel"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-length", &(64 * 1024u64).to_string())
                .insert_header("accept-ranges", "bytes"),
        )
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/cancel"))
        .respond_with(
            ResponseTemplate::new(206)
                .set_body_bytes(body)
                .set_delay(Duration::from_secs(10)),
        )
        .mount(&server)
        .await;

    let storage = Arc::new(MockFileStorage::new());
    let bus = Arc::new(CollectingEventBus::new());
    let engine = make_engine(storage, bus.clone());

    let url = format!("{}/cancel", server.uri());
    let download = make_download(4, &url);

    engine.start(&download).unwrap();

    // Wait for DownloadStarted
    bus.wait_for_event_async(
        |e| matches!(e, DomainEvent::DownloadStarted { id } if id.0 == 4),
        Duration::from_secs(3),
    )
    .await;

    let cancel_result = engine.cancel(DownloadId(4));
    assert!(cancel_result.is_ok(), "cancel should succeed");

    // Cancel is idempotent — second call succeeds (task removes itself on exit)
    let cancel_again = engine.cancel(DownloadId(4));
    assert!(
        cancel_again.is_ok(),
        "second cancel should succeed (idempotent)"
    );
}

#[tokio::test]
async fn test_dynamic_split_skipped_when_remaining_too_small() {
    // 2 KiB total, 4 segments, min_remaining 4 MiB → split must NOT trigger.
    let server = MockServer::start().await;
    let body = vec![b'a'; 2048];

    Mock::given(method("HEAD"))
        .and(path("/small"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-length", "2048")
                .insert_header("accept-ranges", "bytes"),
        )
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/small"))
        .respond_with(ResponseTemplate::new(206).set_body_bytes(body))
        .mount(&server)
        .await;

    let storage = Arc::new(MockFileStorage::new());
    let bus = Arc::new(CollectingEventBus::new());
    let engine = SegmentedDownloadEngine::new(reqwest::Client::new(), storage, bus.clone(), 4)
        .with_min_segment_bytes(256)
        .with_dynamic_split(true, 4); // 4 MiB threshold blocks 2 KiB file

    let url = format!("{}/small", server.uri());
    let download = make_download(70, &url);
    engine.start(&download).unwrap();

    let found = bus
        .wait_for_event_async(
            |e| matches!(e, DomainEvent::DownloadCompleted { id } if id.0 == 70),
            Duration::from_secs(5),
        )
        .await;
    assert!(found, "download did not complete");

    let events = bus.collected();
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, DomainEvent::SegmentSplit { .. })),
        "no split should fire when remaining < threshold; got {events:?}"
    );
}

#[tokio::test]
async fn test_dynamic_split_disabled_via_config_does_not_split() {
    let server = MockServer::start().await;
    let body = vec![b'x'; 64 * 1024];

    Mock::given(method("HEAD"))
        .and(path("/disabled"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-length", "65536")
                .insert_header("accept-ranges", "bytes"),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/disabled"))
        .respond_with(ResponseTemplate::new(206).set_body_bytes(body))
        .mount(&server)
        .await;

    let storage = Arc::new(MockFileStorage::new());
    let bus = Arc::new(CollectingEventBus::new());
    let engine = SegmentedDownloadEngine::new(reqwest::Client::new(), storage, bus.clone(), 4)
        .with_min_segment_bytes(1024)
        .with_dynamic_split(false, 0);

    let url = format!("{}/disabled", server.uri());
    let download = make_download(71, &url);
    engine.start(&download).unwrap();

    let found = bus
        .wait_for_event_async(
            |e| matches!(e, DomainEvent::DownloadCompleted { id } if id.0 == 71),
            Duration::from_secs(5),
        )
        .await;
    assert!(found);
    let events = bus.collected();
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, DomainEvent::SegmentSplit { .. })),
        "split must not fire when disabled"
    );
}

#[test]
fn test_pick_split_target_prefers_slowest_above_threshold() {
    let make = |start: u64, end: u64, downloaded: u64, age_ms: u64| SegmentRuntimeState {
        end_tx: watch::channel(end).0,
        progress: Arc::new(AtomicU64::new(downloaded)),
        started_at: std::time::Instant::now() - std::time::Duration::from_millis(age_ms),
        start_byte: start,
        initial_end: end,
        completed: false,
    };
    let segs = [
        // fast: 1 MiB downloaded in 1500 ms → ~700 KiB/s
        make(0, 16 * 1024 * 1024, 1024 * 1024, 1500),
        // slow: 100 KiB in 1000 ms → ~100 KiB/s, plenty of remaining
        make(16 * 1024 * 1024, 32 * 1024 * 1024, 100 * 1024, 1000),
        // tiny remaining → must be filtered
        make(32 * 1024 * 1024, 32 * 1024 * 1024 + 1024, 512, 600),
    ];
    let pick = pick_split_target(&segs, 4 * 1024 * 1024);
    assert_eq!(
        pick.map(|(i, _)| i),
        Some(1),
        "expected slot 1 (slowest with enough remaining), got {pick:?}"
    );
    let (_, split_at) = pick.unwrap();
    assert!(
        split_at > 16 * 1024 * 1024 + 100 * 1024,
        "split must be above current offset"
    );
    assert!(
        split_at < 32 * 1024 * 1024,
        "split must be below initial_end"
    );
}

#[test]
fn test_pick_split_target_returns_none_when_all_below_threshold() {
    let make = |start: u64, end: u64, downloaded: u64| SegmentRuntimeState {
        end_tx: watch::channel(end).0,
        progress: Arc::new(AtomicU64::new(downloaded)),
        started_at: std::time::Instant::now() - std::time::Duration::from_millis(800),
        start_byte: start,
        initial_end: end,
        completed: false,
    };
    let segs = [make(0, 1024, 100), make(1024, 2048, 1), make(2048, 3072, 1)];
    let pick = pick_split_target(&segs, 4 * 1024 * 1024);
    assert!(pick.is_none(), "got {pick:?}");
}

#[test]
fn test_pick_split_target_skips_fresh_segments() {
    // Brand-new split children should not be candidates: no throughput
    // sample yet (downloaded == 0) and elapsed below MIN_SPLIT_SAMPLE_DURATION.
    // A genuinely slow neighbor (1000 ms / 100 KiB) sits next to them.
    let mk = |start: u64, end: u64, downloaded: u64, age_ms: u64| SegmentRuntimeState {
        end_tx: watch::channel(end).0,
        progress: Arc::new(AtomicU64::new(downloaded)),
        started_at: std::time::Instant::now() - std::time::Duration::from_millis(age_ms),
        start_byte: start,
        initial_end: end,
        completed: false,
    };
    let segs = [
        // fresh child: 0 bytes, 50 ms — must be skipped despite being "slowest"
        mk(0, 16 * 1024 * 1024, 0, 50),
        // slightly older but still no sample — must be skipped
        mk(16 * 1024 * 1024, 32 * 1024 * 1024, 0, 200),
        // genuinely slow but mature
        mk(32 * 1024 * 1024, 48 * 1024 * 1024, 100 * 1024, 1000),
    ];
    let pick = pick_split_target(&segs, 4 * 1024 * 1024);
    assert_eq!(pick.map(|(i, _)| i), Some(2), "got {pick:?}");
}

#[test]
fn test_pick_split_target_skips_completed_segments() {
    // A completed slot must never be picked even if its throughput was the
    // slowest before completion.
    let mk = |start: u64, end: u64, downloaded: u64, completed: bool| SegmentRuntimeState {
        end_tx: watch::channel(end).0,
        progress: Arc::new(AtomicU64::new(downloaded)),
        started_at: std::time::Instant::now() - std::time::Duration::from_millis(1000),
        start_byte: start,
        initial_end: end,
        completed,
    };
    let segs = [
        // completed slow segment — must be ignored
        mk(0, 16 * 1024 * 1024, 16 * 1024 * 1024, true),
        // live, slower in absolute terms but only it is eligible
        mk(16 * 1024 * 1024, 32 * 1024 * 1024, 100 * 1024, false),
    ];
    let pick = pick_split_target(&segs, 4 * 1024 * 1024);
    assert_eq!(pick.map(|(i, _)| i), Some(1));
}

#[tokio::test]
async fn test_pause_unknown_id_returns_not_found() {
    let storage = Arc::new(MockFileStorage::new());
    let bus = Arc::new(CollectingEventBus::new());
    let engine = make_engine(storage, bus);

    let result = engine.pause(DownloadId(999));
    assert!(
        matches!(result, Err(DomainError::NotFound(_))),
        "expected NotFound, got {result:?}"
    );
}

#[tokio::test]
async fn test_cancel_unknown_id_returns_not_found() {
    let storage = Arc::new(MockFileStorage::new());
    let bus = Arc::new(CollectingEventBus::new());
    let engine = make_engine(storage, bus);

    let result = engine.cancel(DownloadId(888));
    assert!(
        matches!(result, Err(DomainError::NotFound(_))),
        "expected NotFound, got {result:?}"
    );
}

fn make_download_with_mirrors(id: u64, mirrors: Vec<crate::domain::model::Mirror>) -> Download {
    let download_id = DownloadId(id);
    // The canonical URL is intentionally invalid for the mock server so
    // failing this fallback still surfaces a clear test signal — we want
    // every fetch to flow through `mirrors`.
    let parsed_url = Url::new("https://invalid-canonical.example.invalid/file.bin").unwrap();
    Download::new(
        download_id,
        parsed_url,
        "test_file.bin".to_string(),
        "/tmp/test_file.bin".to_string(),
    )
    .with_mirrors(mirrors)
}

#[tokio::test]
async fn mirror_retry_never_cleans_artifacts_owned_by_another_download() {
    let server = MockServer::start().await;
    for route in ["/m1", "/m2"] {
        Mock::given(method("HEAD"))
            .and(path(route))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-length", "4")
                    .insert_header("accept-ranges", "bytes"),
            )
            .mount(&server)
            .await;
    }
    let mirrors = [90, 70]
        .into_iter()
        .enumerate()
        .map(|(index, priority)| {
            crate::domain::model::Mirror::new(
                Url::new(&format!("{}/m{}", server.uri(), index + 1)).unwrap(),
                priority,
                None,
            )
            .unwrap()
        })
        .collect();
    let temp = tempfile::tempdir().unwrap();
    let destination = temp.path().join("collision.bin");
    std::fs::write(&destination, b"mine").unwrap();
    let storage = Arc::new(FsFileStorage::new());
    storage
        .write_meta(
            &destination,
            &DownloadMeta {
                download_id: DownloadId(999),
                url: "https://invalid-canonical.example.invalid/file.bin".into(),
                file_name: "collision.bin".into(),
                total_bytes: Some(4),
                segments: Vec::new(),
                checksum_expected: None,
                created_at: 0,
                updated_at: 0,
            },
        )
        .unwrap();
    let bus = Arc::new(CollectingEventBus::new());
    let engine = make_engine(storage.clone(), bus.clone());
    let download = Download::new(
        DownloadId(305),
        Url::new("https://invalid-canonical.example.invalid/file.bin").unwrap(),
        "collision.bin".into(),
        destination.to_string_lossy().into_owned(),
    )
    .with_mirrors(mirrors);

    engine.start(&download).unwrap();
    assert!(
        bus.wait_for_event_async(
            |event| matches!(event, DomainEvent::DownloadFailed { id, .. } if id.0 == 305),
            Duration::from_secs(5),
        )
        .await
    );

    assert_eq!(std::fs::read(&destination).unwrap(), b"mine");
    assert!(storage.read_meta(&destination).unwrap().is_some());
    assert!(
        !bus.collected()
            .iter()
            .any(|event| matches!(event, DomainEvent::MirrorSwitched { .. }))
    );
}

#[tokio::test]
async fn test_three_mirrors_first_404_triggers_failover_to_second() {
    // 3 mirrors. First returns 404 for the GET range request so the
    // attempt fails; second succeeds. Engine must publish
    // MirrorSwitched then DownloadCompleted.
    let server = MockServer::start().await;
    let body = vec![b'm'; 256];

    // Mirror 1 — HEAD 404 (probe fail) so attempt fails fast.
    Mock::given(method("HEAD"))
        .and(path("/m1"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/m1"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;

    // Mirror 2 — works.
    Mock::given(method("HEAD"))
        .and(path("/m2"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-length", "256")
                .insert_header("accept-ranges", "bytes"),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/m2"))
        .respond_with(ResponseTemplate::new(206).set_body_bytes(body.clone()))
        .mount(&server)
        .await;

    // Mirror 3 — also works but should not be hit (m2 succeeds first).
    Mock::given(method("HEAD"))
        .and(path("/m3"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-length", "256")
                .insert_header("accept-ranges", "bytes"),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/m3"))
        .respond_with(ResponseTemplate::new(206).set_body_bytes(body))
        .mount(&server)
        .await;

    let mirrors = vec![
        crate::domain::model::Mirror::new(
            Url::new(&format!("{}/m1", server.uri())).unwrap(),
            90, // highest priority — tried first
            None,
        )
        .unwrap(),
        crate::domain::model::Mirror::new(
            Url::new(&format!("{}/m2", server.uri())).unwrap(),
            70,
            None,
        )
        .unwrap(),
        crate::domain::model::Mirror::new(
            Url::new(&format!("{}/m3", server.uri())).unwrap(),
            50,
            None,
        )
        .unwrap(),
    ];

    let storage = Arc::new(MockFileStorage::new());
    let bus = Arc::new(CollectingEventBus::new());
    let engine = make_engine(storage, bus.clone());

    let download = make_download_with_mirrors(100, mirrors);
    engine.start(&download).unwrap();

    let completed = bus
        .wait_for_event_async(
            |e| {
                matches!(
                    e,
                    DomainEvent::DownloadCompleted { id } | DomainEvent::DownloadFailed { id, .. }
                    if id.0 == 100
                )
            },
            Duration::from_secs(10),
        )
        .await;
    assert!(completed, "engine did not finish");

    let events = bus.collected();
    assert!(
        events
            .iter()
            .any(|e| matches!(e, DomainEvent::DownloadCompleted { id } if id.0 == 100)),
        "expected DownloadCompleted, events: {events:?}"
    );

    let switched: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            DomainEvent::MirrorSwitched {
                id,
                new_mirror_index,
                new_url,
            } if id.0 == 100 => Some((*new_mirror_index, new_url.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(
        switched.len(),
        1,
        "exactly one mirror switch expected, got {switched:?}"
    );
    let (idx, url) = &switched[0];
    assert_eq!(*idx, 1, "switched to slot 1 (priority 70)");
    assert!(url.ends_with("/m2"), "expected /m2 mirror url, got {url}");
}

#[tokio::test]
async fn test_all_mirrors_fail_publishes_download_failed() {
    let server = MockServer::start().await;

    for p in ["/all1", "/all2"] {
        Mock::given(method("HEAD"))
            .and(path(p))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(p))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;
    }

    let mirrors = vec![
        crate::domain::model::Mirror::new(
            Url::new(&format!("{}/all1", server.uri())).unwrap(),
            80,
            None,
        )
        .unwrap(),
        crate::domain::model::Mirror::new(
            Url::new(&format!("{}/all2", server.uri())).unwrap(),
            40,
            None,
        )
        .unwrap(),
    ];

    let storage = Arc::new(MockFileStorage::new());
    let bus = Arc::new(CollectingEventBus::new());
    let engine = make_engine(storage, bus.clone());

    let download = make_download_with_mirrors(101, mirrors);
    engine.start(&download).unwrap();

    let done = bus
        .wait_for_event_async(
            |e| matches!(e, DomainEvent::DownloadFailed { id, .. } if id.0 == 101),
            Duration::from_secs(10),
        )
        .await;
    assert!(done, "expected DownloadFailed after all mirrors exhausted");

    let events = bus.collected();
    // Exactly one MirrorSwitched (between mirror 1 and mirror 2).
    let switches: Vec<_> = events
        .iter()
        .filter(|e| matches!(e, DomainEvent::MirrorSwitched { id, .. } if id.0 == 101))
        .collect();
    assert_eq!(switches.len(), 1, "switched once before final failure");
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, DomainEvent::DownloadCompleted { id } if id.0 == 101)),
        "must not emit DownloadCompleted on full mirror exhaustion"
    );
}

#[tokio::test]
async fn test_priority_respected_highest_first() {
    // Priority order: low (10) < mid (50) < high (90). The engine
    // must pick `high` first; we confirm by failing only `high` and
    // observing exactly one MirrorSwitched ending on `mid`.
    let server = MockServer::start().await;
    let body = vec![b'p'; 128];

    // high — fails
    Mock::given(method("HEAD"))
        .and(path("/high"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/high"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&server)
        .await;
    // mid — succeeds
    Mock::given(method("HEAD"))
        .and(path("/mid"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-length", "128")
                .insert_header("accept-ranges", "bytes"),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/mid"))
        .respond_with(ResponseTemplate::new(206).set_body_bytes(body))
        .mount(&server)
        .await;
    // low — must not be reached
    Mock::given(method("HEAD"))
        .and(path("/low"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;

    // Insert in non-priority-order to assert the engine sorts.
    let mirrors = vec![
        crate::domain::model::Mirror::new(
            Url::new(&format!("{}/low", server.uri())).unwrap(),
            10,
            None,
        )
        .unwrap(),
        crate::domain::model::Mirror::new(
            Url::new(&format!("{}/high", server.uri())).unwrap(),
            90,
            None,
        )
        .unwrap(),
        crate::domain::model::Mirror::new(
            Url::new(&format!("{}/mid", server.uri())).unwrap(),
            50,
            None,
        )
        .unwrap(),
    ];

    let storage = Arc::new(MockFileStorage::new());
    let bus = Arc::new(CollectingEventBus::new());
    let engine = make_engine(storage, bus.clone());

    let download = make_download_with_mirrors(102, mirrors);
    engine.start(&download).unwrap();

    let done = bus
        .wait_for_event_async(
            |e| matches!(e, DomainEvent::DownloadCompleted { id } if id.0 == 102),
            Duration::from_secs(10),
        )
        .await;
    assert!(done, "expected DownloadCompleted via mid mirror");

    let events = bus.collected();
    let switches: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            DomainEvent::MirrorSwitched { id, new_url, .. } if id.0 == 102 => Some(new_url.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(switches.len(), 1, "exactly one switch (high → mid)");
    assert!(
        switches[0].ends_with("/mid"),
        "expected switch to /mid, got {}",
        switches[0]
    );
}
