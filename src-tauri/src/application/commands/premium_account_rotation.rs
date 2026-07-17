use crate::application::error::AppError;
use crate::application::services::account_rotator::NextAccountOutcome;
use crate::domain::error::DomainError;
use crate::domain::model::account::AccountId;
use crate::domain::model::download::Download;
use crate::domain::ports::driven::{
    ExtractedHosterLink, ResolutionCancellation, ResolvedDownloadSource,
};

use super::{ResolveHosterSourceHandler, ResolvePremiumSourceCommand};

impl ResolveHosterSourceHandler {
    pub(super) fn resolve_download(
        &self,
        download: &Download,
        cancellation: &ResolutionCancellation,
    ) -> Result<ResolvedDownloadSource, DomainError> {
        cancellation.ensure_active()?;
        let service = download.module_name().ok_or_else(|| {
            DomainError::ValidationError("premium download has no plugin association".into())
        })?;
        let mut account_id = download.account_id().cloned().ok_or_else(|| {
            DomainError::ValidationError("premium download has no account association".into())
        })?;
        let attempts = self.repo.list_by_service(service)?.len().saturating_add(1);
        let mut last_error = None;

        for _ in 0..attempts {
            cancellation.ensure_active()?;
            match self.resolve_once(download, service, &account_id, cancellation) {
                Ok(link) => {
                    cancellation.ensure_active()?;
                    return protected_source(link);
                }
                Err(error) if is_rotatable(&error) => last_error = Some(error),
                Err(error) => return Err(error),
            }
            account_id = match self.next_account(service, cancellation)? {
                NextAccountOutcome::Picked(account) => account.id().clone(),
                NextAccountOutcome::AllExhausted { reason, .. } => {
                    return Err(last_error.unwrap_or_else(|| reason.into_domain_error()));
                }
                NextAccountOutcome::NoneAvailable => break,
            };
        }

        Err(last_error.unwrap_or_else(|| {
            DomainError::ValidationError("no premium account is available".into())
        }))
    }

    fn resolve_once(
        &self,
        download: &Download,
        service: &str,
        account_id: &AccountId,
        cancellation: &ResolutionCancellation,
    ) -> Result<ExtractedHosterLink, DomainError> {
        let lock = self.account_lock(account_id)?;
        let _guard = lock.blocking_lock();
        let command = ResolvePremiumSourceCommand::new(
            account_id.clone(),
            service.to_string(),
            download.url().as_str().to_string(),
        );
        let link = self.resolve_locked(command, cancellation)?;
        let expected = download.account_id().ok_or_else(|| {
            DomainError::ValidationError("premium download has no account association".into())
        })?;
        let updated = cancellation.run_if_active(|| {
            self.downloads
                .compare_and_set_account_reference(download.id(), expected, account_id)
        })?;
        if !updated {
            let current = self.downloads.find_by_id(download.id())?;
            match current {
                None => {
                    return Err(DomainError::NotFound(format!(
                        "download {}",
                        download.id().0
                    )));
                }
                Some(current) if current.account_id() == Some(account_id) => {}
                Some(_) => {
                    return Err(DomainError::ValidationError(
                        "download account association changed during resolution".into(),
                    ));
                }
            }
        }
        cancellation.run_if_active(|| {
            self.publish_success(account_id);
            Ok(())
        })?;
        Ok(link)
    }

    fn next_account(
        &self,
        service: &str,
        cancellation: &ResolutionCancellation,
    ) -> Result<NextAccountOutcome, DomainError> {
        cancellation.ensure_active()?;
        let strategy = self.config.get_config()?.account_selection_strategy;
        self.rotator
            .next_account(service, strategy)
            .map_err(app_error_to_domain)
    }
}

fn protected_source(link: ExtractedHosterLink) -> Result<ResolvedDownloadSource, DomainError> {
    let direct_url = link
        .direct_url
        .filter(|url| !url.trim().is_empty())
        .ok_or(DomainError::HosterNoFile)?;
    Ok(ResolvedDownloadSource::protected(direct_url)
        .with_request_headers(link.request_headers)
        .with_metadata(link.filename, link.size_bytes, link.resumable))
}

fn is_rotatable(error: &DomainError) -> bool {
    match error {
        DomainError::AccountInvalidCredentials
        | DomainError::AccountExpired
        | DomainError::AccountCooldown
        | DomainError::AccountQuotaExceeded => true,
        DomainError::NotFound(message) => message.starts_with("credential for account "),
        DomainError::ValidationError(message) => message == "premium account is unavailable",
        _ => false,
    }
}

fn app_error_to_domain(error: AppError) -> DomainError {
    match error {
        AppError::Domain(error) => error,
        AppError::NotFound(message) => DomainError::NotFound(message),
        AppError::Validation(message) => DomainError::ValidationError(message),
        other => DomainError::StorageError(other.to_string()),
    }
}
