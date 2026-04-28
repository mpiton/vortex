use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Accounts::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(Accounts::Id).text().not_null().primary_key())
                    .col(ColumnDef::new(Accounts::ServiceName).text().not_null())
                    .col(ColumnDef::new(Accounts::Username).text().not_null())
                    .col(ColumnDef::new(Accounts::AccountType).text().not_null())
                    .col(
                        ColumnDef::new(Accounts::Enabled)
                            .integer()
                            .not_null()
                            .default(1),
                    )
                    .col(ColumnDef::new(Accounts::TrafficLeft).big_integer().null())
                    .col(ColumnDef::new(Accounts::TrafficTotal).big_integer().null())
                    .col(ColumnDef::new(Accounts::ValidUntil).big_integer().null())
                    .col(ColumnDef::new(Accounts::LastValidated).big_integer().null())
                    .col(ColumnDef::new(Accounts::CreatedAt).big_integer().not_null())
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_accounts_service_username")
                    .table(Accounts::Table)
                    .col(Accounts::ServiceName)
                    .col(Accounts::Username)
                    .unique()
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_accounts_service")
                    .table(Accounts::Table)
                    .col(Accounts::ServiceName)
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Accounts::Table).if_exists().to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum Accounts {
    Table,
    Id,
    ServiceName,
    Username,
    AccountType,
    Enabled,
    TrafficLeft,
    TrafficTotal,
    ValidUntil,
    LastValidated,
    CreatedAt,
}
