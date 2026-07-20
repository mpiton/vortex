use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::{
    EnqueueCaptchaCommand, RetryCaptchaCommand, SkipCaptchaCommand, SolveCaptchaCommand,
    TimeoutCaptchaCommand,
};
use crate::application::command_bus::CommandBus;
use crate::application::error::AppError;
use crate::domain::error::DomainError;
use crate::domain::event::DomainEvent;
use crate::domain::model::captcha::{
    CaptchaChallenge, CaptchaId, CaptchaSolution, CaptchaSolverAttempt,
    CaptchaSolverAttemptOutcome, CaptchaStatus, CaptchaType,
};
use crate::domain::ports::driven::{
    CaptchaInteraction, CaptchaRepository, CaptchaSolver, CaptchaSolverOutcome, Clock, ConfigStore,
    DownloadRepository, EventBus,
};
use crate::domain::ports::driving::CommandHandler;

pub struct ManualCaptchaSolver;

impl CaptchaSolver for ManualCaptchaSolver {
    fn name(&self) -> &str {
        "manual"
    }

    fn solve(
        &self,
        challenge: &CaptchaChallenge,
        solution: &str,
    ) -> Result<CaptchaSolverOutcome, DomainError> {
        if !matches!(
            challenge.challenge_type(),
            CaptchaType::Image | CaptchaType::TextInput
        ) {
            return Ok(CaptchaSolverOutcome::Unavailable);
        }
        match CaptchaSolution::try_new(solution) {
            Ok(solution) => Ok(CaptchaSolverOutcome::Solved(solution)),
            Err(_) => Ok(CaptchaSolverOutcome::Rejected),
        }
    }
}

#[derive(Clone)]
pub struct CaptchaCommandHandler {
    captchas: Arc<dyn CaptchaRepository>,
    downloads: Arc<dyn DownloadRepository>,
    events: Arc<dyn EventBus>,
    config: Arc<dyn ConfigStore>,
    clock: Arc<dyn Clock>,
    solvers: Arc<Vec<Arc<dyn CaptchaSolver>>>,
    interaction: Option<Arc<dyn CaptchaInteraction>>,
    timers: Arc<Mutex<HashMap<CaptchaId, CancellationToken>>>,
    solver_runs: Arc<Mutex<HashMap<CaptchaId, CancellationToken>>>,
    mutation_lock: Arc<tokio::sync::Mutex<()>>,
}

impl CaptchaCommandHandler {
    pub fn new(
        captchas: Arc<dyn CaptchaRepository>,
        downloads: Arc<dyn DownloadRepository>,
        events: Arc<dyn EventBus>,
        config: Arc<dyn ConfigStore>,
        clock: Arc<dyn Clock>,
        solvers: Vec<Arc<dyn CaptchaSolver>>,
    ) -> Self {
        Self {
            captchas,
            downloads,
            events,
            config,
            clock,
            solvers: Arc::new(solvers),
            interaction: None,
            timers: Arc::new(Mutex::new(HashMap::new())),
            solver_runs: Arc::new(Mutex::new(HashMap::new())),
            mutation_lock: Arc::new(tokio::sync::Mutex::new(())),
        }
    }

    pub fn with_interaction(mut self, interaction: Arc<dyn CaptchaInteraction>) -> Self {
        self.interaction = Some(interaction);
        self
    }

    pub fn start_listening(self: &Arc<Self>) {
        let handler = Arc::clone(self);
        self.events.subscribe(Box::new(move |event| {
            let DomainEvent::CaptchaRequired {
                download_id,
                challenge_type,
                challenge_url,
                image_data,
            } = event
            else {
                return;
            };
            let handler = Arc::clone(&handler);
            let command = EnqueueCaptchaCommand {
                download_id: *download_id,
                challenge_type: *challenge_type,
                challenge_url: challenge_url.clone(),
                image_data: image_data.as_ref().map(|image| image.to_vec()),
            };
            tokio::spawn(async move {
                if let Err(error) = CommandHandler::handle(handler.as_ref(), command).await {
                    tracing::error!(error = %error, "failed to enqueue CAPTCHA challenge");
                }
            });
        }));
    }

    pub async fn restore_pending(&self) -> Result<(), DomainError> {
        for challenge in self.captchas.list_pending()? {
            self.schedule_timeout(&challenge);
            self.start_solver_cascade(challenge.id().clone());
        }
        Ok(())
    }

    fn timeout_deadline(&self, now_ms: u64) -> Result<u64, DomainError> {
        let seconds = self.config.get_config()?.captcha_timeout_seconds;
        Ok(now_ms.saturating_add(u64::from(seconds).saturating_mul(1_000)))
    }

    fn schedule_timeout(&self, challenge: &CaptchaChallenge) {
        self.cancel_timer(challenge.id());
        let token = CancellationToken::new();
        timer_map(&self.timers).insert(challenge.id().clone(), token.clone());
        let handler = self.clone();
        let challenge_id = challenge.id().clone();
        let expected_expires_at = challenge.expires_at();
        let delay_ms = challenge
            .expires_at()
            .saturating_sub(self.clock.now_unix_ms());
        tokio::spawn(async move {
            let mut delay = Duration::from_millis(delay_ms);
            loop {
                tokio::select! {
                    _ = token.cancelled() => break,
                    _ = tokio::time::sleep(delay) => {
                        let remaining = expected_expires_at
                            .saturating_sub(handler.clock.now_unix_ms());
                        if remaining > 0 {
                            delay = Duration::from_millis(remaining);
                            continue;
                        }
                        match CommandHandler::handle(
                            &handler,
                            TimeoutCaptchaCommand {
                                challenge_id: challenge_id.clone(),
                                expected_expires_at,
                            },
                        ).await {
                            Ok(()) => break,
                            Err(error) => {
                                tracing::error!(error = %error, "failed to time out CAPTCHA challenge; retrying");
                                delay = Duration::from_secs(5);
                            }
                        }
                    }
                }
            }
        });
    }

    fn cancel_timer(&self, id: &CaptchaId) {
        if let Some(token) = timer_map(&self.timers).remove(id) {
            token.cancel();
        }
    }

    fn start_solver_cascade(&self, id: CaptchaId) {
        let token = CancellationToken::new();
        if let Some(previous) = solver_run_map(&self.solver_runs).insert(id.clone(), token.clone())
        {
            previous.cancel();
        }
        let handler = self.clone();
        tokio::spawn(async move {
            if let Err(error) = handler.run_solver_cascade(id, token).await {
                tracing::warn!(error = %error, "automatic CAPTCHA solver cascade stopped");
            }
        });
    }

    fn cancel_solver_cascade(&self, id: &CaptchaId) {
        if let Some(token) = solver_run_map(&self.solver_runs).remove(id) {
            token.cancel();
        }
    }

    fn dismiss_interaction(&self, id: &CaptchaId) {
        if let Some(interaction) = &self.interaction
            && let Err(error) = interaction.dismiss(id)
        {
            tracing::warn!(error = %error, "failed to dismiss CAPTCHA interaction");
        }
    }

    async fn run_solver_cascade(
        &self,
        id: CaptchaId,
        token: CancellationToken,
    ) -> Result<(), DomainError> {
        let order = self.config.get_config()?.captcha_solver_order;
        for solver_name in order {
            if token.is_cancelled() {
                return Ok(());
            }
            let Some(solver) = self
                .solvers
                .iter()
                .find(|solver| solver.name() == solver_name)
                .cloned()
            else {
                continue;
            };
            let challenge = self.find(&id)?;
            if challenge.status() != CaptchaStatus::Pending
                || challenge.is_expired(self.clock.now_unix_ms())
            {
                return Ok(());
            }

            let attempted_at = self.clock.now_unix_ms();
            let started = Instant::now();
            let solver_challenge = challenge.clone();
            let result =
                tokio::task::spawn_blocking(move || solver.solve(&solver_challenge, "")).await;
            let duration_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
            let (outcome, attempt_outcome) = match result {
                Ok(Ok(outcome)) => {
                    let attempt_outcome = match &outcome {
                        CaptchaSolverOutcome::Solved(_) => CaptchaSolverAttemptOutcome::Solved,
                        CaptchaSolverOutcome::Unavailable => {
                            CaptchaSolverAttemptOutcome::Unavailable
                        }
                        CaptchaSolverOutcome::Rejected => CaptchaSolverAttemptOutcome::Rejected,
                        CaptchaSolverOutcome::InteractionRequired => {
                            CaptchaSolverAttemptOutcome::InteractionRequired
                        }
                    };
                    (Some(outcome), attempt_outcome)
                }
                Ok(Err(DomainError::NotFound(_))) => {
                    (None, CaptchaSolverAttemptOutcome::Unavailable)
                }
                Ok(Err(_)) | Err(_) => (None, CaptchaSolverAttemptOutcome::Failed),
            };

            let _guard = self.mutation_lock.lock().await;
            let mut challenge = self.find(&id)?;
            let now = self.clock.now_unix_ms();
            let cascade_stopped = token.is_cancelled()
                || challenge.status() != CaptchaStatus::Pending
                || challenge.is_expired(now);
            challenge.record_solver_attempt(CaptchaSolverAttempt::new(
                solver_name.clone(),
                attempt_outcome,
                attempted_at,
                duration_ms,
            ))?;
            if cascade_stopped {
                self.captchas.save(&challenge)?;
                return Ok(());
            }
            match outcome {
                Some(CaptchaSolverOutcome::Solved(_solution)) => {
                    return self.finish_as_solved_locked(challenge, now, &solver_name);
                }
                Some(CaptchaSolverOutcome::InteractionRequired) => {
                    self.captchas.save(&challenge)?;
                    drop(_guard);
                    if let Some(interaction) = &self.interaction {
                        interaction.request(&challenge)?;
                    }
                    return Ok(());
                }
                Some(CaptchaSolverOutcome::Unavailable | CaptchaSolverOutcome::Rejected) | None => {
                    self.captchas.save(&challenge)?
                }
            }
        }
        Ok(())
    }

    fn find(&self, id: &CaptchaId) -> Result<CaptchaChallenge, DomainError> {
        self.captchas
            .find_by_id(id)?
            .ok_or_else(|| DomainError::NotFound(format!("CAPTCHA {id}")))
    }

    fn finish_as_solved_locked(
        &self,
        mut challenge: CaptchaChallenge,
        now: u64,
        solver: &str,
    ) -> Result<(), DomainError> {
        let mut download = self
            .downloads
            .find_by_id(challenge.download_id())?
            .ok_or_else(|| DomainError::NotFound("CAPTCHA download".into()))?;
        let queued_event = download.queue_after_wait()?;
        let pending_challenge = challenge.clone();
        challenge.solve(now, solver)?;
        self.captchas.save(&challenge)?;
        if let Err(error) = self.downloads.save(&download) {
            if let Err(rollback_error) = self.captchas.save(&pending_challenge) {
                tracing::error!(error = %rollback_error, "failed to roll back CAPTCHA solve");
            }
            return Err(error);
        }
        self.dismiss_interaction(challenge.id());
        self.cancel_timer(challenge.id());
        self.cancel_solver_cascade(challenge.id());
        self.events.publish(queued_event);
        self.events.publish(DomainEvent::CaptchaSolved {
            challenge_id: challenge.id().clone(),
            download_id: challenge.download_id(),
            solver: solver.to_string(),
            duration_ms: challenge.duration_ms().unwrap_or_default(),
        });
        Ok(())
    }
}

impl CommandHandler<EnqueueCaptchaCommand> for CaptchaCommandHandler {
    type Output = CaptchaId;

    async fn handle(&self, command: EnqueueCaptchaCommand) -> Result<CaptchaId, DomainError> {
        let download_id = command.download_id;
        let result = self.enqueue(command).await;
        if result.is_err() {
            self.events.publish(DomainEvent::DownloadFailed {
                id: download_id,
                error: "CAPTCHA challenge could not be queued".into(),
            });
        }
        let (id, created) = result?;
        if created {
            self.start_solver_cascade(id.clone());
        }
        Ok(id)
    }
}

impl CaptchaCommandHandler {
    async fn enqueue(
        &self,
        command: EnqueueCaptchaCommand,
    ) -> Result<(CaptchaId, bool), DomainError> {
        let _guard = self.mutation_lock.lock().await;
        if let Some(existing) = self
            .captchas
            .find_pending_by_download(command.download_id)?
        {
            return Ok((existing.id().clone(), false));
        }
        let now = self.clock.now_unix_ms();
        let expires_at = self.timeout_deadline(now)?;
        let mut challenge = CaptchaChallenge::new(
            CaptchaId::new(Uuid::new_v4().to_string()),
            command.download_id,
            command.challenge_type,
            command.challenge_url,
            now,
            expires_at,
        )?;
        if let Some(image) = command.image_data {
            challenge = challenge.with_image_data(image)?;
        }
        let mut download = self
            .downloads
            .find_by_id(command.download_id)?
            .ok_or_else(|| DomainError::NotFound(format!("download {}", command.download_id.0)))?;
        let active_download = download.clone();
        let waiting_event = download.wait()?;
        self.downloads.save(&download)?;
        if let Err(error) = self.captchas.save(&challenge) {
            if let Err(rollback_error) = self.downloads.save(&active_download) {
                tracing::error!(error = %rollback_error, "failed to roll back CAPTCHA enqueue");
            }
            return Err(error);
        }
        self.events.publish(waiting_event);
        self.events.publish(DomainEvent::CaptchaPending {
            challenge_id: challenge.id().clone(),
            download_id: command.download_id,
        });
        self.schedule_timeout(&challenge);
        Ok((challenge.id().clone(), true))
    }
}

impl CommandHandler<SolveCaptchaCommand> for CaptchaCommandHandler {
    type Output = ();

    async fn handle(&self, command: SolveCaptchaCommand) -> Result<(), DomainError> {
        let _guard = self.mutation_lock.lock().await;
        let mut challenge = self.find(&command.challenge_id)?;
        let now = self.clock.now_unix_ms();
        if challenge.status() != CaptchaStatus::Pending {
            return Err(DomainError::ValidationError(
                "CAPTCHA challenge is no longer pending".into(),
            ));
        }
        if challenge.is_expired(now) {
            return Err(DomainError::ValidationError(
                "CAPTCHA challenge has expired".into(),
            ));
        }
        let started = Instant::now();
        let outcome = ManualCaptchaSolver.solve(&challenge, &command.solution)?;
        let duration_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        let attempt_outcome = match &outcome {
            CaptchaSolverOutcome::Solved(_) => CaptchaSolverAttemptOutcome::Solved,
            CaptchaSolverOutcome::Unavailable => CaptchaSolverAttemptOutcome::Unavailable,
            CaptchaSolverOutcome::Rejected => CaptchaSolverAttemptOutcome::Rejected,
            CaptchaSolverOutcome::InteractionRequired => {
                CaptchaSolverAttemptOutcome::InteractionRequired
            }
        };
        challenge.record_solver_attempt(CaptchaSolverAttempt::new(
            "manual",
            attempt_outcome,
            now,
            duration_ms,
        ))?;
        if matches!(outcome, CaptchaSolverOutcome::Solved(_)) {
            return self.finish_as_solved_locked(challenge, now, "manual");
        }
        self.captchas.save(&challenge)?;
        Err(DomainError::ValidationError(
            "CAPTCHA solution was rejected".into(),
        ))
    }
}

impl CommandHandler<SkipCaptchaCommand> for CaptchaCommandHandler {
    type Output = ();

    async fn handle(&self, command: SkipCaptchaCommand) -> Result<(), DomainError> {
        self.finish_as_failure(command.challenge_id, None).await
    }
}

impl CommandHandler<TimeoutCaptchaCommand> for CaptchaCommandHandler {
    type Output = ();

    async fn handle(&self, command: TimeoutCaptchaCommand) -> Result<(), DomainError> {
        self.finish_as_failure(command.challenge_id, Some(command.expected_expires_at))
            .await
    }
}

impl CaptchaCommandHandler {
    #[cfg(test)]
    pub(crate) async fn skip_pending_for_download(
        &self,
        download_id: crate::domain::model::download::DownloadId,
    ) -> Result<bool, DomainError> {
        let _guard = self.mutation_lock.lock().await;
        let Some(challenge) = self.captchas.find_pending_by_download(download_id)? else {
            return Ok(false);
        };
        self.finish_as_failure_locked(challenge.id().clone(), None)?;
        Ok(true)
    }

    async fn take_download_for_removal<F>(
        &self,
        download_id: crate::domain::model::download::DownloadId,
        before_delete: F,
    ) -> Result<Option<(crate::domain::model::download::Download, bool)>, DomainError>
    where
        F: FnOnce(&crate::domain::model::download::Download, bool) -> Result<bool, DomainError>,
    {
        let _guard = self.mutation_lock.lock().await;
        let Some(download) = self.downloads.find_by_id(download_id)? else {
            return Ok(None);
        };
        let pending = self.captchas.find_pending_by_download(download_id)?;
        let was_pending = pending.is_some();
        let is_active = before_delete(&download, was_pending)?;
        self.downloads.delete(download_id)?;
        if let Some(challenge) = pending
            && let Err(error) = self.finish_as_failure_locked(challenge.id().clone(), None)
        {
            if let Err(rollback_error) = self.downloads.save(&download) {
                tracing::error!(
                    error = %rollback_error,
                    "failed to restore download after CAPTCHA cleanup failure"
                );
            }
            return Err(error);
        }
        Ok(Some((download, is_active)))
    }

    async fn finish_as_failure(
        &self,
        id: CaptchaId,
        expected_expires_at: Option<u64>,
    ) -> Result<(), DomainError> {
        let _guard = self.mutation_lock.lock().await;
        self.finish_as_failure_locked(id, expected_expires_at)
    }

    fn finish_as_failure_locked(
        &self,
        id: CaptchaId,
        expected_expires_at: Option<u64>,
    ) -> Result<(), DomainError> {
        let mut challenge = self.find(&id)?;
        let now = self.clock.now_unix_ms();
        let timed_out = expected_expires_at.is_some();
        if timed_out
            && (challenge.status() != CaptchaStatus::Pending
                || expected_expires_at != Some(challenge.expires_at())
                || !challenge.is_expired(now))
        {
            return Ok(());
        }
        let pending_challenge = challenge.clone();
        let reason = if timed_out {
            challenge.timeout(now)?;
            "CAPTCHA timed out"
        } else {
            challenge.skip(now, "CAPTCHA skipped by user")?;
            "CAPTCHA skipped by user"
        };
        let failed_download = match self.downloads.find_by_id(challenge.download_id())? {
            Some(mut download) => match download.fail(reason.to_string()) {
                Ok(_) => Some(download),
                Err(DomainError::InvalidTransition { .. }) => None,
                Err(error) => return Err(error),
            },
            None => None,
        };
        self.captchas.save(&challenge)?;
        if let Some(download) = failed_download.as_ref()
            && let Err(error) = self.downloads.save_failed(download, reason)
        {
            if let Err(rollback_error) = self.captchas.save(&pending_challenge) {
                tracing::error!(error = %rollback_error, "failed to roll back CAPTCHA failure");
            }
            return Err(error);
        }
        self.dismiss_interaction(challenge.id());
        self.cancel_timer(challenge.id());
        self.cancel_solver_cascade(challenge.id());
        let event = if timed_out {
            DomainEvent::CaptchaTimedOut {
                challenge_id: challenge.id().clone(),
                download_id: challenge.download_id(),
                duration_ms: challenge.duration_ms().unwrap_or_default(),
            }
        } else {
            DomainEvent::CaptchaSkipped {
                challenge_id: challenge.id().clone(),
                download_id: challenge.download_id(),
                reason: reason.to_string(),
            }
        };
        self.events.publish(event);
        Ok(())
    }
}

impl CommandHandler<RetryCaptchaCommand> for CaptchaCommandHandler {
    type Output = ();

    async fn handle(&self, command: RetryCaptchaCommand) -> Result<(), DomainError> {
        let _guard = self.mutation_lock.lock().await;
        let mut challenge = self.find(&command.challenge_id)?;
        if challenge.status() == CaptchaStatus::Pending {
            let now = self.clock.now_unix_ms();
            if challenge.is_expired(now) {
                return Err(DomainError::ValidationError(
                    "CAPTCHA challenge has expired".into(),
                ));
            }
            challenge.retry(now, self.timeout_deadline(now)?)?;
            self.captchas.save(&challenge)?;
            self.schedule_timeout(&challenge);
            drop(_guard);
            self.start_solver_cascade(challenge.id().clone());
            return Ok(());
        }
        if challenge.status() == CaptchaStatus::Solved {
            return Err(DomainError::ValidationError(
                "Solved CAPTCHA cannot be retried".into(),
            ));
        }
        let mut download = self
            .downloads
            .find_by_id(challenge.download_id())?
            .ok_or_else(|| DomainError::NotFound("CAPTCHA download".into()))?;
        let event = download.retry_manually()?;
        self.downloads.save(&download)?;
        self.events.publish(event);
        Ok(())
    }
}

impl CommandBus {
    pub(crate) async fn delete_download_with_captcha_cleanup<F>(
        &self,
        download_id: crate::domain::model::download::DownloadId,
        before_delete: F,
    ) -> Result<(crate::domain::model::download::Download, bool), AppError>
    where
        F: FnOnce(&crate::domain::model::download::Download, bool) -> Result<bool, DomainError>,
    {
        let removed = match self.captcha_handler_opt() {
            Some(handler) => handler
                .take_download_for_removal(download_id, before_delete)
                .await
                .map_err(AppError::Domain)?,
            None => {
                let Some(download) = self.download_repo().find_by_id(download_id)? else {
                    return Err(AppError::NotFound(format!(
                        "Download {} not found",
                        download_id.0
                    )));
                };
                let is_active = before_delete(&download, false).map_err(AppError::Domain)?;
                self.download_repo().delete(download_id)?;
                Some((download, is_active))
            }
        };
        removed.ok_or_else(|| AppError::NotFound(format!("Download {} not found", download_id.0)))
    }

    pub async fn handle_captcha_solve(&self, command: SolveCaptchaCommand) -> Result<(), AppError> {
        CommandHandler::handle(self.captcha_handler()?, command)
            .await
            .map_err(AppError::Domain)
    }

    pub async fn handle_captcha_skip(&self, command: SkipCaptchaCommand) -> Result<(), AppError> {
        CommandHandler::handle(self.captcha_handler()?, command)
            .await
            .map_err(AppError::Domain)
    }

    pub async fn handle_captcha_retry(&self, command: RetryCaptchaCommand) -> Result<(), AppError> {
        CommandHandler::handle(self.captcha_handler()?, command)
            .await
            .map_err(AppError::Domain)
    }
}

fn timer_map(
    timers: &Mutex<HashMap<CaptchaId, CancellationToken>>,
) -> std::sync::MutexGuard<'_, HashMap<CaptchaId, CancellationToken>> {
    match timers.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn solver_run_map(
    runs: &Mutex<HashMap<CaptchaId, CancellationToken>>,
) -> std::sync::MutexGuard<'_, HashMap<CaptchaId, CancellationToken>> {
    match runs.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}
