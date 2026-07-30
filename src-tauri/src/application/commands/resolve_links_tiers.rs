//! The configurable Premium → Debrid → Free resolution cascade (PRD §4.3).
//!
//! Walking the tiers only *picks* a plugin and an account; no hoster is
//! contacted here. The chosen module is persisted on the download and the
//! unrestrict call happens later, in `ResolveHosterSourceHandler`.

use crate::application::command_bus::CommandBus;
use crate::application::error::AppError;
use crate::application::services::account_rotator::NextAccountOutcome;
use crate::domain::error::DomainError;
use crate::domain::model::config::ResolutionTier;
use crate::domain::model::plugin::PluginCategory;

/// What the cascade decided to do with a URL.
pub(super) enum TierPlan {
    /// Resolve through `module` with this account's credential.
    WithAccount { module: String, account_id: String },
    /// Resolve anonymously through the plugin that owns the URL.
    Anonymous,
}

impl CommandBus {
    /// Walk `resolution_order` until a tier yields a plan.
    ///
    /// `service_name` is the plugin that claimed the URL. Every tier that
    /// declines records why, so the final error names each rung instead of
    /// collapsing to a bare "no source" (R-04).
    pub(super) fn walk_resolution_tiers(
        &self,
        url: &str,
        service_name: &str,
    ) -> Result<TierPlan, AppError> {
        let order = self.config_store().get_config()?.resolution_order;
        let matched_is_debrid = self.is_debrid_plugin(service_name)?;
        let mut skipped: Vec<String> = Vec::with_capacity(order.len());

        for tier in order {
            let outcome = match tier {
                ResolutionTier::Premium if matched_is_debrid => {
                    Err(format!("{service_name} is a debrid service, not a hoster"))
                }
                ResolutionTier::Premium => self.premium_tier(service_name)?,
                ResolutionTier::Debrid => self.debrid_tier(url)?,
                ResolutionTier::Free if matched_is_debrid => {
                    Err("debrid services have no anonymous mode".to_string())
                }
                ResolutionTier::Free => return Ok(TierPlan::Anonymous),
            };
            match outcome {
                Ok(plan) => return Ok(plan),
                Err(reason) => skipped.push(format!("{tier}: {reason}")),
            }
        }
        Err(DomainError::ResolutionExhausted(skipped.join("; ")).into())
    }

    /// A premium account registered against the hoster plugin itself.
    ///
    /// Exhaustion is *not* a skip: the rotator's contract is that the caller
    /// waits for the cooldown rather than silently downgrading a paid account
    /// to the free path.
    fn premium_tier(&self, service_name: &str) -> Result<Result<TierPlan, String>, AppError> {
        if self.account_repo().is_none() {
            return Ok(Err("no account store configured".to_string()));
        }
        match self.next_hoster_account(service_name)? {
            NextAccountOutcome::Picked(account) => Ok(Ok(TierPlan::WithAccount {
                module: service_name.to_string(),
                account_id: account.id().as_str().to_string(),
            })),
            NextAccountOutcome::AllExhausted { reason, .. } => {
                Err(AppError::Domain(reason.into_domain_error()))
            }
            NextAccountOutcome::NoneAvailable => {
                Ok(Err(format!("no premium account for {service_name}")))
            }
        }
    }

    /// Any enabled debrid plugin that covers this hoster and has a usable
    /// account. A covered-but-exhausted debrid falls through (R-04) — unlike
    /// premium, there is no per-hoster subscription to wait on.
    fn debrid_tier(&self, url: &str) -> Result<Result<TierPlan, String>, AppError> {
        if self.account_repo().is_none() {
            return Ok(Err("no account store configured".to_string()));
        }
        let candidates = self.debrid_plugins_covering(url)?;
        if candidates.is_empty() {
            return Ok(Err("no debrid service covers this hoster".to_string()));
        }
        let mut reasons = Vec::with_capacity(candidates.len());
        for name in candidates {
            match self.next_hoster_account(&name)? {
                NextAccountOutcome::Picked(account) => {
                    return Ok(Ok(TierPlan::WithAccount {
                        module: name,
                        account_id: account.id().as_str().to_string(),
                    }));
                }
                NextAccountOutcome::AllExhausted { reason, .. } => {
                    reasons.push(format!("{name} {:?}", reason).to_lowercase());
                }
                NextAccountOutcome::NoneAvailable => reasons.push(format!("{name} no account")),
            }
        }
        Ok(Err(reasons.join(", ")))
    }

    fn debrid_plugins_covering(&self, url: &str) -> Result<Vec<String>, AppError> {
        let mut names: Vec<String> = self
            .plugin_loader()
            .list_loaded()?
            .into_iter()
            .filter(|info| info.is_enabled() && info.category() == PluginCategory::Debrid)
            .map(|info| info.name().to_string())
            .collect();
        names.sort();
        let mut covering = Vec::with_capacity(names.len());
        for name in names {
            if self.plugin_loader().plugin_can_handle(&name, url)? {
                covering.push(name);
            }
        }
        Ok(covering)
    }

    fn next_hoster_account(&self, service_name: &str) -> Result<NextAccountOutcome, AppError> {
        let Some(rotator) = self.account_rotator() else {
            return Ok(match self.resolve_account_for(service_name)? {
                Some(account) => NextAccountOutcome::Picked(account),
                None => NextAccountOutcome::NoneAvailable,
            });
        };
        let strategy = self.config_store().get_config()?.account_selection_strategy;
        rotator.next_account(service_name, strategy)
    }

    fn is_debrid_plugin(&self, service_name: &str) -> Result<bool, AppError> {
        Ok(self
            .plugin_loader()
            .list_loaded()?
            .into_iter()
            .any(|info| info.name() == service_name && info.category() == PluginCategory::Debrid))
    }
}

#[cfg(test)]
#[path = "resolve_links_tiers_tests.rs"]
mod tests;
