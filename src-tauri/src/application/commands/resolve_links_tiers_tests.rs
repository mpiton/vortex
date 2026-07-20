//! Cascade tests for the configurable Premium → Debrid → Free order (R-03/R-04).

use std::sync::{Arc, Mutex};

use super::*;
use crate::application::commands::tests_support::{
    InMemoryAccountRepo, build_account_bus_with_config,
};
use crate::application::commands::{ResolveLinksCommand, ResolvedLinkDto};
use crate::domain::model::account::{Account, AccountId, AccountStatus, AccountType};
use crate::domain::model::config::{AppConfig, ConfigPatch, default_resolution_order};
use crate::domain::model::credential::Credential;
use crate::domain::model::plugin::{PluginInfo, PluginManifest};
use crate::domain::ports::driven::{
    AccountRepository, ConfigStore, ExtractedHosterLink, PluginLoader,
};

const HOSTER: &str = "vortex-mod-mediafire";
const DEBRID: &str = "vortex-mod-alldebrid";
const URL: &str = "https://www.mediafire.com/file/abc/archive.zip/file";

/// One hoster plugin owning the URL plus one debrid plugin that also
/// claims it, which is the configuration the cascade exists to arbitrate.
struct CascadeLoader {
    debrid_covers: bool,
    anonymous_calls: Mutex<Vec<String>>,
}

impl CascadeLoader {
    fn new(debrid_covers: bool) -> Self {
        Self {
            debrid_covers,
            anonymous_calls: Mutex::new(Vec::new()),
        }
    }

    fn info(name: &str) -> PluginInfo {
        let category = if name == DEBRID {
            PluginCategory::Debrid
        } else {
            PluginCategory::Hoster
        };
        PluginInfo::new(
            name.to_string(),
            "1.0.0".into(),
            name.to_string(),
            "vortex".into(),
            category,
        )
    }
}

impl PluginLoader for CascadeLoader {
    fn load(&self, _: &PluginManifest) -> Result<(), DomainError> {
        Ok(())
    }

    fn unload(&self, _: &str) -> Result<(), DomainError> {
        Ok(())
    }

    fn resolve_url(&self, _: &str) -> Result<Option<PluginInfo>, DomainError> {
        Ok(Some(Self::info(HOSTER)))
    }

    fn plugin_can_handle(&self, name: &str, _: &str) -> Result<bool, DomainError> {
        Ok(name != DEBRID || self.debrid_covers)
    }

    fn list_loaded(&self) -> Result<Vec<PluginInfo>, DomainError> {
        Ok(vec![Self::info(HOSTER), Self::info(DEBRID)])
    }

    fn set_enabled(&self, _: &str, _: bool) -> Result<(), DomainError> {
        Ok(())
    }

    fn extract_hoster_links(
        &self,
        service_name: &str,
        url: &str,
        credential: Option<&Credential>,
    ) -> Result<Vec<ExtractedHosterLink>, DomainError> {
        assert!(credential.is_none(), "the free tier is anonymous");
        self.anonymous_calls
            .lock()
            .expect("call log")
            .push(service_name.to_string());
        Ok(vec![ExtractedHosterLink {
            source_url: url.to_string(),
            filename: Some("archive.zip".into()),
            size_bytes: Some(42),
            direct_url: Some("https://cdn.example/archive.zip".into()),
            resumable: Some(true),
            request_headers: Vec::new(),
            traffic_used_bytes: None,
            traffic_total_bytes: None,
            captcha: None,
        }])
    }
}

struct OrderedConfigStore(Vec<ResolutionTier>);

impl ConfigStore for OrderedConfigStore {
    fn get_config(&self) -> Result<AppConfig, DomainError> {
        Ok(AppConfig {
            resolution_order: self.0.clone(),
            ..AppConfig::default()
        })
    }

    fn update_config(&self, _: ConfigPatch) -> Result<AppConfig, DomainError> {
        self.get_config()
    }
}

fn account(id: &str, service: &str, status: AccountStatus) -> Account {
    let account_type = if service == DEBRID {
        AccountType::Debrid
    } else {
        AccountType::Premium
    };
    let mut account = Account::new(
        AccountId::new(id),
        service.to_string(),
        "user".into(),
        account_type,
        0,
    );
    account.set_status(status);
    account
}

async fn resolve(
    order: Vec<ResolutionTier>,
    accounts: Vec<Account>,
    debrid_covers: bool,
) -> (Vec<ResolvedLinkDto>, Arc<CascadeLoader>) {
    let repo = Arc::new(InMemoryAccountRepo::new());
    for account in accounts {
        repo.save(&account).expect("seed account");
    }
    let plugins = Arc::new(CascadeLoader::new(debrid_covers));
    let bus =
        build_account_bus_with_config(repo, plugins.clone(), Arc::new(OrderedConfigStore(order)));
    let resolved = bus
        .handle_resolve_links(ResolveLinksCommand {
            urls: vec![URL.into()],
        })
        .await
        .expect("resolution reports per-link status, never a bus error");
    (resolved, plugins)
}

#[tokio::test]
async fn test_premium_account_wins_over_debrid_in_the_default_order() {
    let (resolved, plugins) = resolve(
        default_resolution_order(),
        vec![
            account("premium-1", HOSTER, AccountStatus::Valid),
            account("debrid-1", DEBRID, AccountStatus::Valid),
        ],
        true,
    )
    .await;

    assert_eq!(resolved[0].module_name, HOSTER);
    assert_eq!(resolved[0].account_id.as_deref(), Some("premium-1"));
    assert!(plugins.anonymous_calls.lock().expect("call log").is_empty());
}

#[tokio::test]
async fn test_debrid_resolves_the_link_when_no_premium_account_exists() {
    let (resolved, plugins) = resolve(
        default_resolution_order(),
        vec![account("debrid-1", DEBRID, AccountStatus::Valid)],
        true,
    )
    .await;

    assert_eq!(resolved[0].module_name, DEBRID);
    assert_eq!(resolved[0].account_id.as_deref(), Some("debrid-1"));
    assert!(plugins.anonymous_calls.lock().expect("call log").is_empty());
}

#[tokio::test]
async fn test_reordering_the_cascade_puts_debrid_ahead_of_a_premium_account() {
    let (resolved, _) = resolve(
        vec![
            ResolutionTier::Debrid,
            ResolutionTier::Premium,
            ResolutionTier::Free,
        ],
        vec![
            account("premium-1", HOSTER, AccountStatus::Valid),
            account("debrid-1", DEBRID, AccountStatus::Valid),
        ],
        true,
    )
    .await;

    assert_eq!(resolved[0].module_name, DEBRID);
    assert_eq!(resolved[0].account_id.as_deref(), Some("debrid-1"));
}

#[tokio::test]
async fn test_debrid_that_does_not_cover_the_hoster_falls_through_to_free() {
    let (resolved, plugins) = resolve(
        default_resolution_order(),
        vec![account("debrid-1", DEBRID, AccountStatus::Valid)],
        false,
    )
    .await;

    assert_eq!(resolved[0].module_name, HOSTER);
    assert_eq!(resolved[0].account_id, None);
    assert_eq!(resolved[0].status, "online");
    assert_eq!(*plugins.anonymous_calls.lock().expect("call log"), [HOSTER]);
}

#[tokio::test]
async fn test_exhausted_debrid_account_falls_through_to_free() {
    let (resolved, plugins) = resolve(
        default_resolution_order(),
        vec![account("debrid-1", DEBRID, AccountStatus::QuotaExhausted)],
        true,
    )
    .await;

    assert_eq!(resolved[0].module_name, HOSTER);
    assert_eq!(resolved[0].status, "online");
    assert_eq!(*plugins.anonymous_calls.lock().expect("call log"), [HOSTER]);
}

#[tokio::test]
async fn test_an_exhausted_premium_account_aborts_instead_of_downgrading_to_free() {
    // The rotator's contract is that the caller waits out the cooldown, so
    // premium exhaustion is not a skip. Downgrading here would quietly turn
    // a paid download into a throttled anonymous one.
    let mut premium = account("premium-1", HOSTER, AccountStatus::Valid);
    // A deadline no clock reaches: the account is in cooldown, not absent,
    // which is what separates exhaustion from "no account configured".
    premium.mark_exhausted(u64::MAX);

    let (resolved, plugins) = resolve(default_resolution_order(), vec![premium], true).await;

    assert_eq!(resolved[0].status, "error");
    assert!(plugins.anonymous_calls.lock().expect("call log").is_empty());
}

#[tokio::test]
async fn test_cascade_with_no_usable_tier_reports_every_rung_it_tried() {
    let (resolved, plugins) = resolve(
        vec![ResolutionTier::Premium, ResolutionTier::Debrid],
        Vec::new(),
        true,
    )
    .await;

    assert_eq!(resolved[0].status, "error");
    let message = resolved[0]
        .error_message
        .as_deref()
        .expect("an exhausted cascade explains itself");
    assert!(message.contains("premium"), "{message}");
    assert!(message.contains("debrid"), "{message}");
    assert!(plugins.anonymous_calls.lock().expect("call log").is_empty());
}
