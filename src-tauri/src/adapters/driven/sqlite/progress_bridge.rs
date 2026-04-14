//! Persists download progress and segment state to SQLite.
//!
//! This bridge subscribes to the domain event bus and updates the read-model
//! tables (`downloads`, `download_segments`) so that queries reflect live
//! progress without the download engine touching the database directly.
//!
//! Segment row IDs are computed deterministically as
//! `download_id * 100 + segment_index`, which is safe for the current
//! snowflake-based download IDs (~7 × 10¹⁵) and the bounded segment count
//! (≤ 16 per download by default).

use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};

use crate::domain::event::DomainEvent;
use crate::domain::ports::driven::EventBus;

/// Register the SQLite progress bridge on the given event bus.
///
/// Each relevant domain event spawns a fire-and-forget tokio task that writes
/// to the database. Failures are logged as warnings and do not crash the app.
pub fn spawn_sqlite_progress_bridge(event_bus: &dyn EventBus, db: DatabaseConnection) {
    event_bus.subscribe(Box::new(move |event: &DomainEvent| {
        handle_event(db.clone(), event);
    }));
}

fn segment_row_id(download_id: u64, segment_index: u32) -> i64 {
    (download_id as i64)
        .saturating_mul(100)
        .saturating_add(segment_index as i64)
}

fn handle_event(db: DatabaseConnection, event: &DomainEvent) {
    match event {
        DomainEvent::DownloadProgress {
            id,
            downloaded_bytes,
            total_bytes: _,
        } => {
            let download_id = id.0 as i64;
            let downloaded = *downloaded_bytes as i64;
            tokio::spawn(async move {
                update_download_progress(&db, download_id, downloaded).await;
            });
        }

        DomainEvent::SegmentStarted {
            download_id,
            segment_id: segment_index,
            start_byte,
            end_byte,
        } => {
            let row_id = segment_row_id(download_id.0, *segment_index);
            let did = download_id.0 as i64;
            let sidx = *segment_index as i32;
            let sb = *start_byte as i64;
            let eb = *end_byte as i64;
            tokio::spawn(async move {
                insert_segment(&db, row_id, did, sidx, sb, eb).await;
            });
        }

        DomainEvent::SegmentCompleted {
            download_id,
            segment_id: segment_index,
        } => {
            let did = download_id.0 as i64;
            let sidx = *segment_index as i32;
            tokio::spawn(async move {
                complete_segment(&db, did, sidx).await;
            });
        }

        DomainEvent::SegmentFailed {
            download_id,
            segment_id: segment_index,
            ..
        } => {
            let did = download_id.0 as i64;
            let sidx = *segment_index as i32;
            tokio::spawn(async move {
                fail_segment(&db, did, sidx).await;
            });
        }

        _ => {}
    }
}

async fn update_download_progress(
    db: &DatabaseConnection,
    download_id: i64,
    downloaded_bytes: i64,
) {
    let sql = "UPDATE downloads SET downloaded_bytes = ? WHERE id = ?";
    let stmt = Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        sql,
        [downloaded_bytes.into(), download_id.into()],
    );
    if let Err(e) = db.execute(stmt).await {
        tracing::warn!(download_id, error = %e, "progress_bridge: failed to update downloaded_bytes");
    }
}

async fn insert_segment(
    db: &DatabaseConnection,
    row_id: i64,
    download_id: i64,
    segment_index: i32,
    start_byte: i64,
    end_byte: i64,
) {
    let sql = "INSERT OR REPLACE INTO download_segments \
               (id, download_id, segment_index, start_byte, end_byte, downloaded_bytes, state) \
               VALUES (?, ?, ?, ?, ?, 0, 'Downloading')";
    let stmt = Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        sql,
        [
            row_id.into(),
            download_id.into(),
            (segment_index as i64).into(),
            start_byte.into(),
            end_byte.into(),
        ],
    );
    if let Err(e) = db.execute(stmt).await {
        tracing::warn!(
            download_id,
            segment_index,
            error = %e,
            "progress_bridge: failed to insert segment"
        );
    }
}

async fn complete_segment(db: &DatabaseConnection, download_id: i64, segment_index: i32) {
    let sql = "UPDATE download_segments \
               SET state = 'Completed', downloaded_bytes = end_byte - start_byte \
               WHERE download_id = ? AND segment_index = ?";
    let stmt = Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        sql,
        [download_id.into(), (segment_index as i64).into()],
    );
    if let Err(e) = db.execute(stmt).await {
        tracing::warn!(
            download_id,
            segment_index,
            error = %e,
            "progress_bridge: failed to mark segment completed"
        );
    }
}

async fn fail_segment(db: &DatabaseConnection, download_id: i64, segment_index: i32) {
    let sql = "UPDATE download_segments \
               SET state = 'Error' \
               WHERE download_id = ? AND segment_index = ?";
    let stmt = Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        sql,
        [download_id.into(), (segment_index as i64).into()],
    );
    if let Err(e) = db.execute(stmt).await {
        tracing::warn!(
            download_id,
            segment_index,
            error = %e,
            "progress_bridge: failed to mark segment failed"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_segment_row_id_is_deterministic() {
        assert_eq!(segment_row_id(42, 0), 4200);
        assert_eq!(segment_row_id(42, 3), 4203);
    }

    #[test]
    fn test_segment_row_id_no_overflow_for_snowflake_id() {
        // Snowflake IDs are ~(ts_ms << 12) ≈ 6.97 × 10^15 at current time
        let typical_id: u64 = 1_744_000_000_000 << 12; // ~7.1 × 10^15
        let row_id = segment_row_id(typical_id, 15);
        // Must fit in i64 (max 9.22 × 10^18)
        assert!(row_id > 0, "row_id must not overflow to negative");
    }
}
