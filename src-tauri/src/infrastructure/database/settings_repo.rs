use async_trait::async_trait;
use sqlx::SqlitePool;

use super::activity_repo::record;
use super::now_utc;
use crate::domain::activity::{Actor, NewActivity};
use crate::domain::settings::Setting;
use crate::error::DomainResult;

#[async_trait]
pub trait SettingsRepository: Send + Sync {
    async fn get(&self, key: &str) -> DomainResult<Option<Setting>>;
    async fn list(&self) -> DomainResult<Vec<Setting>>;
    /// Upsert a (pre-validated) setting and write its activity_log row in one transaction.
    async fn set(&self, key: &str, value: &str, actor: Actor) -> DomainResult<Setting>;
}

pub struct SqliteSettingsRepository {
    pool: SqlitePool,
}

impl SqliteSettingsRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl SettingsRepository for SqliteSettingsRepository {
    async fn get(&self, key: &str) -> DomainResult<Option<Setting>> {
        Ok(sqlx::query_as::<_, Setting>(
            "SELECT key, value, updated_at FROM settings WHERE key = ?",
        )
        .bind(key)
        .fetch_optional(&self.pool)
        .await?)
    }

    async fn list(&self) -> DomainResult<Vec<Setting>> {
        Ok(
            sqlx::query_as::<_, Setting>(
                "SELECT key, value, updated_at FROM settings ORDER BY key",
            )
            .fetch_all(&self.pool)
            .await?,
        )
    }

    async fn set(&self, key: &str, value: &str, actor: Actor) -> DomainResult<Setting> {
        let mut tx = self.pool.begin().await?;
        let before: Option<Setting> =
            sqlx::query_as("SELECT key, value, updated_at FROM settings WHERE key = ?")
                .bind(key)
                .fetch_optional(&mut *tx)
                .await?;
        let after: Setting = sqlx::query_as(
            "INSERT INTO settings (key, value, updated_at) VALUES (?, ?, ?)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at
             RETURNING key, value, updated_at",
        )
        .bind(key)
        .bind(value)
        .bind(now_utc())
        .fetch_one(&mut *tx)
        .await?;
        record(
            &mut tx,
            &NewActivity {
                entity_type: "setting",
                entity_id: key.to_string(),
                action: if before.is_some() { "update" } else { "create" },
                actor,
                before_json: before.map(|b| b.value),
                after_json: Some(after.value.clone()),
                batch_id: None,
            },
        )
        .await?;
        tx.commit().await?;
        Ok(after)
    }
}
