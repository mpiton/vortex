//! Per-account serialization for metadata, keyring, and plugin operations.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, Weak};
use tokio::sync::Mutex as AsyncMutex;

use crate::application::error::AppError;
use crate::domain::model::account::AccountId;

#[derive(Default)]
pub struct AccountOperationLocks {
    entries: Mutex<HashMap<AccountId, Weak<AsyncMutex<()>>>>,
}

impl AccountOperationLocks {
    pub fn lock_for(&self, id: &AccountId) -> Result<Arc<AsyncMutex<()>>, AppError> {
        let mut entries = self
            .entries
            .lock()
            .map_err(|_| AppError::Validation("account operation locks mutex poisoned".into()))?;
        entries.retain(|_, lock| lock.strong_count() > 0);
        if let Some(lock) = entries.get(id).and_then(Weak::upgrade) {
            return Ok(lock);
        }
        let lock = Arc::new(AsyncMutex::new(()));
        entries.insert(id.clone(), Arc::downgrade(&lock));
        Ok(lock)
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

    #[test]
    fn abandoned_account_locks_are_pruned() {
        let locks = AccountOperationLocks::default();
        let abandoned = locks.lock_for(&AccountId::new("abandoned")).unwrap();
        drop(abandoned);

        let live = locks.lock_for(&AccountId::new("live")).unwrap();
        let _new = locks.lock_for(&AccountId::new("new")).unwrap();

        assert_eq!(locks.entries.lock().unwrap().len(), 2);
        assert!(Arc::strong_count(&live) >= 1);
    }
}
