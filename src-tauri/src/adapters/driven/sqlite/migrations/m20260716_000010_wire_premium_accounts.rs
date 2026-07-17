use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Accounts::Table)
                    .add_column(
                        ColumnDef::new(Accounts::Status)
                            .text()
                            .not_null()
                            .default("unverified"),
                    )
                    .to_owned(),
            )
            .await?;
        manager
            .alter_table(
                Table::alter()
                    .table(Accounts::Table)
                    .add_column(ColumnDef::new(Accounts::CooldownUntil).big_integer().null())
                    .to_owned(),
            )
            .await?;
        manager
            .alter_table(
                Table::alter()
                    .table(Downloads::Table)
                    .add_column(ColumnDef::new(Downloads::AccountRef).text().null())
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_downloads_account_ref")
                    .table(Downloads::Table)
                    .col(Downloads::AccountRef)
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(
                Index::drop()
                    .name("idx_downloads_account_ref")
                    .table(Downloads::Table)
                    .to_owned(),
            )
            .await?;
        manager
            .alter_table(
                Table::alter()
                    .table(Downloads::Table)
                    .drop_column(Downloads::AccountRef)
                    .to_owned(),
            )
            .await?;
        manager
            .alter_table(
                Table::alter()
                    .table(Accounts::Table)
                    .drop_column(Accounts::CooldownUntil)
                    .to_owned(),
            )
            .await?;
        manager
            .alter_table(
                Table::alter()
                    .table(Accounts::Table)
                    .drop_column(Accounts::Status)
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum Accounts {
    Table,
    Status,
    CooldownUntil,
}

#[derive(DeriveIden)]
enum Downloads {
    Table,
    AccountRef,
}
