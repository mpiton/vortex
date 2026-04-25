use std::collections::HashMap;

use sea_orm::{
    ActiveValue::Set, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter,
    sea_query::OnConflict,
};

use crate::domain::error::DomainError;
use crate::domain::ports::driven::plugin_config_store::PluginConfigStore;

use super::entities::plugin_config;
use super::util::{block_on, map_db_err};

pub struct SqlitePluginConfigRepo {
    db: DatabaseConnection,
}

impl SqlitePluginConfigRepo {
    pub fn new(db: DatabaseConnection) -> Self {
        Self { db }
    }
}

impl PluginConfigStore for SqlitePluginConfigRepo {
    fn get_values(&self, plugin_name: &str) -> Result<HashMap<String, String>, DomainError> {
        let db = self.db.clone();
        let plugin = plugin_name.to_string();
        block_on(async move {
            let rows = plugin_config::Entity::find()
                .filter(plugin_config::Column::PluginName.eq(plugin))
                .all(&db)
                .await
                .map_err(map_db_err)?;
            Ok(rows.into_iter().map(|m| (m.key, m.value)).collect())
        })
    }

    fn set_value(&self, plugin_name: &str, key: &str, value: &str) -> Result<(), DomainError> {
        let db = self.db.clone();
        let model = plugin_config::ActiveModel {
            plugin_name: Set(plugin_name.to_string()),
            key: Set(key.to_string()),
            value: Set(value.to_string()),
        };
        block_on(async move {
            plugin_config::Entity::insert(model)
                .on_conflict(
                    OnConflict::columns([
                        plugin_config::Column::PluginName,
                        plugin_config::Column::Key,
                    ])
                    .update_column(plugin_config::Column::Value)
                    .to_owned(),
                )
                .exec(&db)
                .await
                .map_err(map_db_err)?;
            Ok(())
        })
    }

    fn list_all(&self) -> Result<HashMap<String, HashMap<String, String>>, DomainError> {
        let db = self.db.clone();
        block_on(async move {
            let rows = plugin_config::Entity::find()
                .all(&db)
                .await
                .map_err(map_db_err)?;
            let mut out: HashMap<String, HashMap<String, String>> = HashMap::new();
            for row in rows {
                out.entry(row.plugin_name)
                    .or_default()
                    .insert(row.key, row.value);
            }
            Ok(out)
        })
    }

    fn delete_all(&self, plugin_name: &str) -> Result<(), DomainError> {
        let db = self.db.clone();
        let plugin = plugin_name.to_string();
        block_on(async move {
            plugin_config::Entity::delete_many()
                .filter(plugin_config::Column::PluginName.eq(plugin))
                .exec(&db)
                .await
                .map_err(map_db_err)?;
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::driven::sqlite::connection::setup_test_db;

    async fn make_repo() -> SqlitePluginConfigRepo {
        let db = setup_test_db().await.unwrap();
        SqlitePluginConfigRepo::new(db)
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_get_values_returns_empty_map_when_none_persisted() {
        let repo = make_repo().await;
        let values = repo.get_values("ghost").unwrap();
        assert!(values.is_empty());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_set_then_get_round_trip() {
        let repo = make_repo().await;
        repo.set_value("youtube", "default_quality", "1080p")
            .unwrap();
        let values = repo.get_values("youtube").unwrap();
        assert_eq!(values.get("default_quality"), Some(&"1080p".to_string()));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_set_value_upserts_existing_key() {
        let repo = make_repo().await;
        repo.set_value("youtube", "default_quality", "720p")
            .unwrap();
        repo.set_value("youtube", "default_quality", "1080p")
            .unwrap();
        let values = repo.get_values("youtube").unwrap();
        assert_eq!(values.len(), 1);
        assert_eq!(values.get("default_quality"), Some(&"1080p".to_string()));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_set_value_isolates_plugins() {
        let repo = make_repo().await;
        repo.set_value("youtube", "k", "yt").unwrap();
        repo.set_value("vimeo", "k", "vm").unwrap();
        assert_eq!(
            repo.get_values("youtube").unwrap().get("k"),
            Some(&"yt".to_string())
        );
        assert_eq!(
            repo.get_values("vimeo").unwrap().get("k"),
            Some(&"vm".to_string())
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_list_all_groups_by_plugin() {
        let repo = make_repo().await;
        repo.set_value("youtube", "a", "1").unwrap();
        repo.set_value("youtube", "b", "2").unwrap();
        repo.set_value("vimeo", "x", "9").unwrap();

        let all = repo.list_all().unwrap();
        assert_eq!(all.len(), 2);
        let yt = all.get("youtube").unwrap();
        assert_eq!(yt.get("a"), Some(&"1".to_string()));
        assert_eq!(yt.get("b"), Some(&"2".to_string()));
        assert_eq!(all.get("vimeo").unwrap().get("x"), Some(&"9".to_string()));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_delete_all_removes_only_target_plugin() {
        let repo = make_repo().await;
        repo.set_value("youtube", "k", "v").unwrap();
        repo.set_value("vimeo", "k", "v").unwrap();
        repo.delete_all("youtube").unwrap();
        assert!(repo.get_values("youtube").unwrap().is_empty());
        assert_eq!(
            repo.get_values("vimeo").unwrap().get("k"),
            Some(&"v".to_string())
        );
    }
}
