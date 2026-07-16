//! Per-account serialization for metadata, keyring, and plugin operations.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::Mutex as AsyncMutex;

use crate::application::error::AppError;
use crate::domain::model::account::AccountId;

#[derive(Default)]
pub struct AccountOperationLocks {
    entries: Mutex<HashMap<AccountId, Arc<AsyncMutex<()>>>>,
}

impl AccountOperationLocks {
    pub fn lock_for(&self, id: &AccountId) -> Result<Arc<AsyncMutex<()>>, AppError> {
        let mut entries = self
            .entries
            .lock()
            .map_err(|_| AppError::Validation("account operation locks mutex poisoned".into()))?;
        Ok(entries
            .entry(id.clone())
            .or_insert_with(|| Arc::new(AsyncMutex::new(())))
            .clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_account_reuses_one_lock_while_distinct_accounts_do_not() {
        let locks = AccountOperationLocks::default();
        let first = locks.lock_for(&AccountId::new("first")).unwrap();
        let same = locks.lock_for(&AccountId::new("first")).unwrap();
        let other = locks.lock_for(&AccountId::new("other")).unwrap();

        assert!(Arc::ptr_eq(&first, &same));
        assert!(!Arc::ptr_eq(&first, &other));
    }
}
