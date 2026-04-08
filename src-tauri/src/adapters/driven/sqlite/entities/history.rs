use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "history")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub download_id: i64,
    pub file_name: String,
    pub url: String,
    pub total_bytes: i64,
    pub completed_at: i64,
    pub duration_seconds: i64,
    pub avg_speed: i64,
    pub destination_path: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
