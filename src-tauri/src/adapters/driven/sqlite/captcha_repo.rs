use sea_orm::DatabaseConnection;

pub struct SqliteCaptchaRepo {
    db: DatabaseConnection,
}

impl SqliteCaptchaRepo {
    pub fn new(db: DatabaseConnection) -> Self {
        Self { db }
    }
}
