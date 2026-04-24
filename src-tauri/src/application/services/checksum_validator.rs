//! Application service that validates a download against its expected checksum.
//!
//! Pulls the [`ChecksumComputer`] port to stream-hash the file, then compares
//! the result against `Download::checksum_expected()` and persists the
//! computed value via the [`DownloadRepository`]. Emits the matching
//! `ChecksumVerified` / `ChecksumMismatch` domain event so subscribers
//! (queue manager, frontend bridge) can react.

use std::path::Path;
use std::sync::Arc;

use crate::application::error::AppError;
use crate::domain::error::DomainError;
use crate::domain::event::DomainEvent;
use crate::domain::model::checksum::ChecksumAlgorithm;
use crate::domain::model::download::{Download, DownloadId, DownloadState};
use crate::domain::ports::driven::{ChecksumComputer, DownloadRepository, EventBus};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChecksumOutcome {
    /// Computed value matched the expected one — download is now `Completed`.
    Verified,
    /// Computed value differed from expected — download is now `Error`.
    Mismatch,
    /// No expected checksum on the download → validator is a no-op.
    NoExpectedChecksum,
    /// `verify_checksums` setting is off → validator is a no-op (caller must
    /// finalise the completion path separately).
    Skipped,
}

pub struct ChecksumValidatorService {
    download_repo: Arc<dyn DownloadRepository>,
    computer: Arc<dyn ChecksumComputer>,
    event_bus: Arc<dyn EventBus>,
}

impl ChecksumValidatorService {
    pub fn new(
        download_repo: Arc<dyn DownloadRepository>,
        computer: Arc<dyn ChecksumComputer>,
        event_bus: Arc<dyn EventBus>,
    ) -> Self {
        Self {
            download_repo,
            computer,
            event_bus,
        }
    }

    /// Run validation for `id`. Caller must guarantee the download has
    /// already transitioned to `Checking`.
    pub fn validate(&self, id: DownloadId) -> Result<ChecksumOutcome, AppError> {
        let mut download = self
            .download_repo
            .find_by_id(id)?
            .ok_or_else(|| AppError::NotFound(format!("Download {} not found", id.0)))?;

        if download.state() != DownloadState::Checking {
            return Err(AppError::Domain(DomainError::InvalidTransition {
                from: download.state(),
                to: DownloadState::Completed,
            }));
        }

        let Some(expected) = download.checksum_expected().map(str::to_string) else {
            return Ok(ChecksumOutcome::NoExpectedChecksum);
        };
        // Persisted algorithm wins; fall back to detection from the expected
        // hash so rows migrated from earlier schemas (no checksum_algorithm
        // column) still validate correctly.
        let algorithm = download
            .checksum_algorithm()
            .or_else(|| ChecksumAlgorithm::detect_from_hex(&expected))
            .ok_or_else(|| {
                AppError::Domain(DomainError::UnsupportedChecksumFormat(expected.clone()))
            })?;
        let path = Path::new(download.destination_path()).to_path_buf();
        let computed = self.computer.compute(&path, algorithm)?;

        let event = if checksum_matches(&expected, &computed) {
            download.record_checksum_match(algorithm, computed.clone())?
        } else {
            download.record_checksum_mismatch(algorithm, expected, computed.clone())?
        };

        match download.state() {
            DownloadState::Completed => self.download_repo.save(&download)?,
            DownloadState::Error => {
                let msg = match &event {
                    DomainEvent::ChecksumMismatch { algorithm, .. } => {
                        format!("{algorithm} checksum mismatch")
                    }
                    _ => "checksum mismatch".to_string(),
                };
                self.download_repo.save_failed(&download, &msg)?;
            }
            other => {
                return Err(AppError::Domain(DomainError::InvalidTransition {
                    from: DownloadState::Checking,
                    to: other,
                }));
            }
        }
        let outcome = if matches!(event, DomainEvent::ChecksumVerified { .. }) {
            ChecksumOutcome::Verified
        } else {
            ChecksumOutcome::Mismatch
        };
        self.event_bus.publish(event);
        Ok(outcome)
    }

    /// Preflight check that a download is eligible for validation. Pure
    /// helper so other services can decide whether to transition into
    /// `Checking` at all.
    pub fn should_validate(download: &Download, verify_setting: bool) -> bool {
        verify_setting && download.checksum_expected().is_some()
    }
}

fn checksum_matches(expected: &str, computed: &str) -> bool {
    let expected = expected.trim().to_ascii_lowercase();
    let computed = computed.trim().to_ascii_lowercase();
    expected == computed
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Mutex;

    use super::*;
    use crate::domain::error::DomainError;
    use crate::domain::event::DomainEvent;
    use crate::domain::model::checksum::ChecksumAlgorithm;
    use crate::domain::model::download::{Download, DownloadId, Url};

    struct MockRepo {
        store: Mutex<HashMap<u64, Download>>,
        failed: Mutex<Vec<(DownloadId, String)>>,
    }

    impl MockRepo {
        fn new(downloads: Vec<Download>) -> Self {
            Self {
                store: Mutex::new(downloads.into_iter().map(|d| (d.id().0, d)).collect()),
                failed: Mutex::new(Vec::new()),
            }
        }
    }

    impl DownloadRepository for MockRepo {
        fn find_by_id(&self, id: DownloadId) -> Result<Option<Download>, DomainError> {
            Ok(self.store.lock().unwrap().get(&id.0).cloned())
        }

        fn save(&self, d: &Download) -> Result<(), DomainError> {
            self.store.lock().unwrap().insert(d.id().0, d.clone());
            Ok(())
        }

        fn save_failed(&self, d: &Download, msg: &str) -> Result<(), DomainError> {
            self.failed.lock().unwrap().push((d.id(), msg.to_string()));
            self.store.lock().unwrap().insert(d.id().0, d.clone());
            Ok(())
        }

        fn delete(&self, id: DownloadId) -> Result<(), DomainError> {
            self.store.lock().unwrap().remove(&id.0);
            Ok(())
        }

        fn find_by_state(&self, _s: DownloadState) -> Result<Vec<Download>, DomainError> {
            Ok(vec![])
        }
    }

    struct StubComputer {
        next_value: String,
        fail: Option<DomainError>,
    }

    impl ChecksumComputer for StubComputer {
        fn compute(
            &self,
            _path: &Path,
            _algorithm: ChecksumAlgorithm,
        ) -> Result<String, DomainError> {
            if let Some(err) = &self.fail {
                return Err(err.clone());
            }
            Ok(self.next_value.clone())
        }
    }

    struct CapturingBus {
        events: Mutex<Vec<DomainEvent>>,
    }

    impl EventBus for CapturingBus {
        fn publish(&self, event: DomainEvent) {
            self.events.lock().unwrap().push(event);
        }

        fn subscribe(&self, _handler: Box<dyn Fn(&DomainEvent) + Send + Sync>) {}
    }

    fn make_download(id: u64, expected: &str) -> Download {
        let mut d = Download::new(
            DownloadId(id),
            Url::new("https://example.com/file.bin").unwrap(),
            "file.bin".into(),
            "/tmp/file.bin".into(),
        )
        .with_expected_checksum(expected.to_string())
        .unwrap();
        d.start().unwrap();
        d.start_checking().unwrap();
        d
    }

    #[test]
    fn test_validate_marks_completed_when_checksum_matches() {
        let expected = "d41d8cd98f00b204e9800998ecf8427e".to_string();
        let download = make_download(1, &expected);

        let repo = Arc::new(MockRepo::new(vec![download]));
        let computer = Arc::new(StubComputer {
            next_value: expected.clone(),
            fail: None,
        });
        let bus = Arc::new(CapturingBus {
            events: Mutex::new(vec![]),
        });

        let svc = ChecksumValidatorService::new(repo.clone(), computer, bus.clone());
        let outcome = svc.validate(DownloadId(1)).unwrap();

        assert_eq!(outcome, ChecksumOutcome::Verified);
        let saved = repo.find_by_id(DownloadId(1)).unwrap().unwrap();
        assert_eq!(saved.state(), DownloadState::Completed);
        assert_eq!(saved.checksum_computed(), Some(expected.as_str()));
        assert!(matches!(
            bus.events.lock().unwrap().first(),
            Some(DomainEvent::ChecksumVerified { .. })
        ));
    }

    #[test]
    fn test_validate_marks_error_when_checksum_mismatches() {
        let expected = "d41d8cd98f00b204e9800998ecf8427e".to_string();
        let computed = "00000000000000000000000000000000".to_string();
        let download = make_download(2, &expected);

        let repo = Arc::new(MockRepo::new(vec![download]));
        let computer = Arc::new(StubComputer {
            next_value: computed.clone(),
            fail: None,
        });
        let bus = Arc::new(CapturingBus {
            events: Mutex::new(vec![]),
        });

        let svc = ChecksumValidatorService::new(repo.clone(), computer, bus.clone());
        let outcome = svc.validate(DownloadId(2)).unwrap();

        assert_eq!(outcome, ChecksumOutcome::Mismatch);
        let saved = repo.find_by_id(DownloadId(2)).unwrap().unwrap();
        assert_eq!(saved.state(), DownloadState::Error);
        assert_eq!(saved.checksum_computed(), Some(computed.as_str()));
        match bus.events.lock().unwrap().first() {
            Some(DomainEvent::ChecksumMismatch {
                expected: e,
                computed: c,
                ..
            }) => {
                assert_eq!(e, &expected);
                assert_eq!(c, &computed);
            }
            other => panic!("expected ChecksumMismatch, got {other:?}"),
        }
        // save_failed must be invoked with a descriptive message
        let failed = repo.failed.lock().unwrap().clone();
        assert_eq!(failed.len(), 1);
        assert!(failed[0].1.contains("MD5"));
    }

    #[test]
    fn test_validate_returns_no_expected_checksum_when_field_unset() {
        // Build a download in Checking state with no expected checksum
        let mut d = Download::new(
            DownloadId(3),
            Url::new("https://example.com/file.bin").unwrap(),
            "file.bin".into(),
            "/tmp/file.bin".into(),
        );
        d.start().unwrap();
        d.start_checking().unwrap();

        let repo = Arc::new(MockRepo::new(vec![d]));
        let computer = Arc::new(StubComputer {
            next_value: "irrelevant".into(),
            fail: None,
        });
        let bus = Arc::new(CapturingBus {
            events: Mutex::new(vec![]),
        });

        let svc = ChecksumValidatorService::new(repo, computer, bus.clone());
        let outcome = svc.validate(DownloadId(3)).unwrap();

        assert_eq!(outcome, ChecksumOutcome::NoExpectedChecksum);
        assert!(bus.events.lock().unwrap().is_empty());
    }

    #[test]
    fn test_validate_propagates_io_error_from_computer() {
        let download = make_download(4, "d41d8cd98f00b204e9800998ecf8427e");
        let repo = Arc::new(MockRepo::new(vec![download]));
        let computer = Arc::new(StubComputer {
            next_value: String::new(),
            fail: Some(DomainError::StorageError("disk on fire".into())),
        });
        let bus = Arc::new(CapturingBus {
            events: Mutex::new(vec![]),
        });

        let svc = ChecksumValidatorService::new(repo, computer, bus);
        let err = svc.validate(DownloadId(4)).unwrap_err();
        assert!(matches!(
            err,
            AppError::Domain(DomainError::StorageError(_))
        ));
    }

    #[test]
    fn test_validate_rejects_download_not_in_checking_state() {
        let mut d = Download::new(
            DownloadId(5),
            Url::new("https://example.com/file.bin").unwrap(),
            "file.bin".into(),
            "/tmp/file.bin".into(),
        );
        d.start().unwrap(); // Downloading, not Checking

        let repo = Arc::new(MockRepo::new(vec![d]));
        let computer = Arc::new(StubComputer {
            next_value: "x".into(),
            fail: None,
        });
        let bus = Arc::new(CapturingBus {
            events: Mutex::new(vec![]),
        });

        let svc = ChecksumValidatorService::new(repo, computer, bus);
        let err = svc.validate(DownloadId(5)).unwrap_err();
        assert!(matches!(
            err,
            AppError::Domain(DomainError::InvalidTransition { .. })
        ));
    }

    #[test]
    fn test_should_validate_returns_true_only_when_setting_on_and_checksum_present() {
        let with_checksum = Download::new(
            DownloadId(6),
            Url::new("https://example.com/x").unwrap(),
            "x".into(),
            "/tmp/x".into(),
        )
        .with_expected_checksum("d41d8cd98f00b204e9800998ecf8427e".into())
        .unwrap();
        let no_checksum = Download::new(
            DownloadId(7),
            Url::new("https://example.com/y").unwrap(),
            "y".into(),
            "/tmp/y".into(),
        );

        assert!(ChecksumValidatorService::should_validate(
            &with_checksum,
            true
        ));
        assert!(!ChecksumValidatorService::should_validate(
            &with_checksum,
            false
        ));
        assert!(!ChecksumValidatorService::should_validate(
            &no_checksum,
            true
        ));
    }

    #[test]
    fn test_checksum_matches_is_case_insensitive() {
        assert!(checksum_matches("ABC", "abc"));
        assert!(checksum_matches(" deadBEEF ", "DEADBEEF"));
        assert!(!checksum_matches("abc", "abd"));
    }
}
