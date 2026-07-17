use std::fmt;
use std::str::FromStr;

use crate::domain::error::DomainError;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AccountId(pub String);

impl AccountId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for AccountId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccountType {
    Free,
    Premium,
    Debrid,
}

impl fmt::Display for AccountType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            AccountType::Free => "free",
            AccountType::Premium => "premium",
            AccountType::Debrid => "debrid",
        };
        f.write_str(s)
    }
}

impl FromStr for AccountType {
    type Err = DomainError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "free" => Ok(AccountType::Free),
            "premium" => Ok(AccountType::Premium),
            "debrid" => Ok(AccountType::Debrid),
            other => Err(DomainError::ValidationError(format!(
                "invalid account type: {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum AccountStatus {
    #[default]
    Unverified,
    Valid,
    InvalidCredentials,
    MissingCredential,
    Expired,
    QuotaExhausted,
    Cooldown,
    Error,
}

impl AccountStatus {
    pub fn is_temporary(self) -> bool {
        matches!(self, Self::QuotaExhausted | Self::Cooldown)
    }
}

impl fmt::Display for AccountStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Self::Unverified => "unverified",
            Self::Valid => "valid",
            Self::InvalidCredentials => "invalid_credentials",
            Self::MissingCredential => "missing_credential",
            Self::Expired => "expired",
            Self::QuotaExhausted => "quota_exhausted",
            Self::Cooldown => "cooldown",
            Self::Error => "error",
        };
        f.write_str(value)
    }
}

impl FromStr for AccountStatus {
    type Err = DomainError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "unverified" => Ok(Self::Unverified),
            "valid" => Ok(Self::Valid),
            "invalid_credentials" => Ok(Self::InvalidCredentials),
            "missing_credential" => Ok(Self::MissingCredential),
            "expired" => Ok(Self::Expired),
            "quota_exhausted" => Ok(Self::QuotaExhausted),
            "cooldown" => Ok(Self::Cooldown),
            "error" => Ok(Self::Error),
            other => Err(DomainError::ValidationError(format!(
                "invalid account status: {other}"
            ))),
        }
    }
}

/// Strategy used by `AccountSelector` to pick the next account when several
/// exist for the same service. `BestTraffic` is the default.
///
/// PRD §6.4 — "Auto-select du meilleur compte disponible".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccountSelectionStrategy {
    /// Pick the enabled, non-expired account with the most traffic left.
    /// Unlimited traffic (`None`) ranks above any finite traffic value.
    BestTraffic,
    /// Round-robin across enabled, non-expired candidates ordered by id.
    /// Each `select_best(service)` call advances the cursor for that service.
    RoundRobin,
    /// Defer to a user-pinned account; if none is pinned, fall back to
    /// `BestTraffic`. Pinning UI is a future iteration; today this acts
    /// as a no-op alias of `BestTraffic`.
    Manual,
}

impl AccountSelectionStrategy {
    pub const DEFAULT: Self = AccountSelectionStrategy::BestTraffic;
}

impl fmt::Display for AccountSelectionStrategy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            AccountSelectionStrategy::BestTraffic => "best_traffic",
            AccountSelectionStrategy::RoundRobin => "round_robin",
            AccountSelectionStrategy::Manual => "manual",
        };
        f.write_str(s)
    }
}

impl FromStr for AccountSelectionStrategy {
    type Err = DomainError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "best_traffic" => Ok(AccountSelectionStrategy::BestTraffic),
            "round_robin" => Ok(AccountSelectionStrategy::RoundRobin),
            "manual" => Ok(AccountSelectionStrategy::Manual),
            other => Err(DomainError::ValidationError(format!(
                "invalid account selection strategy: {other}"
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    id: AccountId,
    service_name: String,
    username: String,
    account_type: AccountType,
    enabled: bool,
    traffic_left: Option<u64>,
    traffic_total: Option<u64>,
    valid_until: Option<u64>,
    last_validated: Option<u64>,
    created_at: u64,
    status: AccountStatus,
    /// Quota/rate-limit deadline (Unix epoch ms), persisted so the Accounts
    /// view and a restarted selector observe the same availability state.
    exhausted_until: Option<u64>,
}

impl Account {
    pub fn new(
        id: AccountId,
        service_name: String,
        username: String,
        account_type: AccountType,
        created_at: u64,
    ) -> Self {
        Self {
            id,
            service_name,
            username,
            account_type,
            enabled: true,
            traffic_left: None,
            traffic_total: None,
            valid_until: None,
            last_validated: None,
            created_at,
            status: AccountStatus::Unverified,
            exhausted_until: None,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn reconstruct(
        id: AccountId,
        service_name: String,
        username: String,
        account_type: AccountType,
        enabled: bool,
        traffic_left: Option<u64>,
        traffic_total: Option<u64>,
        valid_until: Option<u64>,
        last_validated: Option<u64>,
        created_at: u64,
    ) -> Self {
        let status = if last_validated.is_some() {
            AccountStatus::Valid
        } else {
            AccountStatus::Unverified
        };
        Self::reconstruct_with_status(
            id,
            service_name,
            username,
            account_type,
            enabled,
            traffic_left,
            traffic_total,
            valid_until,
            last_validated,
            created_at,
            status,
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn reconstruct_with_status(
        id: AccountId,
        service_name: String,
        username: String,
        account_type: AccountType,
        enabled: bool,
        traffic_left: Option<u64>,
        traffic_total: Option<u64>,
        valid_until: Option<u64>,
        last_validated: Option<u64>,
        created_at: u64,
        status: AccountStatus,
        exhausted_until: Option<u64>,
    ) -> Self {
        Self {
            id,
            service_name,
            username,
            account_type,
            enabled,
            traffic_left,
            traffic_total,
            valid_until,
            last_validated,
            created_at,
            status,
            exhausted_until,
        }
    }

    pub fn enable(&mut self) {
        self.enabled = true;
    }

    pub fn disable(&mut self) {
        self.enabled = false;
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub fn is_premium(&self) -> bool {
        matches!(
            self.account_type,
            AccountType::Premium | AccountType::Debrid
        )
    }

    pub fn set_traffic_left(&mut self, bytes: u64) {
        self.traffic_left = Some(bytes);
    }

    pub fn set_traffic_total(&mut self, bytes: u64) {
        self.traffic_total = Some(bytes);
    }

    pub fn set_valid_until(&mut self, timestamp: u64) {
        self.valid_until = Some(timestamp);
    }

    pub fn replace_valid_until(&mut self, timestamp: Option<u64>) {
        self.valid_until = timestamp;
    }

    pub fn set_last_validated(&mut self, timestamp: u64) {
        self.last_validated = Some(timestamp);
    }

    pub fn set_status(&mut self, status: AccountStatus) {
        self.status = status;
        if !status.is_temporary() {
            self.exhausted_until = None;
        }
    }

    pub fn status(&self) -> AccountStatus {
        self.status
    }

    fn mark_temporarily_unavailable(&mut self, status: AccountStatus, until_ms: u64) {
        self.status = status;
        self.exhausted_until = Some(until_ms);
    }

    pub fn is_selectable(&self, now_ms: u64) -> bool {
        if !self.enabled || !self.is_premium() || self.is_expired(now_ms) {
            return false;
        }
        match self.status {
            AccountStatus::Valid => true,
            AccountStatus::QuotaExhausted | AccountStatus::Cooldown => {
                self.exhausted_until.is_some_and(|until| now_ms >= until)
            }
            AccountStatus::Unverified
            | AccountStatus::InvalidCredentials
            | AccountStatus::MissingCredential
            | AccountStatus::Expired
            | AccountStatus::Error => false,
        }
    }

    pub fn is_expired(&self, now: u64) -> bool {
        match self.valid_until {
            Some(expiry) => now > expiry,
            None => false,
        }
    }

    /// Mark this account as quota-exhausted until `until_ms` (Unix epoch
    /// ms). Adapters persist the status and deadline with the aggregate.
    pub fn mark_exhausted(&mut self, until_ms: u64) {
        self.mark_temporarily_unavailable(AccountStatus::QuotaExhausted, until_ms);
    }

    /// Mark this account as rate-limited until `until_ms` (Unix epoch ms).
    pub fn mark_cooldown(&mut self, until_ms: u64) {
        self.mark_temporarily_unavailable(AccountStatus::Cooldown, until_ms);
    }

    /// Drop any pending quota-exhaustion marker, regardless of the
    /// remaining cooldown.
    pub fn clear_exhausted(&mut self) {
        self.exhausted_until = None;
        if self.status.is_temporary() {
            self.status = AccountStatus::Valid;
        }
    }

    /// Active quota-exhaustion deadline (Unix epoch ms) when set, else
    /// `None`. The marker is informational; expiration is decided by
    /// `is_exhausted(now)`.
    pub fn exhausted_until(&self) -> Option<u64> {
        self.exhausted_until
    }

    /// `true` when the exhaustion marker is active at `now` (Unix epoch
    /// ms). Mirrors `is_expired`: the deadline is exclusive — exactly
    /// at `now == until` the cooldown is considered just elapsed.
    pub fn is_exhausted(&self, now: u64) -> bool {
        match self.exhausted_until {
            Some(until) => now < until,
            None => false,
        }
    }

    pub fn id(&self) -> &AccountId {
        &self.id
    }

    pub fn service_name(&self) -> &str {
        &self.service_name
    }

    pub fn username(&self) -> &str {
        &self.username
    }

    pub fn account_type(&self) -> AccountType {
        self.account_type
    }

    pub fn traffic_left(&self) -> Option<u64> {
        self.traffic_left
    }

    pub fn traffic_total(&self) -> Option<u64> {
        self.traffic_total
    }

    pub fn valid_until(&self) -> Option<u64> {
        self.valid_until
    }

    pub fn last_validated(&self) -> Option<u64> {
        self.last_validated
    }

    pub fn created_at(&self) -> u64 {
        self.created_at
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_account() -> Account {
        Account::new(
            AccountId::new("acc-1"),
            "ExampleHost".to_string(),
            "user@example.com".to_string(),
            AccountType::Free,
            1_700_000_000_000,
        )
    }

    #[test]
    fn test_account_new_initialises_defaults() {
        let acc = make_account();
        assert_eq!(acc.id().as_str(), "acc-1");
        assert_eq!(acc.service_name(), "ExampleHost");
        assert_eq!(acc.username(), "user@example.com");
        assert_eq!(acc.account_type(), AccountType::Free);
        assert!(acc.is_enabled());
        assert!(acc.traffic_left().is_none());
        assert!(acc.traffic_total().is_none());
        assert!(acc.valid_until().is_none());
        assert!(acc.last_validated().is_none());
        assert_eq!(acc.created_at(), 1_700_000_000_000);
    }

    #[test]
    fn test_account_enable_disable_toggles_flag() {
        let mut acc = make_account();
        assert!(acc.is_enabled());
        acc.disable();
        assert!(!acc.is_enabled());
        acc.enable();
        assert!(acc.is_enabled());
    }

    #[test]
    fn test_account_is_premium_distinguishes_types() {
        let free = Account::new(
            AccountId::new("a"),
            "H".to_string(),
            "u".to_string(),
            AccountType::Free,
            0,
        );
        let premium = Account::new(
            AccountId::new("b"),
            "H".to_string(),
            "u".to_string(),
            AccountType::Premium,
            0,
        );
        let debrid = Account::new(
            AccountId::new("c"),
            "H".to_string(),
            "u".to_string(),
            AccountType::Debrid,
            0,
        );
        assert!(!free.is_premium());
        assert!(premium.is_premium());
        assert!(debrid.is_premium());
    }

    #[test]
    fn test_account_expiry_is_inclusive_of_valid_until() {
        let mut acc = make_account();
        assert!(!acc.is_expired(1000));
        acc.set_valid_until(500);
        assert!(acc.is_expired(501));
        assert!(!acc.is_expired(500));
        assert!(!acc.is_expired(499));
    }

    #[test]
    fn test_account_traffic_setters_store_values() {
        let mut acc = make_account();
        assert!(acc.traffic_left().is_none());
        assert!(acc.traffic_total().is_none());
        acc.set_traffic_left(1_000_000);
        acc.set_traffic_total(5_000_000);
        assert_eq!(acc.traffic_left(), Some(1_000_000));
        assert_eq!(acc.traffic_total(), Some(5_000_000));
    }

    #[test]
    fn test_account_last_validated_setter_stores_timestamp() {
        let mut acc = make_account();
        assert!(acc.last_validated().is_none());
        acc.set_last_validated(1_700_000_500_000);
        assert_eq!(acc.last_validated(), Some(1_700_000_500_000));
    }

    #[test]
    fn test_account_type_round_trip_via_string() {
        for t in [AccountType::Free, AccountType::Premium, AccountType::Debrid] {
            let s = t.to_string();
            let parsed: AccountType = s.parse().expect("round-trip parse");
            assert_eq!(parsed, t);
        }
    }

    #[test]
    fn test_account_type_from_str_rejects_unknown() {
        let result: Result<AccountType, _> = "unknown".parse();
        assert!(matches!(result, Err(DomainError::ValidationError(_))));
    }

    #[test]
    fn test_account_id_display_returns_inner_value() {
        let id = AccountId::new("xyz-42");
        assert_eq!(id.to_string(), "xyz-42");
        assert_eq!(id.as_str(), "xyz-42");
    }

    #[test]
    fn test_account_selection_strategy_round_trip_via_string() {
        for s in [
            AccountSelectionStrategy::BestTraffic,
            AccountSelectionStrategy::RoundRobin,
            AccountSelectionStrategy::Manual,
        ] {
            let rendered = s.to_string();
            let parsed: AccountSelectionStrategy = rendered.parse().expect("round trip");
            assert_eq!(parsed, s);
        }
    }

    #[test]
    fn test_account_selection_strategy_default_is_best_traffic() {
        assert_eq!(
            AccountSelectionStrategy::DEFAULT,
            AccountSelectionStrategy::BestTraffic
        );
    }

    #[test]
    fn test_account_selection_strategy_from_str_rejects_unknown() {
        let result: Result<AccountSelectionStrategy, _> = "best".parse();
        assert!(matches!(result, Err(DomainError::ValidationError(_))));
    }

    #[test]
    fn test_account_status_round_trip_via_string() {
        for status in [
            AccountStatus::Unverified,
            AccountStatus::Valid,
            AccountStatus::InvalidCredentials,
            AccountStatus::MissingCredential,
            AccountStatus::Expired,
            AccountStatus::QuotaExhausted,
            AccountStatus::Cooldown,
            AccountStatus::Error,
        ] {
            let rendered = status.to_string();
            let parsed: AccountStatus = rendered.parse().expect("round trip");
            assert_eq!(parsed, status);
        }
    }

    #[test]
    fn test_only_valid_or_elapsed_cooldown_accounts_are_selectable() {
        let mut account = Account::new(
            AccountId::new("premium-1"),
            "ExampleHost".to_string(),
            "user@example.com".to_string(),
            AccountType::Premium,
            1_700_000_000_000,
        );
        assert!(!account.is_selectable(1_000));

        account.set_status(AccountStatus::Valid);
        assert!(account.is_selectable(1_000));

        account.set_status(AccountStatus::InvalidCredentials);
        assert!(!account.is_selectable(1_000));

        account.mark_cooldown(2_000);
        assert!(!account.is_selectable(1_999));
        assert!(account.is_selectable(2_000));

        account.mark_exhausted(3_000);
        assert!(!account.is_selectable(2_999));
        assert!(account.is_selectable(3_000));
    }

    #[test]
    fn test_free_account_is_never_selectable_for_premium_resolution() {
        let mut account = make_account();
        account.set_status(AccountStatus::Valid);

        assert!(!account.is_selectable(1_000));
    }

    #[test]
    fn test_reconstruct_with_status_preserves_operational_state() {
        let account = Account::reconstruct_with_status(
            AccountId::new("account-1"),
            "vortex-mod-1fichier".into(),
            "alice".into(),
            AccountType::Premium,
            true,
            None,
            None,
            None,
            Some(500),
            100,
            AccountStatus::Cooldown,
            Some(2_000),
        );
        assert_eq!(account.status(), AccountStatus::Cooldown);
        assert_eq!(account.exhausted_until(), Some(2_000));
    }

    #[test]
    fn test_account_reconstruct_preserves_all_fields() {
        let acc = Account::reconstruct(
            AccountId::new("k"),
            "Host".to_string(),
            "u".to_string(),
            AccountType::Premium,
            false,
            Some(123),
            Some(456),
            Some(789),
            Some(101),
            42,
        );
        assert_eq!(acc.id().as_str(), "k");
        assert!(!acc.is_enabled());
        assert_eq!(acc.traffic_left(), Some(123));
        assert_eq!(acc.traffic_total(), Some(456));
        assert_eq!(acc.valid_until(), Some(789));
        assert_eq!(acc.last_validated(), Some(101));
        assert_eq!(acc.created_at(), 42);
    }

    #[test]
    fn test_account_new_has_no_exhaustion_marker() {
        let acc = make_account();
        assert!(acc.exhausted_until().is_none());
        assert!(!acc.is_exhausted(0));
        assert!(!acc.is_exhausted(u64::MAX));
    }

    #[test]
    fn test_account_reconstruct_resets_exhausted_marker_to_none() {
        // Transient state must NOT survive a reload from SQLite — the
        // rotator owns the lifetime in memory.
        let acc = Account::reconstruct(
            AccountId::new("k"),
            "Host".to_string(),
            "u".to_string(),
            AccountType::Premium,
            true,
            None,
            None,
            None,
            None,
            0,
        );
        assert!(acc.exhausted_until().is_none());
    }

    #[test]
    fn test_mark_exhausted_records_deadline_and_flips_is_exhausted() {
        let mut acc = make_account();
        acc.mark_exhausted(1_000);
        assert_eq!(acc.exhausted_until(), Some(1_000));
        assert!(acc.is_exhausted(0));
        assert!(acc.is_exhausted(999));
        assert!(
            !acc.is_exhausted(1_000),
            "deadline is exclusive — at exact equality cooldown is over"
        );
        assert!(!acc.is_exhausted(1_001));
    }

    #[test]
    fn test_clear_exhausted_drops_marker_regardless_of_clock() {
        let mut acc = make_account();
        acc.mark_exhausted(u64::MAX);
        assert!(acc.is_exhausted(0));
        acc.clear_exhausted();
        assert!(acc.exhausted_until().is_none());
        assert!(!acc.is_exhausted(0));
    }
}
