use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use super::{
    EnqueueCaptchaCommand, RetryCaptchaCommand, SkipCaptchaCommand, SolveCaptchaCommand,
    TimeoutCaptchaCommand,
};
use crate::application::commands::captcha::{CaptchaCommandHandler, ManualCaptchaSolver};
use crate::application::commands::tests_support::{CapturingEventBus, InMemoryDownloadRepo};
use crate::domain::error::DomainError;
use crate::domain::event::DomainEvent;
use crate::domain::model::captcha::{
    CaptchaChallenge, CaptchaId, CaptchaSolution, CaptchaSolverAttemptOutcome, CaptchaStatus,
    CaptchaType,
};
use crate::domain::model::config::{
    AppConfig, CAPTCHA_SOLVER_ANTICAPTCHA, CAPTCHA_SOLVER_BROWSER, CAPTCHA_SOLVER_OCR, ConfigPatch,
};
use crate::domain::model::download::{Download, DownloadId, DownloadState, Url};
use crate::domain::ports::driven::{
    CaptchaInteraction, CaptchaRepository, Clock, ConfigStore, DownloadRepository,
};
use crate::domain::ports::driven::{CaptchaSolver, CaptchaSolverOutcome};
use crate::domain::ports::driving::CommandHandler;

struct MemoryCaptchaRepo {
    items: Mutex<HashMap<CaptchaId, CaptchaChallenge>>,
    fail_next_save: AtomicBool,
}

impl MemoryCaptchaRepo {
    fn new() -> Self {
        Self {
            items: Mutex::new(HashMap::new()),
            fail_next_save: AtomicBool::new(false),
        }
    }

    fn fail_next_save(&self) {
        self.fail_next_save.store(true, Ordering::SeqCst);
    }
}

impl CaptchaRepository for MemoryCaptchaRepo {
    fn save(&self, challenge: &CaptchaChallenge) -> Result<(), DomainError> {
        if self.fail_next_save.swap(false, Ordering::SeqCst) {
            return Err(DomainError::StorageError(
                "injected CAPTCHA save failure".into(),
            ));
        }
        self.items
            .lock()
            .unwrap()
            .insert(challenge.id().clone(), challenge.clone());
        Ok(())
    }

    fn find_by_id(&self, id: &CaptchaId) -> Result<Option<CaptchaChallenge>, DomainError> {
        Ok(self.items.lock().unwrap().get(id).cloned())
    }

    fn list(&self) -> Result<Vec<CaptchaChallenge>, DomainError> {
        Ok(self.items.lock().unwrap().values().cloned().collect())
    }

    fn list_pending(&self) -> Result<Vec<CaptchaChallenge>, DomainError> {
        Ok(self
            .items
            .lock()
            .unwrap()
            .values()
            .filter(|item| item.status() == CaptchaStatus::Pending)
            .cloned()
            .collect())
    }
}

struct FixedClock(AtomicU64);

impl Clock for FixedClock {
    fn now_unix_secs(&self) -> u64 {
        self.0.load(Ordering::SeqCst) / 1_000
    }

    fn now_unix_ms(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }
}

#[derive(Default)]
struct FixedConfig(AppConfig);

impl ConfigStore for FixedConfig {
    fn get_config(&self) -> Result<AppConfig, DomainError> {
        Ok(self.0.clone())
    }

    fn update_config(&self, _: ConfigPatch) -> Result<AppConfig, DomainError> {
        self.get_config()
    }
}

type Fixture = (
    CaptchaCommandHandler,
    Arc<MemoryCaptchaRepo>,
    Arc<InMemoryDownloadRepo>,
    Arc<CapturingEventBus>,
    Arc<FixedClock>,
);

fn fixture() -> Fixture {
    fixture_with_solvers(vec![Arc::new(ManualCaptchaSolver)])
}

fn fixture_with_solvers(solvers: Vec<Arc<dyn CaptchaSolver>>) -> Fixture {
    fixture_with_config_and_solvers(AppConfig::default(), solvers)
}

fn fixture_with_config_and_solvers(
    config: AppConfig,
    solvers: Vec<Arc<dyn CaptchaSolver>>,
) -> Fixture {
    let captchas = Arc::new(MemoryCaptchaRepo::new());
    let downloads = Arc::new(InMemoryDownloadRepo::new());
    let events = Arc::new(CapturingEventBus::new());
    let clock = Arc::new(FixedClock(AtomicU64::new(1_000)));
    let mut download = Download::new(
        DownloadId(42),
        Url::new("https://hoster.example/file").unwrap(),
        "file.zip".into(),
        "/tmp/file.zip".into(),
    );
    download.start().unwrap();
    downloads.seed(download);
    let handler = CaptchaCommandHandler::new(
        captchas.clone(),
        downloads.clone(),
        events.clone(),
        Arc::new(FixedConfig(config)),
        clock.clone(),
        solvers,
    );
    (handler, captchas, downloads, events, clock)
}

struct FailingCaptchaSolver;

impl CaptchaSolver for FailingCaptchaSolver {
    fn name(&self) -> &str {
        "failing"
    }

    fn solve(
        &self,
        _challenge: &CaptchaChallenge,
        _solution: &str,
    ) -> Result<CaptchaSolverOutcome, DomainError> {
        Err(DomainError::PluginError("solver unavailable".into()))
    }
}

struct RecordedCaptchaSolver {
    name: &'static str,
    outcome: CaptchaSolverOutcome,
    calls: Arc<Mutex<Vec<String>>>,
}

struct BlockingCaptchaSolver {
    started: Arc<AtomicBool>,
    release: Arc<AtomicBool>,
}

struct BlockingSolverRelease(Arc<AtomicBool>);

impl BlockingSolverRelease {
    fn release(&self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

impl Drop for BlockingSolverRelease {
    fn drop(&mut self) {
        self.release();
    }
}

impl CaptchaSolver for BlockingCaptchaSolver {
    fn name(&self) -> &str {
        CAPTCHA_SOLVER_OCR
    }

    fn solve(
        &self,
        _challenge: &CaptchaChallenge,
        _solution: &str,
    ) -> Result<CaptchaSolverOutcome, DomainError> {
        self.started.store(true, Ordering::SeqCst);
        while !self.release.load(Ordering::SeqCst) {
            std::thread::yield_now();
        }
        Ok(CaptchaSolverOutcome::Rejected)
    }
}

#[derive(Default)]
struct RecordingCaptchaInteraction(Mutex<Vec<CaptchaId>>);

impl CaptchaInteraction for RecordingCaptchaInteraction {
    fn request(&self, challenge: &CaptchaChallenge) -> Result<(), DomainError> {
        self.0.lock().unwrap().push(challenge.id().clone());
        Ok(())
    }
}

impl CaptchaSolver for RecordedCaptchaSolver {
    fn name(&self) -> &str {
        self.name
    }

    fn solve(
        &self,
        _challenge: &CaptchaChallenge,
        _solution: &str,
    ) -> Result<CaptchaSolverOutcome, DomainError> {
        self.calls.lock().unwrap().push(self.name.to_string());
        Ok(self.outcome.clone())
    }
}

async fn wait_for_solver_attempts(captchas: &MemoryCaptchaRepo, id: &CaptchaId, expected: usize) {
    tokio::time::timeout(std::time::Duration::from_secs(1), async {
        loop {
            if captchas
                .find_by_id(id)
                .unwrap()
                .is_some_and(|challenge| challenge.solver_attempts().len() >= expected)
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("automatic CAPTCHA cascade should finish");
}

fn png_image() -> Vec<u8> {
    b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x01\0\0\0\x01".to_vec()
}

async fn enqueue(handler: &CaptchaCommandHandler) -> CaptchaId {
    <CaptchaCommandHandler as CommandHandler<EnqueueCaptchaCommand>>::handle(
        handler,
        EnqueueCaptchaCommand {
            download_id: DownloadId(42),
            challenge_type: CaptchaType::Image,
            challenge_url: "https://hoster.example/file".into(),
            image_data: Some(png_image()),
        },
    )
    .await
    .expect("enqueue")
}

#[tokio::test]
async fn enqueue_parks_download_and_emits_pending() {
    let (handler, captchas, downloads, events, _) = fixture();
    let id = enqueue(&handler).await;

    assert_eq!(
        downloads
            .find_by_id(DownloadId(42))
            .unwrap()
            .unwrap()
            .state(),
        DownloadState::Waiting
    );
    assert_eq!(
        captchas.find_by_id(&id).unwrap().unwrap().status(),
        CaptchaStatus::Pending
    );
    assert!(events.snapshot().iter().any(|event| matches!(event, DomainEvent::CaptchaPending { challenge_id, download_id } if challenge_id == &id && *download_id == DownloadId(42))));
}

#[tokio::test]
async fn enqueue_runs_enabled_solvers_in_order_until_one_succeeds() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let solvers: Vec<Arc<dyn CaptchaSolver>> = vec![
        Arc::new(RecordedCaptchaSolver {
            name: CAPTCHA_SOLVER_BROWSER,
            outcome: CaptchaSolverOutcome::InteractionRequired,
            calls: calls.clone(),
        }),
        Arc::new(RecordedCaptchaSolver {
            name: CAPTCHA_SOLVER_ANTICAPTCHA,
            outcome: CaptchaSolverOutcome::Solved(
                CaptchaSolution::try_new("ephemeral-answer").unwrap(),
            ),
            calls: calls.clone(),
        }),
        Arc::new(RecordedCaptchaSolver {
            name: CAPTCHA_SOLVER_OCR,
            outcome: CaptchaSolverOutcome::Unavailable,
            calls: calls.clone(),
        }),
    ];
    let (handler, captchas, downloads, _, _) =
        fixture_with_config_and_solvers(AppConfig::default(), solvers);

    let id = enqueue(&handler).await;
    wait_for_solver_attempts(&captchas, &id, 2).await;

    assert_eq!(
        calls.lock().unwrap().as_slice(),
        [CAPTCHA_SOLVER_OCR, CAPTCHA_SOLVER_ANTICAPTCHA]
    );
    let stored = captchas.find_by_id(&id).unwrap().unwrap();
    assert_eq!(stored.status(), CaptchaStatus::Solved);
    assert_eq!(stored.solver(), Some(CAPTCHA_SOLVER_ANTICAPTCHA));
    assert_eq!(stored.solver_attempts().len(), 2);
    assert_eq!(
        stored.solver_attempts()[0].outcome(),
        CaptchaSolverAttemptOutcome::Unavailable
    );
    assert_eq!(
        stored.solver_attempts()[1].outcome(),
        CaptchaSolverAttemptOutcome::Solved
    );
    assert_eq!(
        downloads
            .find_by_id(DownloadId(42))
            .unwrap()
            .unwrap()
            .state(),
        DownloadState::Queued
    );
}

#[tokio::test]
async fn late_automatic_result_is_logged_after_manual_resolution() {
    let started = Arc::new(AtomicBool::new(false));
    let release = Arc::new(AtomicBool::new(false));
    let release_guard = BlockingSolverRelease(release.clone());
    let config = AppConfig {
        captcha_solver_order: vec![CAPTCHA_SOLVER_OCR.into()],
        ..AppConfig::default()
    };
    let (handler, captchas, _, _, _) = fixture_with_config_and_solvers(
        config,
        vec![Arc::new(BlockingCaptchaSolver {
            started: started.clone(),
            release: release.clone(),
        })],
    );
    let id = enqueue(&handler).await;
    tokio::time::timeout(std::time::Duration::from_secs(1), async {
        while !started.load(Ordering::SeqCst) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("automatic solver should start");

    <CaptchaCommandHandler as CommandHandler<SolveCaptchaCommand>>::handle(
        &handler,
        SolveCaptchaCommand {
            challenge_id: id.clone(),
            solution: "manual-answer".into(),
        },
    )
    .await
    .expect("manual resolution");
    release_guard.release();
    wait_for_solver_attempts(&captchas, &id, 2).await;

    let stored = captchas.find_by_id(&id).unwrap().unwrap();
    assert_eq!(stored.status(), CaptchaStatus::Solved);
    assert_eq!(stored.solver(), Some("manual"));
    assert_eq!(
        stored
            .solver_attempts()
            .iter()
            .map(|attempt| (attempt.solver(), attempt.outcome()))
            .collect::<Vec<_>>(),
        vec![
            ("manual", CaptchaSolverAttemptOutcome::Solved),
            (CAPTCHA_SOLVER_OCR, CaptchaSolverAttemptOutcome::Rejected),
        ]
    );
}

#[tokio::test]
async fn automatic_failures_fall_through_and_are_logged_per_solver() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let solvers: Vec<Arc<dyn CaptchaSolver>> = vec![
        Arc::new(FailingCaptchaSolver),
        Arc::new(RecordedCaptchaSolver {
            name: CAPTCHA_SOLVER_ANTICAPTCHA,
            outcome: CaptchaSolverOutcome::Rejected,
            calls: calls.clone(),
        }),
        Arc::new(RecordedCaptchaSolver {
            name: CAPTCHA_SOLVER_BROWSER,
            outcome: CaptchaSolverOutcome::InteractionRequired,
            calls: calls.clone(),
        }),
    ];
    let config = AppConfig {
        captcha_solver_order: vec![
            "failing".into(),
            CAPTCHA_SOLVER_ANTICAPTCHA.into(),
            CAPTCHA_SOLVER_BROWSER.into(),
        ],
        ..AppConfig::default()
    };
    let (handler, captchas, downloads, _, _) = fixture_with_config_and_solvers(config, solvers);

    let id = enqueue(&handler).await;
    wait_for_solver_attempts(&captchas, &id, 3).await;

    let stored = captchas.find_by_id(&id).unwrap().unwrap();
    assert_eq!(stored.status(), CaptchaStatus::Pending);
    assert_eq!(
        stored
            .solver_attempts()
            .iter()
            .map(|attempt| (attempt.solver(), attempt.outcome()))
            .collect::<Vec<_>>(),
        vec![
            ("failing", CaptchaSolverAttemptOutcome::Failed),
            (
                CAPTCHA_SOLVER_ANTICAPTCHA,
                CaptchaSolverAttemptOutcome::Rejected,
            ),
            (
                CAPTCHA_SOLVER_BROWSER,
                CaptchaSolverAttemptOutcome::InteractionRequired,
            ),
        ]
    );
    assert_eq!(
        downloads
            .find_by_id(DownloadId(42))
            .unwrap()
            .unwrap()
            .state(),
        DownloadState::Waiting
    );
}

#[tokio::test]
async fn browser_fallback_requests_a_human_interaction_window() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let interaction = Arc::new(RecordingCaptchaInteraction::default());
    let config = AppConfig {
        captcha_solver_order: vec![CAPTCHA_SOLVER_BROWSER.into()],
        ..AppConfig::default()
    };
    let (handler, captchas, _, _, _) = fixture_with_config_and_solvers(
        config,
        vec![Arc::new(RecordedCaptchaSolver {
            name: CAPTCHA_SOLVER_BROWSER,
            outcome: CaptchaSolverOutcome::InteractionRequired,
            calls,
        })],
    );
    let handler = handler.with_interaction(interaction.clone());

    let id = enqueue(&handler).await;
    wait_for_solver_attempts(&captchas, &id, 1).await;
    tokio::time::timeout(std::time::Duration::from_secs(1), async {
        loop {
            if interaction.0.lock().unwrap().as_slice() == [id.clone()] {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("browser interaction should be requested");
}

#[tokio::test]
async fn enqueue_failure_restores_the_download_and_emits_a_safe_failure() {
    let (handler, captchas, downloads, events, _) = fixture();
    captchas.fail_next_save();

    let result = <CaptchaCommandHandler as CommandHandler<EnqueueCaptchaCommand>>::handle(
        &handler,
        EnqueueCaptchaCommand {
            download_id: DownloadId(42),
            challenge_type: CaptchaType::Image,
            challenge_url: "https://hoster.example/file".into(),
            image_data: Some(png_image()),
        },
    )
    .await;

    assert!(result.is_err());
    assert_eq!(
        downloads
            .find_by_id(DownloadId(42))
            .unwrap()
            .unwrap()
            .state(),
        DownloadState::Downloading
    );
    assert!(events.snapshot().iter().any(|event| matches!(
        event,
        DomainEvent::DownloadFailed { id, error }
            if *id == DownloadId(42) && error == "CAPTCHA challenge could not be queued"
    )));
}

#[tokio::test]
async fn manual_solve_logs_metadata_and_requeues_without_persisting_answer() {
    let (handler, captchas, downloads, events, clock) = fixture();
    let id = enqueue(&handler).await;
    clock.0.store(4_000, Ordering::SeqCst);

    <CaptchaCommandHandler as CommandHandler<SolveCaptchaCommand>>::handle(
        &handler,
        SolveCaptchaCommand {
            challenge_id: id.clone(),
            solution: "abc123".into(),
        },
    )
    .await
    .expect("solve");

    let stored = captchas.find_by_id(&id).unwrap().unwrap();
    assert_eq!(stored.status(), CaptchaStatus::Solved);
    assert_eq!(stored.solver(), Some("manual"));
    assert_eq!(stored.duration_ms(), Some(3_000));
    assert!(stored.image_data().is_none());
    assert_eq!(
        downloads
            .find_by_id(DownloadId(42))
            .unwrap()
            .unwrap()
            .state(),
        DownloadState::Queued
    );
    assert!(events.snapshot().iter().any(|event| matches!(event, DomainEvent::CaptchaSolved { challenge_id, .. } if challenge_id == &id)));
}

#[tokio::test]
async fn rejected_manual_solution_is_logged_without_leaving_the_pending_state() {
    let (handler, captchas, _, _, _) = fixture();
    let id = enqueue(&handler).await;

    let result = <CaptchaCommandHandler as CommandHandler<SolveCaptchaCommand>>::handle(
        &handler,
        SolveCaptchaCommand {
            challenge_id: id.clone(),
            solution: "   ".into(),
        },
    )
    .await;

    assert!(matches!(result, Err(DomainError::ValidationError(_))));
    let stored = captchas.find_by_id(&id).unwrap().unwrap();
    assert_eq!(stored.status(), CaptchaStatus::Pending);
    assert_eq!(stored.solver_attempts().len(), 1);
    assert_eq!(stored.solver_attempts()[0].solver(), "manual");
    assert_eq!(
        stored.solver_attempts()[0].outcome(),
        CaptchaSolverAttemptOutcome::Rejected
    );
}

#[tokio::test]
async fn solve_falls_through_when_an_earlier_solver_errors() {
    let (handler, captchas, _, _, _) = fixture_with_solvers(vec![
        Arc::new(FailingCaptchaSolver),
        Arc::new(ManualCaptchaSolver),
    ]);
    let id = enqueue(&handler).await;

    <CaptchaCommandHandler as CommandHandler<SolveCaptchaCommand>>::handle(
        &handler,
        SolveCaptchaCommand {
            challenge_id: id.clone(),
            solution: "abc123".into(),
        },
    )
    .await
    .expect("manual solver should run after the failing solver");

    assert_eq!(
        captchas.find_by_id(&id).unwrap().unwrap().solver(),
        Some("manual")
    );
}

#[tokio::test]
async fn solve_rolls_back_the_challenge_when_download_persistence_fails() {
    let (handler, captchas, downloads, _, _) = fixture();
    let id = enqueue(&handler).await;
    downloads.fail_next_save();

    let result = <CaptchaCommandHandler as CommandHandler<SolveCaptchaCommand>>::handle(
        &handler,
        SolveCaptchaCommand {
            challenge_id: id.clone(),
            solution: "abc123".into(),
        },
    )
    .await;

    assert!(result.is_err());
    let stored = captchas.find_by_id(&id).unwrap().unwrap();
    assert_eq!(stored.status(), CaptchaStatus::Pending);
    assert!(stored.image_data().is_some());
    assert_eq!(
        downloads
            .find_by_id(DownloadId(42))
            .unwrap()
            .unwrap()
            .state(),
        DownloadState::Waiting
    );
}

#[tokio::test]
async fn skip_is_terminal_and_does_not_enter_automatic_retry() {
    let (handler, captchas, downloads, events, _) = fixture();
    let id = enqueue(&handler).await;

    <CaptchaCommandHandler as CommandHandler<SkipCaptchaCommand>>::handle(
        &handler,
        SkipCaptchaCommand {
            challenge_id: id.clone(),
        },
    )
    .await
    .expect("skip");

    assert_eq!(
        captchas.find_by_id(&id).unwrap().unwrap().status(),
        CaptchaStatus::Skipped
    );
    assert_eq!(
        downloads
            .find_by_id(DownloadId(42))
            .unwrap()
            .unwrap()
            .state(),
        DownloadState::Error
    );
    assert!(
        !events
            .snapshot()
            .iter()
            .any(|event| matches!(event, DomainEvent::DownloadFailed { .. }))
    );
}

#[tokio::test]
async fn skip_rolls_back_the_challenge_when_download_persistence_fails() {
    let (handler, captchas, downloads, _, _) = fixture();
    let id = enqueue(&handler).await;
    downloads.fail_next_save();

    let result = <CaptchaCommandHandler as CommandHandler<SkipCaptchaCommand>>::handle(
        &handler,
        SkipCaptchaCommand {
            challenge_id: id.clone(),
        },
    )
    .await;

    assert!(result.is_err());
    assert_eq!(
        captchas.find_by_id(&id).unwrap().unwrap().status(),
        CaptchaStatus::Pending
    );
    assert_eq!(
        downloads
            .find_by_id(DownloadId(42))
            .unwrap()
            .unwrap()
            .state(),
        DownloadState::Waiting
    );
}

#[tokio::test]
async fn removing_a_captcha_download_terminalizes_its_pending_challenge() {
    let (handler, captchas, downloads, _, _) = fixture();
    let id = enqueue(&handler).await;

    assert!(
        handler
            .skip_pending_for_download(DownloadId(42))
            .await
            .expect("terminalize pending CAPTCHA")
    );
    assert_eq!(
        captchas.find_by_id(&id).unwrap().unwrap().status(),
        CaptchaStatus::Skipped
    );
    assert_eq!(
        downloads
            .find_by_id(DownloadId(42))
            .unwrap()
            .unwrap()
            .state(),
        DownloadState::Error
    );
    assert!(
        !handler
            .skip_pending_for_download(DownloadId(42))
            .await
            .expect("already terminal")
    );
}

#[tokio::test]
async fn timeout_skips_by_default_and_retry_renews_a_pending_challenge() {
    let (handler, captchas, _, _, clock) = fixture();
    let id = enqueue(&handler).await;
    clock.0.store(5_000, Ordering::SeqCst);
    <CaptchaCommandHandler as CommandHandler<RetryCaptchaCommand>>::handle(
        &handler,
        RetryCaptchaCommand {
            challenge_id: id.clone(),
        },
    )
    .await
    .expect("retry");
    let retried = captchas.find_by_id(&id).unwrap().unwrap();
    assert_eq!(retried.attempts(), 1);
    assert_eq!(retried.expires_at(), 125_000);

    <CaptchaCommandHandler as CommandHandler<TimeoutCaptchaCommand>>::handle(
        &handler,
        TimeoutCaptchaCommand {
            challenge_id: id.clone(),
            expected_expires_at: 121_000,
        },
    )
    .await
    .expect("stale timeout is ignored");
    assert_eq!(
        captchas.find_by_id(&id).unwrap().unwrap().status(),
        CaptchaStatus::Pending
    );

    clock.0.store(125_000, Ordering::SeqCst);
    <CaptchaCommandHandler as CommandHandler<TimeoutCaptchaCommand>>::handle(
        &handler,
        TimeoutCaptchaCommand {
            challenge_id: id.clone(),
            expected_expires_at: 125_000,
        },
    )
    .await
    .expect("current timeout");
    assert_eq!(
        captchas.find_by_id(&id).unwrap().unwrap().status(),
        CaptchaStatus::TimedOut
    );
}

#[tokio::test]
async fn timeout_terminalizes_a_challenge_even_if_the_download_is_already_terminal() {
    let (handler, captchas, downloads, _, clock) = fixture();
    let id = enqueue(&handler).await;
    let mut completed = Download::new(
        DownloadId(42),
        Url::new("https://hoster.example/file").unwrap(),
        "file.zip".into(),
        "/tmp/file.zip".into(),
    );
    completed.start().unwrap();
    completed.complete().unwrap();
    downloads.seed(completed);
    clock.0.store(121_000, Ordering::SeqCst);

    <CaptchaCommandHandler as CommandHandler<TimeoutCaptchaCommand>>::handle(
        &handler,
        TimeoutCaptchaCommand {
            challenge_id: id.clone(),
            expected_expires_at: 121_000,
        },
    )
    .await
    .expect("terminal download must not keep the CAPTCHA pending");

    assert_eq!(
        captchas.find_by_id(&id).unwrap().unwrap().status(),
        CaptchaStatus::TimedOut
    );
}

#[tokio::test]
async fn retry_requeues_a_skipped_challenge_download() {
    let (handler, _, downloads, events, _) = fixture();
    let id = enqueue(&handler).await;
    <CaptchaCommandHandler as CommandHandler<SkipCaptchaCommand>>::handle(
        &handler,
        SkipCaptchaCommand {
            challenge_id: id.clone(),
        },
    )
    .await
    .unwrap();

    <CaptchaCommandHandler as CommandHandler<RetryCaptchaCommand>>::handle(
        &handler,
        RetryCaptchaCommand { challenge_id: id },
    )
    .await
    .expect("skipped CAPTCHA can be retried manually");

    assert_eq!(
        downloads
            .find_by_id(DownloadId(42))
            .unwrap()
            .unwrap()
            .state(),
        DownloadState::Retry
    );
    assert!(events.snapshot().iter().any(|event| matches!(
        event,
        DomainEvent::DownloadRetrying { id, attempt: 1 } if *id == DownloadId(42)
    )));
}

#[tokio::test]
async fn retry_requeues_a_timed_out_challenge_download() {
    let (handler, _, downloads, _, clock) = fixture();
    let id = enqueue(&handler).await;
    clock.0.store(121_000, Ordering::SeqCst);
    <CaptchaCommandHandler as CommandHandler<TimeoutCaptchaCommand>>::handle(
        &handler,
        TimeoutCaptchaCommand {
            challenge_id: id.clone(),
            expected_expires_at: 121_000,
        },
    )
    .await
    .unwrap();

    <CaptchaCommandHandler as CommandHandler<RetryCaptchaCommand>>::handle(
        &handler,
        RetryCaptchaCommand { challenge_id: id },
    )
    .await
    .expect("timed-out CAPTCHA can be retried manually");

    assert_eq!(
        downloads
            .find_by_id(DownloadId(42))
            .unwrap()
            .unwrap()
            .state(),
        DownloadState::Retry
    );
}

#[tokio::test]
async fn retry_rejects_a_solved_challenge() {
    let (handler, _, _, _, _) = fixture();
    let id = enqueue(&handler).await;
    <CaptchaCommandHandler as CommandHandler<SolveCaptchaCommand>>::handle(
        &handler,
        SolveCaptchaCommand {
            challenge_id: id.clone(),
            solution: "abc123".into(),
        },
    )
    .await
    .unwrap();

    let result = <CaptchaCommandHandler as CommandHandler<RetryCaptchaCommand>>::handle(
        &handler,
        RetryCaptchaCommand { challenge_id: id },
    )
    .await;

    assert!(matches!(result, Err(DomainError::ValidationError(_))));
}

#[tokio::test]
async fn solve_rejects_an_expired_challenge() {
    let (handler, captchas, _, _, clock) = fixture();
    let id = enqueue(&handler).await;
    clock.0.store(121_000, Ordering::SeqCst);

    let result = <CaptchaCommandHandler as CommandHandler<SolveCaptchaCommand>>::handle(
        &handler,
        SolveCaptchaCommand {
            challenge_id: id.clone(),
            solution: "too late".into(),
        },
    )
    .await;

    assert!(result.is_err());
    assert_eq!(
        captchas.find_by_id(&id).unwrap().unwrap().status(),
        CaptchaStatus::Pending
    );
}
