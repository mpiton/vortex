//! Serializable account view DTOs for the frontend.
//!
//! These read models intentionally omit credentials. Passwords / tokens
//! never appear on this type — by construction, not by `#[serde(skip)]`.
//! Code that needs a password must read it from the keyring via the
//! [`AccountCredentialStore`](crate::domain::ports::driven::AccountCredentialStore)
//! port.

use serde::Serialize;

use crate::domain::model::account::Account;

/// Read model for the Accounts list and detail panels.
///
/// Mirrors the persisted columns of the `accounts` table minus any
/// credential reference. The frontend uses [`Self::credential_ref`] only
/// to look up the keyring entry for the "test connection" surface — the
/// actual password is fetched server-side.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountViewDto {
    pub id: String,
    pub service_name: String,
    pub username: String,
    pub account_type: String,
    pub enabled: bool,
    pub traffic_left: Option<u64>,
    pub traffic_total: Option<u64>,
    pub valid_until: Option<u64>,
    pub last_validated: Option<u64>,
    pub created_at: u64,
    /// Opaque keyring URI (`keyring://service/user`) — never the
    /// password itself. Lets the frontend correlate two `AccountView`
    /// rows that share the same stored credential.
    pub credential_ref: String,
}

impl From<Account> for AccountViewDto {
    fn from(account: Account) -> Self {
        Self {
            id: account.id().as_str().to_string(),
            service_name: account.service_name().to_string(),
            username: account.username().to_string(),
            account_type: account.account_type().to_string(),
            enabled: account.is_enabled(),
            traffic_left: account.traffic_left(),
            traffic_total: account.traffic_total(),
            valid_until: account.valid_until(),
            last_validated: account.last_validated(),
            created_at: account.created_at(),
            credential_ref: account.credential_ref(),
        }
    }
}

/// Lightweight DTO returned by the `account_get_traffic` query.
///
/// Reports the persisted traffic counters. The "refresh from upstream"
/// step is performed by the `account_validate` command (task 21);
/// callers that want fresh numbers run validate first, then this query.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountTrafficDto {
    pub id: String,
    pub traffic_left: Option<u64>,
    pub traffic_total: Option<u64>,
    pub valid_until: Option<u64>,
    pub last_validated: Option<u64>,
}

impl From<Account> for AccountTrafficDto {
    fn from(account: Account) -> Self {
        Self {
            id: account.id().as_str().to_string(),
            traffic_left: account.traffic_left(),
            traffic_total: account.traffic_total(),
            valid_until: account.valid_until(),
            last_validated: account.last_validated(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::model::account::{AccountId, AccountType};

    fn make_account() -> Account {
        Account::reconstruct(
            AccountId::new("acc-42"),
            "real-debrid".to_string(),
            "alice".to_string(),
            AccountType::Premium,
            true,
            Some(1_000),
            Some(5_000),
            Some(2_500_000_000_000),
            Some(1_900_000_000_000),
            1_700_000_000_000,
        )
    }

    #[test]
    fn test_account_view_dto_from_domain_copies_metadata() {
        let dto: AccountViewDto = make_account().into();
        assert_eq!(dto.id, "acc-42");
        assert_eq!(dto.service_name, "real-debrid");
        assert_eq!(dto.username, "alice");
        assert_eq!(dto.account_type, "premium");
        assert!(dto.enabled);
        assert_eq!(dto.traffic_left, Some(1_000));
        assert_eq!(dto.traffic_total, Some(5_000));
        assert_eq!(dto.valid_until, Some(2_500_000_000_000));
        assert_eq!(dto.last_validated, Some(1_900_000_000_000));
        assert_eq!(dto.created_at, 1_700_000_000_000);
        assert_eq!(dto.credential_ref, "keyring://real-debrid/alice");
    }

    #[test]
    fn test_account_view_dto_does_not_serialize_password_field() {
        // The DTO has no password member — round-tripping through JSON
        // must never reveal one.
        let dto: AccountViewDto = make_account().into();
        let value = serde_json::to_value(&dto).unwrap();
        let object = value.as_object().expect("dto serializes as object");
        assert!(
            !object.contains_key("password"),
            "AccountViewDto must never expose a password field"
        );
        assert!(
            !object.contains_key("credential"),
            "AccountViewDto must never expose a raw credential field"
        );
    }

    #[test]
    fn test_account_view_dto_serializes_to_camel_case() {
        let dto: AccountViewDto = make_account().into();
        let value = serde_json::to_value(&dto).unwrap();
        let object = value.as_object().unwrap();
        for camel_field in [
            "id",
            "serviceName",
            "username",
            "accountType",
            "enabled",
            "trafficLeft",
            "trafficTotal",
            "validUntil",
            "lastValidated",
            "createdAt",
            "credentialRef",
        ] {
            assert!(
                object.contains_key(camel_field),
                "camelCase field `{camel_field}` missing"
            );
        }
    }

    #[test]
    fn test_account_traffic_dto_from_domain_copies_counters() {
        let dto: AccountTrafficDto = make_account().into();
        assert_eq!(dto.id, "acc-42");
        assert_eq!(dto.traffic_left, Some(1_000));
        assert_eq!(dto.traffic_total, Some(5_000));
        assert_eq!(dto.valid_until, Some(2_500_000_000_000));
        assert_eq!(dto.last_validated, Some(1_900_000_000_000));
    }

    #[test]
    fn test_account_traffic_dto_serializes_to_camel_case() {
        let dto: AccountTrafficDto = make_account().into();
        let value = serde_json::to_value(&dto).unwrap();
        let object = value.as_object().unwrap();
        for camel_field in [
            "id",
            "trafficLeft",
            "trafficTotal",
            "validUntil",
            "lastValidated",
        ] {
            assert!(
                object.contains_key(camel_field),
                "camelCase field `{camel_field}` missing"
            );
        }
        assert!(!object.contains_key("password"));
    }
}
