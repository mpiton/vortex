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
    pub fn from_domain(account: &Account) -> Self {
        use sea_orm::ActiveValue::Set;

        Self {
            id: Set(account.id().as_str().to_string()),
            service_name: Set(account.service_name().to_string()),
            username: Set(account.username().to_string()),
            account_type: Set(account.account_type().to_string()),
            enabled: Set(if account.is_enabled() { 1 } else { 0 }),
            traffic_left: Set(account.traffic_left().map(|b| b as i64)),
            traffic_total: Set(account.traffic_total().map(|b| b as i64)),
            valid_until: Set(account.valid_until().map(|t| t as i64)),
            last_validated: Set(account.last_validated().map(|t| t as i64)),
            created_at: Set(account.created_at() as i64),
        }
    }
}
