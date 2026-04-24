//! `download_verify_checksum` command handler.
//!
//! Re-validates the checksum of an existing download. Drives the
//! [`ChecksumValidatorService`] after transitioning the download into the
//! `Checking` state so the validator's pre-condition is met.

use serde::Serialize;
use std::sync::Arc;

use crate::application::command_bus::CommandBus;
use crate::application::error::AppError;
use crate::application::services::{ChecksumOutcome, ChecksumValidatorService};
use crate::domain::error::DomainError;
use crate::domain::model::download::DownloadState;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum VerifyChecksumOutcome {
    Verified,
    Mismatch,
    NoExpectedChecksum,
}

impl From<ChecksumOutcome> for VerifyChecksumOutcome {
    fn from(value: ChecksumOutcome) -> Self {
        match value {
            ChecksumOutcome::Verified => VerifyChecksumOutcome::Verified,
            ChecksumOutcome::Mismatch => VerifyChecksumOutcome::Mismatch,
            ChecksumOutcome::NoExpectedChecksum | ChecksumOutcome::Skipped => {
                VerifyChecksumOutcome::NoExpectedChecksum
            }
        }
    }
}

impl CommandBus {
    pub async fn handle_verify_checksum(
        &self,
        cmd: super::VerifyChecksumCommand,
    ) -> Result<VerifyChecksumOutcome, AppError> {
        let computer = self.checksum_computer_arc().ok_or_else(|| {
            AppError::Domain(DomainError::PluginError(
                "checksum computer not configured".to_string(),
            ))
        })?;
        let mut download = self
            .download_repo()
            .find_by_id(cmd.id)?
            .ok_or_else(|| AppError::NotFound(format!("Download {} not found", cmd.id.0)))?;

        if download.checksum_expected().is_none() {
            return Ok(VerifyChecksumOutcome::NoExpectedChecksum);
        }

        // Move into Checking so the validator pre-condition holds.
        match download.state() {
            DownloadState::Completed => {
                let event = download.start_checking_from_completed()?;
                self.download_repo().save(&download)?;
                self.event_bus().publish(event);
            }
            DownloadState::Downloading => {
                let event = download.start_checking()?;
                self.download_repo().save(&download)?;
                self.event_bus().publish(event);
            }
            DownloadState::Checking => {
                // Already there — nothing to persist before validation.
            }
            other => {
                return Err(AppError::Domain(DomainError::InvalidTransition {
                    from: other,
                    to: DownloadState::Checking,
                }));
            }
        }

        let svc = ChecksumValidatorService::new(
            self.download_repo_arc(),
            Arc::clone(&computer),
            self.event_bus_arc(),
        );

        match svc.validate(cmd.id) {
            Ok(o) => Ok(o.into()),
            Err(e) => {
                // Validation failed mid-flight. Recover the persisted state
                // so the download isn't stranded in Checking forever: load
                // the latest copy, transition to Error, and persist with the
                // backend message. Log persistence errors loudly — losing
                // them would re-create the very stranding bug we're fixing.
                if let Ok(Some(mut current)) = self.download_repo().find_by_id(cmd.id)
                    && current.state() == DownloadState::Checking
                {
                    let msg = format!("checksum verification failed: {e}");
                    if let Ok(event) = current.fail(msg.clone()) {
                        match self.download_repo().save_failed(&current, &msg) {
                            Ok(()) => self.event_bus().publish(event),
                            Err(save_err) => tracing::error!(
                                download_id = cmd.id.0,
                                error = %save_err,
                                "verify_checksum: save_failed after validate error failed; \
                                download remains in Checking",
                            ),
                        }
                    }
                }
                Err(e)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::Path;
    use std::sync::Mutex;

    use crate::application::commands::VerifyChecksumCommand;
    use crate::domain::error::DomainError;
    use crate::domain::event::DomainEvent;
    use crate::domain::model::archive::{ArchiveEntry, ArchiveFormat, ExtractSummary};
    use crate::domain::model::checksum::ChecksumAlgorithm;
    use crate::domain::model::config::{AppConfig, ConfigPatch};
    use crate::domain::model::credential::Credential;
    use crate::domain::model::download::{Download, DownloadId, Url};
    use crate::domain::model::http::HttpResponse;
    use crate::domain::model::meta::DownloadMeta;
    use crate::domain::model::plugin::{PluginInfo, PluginManifest};
    use crate::domain::ports::driven::{
        ArchiveExtractor, ChecksumComputer, ClipboardObserver, ConfigStore, CredentialStore,
        DownloadEngine, DownloadRepository, EventBus, FileStorage, HttpClient, PluginLoader,
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

    struct Bus {
        events: Mutex<Vec<DomainEvent>>,
    }
    impl EventBus for Bus {
        fn publish(&self, e: DomainEvent) {
            self.events.lock().unwrap().push(e);
        }
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

    struct Cfg {
        verify: bool,
    }
    impl ConfigStore for Cfg {
        fn get_config(&self) -> Result<AppConfig, DomainError> {
            Ok(AppConfig {
                verify_checksums: self.verify,
                ..AppConfig::default()
            })
        }
        fn update_config(&self, _: ConfigPatch) -> Result<AppConfig, DomainError> {
            self.get_config()
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
        fn detect_format(&self, _: &Path) -> Result<Option<ArchiveFormat>, DomainError> {
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
        ) -> Result<ExtractSummary, DomainError> {
            Ok(ExtractSummary {
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
        ) -> Result<Vec<ArchiveEntry>, DomainError> {
            Ok(vec![])
        }
        fn detect_segments(
            &self,
            _: &Path,
        ) -> Result<Option<Vec<std::path::PathBuf>>, DomainError> {
            Ok(None)
        }
    }

    struct Computer {
        result: String,
    }
    impl ChecksumComputer for Computer {
        fn compute(&self, _: &Path, _: ChecksumAlgorithm) -> Result<String, DomainError> {
            Ok(self.result.clone())
        }
    }

    struct FailingComputer;
    impl ChecksumComputer for FailingComputer {
        fn compute(&self, _: &Path, _: ChecksumAlgorithm) -> Result<String, DomainError> {
            Err(DomainError::StorageError("disk on fire".into()))
        }
    }

    fn make_bus(repo: Arc<Repo>, bus: Arc<Bus>, computer: Arc<dyn ChecksumComputer>) -> CommandBus {
        CommandBus::new(
            repo,
            Arc::new(Engine),
            bus,
            Arc::new(FS),
            Arc::new(Http),
            Arc::new(Loader),
            Arc::new(Cfg { verify: true }),
            Arc::new(Creds),
            Arc::new(Clip),
            Arc::new(Arch),
            Arc::new(crate::application::test_support::NoopHistoryRepo),
            None,
        )
        .with_checksum_computer(computer)
    }

    fn make_download_in_completed(id: u64, expected: &str) -> Download {
        let mut d = Download::new(
            DownloadId(id),
            Url::new("https://example.com/x").unwrap(),
            "x".into(),
            "/tmp/x".into(),
        )
        .with_expected_checksum(expected.to_string())
        .unwrap();
        d.start().unwrap();
        d.start_checking().unwrap();
        d.record_checksum_match(ChecksumAlgorithm::Md5, expected.to_string())
            .unwrap();
        d
    }

    #[tokio::test]
    async fn test_handle_verify_checksum_returns_no_expected_when_field_missing() {
        let mut d = Download::new(
            DownloadId(1),
            Url::new("https://example.com/x").unwrap(),
            "x".into(),
            "/tmp/x".into(),
        );
        d.start().unwrap();
        d.complete().unwrap();
        let repo = Arc::new(Repo::new(vec![d]));
        let bus = Arc::new(Bus {
            events: Mutex::new(vec![]),
        });
        let cmd_bus = make_bus(
            repo,
            bus,
            Arc::new(Computer {
                result: "ignored".into(),
            }),
        );

        let outcome = cmd_bus
            .handle_verify_checksum(VerifyChecksumCommand { id: DownloadId(1) })
            .await
            .unwrap();
        assert_eq!(outcome, VerifyChecksumOutcome::NoExpectedChecksum);
    }

    #[tokio::test]
    async fn test_handle_verify_checksum_re_runs_validation_on_completed_download() {
        let expected = "d41d8cd98f00b204e9800998ecf8427e".to_string();
        let download = make_download_in_completed(2, &expected);
        let repo = Arc::new(Repo::new(vec![download]));
        let bus = Arc::new(Bus {
            events: Mutex::new(vec![]),
        });
        let cmd_bus = make_bus(
            repo.clone(),
            bus.clone(),
            Arc::new(Computer {
                result: expected.clone(),
            }),
        );

        let outcome = cmd_bus
            .handle_verify_checksum(VerifyChecksumCommand { id: DownloadId(2) })
            .await
            .unwrap();
        assert_eq!(outcome, VerifyChecksumOutcome::Verified);
        let saved = repo.find_by_id(DownloadId(2)).unwrap().unwrap();
        assert_eq!(
            saved.state(),
            crate::domain::model::download::DownloadState::Completed
        );
        let events = bus.events.lock().unwrap();
        assert!(
            events
                .iter()
                .any(|e| matches!(e, DomainEvent::DownloadChecking { .. }))
        );
        assert!(
            events
                .iter()
                .any(|e| matches!(e, DomainEvent::ChecksumVerified { .. }))
        );
    }

    #[tokio::test]
    async fn test_handle_verify_checksum_returns_mismatch_when_computed_differs() {
        let expected = "d41d8cd98f00b204e9800998ecf8427e".to_string();
        let download = make_download_in_completed(3, &expected);
        let repo = Arc::new(Repo::new(vec![download]));
        let bus = Arc::new(Bus {
            events: Mutex::new(vec![]),
        });
        let cmd_bus = make_bus(
            repo.clone(),
            bus,
            Arc::new(Computer {
                result: "00000000000000000000000000000000".into(),
            }),
        );

        let outcome = cmd_bus
            .handle_verify_checksum(VerifyChecksumCommand { id: DownloadId(3) })
            .await
            .unwrap();
        assert_eq!(outcome, VerifyChecksumOutcome::Mismatch);
    }

    #[tokio::test]
    async fn test_handle_verify_checksum_validates_currently_downloading_download() {
        // Stays in Downloading until the handler walks it through Checking.
        let expected = "d41d8cd98f00b204e9800998ecf8427e".to_string();
        let mut d = Download::new(
            DownloadId(7),
            Url::new("https://example.com/x").unwrap(),
            "x".into(),
            "/tmp/x".into(),
        )
        .with_expected_checksum(expected.clone())
        .unwrap();
        d.start().unwrap(); // Downloading
        let repo = Arc::new(Repo::new(vec![d]));
        let bus = Arc::new(Bus {
            events: Mutex::new(vec![]),
        });
        let cmd_bus = make_bus(
            repo.clone(),
            bus,
            Arc::new(Computer {
                result: expected.clone(),
            }),
        );

        let outcome = cmd_bus
            .handle_verify_checksum(VerifyChecksumCommand { id: DownloadId(7) })
            .await
            .unwrap();
        assert_eq!(outcome, VerifyChecksumOutcome::Verified);
    }

    #[tokio::test]
    async fn test_handle_verify_checksum_rejects_invalid_state() {
        // A Queued download is not eligible for verification.
        let d = Download::new(
            DownloadId(8),
            Url::new("https://example.com/x").unwrap(),
            "x".into(),
            "/tmp/x".into(),
        )
        .with_expected_checksum("d41d8cd98f00b204e9800998ecf8427e".into())
        .unwrap();
        let repo = Arc::new(Repo::new(vec![d.clone()]));
        let bus = Arc::new(Bus {
            events: Mutex::new(vec![]),
        });
        let cmd_bus = make_bus(repo.clone(), bus, Arc::new(Computer { result: "x".into() }));

        let err = cmd_bus
            .handle_verify_checksum(VerifyChecksumCommand { id: DownloadId(8) })
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            AppError::Domain(DomainError::InvalidTransition { .. })
        ));
    }

    #[tokio::test]
    async fn test_handle_verify_checksum_not_found_returns_app_error() {
        let repo = Arc::new(Repo::new(vec![]));
        let bus = Arc::new(Bus {
            events: Mutex::new(vec![]),
        });
        let cmd_bus = make_bus(repo, bus, Arc::new(Computer { result: "x".into() }));

        let err = cmd_bus
            .handle_verify_checksum(VerifyChecksumCommand {
                id: DownloadId(999),
            })
            .await
            .unwrap_err();
        assert!(matches!(err, AppError::NotFound(_)));
    }

    #[tokio::test]
    async fn test_handle_verify_checksum_recovers_from_validate_error() {
        // When the validator returns Err, the download must not be left in
        // Checking — recover by transitioning to Error and persisting the
        // backend message via save_failed.
        let expected = "d41d8cd98f00b204e9800998ecf8427e".to_string();
        let download = make_download_in_completed(42, &expected);
        let repo = Arc::new(Repo::new(vec![download]));
        let bus = Arc::new(Bus {
            events: Mutex::new(vec![]),
        });
        let cmd_bus = make_bus(repo.clone(), bus.clone(), Arc::new(FailingComputer));

        let err = cmd_bus
            .handle_verify_checksum(VerifyChecksumCommand { id: DownloadId(42) })
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            AppError::Domain(DomainError::StorageError(_))
        ));

        let saved = repo.find_by_id(DownloadId(42)).unwrap().unwrap();
        assert_eq!(
            saved.state(),
            crate::domain::model::download::DownloadState::Error,
            "download must be transitioned to Error after validate Err",
        );
        let events = bus.events.lock().unwrap();
        assert!(
            events
                .iter()
                .any(|e| matches!(e, DomainEvent::DownloadFailed { .. })),
            "DownloadFailed must be emitted on validate Err recovery",
        );
    }
}
