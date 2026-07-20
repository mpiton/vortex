use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(CaptchaLog::Table)
                    .add_column(
                        ColumnDef::new(CaptchaLog::SolverAttemptsJson)
                            .text()
                            .not_null()
                            .default("[]"),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(CaptchaLog::Table)
                    .drop_column(CaptchaLog::SolverAttemptsJson)
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum CaptchaLog {
    Table,
    SolverAttemptsJson,
}
