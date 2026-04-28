use sea_orm::entity::prelude::*;

use crate::domain::error::DomainError;
use crate::domain::model::account::{Account, AccountId, AccountType};

use crate::adapters::driven::sqlite::util::safe_u64;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "accounts")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    pub service_name: String,
    pub username: String,
    pub account_type: String,
    pub enabled: i32,
    pub traffic_left: Option<i64>,
    pub traffic_total: Option<i64>,
    pub valid_until: Option<i64>,
    pub last_validated: Option<i64>,
    pub created_at: i64,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

impl Model {
    pub fn into_domain(self) -> Result<Account, DomainError> {
        let account_type: AccountType = self.account_type.parse()?;
        Ok(Account::reconstruct(
            AccountId::new(self.id),
            self.service_name,
            self.username,
            account_type,
            self.enabled != 0,
            self.traffic_left.map(safe_u64),
            self.traffic_total.map(safe_u64),
            self.valid_until.map(safe_u64),
            self.last_validated.map(safe_u64),
            safe_u64(self.created_at),
        ))
    }
}

impl ActiveModel {
    pub fn from_domain(account: &Account) -> Result<Self, DomainError> {
        use sea_orm::ActiveValue::Set;

        let id_str = account.id().as_str().to_string();

        let traffic_left = checked_to_i64_opt(account.traffic_left(), "traffic_left", &id_str)?;
        let traffic_total = checked_to_i64_opt(account.traffic_total(), "traffic_total", &id_str)?;
        let valid_until = checked_to_i64_opt(account.valid_until(), "valid_until", &id_str)?;
        let last_validated =
            checked_to_i64_opt(account.last_validated(), "last_validated", &id_str)?;
        let created_at = i64::try_from(account.created_at()).map_err(|_| {
            DomainError::ValidationError(format!("account {id_str}: created_at exceeds i64::MAX"))
        })?;

        Ok(Self {
            id: Set(id_str),
            service_name: Set(account.service_name().to_string()),
            username: Set(account.username().to_string()),
            account_type: Set(account.account_type().to_string()),
            enabled: Set(if account.is_enabled() { 1 } else { 0 }),
            traffic_left: Set(traffic_left),
            traffic_total: Set(traffic_total),
            valid_until: Set(valid_until),
            last_validated: Set(last_validated),
            created_at: Set(created_at),
        })
    }
}

fn checked_to_i64_opt(
    value: Option<u64>,
    field: &str,
    account_id: &str,
) -> Result<Option<i64>, DomainError> {
    value.map(i64::try_from).transpose().map_err(|_| {
        DomainError::ValidationError(format!("account {account_id}: {field} exceeds i64::MAX"))
    })
}
