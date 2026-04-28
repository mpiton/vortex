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
    /// Format: `keyring://{service_name}/{username}`.
    pub fn credential_ref(&self) -> String {
        format!("keyring://{}/{}", self.service_name, self.username)
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
    fn test_account_credential_ref_uses_keyring_scheme() {
        let acc = make_account();
        assert_eq!(
            acc.credential_ref(),
            "keyring://ExampleHost/user@example.com"
        );
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
