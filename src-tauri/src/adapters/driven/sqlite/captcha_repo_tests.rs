use sea_orm::{ConnectionTrait, Statement};

use super::captcha_repo::SqliteCaptchaRepo;
use super::connection::setup_test_db;
use crate::domain::model::captcha::{CaptchaChallenge, CaptchaId, CaptchaStatus, CaptchaType};
use crate::domain::model::download::DownloadId;
use crate::domain::ports::driven::CaptchaRepository;

fn challenge(id: &str, download_id: u64) -> CaptchaChallenge {
    CaptchaChallenge::new(
        CaptchaId::new(id),
        DownloadId(download_id),
        CaptchaType::Image,
        format!("https://hoster.example/{download_id}"),
        1_000,
        61_000,
    )
    .expect("valid challenge")
    .with_image_data(vec![137, 80, 78, 71])
    .expect("bounded image")
}

#[tokio::test(flavor = "multi_thread")]
async fn captcha_log_round_trip_and_pending_query() {
    let db = setup_test_db().await.expect("test db");
    let repo = SqliteCaptchaRepo::new(db);
    let mut item = challenge("captcha-1", 42);

    repo.save(&item).expect("save pending");
    let pending = repo.list_pending().expect("list pending");
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].image_data(), Some([137, 80, 78, 71].as_slice()));

    item.solve(4_000, "manual").expect("solve");
    repo.save(&item).expect("save solved");
    assert!(repo.list_pending().expect("list pending").is_empty());

    let stored = repo
        .find_by_id(item.id())
        .expect("find")
        .expect("stored challenge");
    assert_eq!(stored.status(), CaptchaStatus::Solved);
    assert_eq!(stored.solver(), Some("manual"));
    assert_eq!(stored.duration_ms(), Some(3_000));
}

#[tokio::test]
async fn captcha_log_never_has_a_solution_column() {
    let db = setup_test_db().await.expect("test db");
    let columns = db
        .query_all(Statement::from_string(
            sea_orm::DatabaseBackend::Sqlite,
            "PRAGMA table_info(captcha_log)".to_string(),
        ))
        .await
        .expect("table info");
    let names: Vec<String> = columns
        .iter()
        .map(|row| row.try_get_by_index::<String>(1).expect("column name"))
        .collect();

    assert!(!names.iter().any(|name| name.contains("solution")));
}
