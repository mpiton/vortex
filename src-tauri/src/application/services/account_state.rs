//! Shared account transitions for typed plugin failures.

use crate::domain::error::DomainError;
use crate::domain::model::account::{Account, AccountStatus};

pub const TEMPORARY_ACCOUNT_FAILURE_MS: u64 = 60_000;

pub fn status_for_plugin_error(error: &DomainError) -> Option<AccountStatus> {
    match error {
        DomainError::AccountInvalidCredentials => Some(AccountStatus::InvalidCredentials),
        DomainError::AccountExpired => Some(AccountStatus::Expired),
        DomainError::AccountCooldown => Some(AccountStatus::Cooldown),
        DomainError::AccountQuotaExceeded => Some(AccountStatus::QuotaExhausted),
        _ => None,
    }
}

pub fn apply_status(account: &mut Account, status: AccountStatus, now_ms: u64) {
    let until_ms = now_ms.saturating_add(TEMPORARY_ACCOUNT_FAILURE_MS);
    match status {
        AccountStatus::QuotaExhausted => account.mark_exhausted(until_ms),
        AccountStatus::Cooldown => account.mark_cooldown(until_ms),
        other => account.set_status(other),
    }
}
