use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::time::Duration;

use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::adapters::driven::filesystem::FsFileStorage;
use crate::domain::event::DomainEvent;
use crate::domain::model::captcha::CaptchaType;
use crate::domain::model::download::{Download, DownloadId, Url};
use crate::domain::model::meta::DownloadMeta;
use crate::domain::ports::driven::FileStorage;

use super::test_support::*;
use super::*;

#[test]
fn captcha_resolution_failure_emits_the_typed_challenge() {
    let event = source_resolution_event(
        DownloadId(42),
        &DomainError::CaptchaRequired {
            challenge_type: CaptchaType::Image,
            challenge_url: "https://hoster.example/file".into(),
            image_data: Some(vec![1, 2, 3]),
        },
        false,
    );

    assert_eq!(
        event,
        DomainEvent::CaptchaRequired {
            download_id: DownloadId(42),
            challenge_type: CaptchaType::Image,
            challenge_url: "https://hoster.example/file".into(),
            image_data: Some(Arc::<[u8]>::from(vec![1, 2, 3])),
        }
    );
}

#[test]
fn cloned_captcha_events_share_the_ephemeral_image_buffer() {
    let event = source_resolution_event(
        DownloadId(42),
        &DomainError::CaptchaRequired {
            challenge_type: CaptchaType::Image,
            challenge_url: "https://hoster.example/file".into(),
            image_data: Some(vec![1, 2, 3]),
        },
        false,
    );
    let cloned = event.clone();

    let DomainEvent::CaptchaRequired {
        image_data: Some(original),
        ..
    } = &event
    else {
        panic!("expected CAPTCHA event");
    };
    let DomainEvent::CaptchaRequired {
        image_data: Some(copy),
        ..
    } = &cloned
    else {
        panic!("expected cloned CAPTCHA event");
    };

    assert_eq!(original.as_ptr(), copy.as_ptr());
}

#[test]
fn mock_file_storage_does_not_probe_the_host_filesystem() {
    let temp = tempfile::tempdir().unwrap();
    let existing = temp.path().join("existing.bin");
    std::fs::write(&existing, b"user-owned").unwrap();

    assert!(!MockFileStorage::new().file_exists(&existing).unwrap());
}

async fn start_staggered_range_server(total_size: u64) -> (String, tokio::task::JoinHandle<()>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut request = vec![0; 4096];
                let mut read = 0;
                while read < request.len()
                    && !request[..read]
                        .windows(4)
                        .any(|window| window == b"\r\n\r\n")
                {
                    let Ok(bytes_read) = stream.read(&mut request[read..]).await else {
                        return;
                    };
                    if bytes_read == 0 {
                        return;
                    }
                    read += bytes_read;
                }
                let request = String::from_utf8_lossy(&request[..read]);
                if request.starts_with("HEAD ") {
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {total_size}\r\nAccept-Ranges: bytes\r\nConnection: close\r\n\r\n"
                    );
                    let _ = stream.write_all(response.as_bytes()).await;
                    return;
                }
                let Some(range) = request
                    .lines()
                    .find(|line| line.to_ascii_lowercase().starts_with("range:"))
                    .and_then(|line| line.split_once(':'))
                    .map(|(_, value)| value.trim())
                    .and_then(|value| value.strip_prefix("bytes="))
                    .and_then(|value| value.split_once('-'))
                else {
                    return;
                };
                let (Ok(start), Ok(end)) = (range.0.parse::<u64>(), range.1.parse::<u64>()) else {
                    return;
                };
                let body = vec![b'x'; (end - start + 1) as usize];
                let response = format!(
                    "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {start}-{end}/{total_size}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                if stream.write_all(response.as_bytes()).await.is_err() {
                    return;
                }
                if start == 0 {
                    tokio::time::sleep(Duration::from_millis(750)).await;
                    let _ = stream.write_all(&body).await;
                    return;
                }
                let chunk_size = body.len().div_ceil(4).max(1);
                for chunk in body.chunks(chunk_size) {
                    if stream.write_all(chunk).await.is_err() || stream.flush().await.is_err() {
                        return;
                    }
                    tokio::time::sleep(Duration::from_millis(300)).await;
                }
            });
        }
    });
    (format!("http://{address}/file"), task)
}

#[tokio::test]
async fn matching_resume_metadata_continues_from_the_persisted_offset() {
    let server = MockServer::start().await;
    Mock::given(method("HEAD"))
        .and(path("/file"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-length", "8")
                .insert_header("accept-ranges", "bytes"),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/file"))
        .and(header("range", "bytes=6-7"))
        .respond_with(
            ResponseTemplate::new(206)
                .insert_header("content-length", "2")
                .insert_header("content-range", "bytes 6-7/8")
                .set_body_bytes(b"gh"),
        )
        .mount(&server)
        .await;

    let temp = tempfile::tempdir().unwrap();
    let destination = temp.path().join("resume.bin");
    let storage = Arc::new(FsFileStorage::new());
    storage.create_file(&destination, 8).unwrap();
    storage.write_segment(&destination, 0, b"abcdef").unwrap();
    let stable_url = "https://example.com/stable";
    storage
        .write_meta(
            &destination,
            &DownloadMeta {
                download_id: DownloadId(312),
                url: stable_url.into(),
                file_name: "resume.bin".into(),
                total_bytes: Some(8),
                segments: vec![
                    crate::domain::model::meta::SegmentMeta {
                        id: 0,
                        start_byte: 0,
                        end_byte: 4,
                        downloaded_bytes: 4,
                        completed: true,
                    },
                    crate::domain::model::meta::SegmentMeta {
                        id: 1,
                        start_byte: 4,
                        end_byte: 8,
                        downloaded_bytes: 2,
                        completed: false,
                    },
                ],
                checksum_expected: None,
                created_at: 1,
                updated_at: 1,
            },
        )
        .unwrap();

    let outcome = run_mirror_attempt(MirrorAttemptParams {
        url: format!("{}/file", server.uri()),
        download_id: DownloadId(312),
        segments_count: 2,
        client: reqwest::Client::new(),
        file_storage: storage.clone(),
        event_bus: Arc::new(CollectingEventBus::new()),
        dest_path: destination.clone(),
        pause_rx: watch::channel(false).1,
        user_cancel_token: CancellationToken::new(),
        attempt_token: CancellationToken::new(),
        min_segment_bytes: 1,
        dynamic_split_enabled: Arc::new(AtomicBool::new(false)),
        dynamic_split_min_remaining_bytes: Arc::new(AtomicU64::new(1)),
        resume_url: stable_url.into(),
        source_policy: SourcePolicy::Direct,
        size_hint: Some(8),
        resume_supported: Some(true),
    })
    .await;

    assert!(matches!(outcome, AttemptOutcome::Completed));
    assert_eq!(std::fs::read(&destination).unwrap(), b"abcdefgh");
    assert_eq!(
        server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .filter(|request| request.method.as_str() == "GET")
            .count(),
        1
    );
}

#[tokio::test]
async fn interrupted_attempt_persists_partial_segment_progress() {
    let total_size = 8_192;
    let (url, server_task) = start_staggered_range_server(total_size).await;
    let temp = tempfile::tempdir().unwrap();
    let destination = temp.path().join("interrupted.bin");
    let storage = Arc::new(FsFileStorage::new());
    let user_cancel_token = CancellationToken::new();
    let attempt_token = user_cancel_token.child_token();
    let attempt = tokio::spawn(run_mirror_attempt(MirrorAttemptParams {
        url: url.clone(),
        download_id: DownloadId(313),
        segments_count: 2,
        client: reqwest::Client::new(),
        file_storage: storage.clone(),
        event_bus: Arc::new(CollectingEventBus::new()),
        dest_path: destination.clone(),
        pause_rx: watch::channel(false).1,
        user_cancel_token: user_cancel_token.clone(),
        attempt_token,
        min_segment_bytes: 1,
        dynamic_split_enabled: Arc::new(AtomicBool::new(false)),
        dynamic_split_min_remaining_bytes: Arc::new(AtomicU64::new(1)),
        resume_url: url,
        source_policy: SourcePolicy::Direct,
        size_hint: Some(total_size),
        resume_supported: Some(true),
    }));

    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let has_partial_segment = storage
                .read_meta(&destination)
                .expect("read in-progress metadata")
                .is_some_and(|metadata| {
                    metadata
                        .segments
                        .iter()
                        .any(|segment| segment.downloaded_bytes > 0 && !segment.completed)
                });
            if has_partial_segment {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("partial segment progress is persisted");
    user_cancel_token.cancel();
    let outcome = tokio::time::timeout(Duration::from_secs(3), attempt)
        .await
        .expect("attempt stops after cancellation")
        .expect("attempt task joins");
    server_task.abort();

    assert!(matches!(outcome, AttemptOutcome::Cancelled));
    let metadata = storage
        .read_meta(&destination)
        .unwrap()
        .expect("interrupted download retains resume metadata");
    assert_eq!(metadata.total_bytes, Some(total_size));
    assert!(
        metadata
            .segments
            .iter()
            .any(|segment| segment.downloaded_bytes > 0 && !segment.completed),
        "partial segment progress must be persisted"
    );
}

#[test]
fn segment_count_does_not_wrap_for_huge_downloads() {
    assert_eq!(
        segment_count_for_attempt(8, u64::from(u32::MAX) + 2, 1, true),
        8
    );
}

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
    let engine = make_engine(storage.clone(), bus.clone());

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
        .respond_with(ResponseTemplate::new(200).set_body_bytes(body.clone()))
        .mount(&server)
        .await;

    let storage = Arc::new(MockFileStorage::new());
    let bus = Arc::new(CollectingEventBus::new());
    let engine = make_engine(storage.clone(), bus.clone());

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
    let requests = server.received_requests().await.unwrap();
    assert!(
        requests
            .iter()
            .filter(|request| request.method.as_str() == "GET")
            .all(|request| !request.headers.contains_key("range")),
        "no-range fallback must issue a full GET"
    );
    let writes = storage.writes.lock().unwrap();
    assert_eq!(
        writes
            .iter()
            .flat_map(|(_, _, bytes)| bytes.iter().copied())
            .collect::<Vec<_>>(),
        body
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
                .set_delay(Duration::from_millis(500)),
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
    assert!(
        bus.wait_for_event_async(
            |event| matches!(event, DomainEvent::DownloadCancelled { id } if id.0 == 4),
            Duration::from_secs(3),
        )
        .await,
        "DownloadCancelled not received"
    );
}

#[tokio::test]
async fn terminal_destination_failure_does_not_emit_mirror_exhaustion() {
    let server = MockServer::start().await;
    Mock::given(method("HEAD"))
        .and(path("/file"))
        .respond_with(ResponseTemplate::new(200).insert_header("content-length", "4"))
        .mount(&server)
        .await;
    let temp = tempfile::tempdir().unwrap();
    let destination = temp.path().join("occupied");
    std::fs::create_dir(&destination).unwrap();
    let storage = Arc::new(FsFileStorage::new());
    let bus = Arc::new(CollectingEventBus::new());
    let engine = make_engine(storage, bus.clone());
    let download = Download::new(
        DownloadId(309),
        Url::new(&format!("{}/file", server.uri())).unwrap(),
        "occupied".into(),
        destination.to_string_lossy().into_owned(),
    );

    engine.start(&download).unwrap();
    assert!(
        bus.wait_for_event_async(
            |event| matches!(event, DomainEvent::DownloadFailed { id, .. } if id.0 == 309),
            Duration::from_secs(3),
        )
        .await
    );
    assert!(
        !bus.collected()
            .iter()
            .any(|event| matches!(event, DomainEvent::AllMirrorsExhausted { id } if id.0 == 309))
    );
}

#[tokio::test]
async fn test_dynamic_split_skipped_when_remaining_too_small() {
    // 2 KiB total, 4 segments, min_remaining 4 MiB → split must NOT trigger.
    let (url, server) = start_staggered_range_server(2048).await;

    let storage = Arc::new(MockFileStorage::new());
    let bus = Arc::new(CollectingEventBus::new());
    let engine = SegmentedDownloadEngine::new(reqwest::Client::new(), storage, bus.clone(), 4)
        .with_min_segment_bytes(256)
        .with_dynamic_split(true, 4); // 4 MiB threshold blocks 2 KiB file

    let download = make_download(70, &url).with_segments_count(4);
    engine.start(&download).unwrap();

    let found = bus
        .wait_for_event_async(
            |e| matches!(e, DomainEvent::DownloadCompleted { id } if id.0 == 70),
            Duration::from_secs(5),
        )
        .await;
    assert!(found, "download did not complete");

    let events = bus.collected();
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, DomainEvent::SegmentStarted { .. }))
            .count(),
        4,
        "threshold coverage must exercise a multi-segment attempt"
    );
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, DomainEvent::SegmentSplit { .. })),
        "no split should fire when remaining < threshold; got {events:?}"
    );
    server.abort();
}

#[tokio::test]
async fn test_dynamic_split_disabled_via_config_does_not_split() {
    let (url, server) = start_staggered_range_server(64 * 1024).await;

    let storage = Arc::new(MockFileStorage::new());
    let bus = Arc::new(CollectingEventBus::new());
    let engine = SegmentedDownloadEngine::new(reqwest::Client::new(), storage, bus.clone(), 4)
        .with_min_segment_bytes(1024)
        .with_dynamic_split(false, 0);

    let download = make_download(71, &url).with_segments_count(4);
    engine.start(&download).unwrap();

    let found = bus
        .wait_for_event_async(
            |e| matches!(e, DomainEvent::DownloadCompleted { id } if id.0 == 71),
            Duration::from_secs(5),
        )
        .await;
    assert!(found);
    let events = bus.collected();
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, DomainEvent::SegmentStarted { .. }))
            .count(),
        4,
        "disabled-split coverage must exercise a multi-segment attempt"
    );
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, DomainEvent::SegmentSplit { .. })),
        "split must not fire when disabled"
    );
    server.abort();
}

#[test]
fn test_pick_split_target_prefers_slowest_above_threshold() {
    let make = |start: u64, end: u64, downloaded: u64, age_ms: u64| SegmentRuntimeState {
        end_tx: watch::channel(end).0,
        progress: Arc::new(AtomicU64::new(downloaded)),
        started_at: std::time::Instant::now() - std::time::Duration::from_millis(age_ms),
        start_byte: start,
        initial_end: end,
        already_downloaded: 0,
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
        already_downloaded: 0,
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
        already_downloaded: 0,
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
        already_downloaded: 0,
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
