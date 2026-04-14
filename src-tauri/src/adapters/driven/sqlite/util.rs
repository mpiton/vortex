use sea_orm::sea_query::{Expr, SimpleExpr};

use crate::domain::error::DomainError;

const MIN_PLAUSIBLE_UNIX_MS: u64 = 946_684_800_000;
const SQLITE_CURRENT_TIMESTAMP_MS_EXPR: &str =
    "CAST((julianday('now') - 2440587.5) * 86400000 AS INTEGER)";

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

pub fn current_timestamp_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

pub fn inferred_download_created_at_order_expr() -> SimpleExpr {
    Expr::cust(format!(
        "CASE WHEN created_at > 0 THEN created_at WHEN ((id >> 12) >= {MIN_PLAUSIBLE_UNIX_MS}) THEN (id >> 12) ELSE {SQLITE_CURRENT_TIMESTAMP_MS_EXPR} END"
    ))
}
