use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

use crate::domain::event::DomainEvent;
use crate::domain::model::download::DownloadId;
use crate::domain::ports::driven::{EventBus, FileStorage};

use super::format_error_chain;

/// Typed error for segment download failures.
#[derive(Debug, PartialEq)]
pub(crate) enum SegmentError {
    Cancelled,
    Http(String),
    Storage(String),
    PauseChannelClosed,
}

/// Parameters for a single segment download.
pub(crate) struct SegmentParams {
    pub client: reqwest::Client,
    pub file_storage: Arc<dyn FileStorage>,
    pub event_bus: Arc<dyn EventBus>,
    pub download_id: DownloadId,
    pub segment_index: u32,
    pub url: String,
    pub start_byte: u64,
    /// Exclusive upper bound of this segment's byte range.
    pub end_byte: u64,
    pub already_downloaded: u64,
    /// Total size of the entire file (used in progress events).
    pub total_file_size: u64,
    pub dest_path: PathBuf,
    pub pause_rx: watch::Receiver<bool>,
    pub cancel_token: CancellationToken,
    /// Shared atomic counter for aggregate progress across all segments.
    pub shared_downloaded: Arc<AtomicU64>,
}

/// Downloads a single byte range and writes it to disk.
///
/// Returns the total number of bytes downloaded for this segment.
/// The caller (DownloadEngine orchestrator) handles retry logic.
pub(crate) async fn download_segment(params: SegmentParams) -> Result<u64, SegmentError> {
    let SegmentParams {
        client,
        file_storage,
        event_bus,
        download_id,
        segment_index,
        url,
        start_byte,
        end_byte,
        already_downloaded,
        total_file_size,
        dest_path,
        mut pause_rx,
        cancel_token,
        shared_downloaded,
    } = params;
    event_bus.publish(DomainEvent::SegmentStarted {
        download_id,
        segment_id: segment_index,
        start_byte,
        end_byte,
    });

    let effective_start = start_byte + already_downloaded;

    if effective_start >= end_byte {
        event_bus.publish(DomainEvent::SegmentCompleted {
            download_id,
            segment_id: segment_index,
        });
        return Ok(0);
    }

    // Build the request, conditionally adding Range header
    let mut req = client.get(&url);
    if end_byte != u64::MAX {
        let range_header = format!("bytes={}-{}", effective_start, end_byte - 1);
        tracing::debug!(
            download_id = download_id.0,
            segment_id = segment_index,
            range = %range_header,
            "starting segment download"
        );
        req = req.header("Range", &range_header);
    } else {
        tracing::debug!(
            download_id = download_id.0,
            segment_id = segment_index,
            "starting full download (no range)"
        );
    }

    let response = req.send().await.map_err(|e| {
        let msg = format!("HTTP request failed: {}", format_error_chain(&e));
        event_bus.publish(DomainEvent::SegmentFailed {
            download_id,
            segment_id: segment_index,
            error: msg.clone(),
        });
        SegmentError::Http(msg)
    })?;

    let status = response.status();
    if !status.is_success() {
        let msg = format!("HTTP error status: {status}");
        event_bus.publish(DomainEvent::SegmentFailed {
            download_id,
            segment_id: segment_index,
            error: msg.clone(),
        });
        return Err(SegmentError::Http(msg));
    }

    // If we requested a range but got 200 (not 206), the server ignored our Range header
    if effective_start > 0 && status == reqwest::StatusCode::OK {
        let msg = "server returned 200 instead of 206 for ranged request".to_string();
        event_bus.publish(DomainEvent::SegmentFailed {
            download_id,
            segment_id: segment_index,
            error: msg.clone(),
        });
        return Err(SegmentError::Http(msg));
    }

    let mut offset = effective_start;
    let mut bytes_downloaded: u64 = 0;
    let mut last_progress = Instant::now();
    let mut response = response;

    loop {
        if cancel_token.is_cancelled() {
            return Err(SegmentError::Cancelled);
        }

        // Check pause state — if paused, wait with cancellation support
        if *pause_rx.borrow() {
            loop {
                tokio::select! {
                    _ = cancel_token.cancelled() => {
                        return Err(SegmentError::Cancelled);
                    }
                    result = pause_rx.changed() => {
                        if result.is_err() {
                            return Err(SegmentError::PauseChannelClosed);
                        }
                        if !*pause_rx.borrow() {
                            break;
                        }
                    }
                }
            }
        }

        let chunk = tokio::time::timeout(Duration::from_secs(30), response.chunk())
            .await
            .map_err(|_| {
                let msg = "chunk read timed out (30s idle)".to_string();
                event_bus.publish(DomainEvent::SegmentFailed {
                    download_id,
                    segment_id: segment_index,
                    error: msg.clone(),
                });
                SegmentError::Http(msg)
            })?
            .map_err(|e| {
                let msg = format!("chunk read error: {}", format_error_chain(&e));
                event_bus.publish(DomainEvent::SegmentFailed {
                    download_id,
                    segment_id: segment_index,
                    error: msg.clone(),
                });
                SegmentError::Http(msg)
            })?;

        let Some(chunk) = chunk else {
            // Stream ended
            break;
        };

        let storage = file_storage.clone();
        let path = dest_path.clone();
        let mut data = chunk.to_vec();
        let mut chunk_len = data.len() as u64;

        // Clamp writes to segment boundary to prevent writing past end_byte
        if end_byte != u64::MAX && offset + chunk_len > end_byte {
            let allowed = end_byte.saturating_sub(offset) as usize;
            data.truncate(allowed);
            chunk_len = allowed as u64;
            if chunk_len == 0 {
                break;
            }
        }

        tokio::task::spawn_blocking(move || storage.write_segment(&path, offset, &data))
            .await
            .map_err(|e| SegmentError::Storage(e.to_string()))?
            .map_err(|e| {
                let msg = e.to_string();
                event_bus.publish(DomainEvent::SegmentFailed {
                    download_id,
                    segment_id: segment_index,
                    error: msg.clone(),
                });
                SegmentError::Storage(msg)
            })?;

        offset += chunk_len;
        bytes_downloaded += chunk_len;

        let total_so_far = shared_downloaded.fetch_add(chunk_len, Ordering::Relaxed) + chunk_len;

        if last_progress.elapsed() >= Duration::from_millis(500) {
            event_bus.publish(DomainEvent::DownloadProgress {
                id: download_id,
                downloaded_bytes: total_so_far,
                total_bytes: total_file_size,
            });
            last_progress = Instant::now();
        }
    }

    // Verify we received the expected number of bytes (for ranged segments)
    let expected_bytes = end_byte
        .saturating_sub(start_byte)
        .saturating_sub(already_downloaded);
    if end_byte != u64::MAX && bytes_downloaded < expected_bytes {
        let msg =
            format!("truncated response: got {bytes_downloaded} bytes, expected {expected_bytes}");
        event_bus.publish(DomainEvent::SegmentFailed {
            download_id,
            segment_id: segment_index,
            error: msg.clone(),
        });
        return Err(SegmentError::Http(msg));
    }

    event_bus.publish(DomainEvent::SegmentCompleted {
        download_id,
        segment_id: segment_index,
    });

    tracing::debug!(
        download_id = download_id.0,
        segment_id = segment_index,
        bytes = bytes_downloaded,
        "segment download complete"
    );

    Ok(bytes_downloaded)
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::path::Path;
    use std::sync::Mutex;
    use std::sync::atomic::AtomicU64;

    use wiremock::matchers::{header_exists, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use crate::domain::error::DomainError;
    use crate::domain::model::meta::DownloadMeta;

    // --- Mock implementations ---

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
    }

    impl EventBus for CollectingEventBus {
        fn publish(&self, event: DomainEvent) {
            self.events.lock().unwrap().push(event);
        }

        fn subscribe(&self, _handler: Box<dyn Fn(&DomainEvent) + Send + Sync + 'static>) {}
    }

    // --- Helpers ---

    fn make_client() -> reqwest::Client {
        reqwest::Client::new()
    }

    // --- Tests ---

    #[tokio::test]
    async fn test_segment_downloads_and_writes_to_file() {
        let server = MockServer::start().await;
        let body = vec![b'a'; 1000];

        Mock::given(method("GET"))
            .and(path("/file"))
            .and(header_exists("Range"))
            .respond_with(ResponseTemplate::new(206).set_body_bytes(body.clone()))
            .mount(&server)
            .await;

        let storage = Arc::new(MockFileStorage::new());
        let bus = Arc::new(CollectingEventBus::new());
        let (_, pause_rx) = watch::channel(false);
        let cancel = CancellationToken::new();
        let dest = PathBuf::from("/tmp/test_segment.bin");

        let result = download_segment(SegmentParams {
            client: make_client(),
            file_storage: storage.clone(),
            event_bus: bus.clone(),
            download_id: DownloadId(1),
            segment_index: 0,
            url: format!("{}/file", server.uri()),
            start_byte: 0,
            end_byte: 1000,
            already_downloaded: 0,
            total_file_size: 0,
            dest_path: dest.clone(),
            pause_rx,
            cancel_token: cancel,
            shared_downloaded: Arc::new(AtomicU64::new(0)),
        })
        .await;

        assert!(result.is_ok(), "expected Ok, got {result:?}");
        assert_eq!(result.unwrap(), 1000);

        let writes = storage.writes.lock().unwrap();
        assert!(!writes.is_empty(), "expected writes to file");

        let total_written: u64 = writes.iter().map(|(_, _, data)| data.len() as u64).sum();
        assert_eq!(total_written, 1000);

        // First write must start at offset 0
        assert_eq!(writes[0].1, 0);
    }

    #[tokio::test]
    async fn test_segment_cancellation_stops_download() {
        let server = MockServer::start().await;
        // Serve a large body so streaming has multiple chunks
        let body = vec![b'x'; 100_000];

        Mock::given(method("GET"))
            .and(path("/file"))
            .respond_with(ResponseTemplate::new(206).set_body_bytes(body))
            .mount(&server)
            .await;

        let storage = Arc::new(MockFileStorage::new());
        let bus = Arc::new(CollectingEventBus::new());
        let (_, pause_rx) = watch::channel(false);
        let cancel = CancellationToken::new();

        // Cancel immediately
        cancel.cancel();

        let result = download_segment(SegmentParams {
            client: make_client(),
            file_storage: storage.clone(),
            event_bus: bus.clone(),
            download_id: DownloadId(2),
            segment_index: 0,
            url: format!("{}/file", server.uri()),
            start_byte: 0,
            end_byte: 100_000,
            already_downloaded: 0,
            total_file_size: 0,
            dest_path: PathBuf::from("/tmp/cancel_test.bin"),
            pause_rx,
            cancel_token: cancel,
            shared_downloaded: Arc::new(AtomicU64::new(0)),
        })
        .await;

        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), SegmentError::Cancelled);
    }

    #[tokio::test]
    async fn test_segment_pause_and_resume() {
        let server = MockServer::start().await;
        let body = vec![b'p'; 1000];

        Mock::given(method("GET"))
            .and(path("/file"))
            .respond_with(ResponseTemplate::new(206).set_body_bytes(body))
            .mount(&server)
            .await;

        let storage = Arc::new(MockFileStorage::new());
        let bus = Arc::new(CollectingEventBus::new());
        let (pause_tx, pause_rx) = watch::channel(false);
        let cancel = CancellationToken::new();

        // Start paused, then resume after a short delay
        pause_tx.send(true).unwrap();

        let cancel_clone = cancel.clone();
        let pause_tx_clone = pause_tx.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            pause_tx_clone.send(false).unwrap();
            // Give the download time to complete, then cancel if not done
            tokio::time::sleep(Duration::from_millis(500)).await;
            cancel_clone.cancel();
        });

        let result = download_segment(SegmentParams {
            client: make_client(),
            file_storage: storage.clone(),
            event_bus: bus.clone(),
            download_id: DownloadId(3),
            segment_index: 0,
            url: format!("{}/file", server.uri()),
            start_byte: 0,
            end_byte: 1000,
            already_downloaded: 0,
            total_file_size: 0,
            dest_path: PathBuf::from("/tmp/pause_test.bin"),
            pause_rx,
            cancel_token: cancel,
            shared_downloaded: Arc::new(AtomicU64::new(0)),
        })
        .await;

        // Should complete after being unpaused, not be cancelled
        // (cancel fires at 550ms, download should finish in <500ms after unpausing)
        assert!(
            result.is_ok(),
            "expected completion after resume, got {result:?}"
        );
    }

    #[tokio::test]
    async fn test_segment_publishes_progress_events() {
        let server = MockServer::start().await;
        // Body larger than a single chunk to trigger progress events
        let body = vec![b'z'; 10_000];

        Mock::given(method("GET"))
            .and(path("/file"))
            .respond_with(ResponseTemplate::new(206).set_body_bytes(body))
            .mount(&server)
            .await;

        let storage = Arc::new(MockFileStorage::new());
        let bus = Arc::new(CollectingEventBus::new());
        let (_, pause_rx) = watch::channel(false);
        let cancel = CancellationToken::new();

        let _result = download_segment(SegmentParams {
            client: make_client(),
            file_storage: storage.clone(),
            event_bus: bus.clone(),
            download_id: DownloadId(4),
            segment_index: 0,
            url: format!("{}/file", server.uri()),
            start_byte: 0,
            end_byte: 10_000,
            already_downloaded: 0,
            total_file_size: 0,
            dest_path: PathBuf::from("/tmp/progress_test.bin"),
            pause_rx,
            cancel_token: cancel,
            shared_downloaded: Arc::new(AtomicU64::new(0)),
        })
        .await;

        let events = bus.collected();
        // At least SegmentStarted and SegmentCompleted must be present
        let has_started = events.iter().any(|e| {
            matches!(
                e,
                DomainEvent::SegmentStarted {
                    download_id: DownloadId(4),
                    segment_id: 0,
                    start_byte: 0,
                    end_byte: 10_000,
                }
            )
        });
        let has_completed = events.iter().any(|e| {
            matches!(
                e,
                DomainEvent::SegmentCompleted {
                    download_id: DownloadId(4),
                    segment_id: 0
                }
            )
        });
        assert!(has_started, "SegmentStarted not published");
        assert!(has_completed, "SegmentCompleted not published");
    }

    #[tokio::test]
    async fn test_segment_publishes_start_and_complete() {
        let server = MockServer::start().await;
        let body = vec![b's'; 512];

        Mock::given(method("GET"))
            .and(path("/file"))
            .respond_with(ResponseTemplate::new(206).set_body_bytes(body))
            .mount(&server)
            .await;

        let storage = Arc::new(MockFileStorage::new());
        let bus = Arc::new(CollectingEventBus::new());
        let (_, pause_rx) = watch::channel(false);
        let cancel = CancellationToken::new();

        let result = download_segment(SegmentParams {
            client: make_client(),
            file_storage: storage.clone(),
            event_bus: bus.clone(),
            download_id: DownloadId(5),
            segment_index: 1,
            url: format!("{}/file", server.uri()),
            start_byte: 0,
            end_byte: 512,
            already_downloaded: 0,
            total_file_size: 0,
            dest_path: PathBuf::from("/tmp/events_test.bin"),
            pause_rx,
            cancel_token: cancel,
            shared_downloaded: Arc::new(AtomicU64::new(0)),
        })
        .await;

        assert!(result.is_ok());

        let events = bus.collected();
        assert!(
            events.iter().any(|e| matches!(
                e,
                DomainEvent::SegmentStarted {
                    download_id: DownloadId(5),
                    segment_id: 1,
                    start_byte: 0,
                    end_byte: 512,
                }
            )),
            "SegmentStarted missing"
        );
        assert!(
            events.iter().any(|e| matches!(
                e,
                DomainEvent::SegmentCompleted {
                    download_id: DownloadId(5),
                    segment_id: 1
                }
            )),
            "SegmentCompleted missing"
        );
    }

    #[tokio::test]
    async fn test_segment_already_completed() {
        // already_downloaded == end_byte - start_byte  →  segment done, return Ok(0)
        let storage = Arc::new(MockFileStorage::new());
        let bus = Arc::new(CollectingEventBus::new());
        let (_, pause_rx) = watch::channel(false);
        let cancel = CancellationToken::new();

        let result = download_segment(SegmentParams {
            client: make_client(),
            file_storage: storage.clone(),
            event_bus: bus.clone(),
            download_id: DownloadId(6),
            segment_index: 0,
            url: "http://unused.example.com/file".to_string(),
            start_byte: 0,
            end_byte: 1000,
            already_downloaded: 1000, // already fully downloaded
            total_file_size: 1000,
            dest_path: PathBuf::from("/tmp/done_test.bin"),
            pause_rx,
            cancel_token: cancel,
            shared_downloaded: Arc::new(AtomicU64::new(0)),
        })
        .await;

        assert_eq!(
            result,
            Ok(0),
            "expected Ok(0) for already-completed segment"
        );

        let events = bus.collected();
        assert!(
            events.iter().any(|e| matches!(
                e,
                DomainEvent::SegmentStarted {
                    download_id: DownloadId(6),
                    segment_id: 0,
                    start_byte: 0,
                    end_byte: 1000,
                }
            )),
            "SegmentStarted missing"
        );
        assert!(
            events.iter().any(|e| matches!(
                e,
                DomainEvent::SegmentCompleted {
                    download_id: DownloadId(6),
                    segment_id: 0
                }
            )),
            "SegmentCompleted missing"
        );
        // No writes to storage
        assert!(storage.writes.lock().unwrap().is_empty());
    }
}
