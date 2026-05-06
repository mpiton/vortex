//! Persists download progress and segment state to SQLite.
//!
//! This bridge subscribes to the domain event bus and updates the read-model
//! tables (`downloads`, `download_segments`) so that queries reflect live
//! progress without the download engine touching the database directly.
//!
//! Segment row IDs are computed deterministically as
//! `download_id * 100 + segment_index`, which is safe for the current
//! snowflake-based download IDs (~7 × 10¹⁵) and the bounded segment count
//! (`segment_index < 100`; config validation currently caps segments at 32).
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
        /// `0` when total size is unknown.
        total_bytes: i64,
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
    MirrorSwitched {
        download_id: i64,
        new_mirror_index: i32,
    },
    /// Reset the cursor on terminal failure so the next manual / automatic
    /// retry restarts from the highest-priority mirror instead of resuming on
    /// the last-tried slot (which is where a full exhaustion run leaves it).
    MirrorCursorReset { download_id: i64 },
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
                    total_bytes,
                } => {
                    update_download_progress(&db, download_id, downloaded_bytes, total_bytes).await;
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
                BridgeMessage::MirrorSwitched {
                    download_id,
                    new_mirror_index,
                } => {
                    update_mirror_cursor(&db, download_id, new_mirror_index).await;
                    // The next mirror may pick a different segment plan
                    // (no range support, fewer splits). Without this purge
                    // the read model would carry phantom segment rows from
                    // the failed attempt — the detail panel and list
                    // segment counts both query by download_id.
                    clear_segments_for_download(&db, download_id).await;
                }
                BridgeMessage::MirrorCursorReset { download_id } => {
                    update_mirror_cursor(&db, download_id, 0).await;
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

fn segment_row_id(download_id: u64, segment_index: u32) -> Option<i64> {
    if segment_index >= 100 {
        return None;
    }

    Some(
        (download_id as i64)
            .saturating_mul(100)
            .saturating_add(segment_index as i64),
    )
}

fn event_to_message(event: &DomainEvent) -> Option<BridgeMessage> {
    match event {
        DomainEvent::DownloadProgress {
            id,
            downloaded_bytes,
            total_bytes,
        } => Some(BridgeMessage::Progress {
            download_id: id.0 as i64,
            downloaded_bytes: *downloaded_bytes as i64,
            total_bytes: *total_bytes as i64,
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
            let Some(row_id) = segment_row_id(download_id.0, *segment_index) else {
                tracing::warn!(
                    download_id = download_id.0,
                    segment_index = *segment_index,
                    "progress_bridge: refusing segment index >= 100 to avoid row-id collisions"
                );
                return None;
            };
            Some(BridgeMessage::SegmentStarted {
                row_id,
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

        DomainEvent::MirrorSwitched {
            id,
            new_mirror_index,
            ..
        } => Some(BridgeMessage::MirrorSwitched {
            download_id: id.0 as i64,
            new_mirror_index: *new_mirror_index as i32,
        }),

        // After a terminal failure (mirror exhaustion or any other path that
        // surfaces `DownloadFailed`) the persisted cursor is wherever the last
        // attempt left it — usually the bottom-priority slot. Reset to 0 so
        // the next retry walks the mirror list from the top instead of
        // skipping the higher-priority entries that may have recovered.
        DomainEvent::DownloadFailed { id, .. } => Some(BridgeMessage::MirrorCursorReset {
            download_id: id.0 as i64,
        }),

        _ => None,
    }
}

/// Update `downloads.downloaded_bytes` and, when `total_bytes > 0`, also set
/// `total_bytes` (monotonic). The `MAX` guard prevents stale writes from
/// regressing the value.
async fn update_download_progress(
    db: &DatabaseConnection,
    download_id: i64,
    downloaded_bytes: i64,
    total_bytes: i64,
) {
    let sql = if total_bytes > 0 {
        "UPDATE downloads \
         SET downloaded_bytes = MAX(downloaded_bytes, ?), \
             total_bytes = COALESCE(NULLIF(total_bytes, 0), ?) \
         WHERE id = ?"
    } else {
        // total_bytes unknown — only update progress, keep existing total_bytes
        "UPDATE downloads SET downloaded_bytes = MAX(downloaded_bytes, ?) WHERE id = ?"
    };

    let stmt = if total_bytes > 0 {
        Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            sql,
            [
                downloaded_bytes.into(),
                total_bytes.into(),
                download_id.into(),
            ],
        )
    } else {
        Statement::from_sql_and_values(
            DatabaseBackend::Sqlite,
            sql,
            [downloaded_bytes.into(), download_id.into()],
        )
    };

    if let Err(e) = db.execute(stmt).await {
        tracing::warn!(download_id, error = %e, "progress_bridge: failed to update progress");
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

/// Persist the active mirror cursor whenever the engine switches over to a
/// different source. Without this update, `download_read_repo` keeps reporting
/// slot 0 as active even after a runtime failover, so the details panel shows
/// the wrong mirror as the live one.
async fn update_mirror_cursor(db: &DatabaseConnection, download_id: i64, new_mirror_index: i32) {
    let stmt = Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        "UPDATE downloads SET current_mirror_index = ? WHERE id = ?",
        [new_mirror_index.into(), download_id.into()],
    );
    if let Err(e) = db.execute(stmt).await {
        tracing::warn!(
            download_id,
            new_mirror_index,
            error = %e,
            "progress_bridge: failed to persist mirror cursor"
        );
    }
}

/// Drop every segment row belonging to a download. Called from the bridge's
/// `MirrorSwitched` handler so the next mirror attempt starts with an empty
/// segments table — without this the read model surfaces stale segments from
/// the failed attempt, including ones whose state was `Error` at the moment
/// of the switch.
async fn clear_segments_for_download(db: &DatabaseConnection, download_id: i64) {
    let stmt = Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        "DELETE FROM download_segments WHERE download_id = ?",
        [download_id.into()],
    );
    if let Err(e) = db.execute(stmt).await {
        tracing::warn!(
            download_id,
            error = %e,
            "progress_bridge: failed to clear segments before mirror retry"
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
    use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};

    use crate::adapters::driven::sqlite::connection::setup_test_db;
    use crate::adapters::driven::sqlite::entities::{download, download_segment};

    #[test]
    fn test_segment_row_id_is_deterministic() {
        assert_eq!(segment_row_id(42, 0), Some(4200));
        assert_eq!(segment_row_id(42, 3), Some(4203));
    }

    #[test]
    fn test_segment_row_id_no_overflow_for_snowflake_id() {
        // Snowflake IDs are ~(ts_ms << 12) ≈ 6.97 × 10^15 at current time.
        // typical_id = 1_744_000_000_000 * 4_096 = 7_143_424_000_000_000
        let typical_id: u64 = 1_744_000_000_000 << 12;
        let row_id = segment_row_id(typical_id, 15).expect("segment_index < 100");
        // 7_143_424_000_000_000 * 100 + 15 = 714_342_400_000_000_015
        // i64::MAX = 9_223_372_036_854_775_807  →  well within range
        assert_eq!(
            row_id, 714_342_400_000_000_015_i64,
            "row_id must be deterministic and not overflow"
        );
    }

    #[test]
    fn test_segment_row_id_rejects_large_segment_index() {
        assert_eq!(segment_row_id(42, 100), None);
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
            queue_position: Set(0),
            total_bytes: Set(total_bytes),
            downloaded_bytes: Set(downloaded_bytes),
            speed_bytes_per_sec: Set(0),
            retry_count: Set(0),
            max_retries: Set(5),
            segments_count: Set(1),
            checksum_expected: Set(None),
            checksum_computed: Set(None),
            checksum_algorithm: Set(None),
            source_hostname: Set("example.test".to_string()),
            protocol: Set("https".to_string()),
            resume_supported: Set(1),
            module_name: Set(None),
            account_id: Set(None),
            destination_path: Set("/tmp/file.bin".to_string()),
            error_message: Set(None),
            mirrors_json: Set(None),
            current_mirror_index: Set(0),
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

        let row_id = segment_row_id(7, 2).expect("segment_index < 100");
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

        let row_id = segment_row_id(9, 0).expect("segment_index < 100");
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

        let row_id = segment_row_id(11, 0).expect("segment_index < 100");
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

    #[tokio::test]
    async fn test_update_progress_also_sets_total_bytes_when_known() {
        let db = setup_test_db().await.unwrap();
        // Start with total_bytes = None (unknown)
        insert_download_row(&db, 42, None, 0).await;

        update_download_progress(&db, 42, 5_000_000, 10_000_000).await;

        let row = download::Entity::find_by_id(42)
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(row.downloaded_bytes, 5_000_000);
        assert_eq!(
            row.total_bytes,
            Some(10_000_000),
            "total_bytes must be set from the first DownloadProgress event that carries it"
        );
    }

    #[tokio::test]
    async fn test_update_progress_does_not_overwrite_existing_total_bytes() {
        let db = setup_test_db().await.unwrap();
        insert_download_row(&db, 43, Some(10_000_000), 0).await;

        // Later progress event with a slightly different total — the original value wins
        update_download_progress(&db, 43, 5_000_000, 9_999_999).await;

        let row = download::Entity::find_by_id(43)
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            row.total_bytes,
            Some(10_000_000),
            "existing non-zero total_bytes must not be overwritten"
        );
    }

    #[tokio::test]
    async fn test_update_mirror_cursor_persists_new_index() {
        let db = setup_test_db().await.unwrap();
        insert_download_row(&db, 77, None, 0).await;

        update_mirror_cursor(&db, 77, 2).await;

        let row = download::Entity::find_by_id(77)
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            row.current_mirror_index, 2,
            "cursor must be written through to the row so the read model surfaces the live mirror"
        );
    }

    #[test]
    fn test_event_to_message_maps_mirror_switched() {
        use crate::domain::model::download::DownloadId;

        let event = DomainEvent::MirrorSwitched {
            id: DownloadId(7),
            new_mirror_index: 3,
            new_url: "https://m4.example.com/file".to_string(),
        };
        match event_to_message(&event) {
            Some(BridgeMessage::MirrorSwitched {
                download_id,
                new_mirror_index,
            }) => {
                assert_eq!(download_id, 7);
                assert_eq!(new_mirror_index, 3);
            }
            other => panic!("expected MirrorSwitched message, got {other:?}"),
        }
    }

    #[test]
    fn test_event_to_message_maps_download_failed_to_cursor_reset() {
        use crate::domain::model::download::DownloadId;

        let event = DomainEvent::DownloadFailed {
            id: DownloadId(11),
            error: "all mirrors exhausted".to_string(),
        };
        match event_to_message(&event) {
            Some(BridgeMessage::MirrorCursorReset { download_id }) => {
                assert_eq!(download_id, 11);
            }
            other => panic!("expected MirrorCursorReset message, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_download_failed_resets_persisted_mirror_cursor_to_zero() {
        let db = setup_test_db().await.unwrap();
        insert_download_row(&db, 88, None, 0).await;
        update_mirror_cursor(&db, 88, 4).await;

        // Reset path: every DownloadFailed event must zero the cursor so the
        // next retry walks the mirror list from the top.
        update_mirror_cursor(&db, 88, 0).await;

        let row = download::Entity::find_by_id(88)
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            row.current_mirror_index, 0,
            "DownloadFailed must reset the persisted mirror cursor so retries start fresh"
        );
    }

    #[tokio::test]
    async fn test_clear_segments_for_download_drops_all_rows_for_id() {
        let db = setup_test_db().await.unwrap();
        insert_download_row(&db, 99, Some(4096), 0).await;

        let row_a = segment_row_id(99, 0).expect("segment_index < 100");
        let row_b = segment_row_id(99, 1).expect("segment_index < 100");
        insert_segment(&db, row_a, 99, 0, 0, 2048).await;
        insert_segment(&db, row_b, 99, 1, 2048, 4096).await;

        // Different download_id — must survive.
        insert_download_row(&db, 100, Some(4096), 0).await;
        let row_c = segment_row_id(100, 0).expect("segment_index < 100");
        insert_segment(&db, row_c, 100, 0, 0, 4096).await;

        clear_segments_for_download(&db, 99).await;

        let remaining_for_99 = download_segment::Entity::find()
            .filter(download_segment::Column::DownloadId.eq(99_i64))
            .all(&db)
            .await
            .unwrap();
        assert!(
            remaining_for_99.is_empty(),
            "MirrorSwitched must drop every segment row for the switching download"
        );

        let remaining_for_100 = download_segment::Entity::find()
            .filter(download_segment::Column::DownloadId.eq(100_i64))
            .all(&db)
            .await
            .unwrap();
        assert_eq!(
            remaining_for_100.len(),
            1,
            "segments belonging to other downloads must not be touched"
        );
    }
}
