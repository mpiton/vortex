use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use tokio::sync::watch;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

use crate::domain::error::DomainError;
use crate::domain::event::DomainEvent;
use crate::domain::model::download::{Download, DownloadId};
use crate::domain::model::meta::{DownloadMeta, SegmentMeta};
use crate::domain::ports::driven::{DownloadEngine, EventBus, FileStorage};

use super::format_error_chain;
use super::segment_worker::{SegmentError, SegmentParams, download_segment};

struct ActiveDownload {
    cancel_token: CancellationToken,
    pause_sender: watch::Sender<bool>,
}

/// Runtime state of one in-flight segment, tracked by the engine so it can
/// shrink the segment's range and observe its throughput for dynamic split.
struct SegmentRuntimeState {
    end_tx: watch::Sender<u64>,
    progress: Arc<AtomicU64>,
    started_at: std::time::Instant,
    start_byte: u64,
    initial_end: u64,
}

/// Pick the slowest active segment whose remaining range is large enough
/// to benefit from a split. Returns the slot index and the byte at which
/// to split (midpoint of the remaining range).
fn pick_split_target(
    segments: &[Option<SegmentRuntimeState>],
    min_remaining_bytes: u64,
) -> Option<(usize, u64)> {
    let mut slowest: Option<(usize, f64, u64)> = None;
    for (idx, slot) in segments.iter().enumerate() {
        let Some(state) = slot else { continue };
        if state.initial_end == u64::MAX {
            continue; // unbounded segments cannot be split
        }
        let downloaded = state.progress.load(Ordering::Relaxed);
        let current_offset = state.start_byte.saturating_add(downloaded);
        if current_offset >= state.initial_end {
            continue; // already at end — completion event will fire shortly
        }
        let remaining = state.initial_end - current_offset;
        if remaining < min_remaining_bytes.max(1) {
            continue;
        }
        let split_at = current_offset.saturating_add(remaining / 2);
        if split_at <= current_offset || split_at >= state.initial_end {
            continue;
        }
        let elapsed = state.started_at.elapsed().as_secs_f64().max(1e-3);
        let bps = downloaded as f64 / elapsed;
        match slowest {
            None => slowest = Some((idx, bps, split_at)),
            Some((_, prev_bps, _)) if bps < prev_bps => {
                slowest = Some((idx, bps, split_at));
            }
            _ => {}
        }
    }
    slowest.map(|(idx, _, split_at)| (idx, split_at))
}

/// Atomically rewrite `.vortex-meta` after a dynamic split so resume after a
/// crash sees the updated segment topology. A failure here only logs — the
/// in-memory split is still valid for the live download.
async fn persist_split_meta(
    file_storage: &Arc<dyn FileStorage>,
    dest_path: &Path,
    download_id: DownloadId,
    url: &str,
    total_size: u64,
    active_segments: &[Option<SegmentRuntimeState>],
) {
    let segments_meta: Vec<SegmentMeta> = active_segments
        .iter()
        .enumerate()
        .filter_map(|(i, slot)| {
            slot.as_ref().map(|st| SegmentMeta {
                id: i as u32,
                start_byte: st.start_byte,
                end_byte: st.initial_end,
                downloaded_bytes: st.progress.load(Ordering::Relaxed),
                completed: false,
            })
        })
        .collect();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let file_name = dest_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_string();
    let snapshot = DownloadMeta {
        download_id,
        url: url.to_string(),
        file_name,
        total_bytes: Some(total_size),
        segments: segments_meta,
        checksum_expected: None,
        created_at: now,
        updated_at: now,
    };
    let storage = file_storage.clone();
    let path = dest_path.to_path_buf();
    let join = tokio::task::spawn_blocking(move || storage.write_meta(&path, &snapshot)).await;
    match join {
        Ok(Ok(())) => {}
        Ok(Err(e)) => tracing::warn!(
            download_id = download_id.0,
            error = %e,
            "persist meta after split failed (download still proceeds)"
        ),
        Err(e) => tracing::warn!(
            download_id = download_id.0,
            error = %e,
            "persist meta after split task panicked"
        ),
    }
}

pub struct SegmentedDownloadEngine {
    client: reqwest::Client,
    file_storage: Arc<dyn FileStorage>,
    event_bus: Arc<dyn EventBus>,
    default_segments: u32,
    min_segment_bytes: u64,
    dynamic_split_enabled: Arc<AtomicBool>,
    dynamic_split_min_remaining_bytes: Arc<AtomicU64>,
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
            dynamic_split_enabled: Arc::new(AtomicBool::new(true)),
            dynamic_split_min_remaining_bytes: Arc::new(AtomicU64::new(4 * 1024 * 1024)),
            active_downloads: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn with_min_segment_bytes(mut self, min_bytes: u64) -> Self {
        self.min_segment_bytes = min_bytes.max(1);
        self
    }

    /// Configure runtime re-splitting of slow segments. PRD §7.1.
    /// `min_remaining_mb == 0` disables the size gate entirely; the engine
    /// then only refuses to split if the candidate has 0 bytes left.
    pub fn with_dynamic_split(self, enabled: bool, min_remaining_mb: u64) -> Self {
        self.set_dynamic_split(enabled, min_remaining_mb);
        self
    }

    /// Update dynamic-split runtime parameters live. Used by the engine
    /// config bridge so settings changes from the UI take effect on
    /// already-running and newly-started downloads without restart.
    pub fn set_dynamic_split(&self, enabled: bool, min_remaining_mb: u64) {
        self.dynamic_split_enabled.store(enabled, Ordering::Relaxed);
        self.dynamic_split_min_remaining_bytes.store(
            min_remaining_mb.saturating_mul(1024 * 1024),
            Ordering::Relaxed,
        );
    }

    /// Read back the current dynamic-split parameters as `(enabled, min_remaining_bytes)`.
    /// Lets the bridge tests prove that a `SettingsUpdated` event actually
    /// reaches the engine; also useful for diagnostics on a running download.
    pub fn dynamic_split_state(&self) -> (bool, u64) {
        (
            self.dynamic_split_enabled.load(Ordering::Relaxed),
            self.dynamic_split_min_remaining_bytes
                .load(Ordering::Relaxed),
        )
    }

    async fn probe_remote_metadata(
        client: &reqwest::Client,
        url: &str,
    ) -> Result<(u64, bool), reqwest::Error> {
        let response = match client.head(url).send().await {
            Ok(response) if response.status().is_success() => response,
            Ok(response) => {
                tracing::warn!(
                    url,
                    status = %response.status(),
                    "HEAD probe returned non-success status, falling back to GET metadata probe"
                );
                client.get(url).send().await?
            }
            Err(err) => {
                tracing::warn!(
                    url,
                    error = %format_error_chain(&err),
                    "HEAD probe failed, falling back to GET metadata probe"
                );
                client.get(url).send().await?
            }
        };

        let content_length = response
            .headers()
            .get("content-length")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0);
        let accepts_ranges = response
            .headers()
            .get("accept-ranges")
            .and_then(|v| v.to_str().ok())
            .map(|v| v.eq_ignore_ascii_case("bytes"))
            .unwrap_or(false);

        Ok((content_length, accepts_ranges))
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
        let dynamic_split_enabled = self.dynamic_split_enabled.clone();
        let dynamic_split_min_remaining_bytes = self.dynamic_split_min_remaining_bytes.clone();

        tokio::spawn(async move {
            let (total_size, supports_range) =
                match Self::probe_remote_metadata(&client, &url).await {
                    Ok(metadata) => metadata,
                    Err(e) => {
                        tracing::error!(
                            download_id = download_id.0,
                            error = %format_error_chain(&e),
                            "metadata probe failed"
                        );
                        event_bus.publish(DomainEvent::DownloadFailed {
                            id: download_id,
                            error: format!("metadata probe failed: {}", format_error_chain(&e)),
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

            // Pre-allocate file if size is known.
            // If a stale file exists from a previous interrupted attempt with no
            // resume state (no .vortex-meta sidecar), delete it first so
            // create_new(true) in create_file can succeed.
            if total_size > 0 {
                let storage = file_storage.clone();
                let path = dest_path.clone();
                match tokio::task::spawn_blocking(move || {
                    if path.exists() {
                        let has_meta = storage.read_meta(&path).ok().flatten().is_some();
                        if !has_meta {
                            // Orphaned file from a previous failed attempt — remove it.
                            if let Err(e) = std::fs::remove_file(&path) {
                                tracing::warn!(
                                    path = %path.display(),
                                    error = %e,
                                    "failed to remove stale download file; create_file may fail"
                                );
                            } else {
                                tracing::debug!(
                                    path = %path.display(),
                                    "removed orphaned download file before re-creating"
                                );
                            }
                        }
                    }
                    storage.create_file(&path, total_size)
                })
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
            let mut join_set: JoinSet<(usize, Result<u64, SegmentError>)> = JoinSet::new();
            let mut segment_state: Vec<SegmentRuntimeState> = Vec::with_capacity(segments.len());
            for (index, (start, end)) in segments.iter().enumerate() {
                let (end_tx, end_rx) = watch::channel(*end);
                let progress = Arc::new(AtomicU64::new(0));
                segment_state.push(SegmentRuntimeState {
                    end_tx,
                    progress: progress.clone(),
                    started_at: std::time::Instant::now(),
                    start_byte: *start,
                    initial_end: *end,
                });
                let params = SegmentParams {
                    client: client.clone(),
                    file_storage: file_storage.clone(),
                    event_bus: event_bus.clone(),
                    download_id,
                    segment_index: index as u32,
                    url: url.clone(),
                    start_byte: *start,
                    end_byte_rx: end_rx,
                    already_downloaded: 0,
                    total_file_size: total_size,
                    dest_path: dest_path.clone(),
                    pause_rx: pause_rx.clone(),
                    cancel_token: cancel_token.clone(),
                    shared_downloaded: shared_downloaded.clone(),
                    segment_progress: progress,
                };
                let slot_idx = index;
                join_set.spawn(async move { (slot_idx, download_segment(params).await) });
            }

            let mut failed = false;
            let mut error_msg = String::new();
            let mut next_segment_id: u32 = segments.len() as u32;
            let mut active_segments: Vec<Option<SegmentRuntimeState>> =
                segment_state.into_iter().map(Some).collect();

            while let Some(result) = join_set.join_next().await {
                match result {
                    Ok((slot_idx, Ok(_bytes))) => {
                        // Clear the completed slot so pick_split_target ignores it
                        // and persist_split_meta reflects the live topology.
                        if slot_idx < active_segments.len() {
                            active_segments[slot_idx] = None;
                        }

                        if dynamic_split_enabled.load(Ordering::Relaxed)
                            && !cancel_token.is_cancelled()
                            && let Some((idx, split_at)) = pick_split_target(
                                &active_segments,
                                dynamic_split_min_remaining_bytes.load(Ordering::Relaxed),
                            )
                        {
                            let new_id = next_segment_id;
                            next_segment_id += 1;
                            // Capture state and update initial_end on success so a
                            // subsequent pick_split_target on the same slot — or a
                            // crash recovery via persist_split_meta — observes the
                            // shrunk range, not the pre-split end.
                            let (initial_end, signal_sent) = {
                                let old_state = active_segments[idx]
                                    .as_mut()
                                    .expect("slot present at split time");
                                let initial_end = old_state.initial_end;
                                let signal_sent = old_state.end_tx.send(split_at).is_ok();
                                if signal_sent {
                                    old_state.initial_end = split_at;
                                }
                                (initial_end, signal_sent)
                            };
                            if !signal_sent {
                                tracing::warn!(
                                    download_id = download_id.0,
                                    original_segment_id = idx as u32,
                                    "split skipped: target worker no longer listening"
                                );
                                continue;
                            }
                            event_bus.publish(DomainEvent::SegmentSplit {
                                download_id,
                                original_segment_id: idx as u32,
                                new_segment_id: new_id,
                                split_at,
                            });

                            let new_progress = Arc::new(AtomicU64::new(0));
                            let (new_end_tx, new_end_rx) = watch::channel(initial_end);
                            let new_slot_idx = active_segments.len();
                            let params = SegmentParams {
                                client: client.clone(),
                                file_storage: file_storage.clone(),
                                event_bus: event_bus.clone(),
                                download_id,
                                segment_index: new_id,
                                url: url.clone(),
                                start_byte: split_at,
                                end_byte_rx: new_end_rx,
                                already_downloaded: 0,
                                total_file_size: total_size,
                                dest_path: dest_path.clone(),
                                pause_rx: pause_rx.clone(),
                                cancel_token: cancel_token.clone(),
                                shared_downloaded: shared_downloaded.clone(),
                                segment_progress: new_progress.clone(),
                            };
                            join_set.spawn(async move {
                                (new_slot_idx, download_segment(params).await)
                            });
                            active_segments.push(Some(SegmentRuntimeState {
                                end_tx: new_end_tx,
                                progress: new_progress,
                                started_at: std::time::Instant::now(),
                                start_byte: split_at,
                                initial_end,
                            }));

                            persist_split_meta(
                                &file_storage,
                                &dest_path,
                                download_id,
                                &url,
                                total_size,
                                &active_segments,
                            )
                            .await;
                        }
                    }
                    Ok((slot_idx, Err(e))) => {
                        if slot_idx < active_segments.len() {
                            active_segments[slot_idx] = None;
                        }
                        match e {
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
                        }
                    }
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
        let make = |start: u64, end: u64, downloaded: u64, age_ms: u64| {
            Some(SegmentRuntimeState {
                end_tx: watch::channel(end).0,
                progress: Arc::new(AtomicU64::new(downloaded)),
                started_at: std::time::Instant::now() - std::time::Duration::from_millis(age_ms),
                start_byte: start,
                initial_end: end,
            })
        };
        let segs = [
            // fast: 1 MiB downloaded in 100 ms → 10 MiB/s
            make(0, 16 * 1024 * 1024, 1024 * 1024, 100),
            // slow: 100 KiB in 1000 ms → ~100 KiB/s, plenty of remaining
            make(16 * 1024 * 1024, 32 * 1024 * 1024, 100 * 1024, 1000),
            // tiny remaining → must be filtered
            make(32 * 1024 * 1024, 32 * 1024 * 1024 + 1024, 512, 200),
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
        let make = |start: u64, end: u64, downloaded: u64| {
            Some(SegmentRuntimeState {
                end_tx: watch::channel(end).0,
                progress: Arc::new(AtomicU64::new(downloaded)),
                started_at: std::time::Instant::now(),
                start_byte: start,
                initial_end: end,
            })
        };
        let segs = [make(0, 1024, 100), make(1024, 2048, 0), make(2048, 3072, 0)];
        let pick = pick_split_target(&segs, 4 * 1024 * 1024);
        assert!(pick.is_none(), "got {pick:?}");
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
