//! `download_open_folder` command handler.
//!
//! Opens the folder containing a completed download, selecting the file when
//! the host file manager supports it. Unlike [`super::open_download_file`]
//! this tolerates the file being gone — users expect "Open folder" to still
//! jump to the destination directory after a manual move or delete.

use std::path::Path;

use crate::application::command_bus::CommandBus;
use crate::application::error::AppError;
use crate::domain::model::download::DownloadState;

impl CommandBus {
    pub async fn handle_open_download_folder(
        &self,
        cmd: super::OpenDownloadFolderCommand,
    ) -> Result<(), AppError> {
        let download = self
            .download_repo()
            .find_by_id(cmd.id)?
            .ok_or_else(|| AppError::NotFound(format!("Download {} not found", cmd.id.0)))?;

        if download.state() != DownloadState::Completed {
            return Err(AppError::Validation(format!(
                "download is not completed (current state: {})",
                download.state()
            )));
        }

        let opener = self
            .file_opener()
            .ok_or_else(|| AppError::Plugin("file opener port not configured".to_string()))?;

        opener
            .reveal_file(Path::new(download.destination_path()))
            .map_err(AppError::Domain)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::Path;
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::application::commands::OpenDownloadFolderCommand;
    use crate::domain::error::DomainError;
    use crate::domain::event::DomainEvent;
    use crate::domain::model::config::{AppConfig, ConfigPatch};
    use crate::domain::model::credential::Credential;
    use crate::domain::model::download::{Download, DownloadId, Url};
    use crate::domain::model::http::HttpResponse;
    use crate::domain::model::meta::DownloadMeta;
    use crate::domain::model::plugin::{PluginInfo, PluginManifest};
    use crate::domain::ports::driven::{
        ArchiveExtractor, ClipboardObserver, ConfigStore, CredentialStore, DownloadEngine,
        DownloadRepository, EventBus, FileOpener, FileStorage, HttpClient, PluginLoader,
    };

    struct Repo {
        store: Mutex<HashMap<u64, Download>>,
    }
    impl Repo {
        fn new(d: Vec<Download>) -> Self {
            Self {
                store: Mutex::new(d.into_iter().map(|x| (x.id().0, x)).collect()),
            }
        }
    }
    impl DownloadRepository for Repo {
        fn find_by_id(&self, id: DownloadId) -> Result<Option<Download>, DomainError> {
            Ok(self.store.lock().unwrap().get(&id.0).cloned())
        }
        fn save(&self, d: &Download) -> Result<(), DomainError> {
            self.store.lock().unwrap().insert(d.id().0, d.clone());
            Ok(())
        }
        fn delete(&self, id: DownloadId) -> Result<(), DomainError> {
            self.store.lock().unwrap().remove(&id.0);
            Ok(())
        }
        fn find_by_state(
            &self,
            _: crate::domain::model::download::DownloadState,
        ) -> Result<Vec<Download>, DomainError> {
            Ok(vec![])
        }
    }

    struct Engine;
    impl DownloadEngine for Engine {
        fn start(&self, _: &Download) -> Result<(), DomainError> {
            Ok(())
        }
        fn pause(&self, _: DownloadId) -> Result<(), DomainError> {
            Ok(())
        }
        fn resume(&self, _: DownloadId) -> Result<(), DomainError> {
            Ok(())
        }
        fn cancel(&self, _: DownloadId) -> Result<(), DomainError> {
            Ok(())
        }
    }

    struct Bus;
    impl EventBus for Bus {
        fn publish(&self, _: DomainEvent) {}
        fn subscribe(&self, _: Box<dyn Fn(&DomainEvent) + Send + Sync>) {}
    }

    struct FS;
    impl FileStorage for FS {
        fn create_file(&self, _: &Path, _: u64) -> Result<(), DomainError> {
            Ok(())
        }
        fn write_segment(&self, _: &Path, _: u64, _: &[u8]) -> Result<(), DomainError> {
            Ok(())
        }
        fn read_meta(&self, _: &Path) -> Result<Option<DownloadMeta>, DomainError> {
            Ok(None)
        }
        fn write_meta(&self, _: &Path, _: &DownloadMeta) -> Result<(), DomainError> {
            Ok(())
        }
        fn delete_meta(&self, _: &Path) -> Result<(), DomainError> {
            Ok(())
        }
    }

    struct Http;
    impl HttpClient for Http {
        fn head(&self, _: &str) -> Result<HttpResponse, DomainError> {
            Ok(HttpResponse {
                status_code: 200,
                headers: HashMap::new(),
                body: vec![],
            })
        }
        fn get_range(&self, _: &str, _: u64, _: u64) -> Result<Vec<u8>, DomainError> {
            Ok(vec![])
        }
        fn supports_range(&self, _: &str) -> Result<bool, DomainError> {
            Ok(true)
        }
    }

    struct Loader;
    impl PluginLoader for Loader {
        fn load(&self, _: &PluginManifest) -> Result<(), DomainError> {
            Ok(())
        }
        fn unload(&self, _: &str) -> Result<(), DomainError> {
            Ok(())
        }
        fn resolve_url(&self, _: &str) -> Result<Option<PluginInfo>, DomainError> {
            Ok(None)
        }
        fn list_loaded(&self) -> Result<Vec<PluginInfo>, DomainError> {
            Ok(vec![])
        }
        fn set_enabled(&self, _: &str, _: bool) -> Result<(), DomainError> {
            Ok(())
        }
    }

    struct Cfg;
    impl ConfigStore for Cfg {
        fn get_config(&self) -> Result<AppConfig, DomainError> {
            Ok(AppConfig::default())
        }
        fn update_config(&self, _: ConfigPatch) -> Result<AppConfig, DomainError> {
            Ok(AppConfig::default())
        }
    }

    struct Creds;
    impl CredentialStore for Creds {
        fn get(&self, _: &str) -> Result<Option<Credential>, DomainError> {
            Ok(None)
        }
        fn store(&self, _: &str, _: &Credential) -> Result<(), DomainError> {
            Ok(())
        }
        fn delete(&self, _: &str) -> Result<(), DomainError> {
            Ok(())
        }
    }

    struct Clip;
    impl ClipboardObserver for Clip {
        fn start(&self) -> Result<(), DomainError> {
            Ok(())
        }
        fn stop(&self) -> Result<(), DomainError> {
            Ok(())
        }
        fn get_urls(&self) -> Result<Vec<String>, DomainError> {
            Ok(vec![])
        }
    }

    struct Arch;
    impl ArchiveExtractor for Arch {
        fn detect_format(
            &self,
            _: &Path,
        ) -> Result<Option<crate::domain::model::archive::ArchiveFormat>, DomainError> {
            Ok(None)
        }
        fn can_extract(&self, _: &Path) -> Result<bool, DomainError> {
            Ok(false)
        }
        fn extract(
            &self,
            _: &Path,
            _: &Path,
            _: Option<&str>,
        ) -> Result<crate::domain::model::archive::ExtractSummary, DomainError> {
            Ok(crate::domain::model::archive::ExtractSummary {
                extracted_files: 0,
                extracted_bytes: 0,
                duration_ms: 0,
                warnings: vec![],
            })
        }
        fn list_contents(
            &self,
            _: &Path,
            _: Option<&str>,
        ) -> Result<Vec<crate::domain::model::archive::ArchiveEntry>, DomainError> {
            Ok(vec![])
        }
        fn detect_segments(
            &self,
            _: &Path,
        ) -> Result<Option<Vec<std::path::PathBuf>>, DomainError> {
            Ok(None)
        }
    }

    struct RecordingOpener {
        revealed: Mutex<Vec<std::path::PathBuf>>,
        reveal_result: Mutex<Result<(), DomainError>>,
    }

    impl RecordingOpener {
        fn ok() -> Arc<Self> {
            Arc::new(Self {
                revealed: Mutex::new(Vec::new()),
                reveal_result: Mutex::new(Ok(())),
            })
        }

        fn failing(err: DomainError) -> Arc<Self> {
            Arc::new(Self {
                revealed: Mutex::new(Vec::new()),
                reveal_result: Mutex::new(Err(err)),
            })
        }
    }

    impl FileOpener for RecordingOpener {
        fn open_file(&self, _: &Path) -> Result<(), DomainError> {
            Ok(())
        }

        fn reveal_file(&self, path: &Path) -> Result<(), DomainError> {
            self.revealed.lock().unwrap().push(path.to_path_buf());
            self.reveal_result.lock().unwrap().clone()
        }
    }

    fn build_bus(repo: Arc<Repo>, opener: Option<Arc<dyn FileOpener>>) -> CommandBus {
        let bus = CommandBus::new(
            repo,
            Arc::new(Engine),
            Arc::new(Bus),
            Arc::new(FS),
            Arc::new(Http),
            Arc::new(Loader),
            Arc::new(Cfg),
            Arc::new(Creds),
            Arc::new(Clip),
            Arc::new(Arch),
            Arc::new(crate::application::test_support::NoopHistoryRepo),
            None,
        );
        match opener {
            Some(o) => bus.with_file_opener(o),
            None => bus,
        }
    }

    fn make_completed(id: u64, dest: &str) -> Download {
        let mut d = Download::new(
            DownloadId(id),
            Url::new("https://example.com/x").unwrap(),
            "file.mp4".to_string(),
            dest.to_string(),
        );
        d.start().unwrap();
        d.complete().unwrap();
        d
    }

    #[tokio::test]
    async fn handle_open_download_folder_reveals_file_when_completed() {
        let download = make_completed(1, "/tmp/vortex-reveal-test.bin");
        let repo = Arc::new(Repo::new(vec![download]));
        let opener = RecordingOpener::ok();
        let bus = build_bus(repo, Some(opener.clone() as Arc<dyn FileOpener>));

        bus.handle_open_download_folder(OpenDownloadFolderCommand { id: DownloadId(1) })
            .await
            .unwrap();

        let revealed = opener.revealed.lock().unwrap();
        assert_eq!(revealed.len(), 1);
        assert_eq!(revealed[0], Path::new("/tmp/vortex-reveal-test.bin"));
    }

    #[tokio::test]
    async fn handle_open_download_folder_returns_not_found_when_missing() {
        let repo = Arc::new(Repo::new(vec![]));
        let opener = RecordingOpener::ok();
        let bus = build_bus(repo, Some(opener.clone() as Arc<dyn FileOpener>));

        let err = bus
            .handle_open_download_folder(OpenDownloadFolderCommand { id: DownloadId(99) })
            .await
            .unwrap_err();
        assert!(matches!(err, AppError::NotFound(_)), "{err:?}");
        assert!(opener.revealed.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn handle_open_download_folder_rejects_non_completed() {
        let download = Download::new(
            DownloadId(2),
            Url::new("https://example.com/z").unwrap(),
            "pending.zip".to_string(),
            "/tmp/pending.zip".to_string(),
        );
        let repo = Arc::new(Repo::new(vec![download]));
        let opener = RecordingOpener::ok();
        let bus = build_bus(repo, Some(opener.clone() as Arc<dyn FileOpener>));

        let err = bus
            .handle_open_download_folder(OpenDownloadFolderCommand { id: DownloadId(2) })
            .await
            .unwrap_err();
        assert!(matches!(err, AppError::Validation(_)), "{err:?}");
    }

    #[tokio::test]
    async fn handle_open_download_folder_propagates_opener_error() {
        let download = make_completed(3, "/tmp/vortex-reveal-missing.bin");
        let repo = Arc::new(Repo::new(vec![download]));
        let opener = RecordingOpener::failing(DomainError::StorageError("xdg-open failed".into()));
        let bus = build_bus(repo, Some(opener.clone() as Arc<dyn FileOpener>));

        let err = bus
            .handle_open_download_folder(OpenDownloadFolderCommand { id: DownloadId(3) })
            .await
            .unwrap_err();
        assert!(
            matches!(err, AppError::Domain(DomainError::StorageError(_))),
            "{err:?}"
        );
    }

    #[tokio::test]
    async fn handle_open_download_folder_errors_when_port_missing() {
        let download = make_completed(4, "/tmp/x");
        let repo = Arc::new(Repo::new(vec![download]));
        let bus = build_bus(repo, None);

        let err = bus
            .handle_open_download_folder(OpenDownloadFolderCommand { id: DownloadId(4) })
            .await
            .unwrap_err();
        assert!(matches!(err, AppError::Plugin(_)), "{err:?}");
    }
}
