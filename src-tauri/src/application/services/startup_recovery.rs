//! Startup recovery — reconciles persisted download state with engine state.
//!
//! On app boot the download engine has no in-memory tasks.  Downloads that were
//! active (`Downloading`, `Waiting`, `Checking`, `Extracting`) in the previous
//! session are orphaned: SQLite still shows an active state but no engine task
//! exists.  This module transitions them to `Error` so the user (or auto-retry)
//! can deal with them.

use crate::domain::error::DomainError;
use crate::domain::model::download::DownloadState;
use crate::domain::ports::driven::CaptchaRepository;
use crate::domain::ports::driven::download_repository::DownloadRepository;

/// States that imply a running engine task.  On fresh startup no task exists,
/// so these downloads are orphaned.
const ORPHAN_STATES: [DownloadState; 4] = [
    DownloadState::Downloading,
    DownloadState::Waiting,
    DownloadState::Checking,
    DownloadState::Extracting,
];

/// Transition every download in an active-but-orphaned state to `Error`.
///
/// Returns the number of downloads recovered.
#[cfg(test)]
pub fn recover_orphaned_downloads(
    download_repo: &dyn DownloadRepository,
) -> Result<usize, DomainError> {
    recover(download_repo, None)
}

pub fn recover_orphaned_downloads_with_captchas(
    download_repo: &dyn DownloadRepository,
    captcha_repo: &dyn CaptchaRepository,
) -> Result<usize, DomainError> {
    recover(download_repo, Some(captcha_repo))
}

fn recover(
    download_repo: &dyn DownloadRepository,
    captcha_repo: Option<&dyn CaptchaRepository>,
) -> Result<usize, DomainError> {
    let mut recovered = 0;

    for state in ORPHAN_STATES {
        let downloads = download_repo.find_by_state(state)?;
        for mut download in downloads {
            if state == DownloadState::Waiting
                && captcha_repo
                    .map(|repo| repo.find_pending_by_download(download.id()))
                    .transpose()?
                    .flatten()
                    .is_some()
            {
                continue;
            }
            // fail() is valid from all ORPHAN_STATES — see domain state machine.
            let error = "Interrupted: app restarted".to_string();
            let _event = download.fail(error.clone())?;
            download_repo.save_failed(&download, &error)?;
            recovered += 1;
        }
    }

    Ok(recovered)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::model::captcha::{CaptchaChallenge, CaptchaId, CaptchaType};
    use crate::domain::model::download::{Download, DownloadId, Url};
    use std::collections::HashMap;
    use std::sync::Mutex;

    // --- In-memory mock ---

    struct InMemoryRepo {
        downloads: Mutex<HashMap<u64, Download>>,
    }

    impl InMemoryRepo {
        fn new(downloads: Vec<Download>) -> Self {
            Self {
                downloads: Mutex::new(downloads.into_iter().map(|d| (d.id().0, d)).collect()),
            }
        }

        fn get(&self, id: u64) -> Option<Download> {
            self.downloads.lock().unwrap().get(&id).cloned()
        }
    }

    impl DownloadRepository for InMemoryRepo {
        fn find_by_id(&self, id: DownloadId) -> Result<Option<Download>, DomainError> {
            Ok(self.downloads.lock().unwrap().get(&id.0).cloned())
        }

        fn save(&self, download: &Download) -> Result<(), DomainError> {
            self.downloads
                .lock()
                .unwrap()
                .insert(download.id().0, download.clone());
            Ok(())
        }

        fn delete(&self, id: DownloadId) -> Result<(), DomainError> {
            self.downloads.lock().unwrap().remove(&id.0);
            Ok(())
        }

        fn find_by_state(&self, state: DownloadState) -> Result<Vec<Download>, DomainError> {
            Ok(self
                .downloads
                .lock()
                .unwrap()
                .values()
                .filter(|d| d.state() == state)
                .cloned()
                .collect())
        }
    }

    struct PendingCaptchaRepo(CaptchaChallenge);

    impl CaptchaRepository for PendingCaptchaRepo {
        fn save(&self, _: &CaptchaChallenge) -> Result<(), DomainError> {
            Ok(())
        }

        fn find_by_id(&self, id: &CaptchaId) -> Result<Option<CaptchaChallenge>, DomainError> {
            Ok((self.0.id() == id).then(|| self.0.clone()))
        }

        fn list(&self) -> Result<Vec<CaptchaChallenge>, DomainError> {
            Ok(vec![self.0.clone()])
        }

        fn list_pending(&self) -> Result<Vec<CaptchaChallenge>, DomainError> {
            Ok(vec![self.0.clone()])
        }
    }

    // --- Helpers ---

    fn make_download(id: u64) -> Download {
        let url = Url::new("https://example.com/file.zip").expect("valid url");
        Download::new(DownloadId(id), url, "file.zip".into(), "/tmp".into())
    }

    fn make_downloading(id: u64) -> Download {
        let mut d = make_download(id);
        d.start().expect("Queued → Downloading");
        d
    }

    fn make_waiting(id: u64) -> Download {
        let mut d = make_downloading(id);
        d.wait().expect("Downloading → Waiting");
        d
    }

    fn make_checking(id: u64) -> Download {
        let mut d = make_downloading(id);
        d.start_checking().expect("Downloading → Checking");
        d
    }

    fn make_extracting(id: u64) -> Download {
        let mut d = make_downloading(id);
        d.start_extracting().expect("Downloading → Extracting");
        d
    }

    fn make_paused(id: u64) -> Download {
        let mut d = make_downloading(id);
        d.pause().expect("Downloading → Paused");
        d
    }

    fn make_completed(id: u64) -> Download {
        let mut d = make_downloading(id);
        d.complete().expect("Downloading → Completed");
        d
    }

    fn make_error(id: u64) -> Download {
        let mut d = make_downloading(id);
        d.fail("some error".into()).expect("Downloading → Error");
        d
    }

    // --- Tests ---

    #[test]
    fn test_recover_downloading_transitions_to_error() {
        let repo = InMemoryRepo::new(vec![make_downloading(1)]);

        let count = recover_orphaned_downloads(&repo).expect("recovery");

        assert_eq!(count, 1);
        let d = repo.get(1).expect("download exists");
        assert_eq!(d.state(), DownloadState::Error);
    }

    #[test]
    fn test_recover_waiting_transitions_to_error() {
        let repo = InMemoryRepo::new(vec![make_waiting(1)]);

        let count = recover_orphaned_downloads(&repo).expect("recovery");

        assert_eq!(count, 1);
        let d = repo.get(1).expect("download exists");
        assert_eq!(d.state(), DownloadState::Error);
    }

    #[test]
    fn test_recover_preserves_waiting_download_with_pending_captcha() {
        let repo = InMemoryRepo::new(vec![make_waiting(1)]);
        let captchas = PendingCaptchaRepo(
            CaptchaChallenge::new(
                CaptchaId::new("captcha-1"),
                DownloadId(1),
                CaptchaType::TextInput,
                "https://hoster.example/file".into(),
                1_000,
                61_000,
            )
            .expect("challenge"),
        );

        let count = recover_orphaned_downloads_with_captchas(&repo, &captchas).expect("recovery");

        assert_eq!(count, 0);
        assert_eq!(repo.get(1).unwrap().state(), DownloadState::Waiting);
    }

    #[test]
    fn test_recover_checking_transitions_to_error() {
        let repo = InMemoryRepo::new(vec![make_checking(1)]);

        let count = recover_orphaned_downloads(&repo).expect("recovery");

        assert_eq!(count, 1);
        let d = repo.get(1).expect("download exists");
        assert_eq!(d.state(), DownloadState::Error);
    }

    #[test]
    fn test_recover_extracting_transitions_to_error() {
        let repo = InMemoryRepo::new(vec![make_extracting(1)]);

        let count = recover_orphaned_downloads(&repo).expect("recovery");

        assert_eq!(count, 1);
        let d = repo.get(1).expect("download exists");
        assert_eq!(d.state(), DownloadState::Error);
    }

    #[test]
    fn test_recover_ignores_completed_paused_error_queued() {
        let repo = InMemoryRepo::new(vec![
            make_download(1),  // Queued
            make_paused(2),    // Paused
            make_completed(3), // Completed
            make_error(4),     // Error
        ]);

        let count = recover_orphaned_downloads(&repo).expect("recovery");

        assert_eq!(count, 0);
        assert_eq!(repo.get(1).unwrap().state(), DownloadState::Queued);
        assert_eq!(repo.get(2).unwrap().state(), DownloadState::Paused);
        assert_eq!(repo.get(3).unwrap().state(), DownloadState::Completed);
        assert_eq!(repo.get(4).unwrap().state(), DownloadState::Error);
    }

    #[test]
    fn test_recover_mixed_states_only_transitions_orphans() {
        let repo = InMemoryRepo::new(vec![
            make_downloading(1),
            make_completed(2),
            make_waiting(3),
            make_paused(4),
            make_checking(5),
            make_download(6),   // Queued
            make_extracting(7), // Extracting — 4th orphan state
            {
                let mut d = make_downloading(8);
                d.fail("err".into()).expect("Downloading → Error");
                d.retry().expect("Error → Retry"); // Retry — must be preserved
                d
            },
        ]);

        let count = recover_orphaned_downloads(&repo).expect("recovery");

        assert_eq!(count, 4); // 1 (Downloading), 3 (Waiting), 5 (Checking), 7 (Extracting)
        assert_eq!(repo.get(1).unwrap().state(), DownloadState::Error);
        assert_eq!(repo.get(2).unwrap().state(), DownloadState::Completed);
        assert_eq!(repo.get(3).unwrap().state(), DownloadState::Error);
        assert_eq!(repo.get(4).unwrap().state(), DownloadState::Paused);
        assert_eq!(repo.get(5).unwrap().state(), DownloadState::Error);
        assert_eq!(repo.get(6).unwrap().state(), DownloadState::Queued);
        assert_eq!(repo.get(7).unwrap().state(), DownloadState::Error);
        assert_eq!(repo.get(8).unwrap().state(), DownloadState::Retry); // must NOT be orphaned
    }

    #[test]
    fn test_recover_empty_repo_returns_zero() {
        let repo = InMemoryRepo::new(vec![]);

        let count = recover_orphaned_downloads(&repo).expect("recovery");

        assert_eq!(count, 0);
    }
}
