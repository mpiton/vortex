use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(CaptchaLog::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(CaptchaLog::Id)
                            .string()
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(CaptchaLog::DownloadId)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(CaptchaLog::ChallengeType)
                            .string()
                            .not_null(),
                    )
                    .col(ColumnDef::new(CaptchaLog::ChallengeUrl).string().not_null())
                    .col(ColumnDef::new(CaptchaLog::ImageData).binary().null())
                    .col(ColumnDef::new(CaptchaLog::Status).string().not_null())
                    .col(ColumnDef::new(CaptchaLog::Solver).string().null())
                    .col(ColumnDef::new(CaptchaLog::Attempts).integer().not_null())
                    .col(
                        ColumnDef::new(CaptchaLog::CreatedAt)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(CaptchaLog::ExpiresAt)
                            .big_integer()
                            .not_null(),
                    )
                    .col(ColumnDef::new(CaptchaLog::ResolvedAt).big_integer().null())
                    .col(ColumnDef::new(CaptchaLog::DurationMs).big_integer().null())
                    .col(ColumnDef::new(CaptchaLog::FailureReason).string().null())
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_captcha_log_status_created")
                    .table(CaptchaLog::Table)
                    .col(CaptchaLog::Status)
                    .col(CaptchaLog::CreatedAt)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_captcha_log_download_status")
                    .table(CaptchaLog::Table)
                    .col(CaptchaLog::DownloadId)
                    .col(CaptchaLog::Status)
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(CaptchaLog::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum CaptchaLog {
    Table,
    Id,
    DownloadId,
    ChallengeType,
    ChallengeUrl,
    ImageData,
    Status,
    Solver,
    Attempts,
    CreatedAt,
    ExpiresAt,
    ResolvedAt,
    DurationMs,
    FailureReason,
}
