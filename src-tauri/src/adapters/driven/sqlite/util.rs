use sea_orm::sea_query::{Expr, SimpleExpr};

use crate::domain::error::DomainError;

pub const MIN_PLAUSIBLE_UNIX_MS: u64 = 946_684_800_000;

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

pub fn infer_timestamp_ms_from_download_id(raw_id: i64) -> Option<u64> {
    let ts = safe_u64(raw_id) >> 12;
    (ts >= MIN_PLAUSIBLE_UNIX_MS).then_some(ts)
}

/// Resolve a download row's `created_at` timestamp with the same
/// fallback chain SQL ordering uses (`inferred_download_created_at_order_expr`):
///   1. `created_at` when persisted as a positive value
///   2. timestamp inferred from the snowflake-style id high bits
///   3. `updated_at` when positive
///   4. `MIN_PLAUSIBLE_UNIX_MS` (the sentinel anchor)
///
/// Legacy rows persisted before the timestamp backfill landed often have
/// `created_at = 0`; reading the raw column there would surface "1970"
/// dates and break the secondary sort key in any view that consumes
/// download rows.
pub fn resolve_download_created_at(raw_created_at: i64, raw_id: i64, raw_updated_at: i64) -> u64 {
    let created_at = safe_u64(raw_created_at);
    if created_at > 0 {
        return created_at;
    }
    if let Some(inferred) = infer_timestamp_ms_from_download_id(raw_id) {
        return inferred;
    }
    let updated_at = safe_u64(raw_updated_at);
    if updated_at > 0 {
        updated_at
    } else {
        MIN_PLAUSIBLE_UNIX_MS
    }
}

pub fn current_timestamp_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u64::MAX as u128) as u64)
        .unwrap_or(MIN_PLAUSIBLE_UNIX_MS)
}

pub fn inferred_download_created_at_order_expr() -> SimpleExpr {
    Expr::cust(format!(
        "CASE \
            WHEN created_at > 0 THEN created_at \
            WHEN ((id >> 12) >= {MIN_PLAUSIBLE_UNIX_MS}) THEN (id >> 12) \
            WHEN updated_at > 0 THEN updated_at \
            ELSE {MIN_PLAUSIBLE_UNIX_MS} \
        END"
    ))
}
