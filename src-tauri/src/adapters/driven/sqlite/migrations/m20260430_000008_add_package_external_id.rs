//! Add `external_id` column on `packages` for natural-key lookups.
//!
//! Auto-grouping (PRD-v2 §P1.11) needs an idempotent way to find the
//! package created from a given external source — a YouTube/SoundCloud
//! playlist, a container file hash, etc. — so that re-resolving the
//! same source does not produce duplicate packages. The column is
//! nullable: only auto-grouped sources fill it; manual packages stay
//! `NULL`.
//!
//! The index is `UNIQUE`. SQLite treats every `NULL` in a UNIQUE index
//! as distinct from every other value (including other `NULL`s), so
//! multiple manual packages with `NULL external_id` remain valid while
//! the one-package-per-`external_id` invariant is enforced at storage
//! level — robust against concurrent writes from multiple processes,
//! not just the in-process `PlaylistGrouper` mutex.

use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Packages::Table)
                    .add_column(ColumnDef::new(Packages::ExternalId).text().null())
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_packages_external_id")
                    .table(Packages::Table)
                    .col(Packages::ExternalId)
                    .unique()
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(
                Index::drop()
                    .name("idx_packages_external_id")
                    .table(Packages::Table)
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(Packages::Table)
                    .drop_column(Packages::ExternalId)
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum Packages {
    Table,
    ExternalId,
}
