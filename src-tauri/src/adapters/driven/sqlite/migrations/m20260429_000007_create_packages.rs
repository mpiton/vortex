//! Recreate the `packages` table with the schema mandated by PRD-v2 §8 P1
//! and add the `downloads.package_id` foreign key column.
//!
//! The legacy `packages` table from migration 1 (BIGINT id, name only) was
//! never wired to any repository or query — it is dropped here without
//! data preservation. Going forward the package id is `TEXT` (caller-chosen
//! string, typically a UUID or slug) and the row carries the persistence
//! fields the future Package CRUD relies on (`source_type`, `folder_path`,
//! `password`, `auto_extract`, `priority`, `created_at`).
//!
//! `downloads.package_id` is added as a nullable `TEXT` foreign key with
//! `ON DELETE SET NULL` semantics: deleting a package detaches its members
//! but keeps every individual download row intact. We use raw `ALTER TABLE`
//! for that column because SQLite's column-level FK syntax is not exposed
//! through sea-orm's column builder.

use sea_orm::{ConnectionTrait, Statement};
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Drop the legacy stub table created by the very first migration.
        manager
            .drop_table(Table::drop().table(Packages::Table).if_exists().to_owned())
            .await?;

        manager
            .create_table(
                Table::create()
                    .table(Packages::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(Packages::Id).text().not_null().primary_key())
                    .col(ColumnDef::new(Packages::Name).text().not_null())
                    .col(ColumnDef::new(Packages::SourceType).text().not_null())
                    .col(ColumnDef::new(Packages::FolderPath).text().null())
                    .col(ColumnDef::new(Packages::Password).text().null())
                    .col(
                        ColumnDef::new(Packages::AutoExtract)
                            .integer()
                            .not_null()
                            .default(1),
                    )
                    .col(
                        ColumnDef::new(Packages::Priority)
                            .integer()
                            .not_null()
                            .default(5),
                    )
                    .col(ColumnDef::new(Packages::CreatedAt).big_integer().not_null())
                    .to_owned(),
            )
            .await?;

        // SQLite supports adding a column with a column-constraint FK in a
        // single `ALTER TABLE` statement; sea-orm's `add_column` builder does
        // not expose `REFERENCES ... ON DELETE`, so issue raw SQL instead.
        let conn = manager.get_connection();
        conn.execute(Statement::from_string(
            sea_orm::DatabaseBackend::Sqlite,
            "ALTER TABLE downloads ADD COLUMN package_id TEXT REFERENCES packages(id) ON DELETE SET NULL"
                .to_string(),
        ))
        .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_downloads_package")
                    .table(Downloads::Table)
                    .col(Downloads::PackageId)
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(
                Index::drop()
                    .name("idx_downloads_package")
                    .table(Downloads::Table)
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(Downloads::Table)
                    .drop_column(Downloads::PackageId)
                    .to_owned(),
            )
            .await?;

        manager
            .drop_table(Table::drop().table(Packages::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum Packages {
    Table,
    Id,
    Name,
    SourceType,
    FolderPath,
    Password,
    AutoExtract,
    Priority,
    CreatedAt,
}

#[derive(DeriveIden)]
enum Downloads {
    Table,
    PackageId,
}
