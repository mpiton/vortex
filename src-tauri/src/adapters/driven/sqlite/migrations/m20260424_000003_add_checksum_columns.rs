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
                    .add_column(ColumnDef::new(Downloads::ChecksumComputed).string().null())
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(Downloads::Table)
                    .add_column(ColumnDef::new(Downloads::ChecksumAlgorithm).string().null())
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Downloads::Table)
                    .drop_column(Downloads::ChecksumAlgorithm)
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(Downloads::Table)
                    .drop_column(Downloads::ChecksumComputed)
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum Downloads {
    Table,
    ChecksumComputed,
    ChecksumAlgorithm,
}
