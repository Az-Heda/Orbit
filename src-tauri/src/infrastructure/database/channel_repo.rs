use async_trait::async_trait;
use sqlx::SqlitePool;

use super::activity_repo::{log_change, record};
use super::now_utc;
use crate::domain::activity::{Actor, NewActivity};
use crate::domain::channels::{CHANNEL_COLS, ChannelRow};
use crate::error::{DomainError, DomainResult};

pub struct ChannelWrite<'a> {
    pub name: &'a str,
    pub kind: &'a str,
    /// Non-secret settings (JSON).
    pub config: &'a str,
    pub template: &'a str,
}

#[async_trait]
pub trait ChannelRepository: Send + Sync {
    async fn create(&self, w: &ChannelWrite<'_>, actor: Actor) -> DomainResult<ChannelRow>;
    async fn get(&self, id: &str) -> DomainResult<Option<ChannelRow>>;
    async fn find_by_name(&self, name: &str) -> DomainResult<Option<ChannelRow>>;
    async fn update(
        &self,
        id: &str,
        w: &ChannelWrite<'_>,
        actor: Actor,
    ) -> DomainResult<ChannelRow>;
    async fn delete(&self, id: &str, actor: Actor) -> DomainResult<()>;
    async fn list(&self) -> DomainResult<Vec<ChannelRow>>;
    /// Record a delivery event (`delivery_failed`, `rate_limited`) in `activity_log`.
    async fn log_event(
        &self,
        id: &str,
        action: &'static str,
        actor: Actor,
        detail: serde_json::Value,
    ) -> DomainResult<()>;
}

pub struct SqliteChannelRepository {
    pool: SqlitePool,
}

impl SqliteChannelRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

fn row_json(r: &ChannelRow) -> serde_json::Value {
    // No secrets exist in the row; the credential store holds them.
    serde_json::json!({
        "id": r.id, "name": r.name, "kind": r.kind,
        "config": serde_json::from_str::<serde_json::Value>(&r.config).ok(),
        "template": r.template,
    })
}

fn map_unique(e: sqlx::Error) -> DomainError {
    if e.to_string().contains("UNIQUE") {
        DomainError::Validation("a channel with that name already exists".into())
    } else {
        e.into()
    }
}

#[async_trait]
impl ChannelRepository for SqliteChannelRepository {
    async fn create(&self, w: &ChannelWrite<'_>, actor: Actor) -> DomainResult<ChannelRow> {
        let mut tx = self.pool.begin().await?;
        let now = now_utc();
        let row: ChannelRow = sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "INSERT INTO notification_channels (id, name, kind, config, template, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?) RETURNING {CHANNEL_COLS}"
        )))
        .bind(uuid::Uuid::new_v4().to_string())
        .bind(w.name)
        .bind(w.kind)
        .bind(w.config)
        .bind(w.template)
        .bind(&now)
        .bind(&now)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_unique)?;
        log_change::<(), _>(
            &mut tx,
            "notification_channel",
            &row.id,
            "create",
            actor,
            None,
            Some(&row_json(&row)),
            None,
        )
        .await?;
        tx.commit().await?;
        Ok(row)
    }

    async fn get(&self, id: &str) -> DomainResult<Option<ChannelRow>> {
        Ok(sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "SELECT {CHANNEL_COLS} FROM notification_channels WHERE id = ? AND deleted_at IS NULL"
        )))
        .bind(id)
        .fetch_optional(&self.pool)
        .await?)
    }

    async fn find_by_name(&self, name: &str) -> DomainResult<Option<ChannelRow>> {
        Ok(sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "SELECT {CHANNEL_COLS} FROM notification_channels
             WHERE lower(name) = lower(?) AND deleted_at IS NULL"
        )))
        .bind(name.trim())
        .fetch_optional(&self.pool)
        .await?)
    }

    async fn update(
        &self,
        id: &str,
        w: &ChannelWrite<'_>,
        actor: Actor,
    ) -> DomainResult<ChannelRow> {
        let mut tx = self.pool.begin().await?;
        let before: ChannelRow = sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "SELECT {CHANNEL_COLS} FROM notification_channels WHERE id = ? AND deleted_at IS NULL"
        )))
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(DomainError::NotFound {
            entity: "notification_channel",
            id: id.to_string(),
        })?;
        let after: ChannelRow = sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "UPDATE notification_channels SET name = ?, kind = ?, config = ?, template = ?, updated_at = ?
             WHERE id = ? RETURNING {CHANNEL_COLS}"
        )))
        .bind(w.name)
        .bind(w.kind)
        .bind(w.config)
        .bind(w.template)
        .bind(now_utc())
        .bind(id)
        .fetch_one(&mut *tx)
        .await
        .map_err(map_unique)?;
        log_change(
            &mut tx,
            "notification_channel",
            id,
            "update",
            actor,
            Some(&row_json(&before)),
            Some(&row_json(&after)),
            None,
        )
        .await?;
        tx.commit().await?;
        Ok(after)
    }

    async fn delete(&self, id: &str, actor: Actor) -> DomainResult<()> {
        let mut tx = self.pool.begin().await?;
        let before: ChannelRow = sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "SELECT {CHANNEL_COLS} FROM notification_channels WHERE id = ? AND deleted_at IS NULL"
        )))
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(DomainError::NotFound {
            entity: "notification_channel",
            id: id.to_string(),
        })?;
        let now = now_utc();
        sqlx::query("UPDATE notification_channels SET deleted_at = ?, updated_at = ? WHERE id = ?")
            .bind(&now)
            .bind(&now)
            .bind(id)
            .execute(&mut *tx)
            .await?;
        log_change::<_, ()>(
            &mut tx,
            "notification_channel",
            id,
            "delete",
            actor,
            Some(&row_json(&before)),
            None,
            None,
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }

    async fn list(&self) -> DomainResult<Vec<ChannelRow>> {
        Ok(sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "SELECT {CHANNEL_COLS} FROM notification_channels
             WHERE deleted_at IS NULL ORDER BY lower(name), rowid"
        )))
        .fetch_all(&self.pool)
        .await?)
    }

    async fn log_event(
        &self,
        id: &str,
        action: &'static str,
        actor: Actor,
        detail: serde_json::Value,
    ) -> DomainResult<()> {
        let mut conn = self.pool.acquire().await?;
        record(
            &mut conn,
            &NewActivity {
                entity_type: "notification_channel",
                entity_id: id.to_string(),
                action,
                actor,
                before_json: None,
                after_json: Some(detail.to_string()),
                batch_id: None,
            },
        )
        .await
    }
}
