use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "download_segments")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: i64,
    pub download_id: i64,
    pub segment_index: i32,
    pub start_byte: i64,
    pub end_byte: i64,
    pub downloaded_bytes: i64,
    pub state: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::download::Entity",
        from = "Column::DownloadId",
        to = "super::download::Column::Id",
        on_delete = "Cascade"
    )]
    Download,
}

impl Related<super::download::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Download.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
