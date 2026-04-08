use crate::domain::error::DomainError;

/// Bridge a sync trait method to async sea-orm by running on a dedicated thread.
/// Uses `std::thread::scope` + `Handle::block_on` to work with both
/// `current_thread` and `multi_thread` tokio runtimes (unlike `block_in_place`).
pub fn block_on<F: std::future::Future + Send>(future: F) -> F::Output
where
    F::Output: Send,
{
    let handle = tokio::runtime::Handle::current();
    std::thread::scope(|s| {
        s.spawn(|| handle.block_on(future))
            .join()
            .expect("db thread panicked")
    })
}

pub fn map_db_err(e: sea_orm::DbErr) -> DomainError {
    DomainError::StorageError(e.to_string())
}

/// Safely convert an i64 from SQLite to u64, defaulting to 0 for negative values.
pub fn safe_u64(val: i64) -> u64 {
    u64::try_from(val).unwrap_or(0)
}

/// Safely convert an i64 from SQLite to u32, defaulting to 0 for out-of-range values.
pub fn safe_u32(val: i64) -> u32 {
    u32::try_from(val).unwrap_or(0)
}
