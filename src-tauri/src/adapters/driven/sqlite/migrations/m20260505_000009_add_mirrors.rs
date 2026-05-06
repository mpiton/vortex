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
                    .add_column(ColumnDef::new(Downloads::MirrorsJson).text().null())
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(Downloads::Table)
                    .add_column(
                        ColumnDef::new(Downloads::CurrentMirrorIndex)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Downloads::Table)
                    .drop_column(Downloads::CurrentMirrorIndex)
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(Downloads::Table)
                    .drop_column(Downloads::MirrorsJson)
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum Downloads {
    Table,
    MirrorsJson,
    CurrentMirrorIndex,
}
