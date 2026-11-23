use async_trait::async_trait;
use sqlx::{SqliteConnection, SqlitePool};

use super::now_utc;
use crate::domain::activity::{ActivityEntry, NewActivity};
use crate::error::DomainResult;

#[async_trait]
pub trait ActivityRepository: Send + Sync {
    async fn recent(&self, limit: i64) -> DomainResult<Vec<ActivityEntry>>;
    async fn for_entity(
        &self,
        entity_type: &str,
        entity_id: &str,
    ) -> DomainResult<Vec<ActivityEntry>>;
}

pub struct SqliteActivityRepository {
    pool: SqlitePool,
}

impl SqliteActivityRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

/// Insert an activity row on an existing connection/transaction. Other repositories call this
/// inside their mutation transaction so the log row commits atomically with the change.
pub(crate) async fn record(conn: &mut SqliteConnection, a: &NewActivity) -> DomainResult<()> {
    sqlx::query(
        "INSERT INTO activity_log (id, entity_type, entity_id, action, actor, before_json, after_json, batch_id, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(uuid::Uuid::new_v4().to_string())
    .bind(a.entity_type)
    .bind(&a.entity_id)
    .bind(a.action)
    .bind(a.actor.as_str())
    .bind(&a.before_json)
    .bind(&a.after_json)
    // An enclosing BATCH scope (one confirmed AI proposal) wins over a repo-local batch id so the
    // whole proposal can be undone as one unit.
    .bind(
        crate::domain::activity::BATCH
            .try_with(|b| b.clone())
            .ok()
            .or_else(|| a.batch_id.clone()),
    )
    .bind(now_utc())
    .execute(conn)
    .await?;
    Ok(())
}

#[async_trait]
impl ActivityRepository for SqliteActivityRepository {
    async fn recent(&self, limit: i64) -> DomainResult<Vec<ActivityEntry>> {
        Ok(sqlx::query_as::<_, ActivityEntry>(
            "SELECT * FROM activity_log ORDER BY created_at DESC, rowid DESC LIMIT ?",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?)
    }

    async fn for_entity(
        &self,
        entity_type: &str,
        entity_id: &str,
    ) -> DomainResult<Vec<ActivityEntry>> {
        Ok(sqlx::query_as::<_, ActivityEntry>(
            "SELECT * FROM activity_log WHERE entity_type = ? AND entity_id = ? ORDER BY created_at DESC, rowid DESC",
        )
        .bind(entity_type)
        .bind(entity_id)
        .fetch_all(&self.pool)
        .await?)
    }
}

/// Log a create/update/delete of a serialisable entity inside the caller's transaction.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn log_change<B: serde::Serialize, A: serde::Serialize>(
    conn: &mut SqliteConnection,
    entity_type: &'static str,
    entity_id: &str,
    action: &'static str,
    actor: crate::domain::activity::Actor,
    before: Option<&B>,
    after: Option<&A>,
    batch_id: Option<&str>,
) -> DomainResult<()> {
    let to_json = |v: Result<String, serde_json::Error>| {
        v.map_err(|e| crate::error::DomainError::Storage(e.to_string()))
    };
    record(
        conn,
        &NewActivity {
            entity_type,
            entity_id: entity_id.to_string(),
            action,
            actor,
            before_json: before
                .map(|b| to_json(serde_json::to_string(b)))
                .transpose()?,
            after_json: after
                .map(|a| to_json(serde_json::to_string(a)))
                .transpose()?,
            batch_id: batch_id.map(str::to_string),
        },
    )
    .await
}
