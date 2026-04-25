use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(PluginConfigs::Table)
                    .col(
                        ColumnDef::new(PluginConfigs::PluginName)
                            .string()
                            .not_null(),
                    )
                    .col(ColumnDef::new(PluginConfigs::Key).string().not_null())
                    .col(ColumnDef::new(PluginConfigs::Value).text().not_null())
                    .primary_key(
                        Index::create()
                            .col(PluginConfigs::PluginName)
                            .col(PluginConfigs::Key),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(
                Table::drop()
                    .table(PluginConfigs::Table)
                    .if_exists()
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum PluginConfigs {
    Table,
    PluginName,
    Key,
    Value,
}
