use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex};

use tokio::sync::watch;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

use crate::domain::error::DomainError;
use crate::domain::event::DomainEvent;
use crate::domain::model::download::{Download, DownloadId};
use crate::domain::ports::driven::{DownloadEngine, EventBus, FileStorage};

use super::segment_worker::{SegmentError, SegmentParams, download_segment};

struct ActiveDownload {
    cancel_token: CancellationToken,
    pause_sender: watch::Sender<bool>,
}

pub struct SegmentedDownloadEngine {
    client: reqwest::Client,
    file_storage: Arc<dyn FileStorage>,
    event_bus: Arc<dyn EventBus>,
    default_segments: u32,
    min_segment_bytes: u64,
    active_downloads: Arc<Mutex<HashMap<DownloadId, ActiveDownload>>>,
}

impl SegmentedDownloadEngine {
    pub fn new(
        client: reqwest::Client,
        file_storage: Arc<dyn FileStorage>,
        event_bus: Arc<dyn EventBus>,
        default_segments: u32,
    ) -> Self {
        Self {
            client,
            file_storage,
            event_bus,
            default_segments: default_segments.max(1),
            min_segment_bytes: 64 * 1024,
            active_downloads: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn with_min_segment_bytes(mut self, min_bytes: u64) -> Self {
        self.min_segment_bytes = min_bytes.max(1);
        self
    }
}

impl DownloadEngine for SegmentedDownloadEngine {
    fn start(&self, download: &Download) -> Result<(), DomainError> {
        let download_id = download.id();
        let url = download.url().as_str().to_string();
        // destination_path already contains the complete file path (dir + filename).
        // Do NOT join file_name again — that would produce "dir/file.bin/file.bin".
        let dest_path = PathBuf::from(download.destination_path());
        let segments_count = if download.segments_count() == 0 {
            self.default_segments
        } else {
            download.segments_count()
        };

        let cancel_token = CancellationToken::new();
        let (pause_tx, pause_rx) = watch::channel(false);

        {
            let mut map = self
                .active_downloads
                .lock()
                .expect("active_downloads lock poisoned");
            if map.contains_key(&download_id) {
                return Err(DomainError::AlreadyExists(format!(
                    "download {}",
                    download_id.0
                )));
            }
            map.insert(
                download_id,
                ActiveDownload {
                    cancel_token: cancel_token.clone(),
                    pause_sender: pause_tx,
                },
            );
        }

        let client = self.client.clone();
        let file_storage = self.file_storage.clone();
        let event_bus = self.event_bus.clone();
        let active_downloads = self.active_downloads.clone();
        let min_segment_bytes = self.min_segment_bytes;

        tokio::spawn(async move {
            // Perform HEAD request to determine size and range support
            let head_result = client.head(&url).send().await;

            let (total_size, supports_range) = match head_result {
                Ok(resp) => {
                    let content_length = resp
                        .headers()
                        .get("content-length")
                        .and_then(|v| v.to_str().ok())
                        .and_then(|s| s.parse::<u64>().ok())
                        .unwrap_or(0);
                    let accepts_ranges = resp
                        .headers()
                        .get("accept-ranges")
                        .and_then(|v| v.to_str().ok())
                        .map(|v| v.eq_ignore_ascii_case("bytes"))
                        .unwrap_or(false);
                    (content_length, accepts_ranges)
                }
                Err(e) => {
                    tracing::error!(
                        download_id = download_id.0,
                        error = %e,
                        "HEAD request failed"
                    );
                    event_bus.publish(DomainEvent::DownloadFailed {
                        id: download_id,
                        error: format!("HEAD request failed: {e}"),
                    });
                    active_downloads
                        .lock()
                        .expect("active_downloads lock poisoned")
                        .remove(&download_id);
                    return;
                }
            };

            // Check if cancelled during HEAD request
            if cancel_token.is_cancelled() {
                event_bus.publish(DomainEvent::DownloadCancelled { id: download_id });
                active_downloads
                    .lock()
                    .expect("active_downloads lock poisoned")
                    .remove(&download_id);
                return;
            }

            // Pre-allocate file if size is known
            if total_size > 0 {
                let storage = file_storage.clone();
                let path = dest_path.clone();
                match tokio::task::spawn_blocking(move || storage.create_file(&path, total_size))
                    .await
                {
                    Err(e) => {
                        tracing::error!(
                            download_id = download_id.0,
                            error = %e,
                            "spawn_blocking for create_file panicked"
                        );
                        event_bus.publish(DomainEvent::DownloadFailed {
                            id: download_id,
                            error: format!("file pre-allocation failed: {e}"),
                        });
                        active_downloads
                            .lock()
                            .expect("active_downloads lock poisoned")
                            .remove(&download_id);
                        return;
                    }
                    Ok(Err(e)) => {
                        event_bus.publish(DomainEvent::DownloadFailed {
                            id: download_id,
                            error: format!("file pre-allocation failed: {e}"),
                        });
                        active_downloads
                            .lock()
                            .expect("active_downloads lock poisoned")
                            .remove(&download_id);
                        return;
                    }
                    Ok(Ok(())) => {}
                }
            }

            // Determine number of segments
            let num_segments = if supports_range && total_size > 0 {
                segments_count
                    .min((total_size / min_segment_bytes).max(1) as u32)
                    .max(1)
            } else {
                1
            };

            // Calculate segment byte ranges
            // end_byte == u64::MAX signals the worker to omit the Range header
            let segments: Vec<(u64, u64)> = if supports_range && total_size > 0 && num_segments > 1
            {
                let segment_size = total_size / num_segments as u64;
                (0..num_segments)
                    .map(|i| {
                        let start = i as u64 * segment_size;
                        let end = if i == num_segments - 1 {
                            total_size
                        } else {
                            (i as u64 + 1) * segment_size
                        };
                        (start, end)
                    })
                    .collect()
            } else if supports_range && total_size > 0 {
                // Single segment with range support: use exact range
                vec![(0, total_size)]
            } else {
                // No range support or unknown size: download without Range header
                vec![(0, u64::MAX)]
            };

            // Check if cancelled during setup
            if cancel_token.is_cancelled() {
                event_bus.publish(DomainEvent::DownloadCancelled { id: download_id });
                active_downloads
                    .lock()
                    .expect("active_downloads lock poisoned")
                    .remove(&download_id);
                return;
            }

            event_bus.publish(DomainEvent::DownloadStarted { id: download_id });

            let shared_downloaded = Arc::new(AtomicU64::new(0));
            let mut join_set = JoinSet::new();
            for (index, (start, end)) in segments.iter().enumerate() {
                join_set.spawn(download_segment(SegmentParams {
                    client: client.clone(),
                    file_storage: file_storage.clone(),
                    event_bus: event_bus.clone(),
                    download_id,
                    segment_index: index as u32,
                    url: url.clone(),
                    start_byte: *start,
                    end_byte: *end,
                    already_downloaded: 0,
                    total_file_size: total_size,
                    dest_path: dest_path.clone(),
                    pause_rx: pause_rx.clone(),
                    cancel_token: cancel_token.clone(),
                    shared_downloaded: shared_downloaded.clone(),
                }));
            }

            let mut failed = false;
            let mut error_msg = String::new();

            while let Some(result) = join_set.join_next().await {
                match result {
                    Ok(Ok(_bytes)) => {}
                    Ok(Err(e)) => match e {
                        SegmentError::Cancelled => {
                            cancel_token.cancel();
                        }
                        _ => {
                            if failed {
                                tracing::warn!(
                                    download_id = download_id.0,
                                    previous_error = %error_msg,
                                    "additional segment failure (overwriting previous error)"
                                );
                            }
                            error_msg = format!("{e:?}");
                            failed = true;
                            cancel_token.cancel();
                        }
                    },
                    Err(e) => {
                        error_msg = format!("segment task panicked: {e}");
                        failed = true;
                        cancel_token.cancel();
                    }
                }
            }

            if failed {
                event_bus.publish(DomainEvent::DownloadFailed {
                    id: download_id,
                    error: error_msg,
                });
            } else if cancel_token.is_cancelled() {
                event_bus.publish(DomainEvent::DownloadCancelled { id: download_id });
            } else {
                event_bus.publish(DomainEvent::DownloadCompleted { id: download_id });
            }

            active_downloads
                .lock()
                .expect("active_downloads lock poisoned")
                .remove(&download_id);
        });

        Ok(())
    }

    fn pause(&self, id: DownloadId) -> Result<(), DomainError> {
        {
            let map = self
                .active_downloads
                .lock()
                .expect("active_downloads lock poisoned");
            let active = map
                .get(&id)
                .ok_or_else(|| DomainError::NotFound(format!("download {}", id.0)))?;
            let _ = active.pause_sender.send(true);
        }
        // Guard dropped — safe to publish without deadlock risk
        self.event_bus.publish(DomainEvent::DownloadPaused { id });
        Ok(())
    }

    fn resume(&self, id: DownloadId) -> Result<(), DomainError> {
        {
            let map = self
                .active_downloads
                .lock()
                .expect("active_downloads lock poisoned");
            let active = map
                .get(&id)
                .ok_or_else(|| DomainError::NotFound(format!("download {}", id.0)))?;
            let _ = active.pause_sender.send(false);
        }
        // Guard dropped — safe to publish without deadlock risk
        self.event_bus.publish(DomainEvent::DownloadResumed { id });
        Ok(())
    }

    fn cancel(&self, id: DownloadId) -> Result<(), DomainError> {
        // Don't remove from map — the spawned task removes itself on exit.
        // This prevents a new start() for the same ID from racing with
        // the old task that is still shutting down.
        let map = self
            .active_downloads
            .lock()
            .expect("active_downloads lock poisoned");
        let active = map
            .get(&id)
            .ok_or_else(|| DomainError::NotFound(format!("download {}", id.0)))?;
        active.cancel_token.cancel();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::path::Path;
    use std::sync::Mutex;
    use std::time::Duration;

    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use crate::domain::model::download::{Download, DownloadId, Url};
    use crate::domain::model::meta::DownloadMeta;
    use crate::domain::ports::driven::{EventBus, FileStorage};

    // --- Mock types ---

    type WriteRecord = (PathBuf, u64, Vec<u8>);

    struct MockFileStorage {
        writes: Arc<Mutex<Vec<WriteRecord>>>,
    }

    impl MockFileStorage {
        fn new() -> Self {
            Self {
                writes: Arc::new(Mutex::new(Vec::new())),
            }
        }
    }

    impl FileStorage for MockFileStorage {
        fn create_file(&self, _path: &Path, _size: u64) -> Result<(), DomainError> {
            Ok(())
        }

        fn write_segment(&self, path: &Path, offset: u64, data: &[u8]) -> Result<(), DomainError> {
            self.writes
                .lock()
                .unwrap()
                .push((path.to_path_buf(), offset, data.to_vec()));
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

    struct CollectingEventBus {
        events: Arc<Mutex<Vec<DomainEvent>>>,
    }

    impl CollectingEventBus {
        fn new() -> Self {
            Self {
                events: Arc::new(Mutex::new(Vec::new())),
            }
        }

        fn collected(&self) -> Vec<DomainEvent> {
            self.events.lock().unwrap().clone()
        }

        async fn wait_for_event_async<F>(&self, predicate: F, timeout: Duration) -> bool
        where
            F: Fn(&DomainEvent) -> bool,
        {
            let deadline = tokio::time::Instant::now() + timeout;
            loop {
                if self.collected().iter().any(&predicate) {
                    return true;
                }
                if tokio::time::Instant::now() >= deadline {
                    return false;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        }
    }

    impl EventBus for CollectingEventBus {
        fn publish(&self, event: DomainEvent) {
            self.events.lock().unwrap().push(event);
        }

        fn subscribe(&self, _handler: Box<dyn Fn(&DomainEvent) + Send + Sync + 'static>) {}
    }

    fn make_download(id: u64, url: &str) -> Download {
        let download_id = DownloadId(id);
        let parsed_url = Url::new(url).unwrap();
        // destination_path must be the full file path (dir + filename),
        // matching how StartDownloadCommand builds it in production.
        Download::new(
            download_id,
            parsed_url,
            "test_file.bin".to_string(),
            "/tmp/test_file.bin".to_string(),
        )
    }

    fn make_engine(
        storage: Arc<dyn FileStorage>,
        bus: Arc<dyn EventBus>,
    ) -> SegmentedDownloadEngine {
        SegmentedDownloadEngine::new(reqwest::Client::new(), storage, bus, 4)
    }

    // --- Tests ---

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
}
