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

    pub fn set_last_validated(&mut self, timestamp: u64) {
        self.last_validated = Some(timestamp);
    }

    pub fn is_expired(&self, now: u64) -> bool {
        match self.valid_until {
            Some(expiry) => now > expiry,
            None => false,
        }
    }

    /// Reference used to look up the credential in the system keyring.
    /// Format: `keyring://{service_name}/{username}`. Both segments are
    /// percent-encoded so reserved characters (`/`, `?`, `#`, `@`...) cannot
    /// produce ambiguous refs that point at the wrong stored credential.
    pub fn credential_ref(&self) -> String {
        format!(
            "keyring://{}/{}",
            percent_encode_segment(&self.service_name),
            percent_encode_segment(&self.username)
        )
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

/// Percent-encode a string so it can be safely embedded as a path segment in
/// `keyring://...` refs. Only RFC 3986 unreserved characters survive
/// untouched; everything else is rendered as `%XX` per UTF-8 byte.
fn percent_encode_segment(s: &str) -> String {
    use std::fmt::Write;

    let mut out = String::with_capacity(s.len());
    for byte in s.bytes() {
        let unreserved = byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~');
        if unreserved {
            out.push(byte as char);
        } else {
            let _ = write!(out, "%{byte:02X}");
        }
    }
    out
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
    fn test_account_credential_ref_uses_keyring_scheme() {
        let acc = make_account();
        // `@` in `user@example.com` is reserved → percent-encoded as %40.
        assert_eq!(
            acc.credential_ref(),
            "keyring://ExampleHost/user%40example.com"
        );
    }

    #[test]
    fn test_account_credential_ref_percent_encodes_reserved_chars() {
        // A `/` in the service or username could otherwise collide with the
        // path separator and point two distinct accounts at the same ref.
        let acc = Account::new(
            AccountId::new("acc-collision"),
            "real-debrid/eu".to_string(),
            "alice/admin".to_string(),
            AccountType::Debrid,
            0,
        );
        assert_eq!(
            acc.credential_ref(),
            "keyring://real-debrid%2Feu/alice%2Fadmin"
        );

        let other = Account::new(
            AccountId::new("acc-other"),
            "real-debrid".to_string(),
            "eu/alice/admin".to_string(),
            AccountType::Debrid,
            0,
        );
        assert_ne!(acc.credential_ref(), other.credential_ref());
    }

    #[test]
    fn test_account_credential_ref_handles_unicode_username() {
        let acc = Account::new(
            AccountId::new("acc-utf8"),
            "host".to_string(),
            "café".to_string(),
            AccountType::Free,
            0,
        );
        // `é` is `0xC3 0xA9` in UTF-8.
        assert_eq!(acc.credential_ref(), "keyring://host/caf%C3%A9");
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
}
