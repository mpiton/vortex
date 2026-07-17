use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::domain::error::DomainError;
use crate::domain::event::DomainEvent;
use crate::domain::model::download::{Download, DownloadId, Url};
use crate::domain::model::meta::DownloadMeta;
use crate::domain::ports::driven::{EventBus, FileStorage};

use super::SegmentedDownloadEngine;

pub(super) type WriteRecord = (PathBuf, u64, Vec<u8>);

pub(super) struct MockFileStorage {
    pub(super) writes: Arc<Mutex<Vec<WriteRecord>>>,
}

impl MockFileStorage {
    pub(super) fn new() -> Self {
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

    fn grow_file(&self, _path: &Path, _minimum_size: u64) -> Result<(), DomainError> {
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

    fn delete_download_artifacts(&self, _path: &Path) -> Result<(), DomainError> {
        Ok(())
    }
}

pub(super) struct CollectingEventBus {
    events: Arc<Mutex<Vec<DomainEvent>>>,
}

impl CollectingEventBus {
    pub(super) fn new() -> Self {
        Self {
            events: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub(super) fn collected(&self) -> Vec<DomainEvent> {
        self.events.lock().unwrap().clone()
    }

    pub(super) async fn wait_for_event_async<F>(&self, predicate: F, timeout: Duration) -> bool
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

pub(super) fn make_download(id: u64, url: &str) -> Download {
    let download_id = DownloadId(id);
    let parsed_url = Url::new(url).unwrap();
    Download::new(
        download_id,
        parsed_url,
        "test_file.bin".to_string(),
        "/tmp/test_file.bin".to_string(),
    )
}

pub(super) fn make_engine(
    storage: Arc<dyn FileStorage>,
    bus: Arc<dyn EventBus>,
) -> SegmentedDownloadEngine {
    SegmentedDownloadEngine::new(reqwest::Client::new(), storage, bus, 4)
}
