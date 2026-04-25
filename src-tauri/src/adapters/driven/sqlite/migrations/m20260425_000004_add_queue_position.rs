use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Downloads::Table)
                    .add_column(
                        ColumnDef::new(Downloads::QueuePosition)
                            .big_integer()
                            .not_null()
                            .default(0),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_downloads_queue_position")
                    .table(Downloads::Table)
                    .col(Downloads::QueuePosition)
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(
                Index::drop()
                    .name("idx_downloads_queue_position")
                    .table(Downloads::Table)
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(Downloads::Table)
                    .drop_column(Downloads::QueuePosition)
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum Downloads {
    Table,
    QueuePosition,
}
