use sea_orm_migration::prelude::*;

mod m20260407_000001_create_tables;
mod m20260415_000002_add_download_error_message;
mod m20260424_000003_add_checksum_columns;
mod m20260425_000004_add_queue_position;
mod m20260425_000005_create_plugin_configs;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20260407_000001_create_tables::Migration),
            Box::new(m20260415_000002_add_download_error_message::Migration),
            Box::new(m20260424_000003_add_checksum_columns::Migration),
            Box::new(m20260425_000004_add_queue_position::Migration),
            Box::new(m20260425_000005_create_plugin_configs::Migration),
        ]
    }
}
