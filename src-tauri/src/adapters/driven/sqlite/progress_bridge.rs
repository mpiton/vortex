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
//!
//! All DB mutations are serialised through a single background worker task,
//! so that `SegmentCompleted` can never execute before its preceding
//! `SegmentStarted` insert, and progress values never regress.

use tokio::sync::mpsc;

use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};

use crate::domain::event::DomainEvent;
use crate::domain::ports::driven::EventBus;

/// Messages sent from the event-bus subscriber to the DB worker.
#[derive(Debug)]
enum BridgeMessage {
    Progress {
        download_id: i64,
        downloaded_bytes: i64,
    },
    SegmentStarted {
        row_id: i64,
        download_id: i64,
        segment_index: i32,
        /// `-1` when `end_byte` was the `u64::MAX` no-range sentinel.
        start_byte: i64,
        end_byte: i64,
    },
    SegmentCompleted {
        download_id: i64,
        segment_index: i32,
    },
    SegmentFailed {
        download_id: i64,
        segment_index: i32,
    },
}

/// Register the SQLite progress bridge on the given event bus.
///
/// All DB mutations are routed through a single background worker, which
/// guarantees that writes are applied in event-bus order and that
/// `SegmentCompleted` cannot outrace its `SegmentStarted` insert.
pub fn spawn_sqlite_progress_bridge(event_bus: &dyn EventBus, db: DatabaseConnection) {
    let (tx, mut rx) = mpsc::unbounded_channel::<BridgeMessage>();

    tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            match msg {
                BridgeMessage::Progress {
                    download_id,
                    downloaded_bytes,
                } => {
                    update_download_progress(&db, download_id, downloaded_bytes).await;
                }
                BridgeMessage::SegmentStarted {
                    row_id,
                    download_id,
                    segment_index,
                    start_byte,
                    end_byte,
                } => {
                    insert_segment(
                        &db,
                        row_id,
                        download_id,
                        segment_index,
                        start_byte,
                        end_byte,
                    )
                    .await;
                }
                BridgeMessage::SegmentCompleted {
                    download_id,
                    segment_index,
                } => {
                    complete_segment(&db, download_id, segment_index).await;
                }
                BridgeMessage::SegmentFailed {
                    download_id,
                    segment_index,
                } => {
                    fail_segment(&db, download_id, segment_index).await;
                }
            }
        }
    });

    event_bus.subscribe(Box::new(move |event: &DomainEvent| {
        let msg = event_to_message(event);
        if let Some(m) = msg
            && let Err(error) = tx.send(m)
        {
            tracing::warn!(event = ?event, ?error, "progress_bridge: worker channel closed");
        }
    }));
}

fn segment_row_id(download_id: u64, segment_index: u32) -> i64 {
    (download_id as i64)
        .saturating_mul(100)
        .saturating_add(segment_index as i64)
}

fn event_to_message(event: &DomainEvent) -> Option<BridgeMessage> {
    match event {
        DomainEvent::DownloadProgress {
            id,
            downloaded_bytes,
            total_bytes: _,
        } => Some(BridgeMessage::Progress {
            download_id: id.0 as i64,
            downloaded_bytes: *downloaded_bytes as i64,
        }),

        DomainEvent::SegmentStarted {
            download_id,
            segment_id: segment_index,
            start_byte,
            end_byte,
        } => {
            // u64::MAX is the sentinel for "no Range header" — cast it to -1
            // so the DB stores a recognisable sentinel that complete_segment
            // can guard against (see CASE WHEN end_byte >= 0 below).
            let eb = if *end_byte == u64::MAX {
                -1_i64
            } else {
                *end_byte as i64
            };
            Some(BridgeMessage::SegmentStarted {
                row_id: segment_row_id(download_id.0, *segment_index),
                download_id: download_id.0 as i64,
                segment_index: *segment_index as i32,
                start_byte: *start_byte as i64,
                end_byte: eb,
            })
        }

        DomainEvent::SegmentCompleted {
            download_id,
            segment_id: segment_index,
        } => Some(BridgeMessage::SegmentCompleted {
            download_id: download_id.0 as i64,
            segment_index: *segment_index as i32,
        }),

        DomainEvent::SegmentFailed {
            download_id,
            segment_id: segment_index,
            ..
        } => Some(BridgeMessage::SegmentFailed {
            download_id: download_id.0 as i64,
            segment_index: *segment_index as i32,
        }),

        _ => None,
    }
}

/// Update `downloads.downloaded_bytes`, never allowing a stale write to
/// decrease the stored value (monotonic via SQL `MAX`).
async fn update_download_progress(
    db: &DatabaseConnection,
    download_id: i64,
    downloaded_bytes: i64,
) {
    let sql = "UPDATE downloads SET downloaded_bytes = MAX(downloaded_bytes, ?) WHERE id = ?";
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

/// Mark a segment Completed and set its downloaded_bytes.
///
/// `end_byte` is exclusive for ranged segments, so the completed byte count is
/// `end_byte - start_byte`. When `end_byte` is the sentinel `-1` (no-Range
/// download), fall back to the parent download's known total bytes, or its
/// aggregate downloaded bytes if the total is still unknown.
async fn complete_segment(db: &DatabaseConnection, download_id: i64, segment_index: i32) {
    let sql = "UPDATE download_segments \
               SET state = 'Completed', \
                   downloaded_bytes = CASE WHEN end_byte >= 0 \
                                          THEN end_byte - start_byte \
                                          ELSE COALESCE( \
                                              (SELECT total_bytes FROM downloads WHERE id = ?), \
                                              (SELECT downloaded_bytes FROM downloads WHERE id = ?), \
                                              downloaded_bytes \
                                          ) END \
               WHERE download_id = ? AND segment_index = ?";
    let stmt = Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        sql,
        [
            download_id.into(),
            download_id.into(),
            download_id.into(),
            (segment_index as i64).into(),
        ],
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
    use sea_orm::{ActiveModelTrait, EntityTrait, Set};

    use crate::adapters::driven::sqlite::connection::setup_test_db;
    use crate::adapters::driven::sqlite::entities::{download, download_segment};

    #[test]
    fn test_segment_row_id_is_deterministic() {
        assert_eq!(segment_row_id(42, 0), 4200);
        assert_eq!(segment_row_id(42, 3), 4203);
    }

    #[test]
    fn test_segment_row_id_no_overflow_for_snowflake_id() {
        // Snowflake IDs are ~(ts_ms << 12) ≈ 6.97 × 10^15 at current time.
        // typical_id = 1_744_000_000_000 * 4_096 = 7_143_424_000_000_000
        let typical_id: u64 = 1_744_000_000_000 << 12;
        let row_id = segment_row_id(typical_id, 15);
        // 7_143_424_000_000_000 * 100 + 15 = 714_342_400_000_000_015
        // i64::MAX = 9_223_372_036_854_775_807  →  well within range
        assert_eq!(
            row_id, 714_342_400_000_000_015_i64,
            "row_id must be deterministic and not overflow"
        );
    }

    #[test]
    fn test_event_to_message_sentinel_end_byte() {
        use crate::domain::model::download::DownloadId;

        let event = DomainEvent::SegmentStarted {
            download_id: DownloadId(1),
            segment_id: 0,
            start_byte: 0,
            end_byte: u64::MAX,
        };
        match event_to_message(&event) {
            Some(BridgeMessage::SegmentStarted { end_byte, .. }) => {
                assert_eq!(end_byte, -1_i64, "u64::MAX sentinel must become -1i64");
            }
            _ => panic!("expected SegmentStarted message"),
        }
    }

    #[test]
    fn test_event_to_message_normal_end_byte() {
        use crate::domain::model::download::DownloadId;

        let event = DomainEvent::SegmentStarted {
            download_id: DownloadId(1),
            segment_id: 0,
            start_byte: 0,
            end_byte: 1024,
        };
        match event_to_message(&event) {
            Some(BridgeMessage::SegmentStarted { end_byte, .. }) => {
                assert_eq!(end_byte, 1024_i64);
            }
            _ => panic!("expected SegmentStarted message"),
        }
    }

    async fn insert_download_row(
        db: &DatabaseConnection,
        id: i64,
        total_bytes: Option<i64>,
        downloaded_bytes: i64,
    ) {
        download::ActiveModel {
            id: Set(id),
            url: Set("https://example.test/file.bin".to_string()),
            file_name: Set("file.bin".to_string()),
            state: Set("Downloading".to_string()),
            priority: Set(5),
            total_bytes: Set(total_bytes),
            downloaded_bytes: Set(downloaded_bytes),
            speed_bytes_per_sec: Set(0),
            retry_count: Set(0),
            max_retries: Set(5),
            segments_count: Set(1),
            checksum_expected: Set(None),
            source_hostname: Set("example.test".to_string()),
            protocol: Set("https".to_string()),
            resume_supported: Set(1),
            module_name: Set(None),
            account_id: Set(None),
            destination_path: Set("/tmp/file.bin".to_string()),
            created_at: Set(1),
            updated_at: Set(1),
        }
        .insert(db)
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn test_complete_segment_sets_ranged_segment_bytes() {
        let db = setup_test_db().await.unwrap();
        insert_download_row(&db, 7, Some(4096), 0).await;

        let row_id = segment_row_id(7, 2);
        insert_segment(&db, row_id, 7, 2, 128, 640).await;
        complete_segment(&db, 7, 2).await;

        let model = download_segment::Entity::find_by_id(row_id)
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(model.state, "Completed");
        assert_eq!(
            model.downloaded_bytes, 512,
            "exclusive end-byte ranges should complete to end - start"
        );
    }

    #[tokio::test]
    async fn test_complete_segment_sets_no_range_bytes_from_download_total() {
        let db = setup_test_db().await.unwrap();
        insert_download_row(&db, 9, Some(2048), 1536).await;

        let row_id = segment_row_id(9, 0);
        insert_segment(&db, row_id, 9, 0, 0, -1).await;
        complete_segment(&db, 9, 0).await;

        let model = download_segment::Entity::find_by_id(row_id)
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(model.state, "Completed");
        assert_eq!(
            model.downloaded_bytes, 2048,
            "no-range segments should complete to the download's total size when known"
        );
    }

    #[tokio::test]
    async fn test_complete_segment_falls_back_to_downloaded_bytes_when_total_unknown() {
        let db = setup_test_db().await.unwrap();
        insert_download_row(&db, 11, None, 777).await;

        let row_id = segment_row_id(11, 0);
        insert_segment(&db, row_id, 11, 0, 0, -1).await;
        complete_segment(&db, 11, 0).await;

        let model = download_segment::Entity::find_by_id(row_id)
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(model.state, "Completed");
        assert_eq!(
            model.downloaded_bytes, 777,
            "no-range segments should fall back to aggregate download progress when total size is unknown"
        );
    }
}
