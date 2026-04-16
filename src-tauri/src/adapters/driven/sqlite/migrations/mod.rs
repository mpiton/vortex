use sea_orm_migration::prelude::*;

mod m20260407_000001_create_tables;
mod m20260415_000002_add_download_error_message;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20260407_000001_create_tables::Migration),
            Box::new(m20260415_000002_add_download_error_message::Migration),
        ]
    }
}
