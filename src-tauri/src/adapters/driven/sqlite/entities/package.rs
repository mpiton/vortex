use sea_orm::entity::prelude::*;

use crate::domain::error::DomainError;
use crate::domain::model::package::{Package, PackageId, PackageSourceType};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "packages")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    pub name: String,
    pub source_type: String,
    pub folder_path: Option<String>,
    pub password: Option<String>,
    pub auto_extract: i32,
    pub priority: i32,
    pub created_at: i64,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

impl Model {
    pub fn into_domain(self) -> Result<Package, DomainError> {
        let source_type: PackageSourceType = self.source_type.parse()?;
        let auto_extract = match self.auto_extract {
            0 => false,
            1 => true,
            other => {
                return Err(DomainError::ValidationError(format!(
                    "package {}: auto_extract {other} out of bool range",
                    self.id
                )));
            }
        };
        let priority = u8::try_from(self.priority).map_err(|_| {
            DomainError::ValidationError(format!(
                "package {}: priority {} out of u8 range",
                self.id, self.priority
            ))
        })?;
        let created_at = u64::try_from(self.created_at).map_err(|_| {
            DomainError::ValidationError(format!(
                "package {}: created_at {} out of u64 range",
                self.id, self.created_at
            ))
        })?;
        Package::reconstruct(
            PackageId::new(self.id),
            self.name,
            source_type,
            self.folder_path,
            self.password,
            auto_extract,
            priority,
            created_at,
        )
    }
}

impl ActiveModel {
    pub fn from_domain(package: &Package) -> Result<Self, DomainError> {
        use sea_orm::ActiveValue::Set;

        let id_str = package.id().as_str().to_string();
        let created_at = i64::try_from(package.created_at()).map_err(|_| {
            DomainError::ValidationError(format!("package {id_str}: created_at exceeds i64::MAX"))
        })?;

        Ok(Self {
            id: Set(id_str),
            name: Set(package.name().to_string()),
            source_type: Set(package.source_type().to_string()),
            folder_path: Set(package.folder_path().map(str::to_string)),
            password: Set(package.password().map(str::to_string)),
            auto_extract: Set(if package.auto_extract() { 1 } else { 0 }),
            priority: Set(i32::from(package.priority())),
            created_at: Set(created_at),
        })
    }
}
