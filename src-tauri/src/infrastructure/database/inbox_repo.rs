use async_trait::async_trait;
use sqlx::{SqliteConnection, SqlitePool};

use super::activity_repo::log_change;
use super::now_utc;
use crate::domain::activity::Actor;
use crate::domain::inbox::{INBOX_COLS, InboxItem};
use crate::error::{DomainError, DomainResult};

#[async_trait]
pub trait InboxRepository: Send + Sync {
    async fn capture(&self, raw_text: &str, actor: Actor) -> DomainResult<InboxItem>;
    async fn get(&self, id: &str) -> DomainResult<Option<InboxItem>>;
    /// `status` = None lists every non-deleted item. Newest first.
    async fn list(&self, status: Option<&str>, limit: i64) -> DomainResult<Vec<InboxItem>>;
    async fn count_new(&self) -> DomainResult<i64>;
    /// new -> triaged, linking the resulting entity. Fails if the item is not "new".
    async fn mark_triaged(
        &self,
        id: &str,
        entity_type: &str,
        entity_id: &str,
        actor: Actor,
    ) -> DomainResult<InboxItem>;
    async fn dismiss(&self, id: &str, actor: Actor) -> DomainResult<InboxItem>;
}

pub struct SqliteInboxRepository {
    pool: SqlitePool,
}

impl SqliteInboxRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

async fn fetch(conn: &mut SqliteConnection, id: &str) -> DomainResult<Option<InboxItem>> {
    Ok(sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {INBOX_COLS} FROM inbox_items WHERE id = ? AND deleted_at IS NULL"
    )))
    .bind(id)
    .fetch_optional(conn)
    .await?)
}

async fn transition(
    pool: &SqlitePool,
    id: &str,
    status: &str,
    link: Option<(&str, &str)>,
    actor: Actor,
) -> DomainResult<InboxItem> {
    let mut tx = pool.begin().await?;
    let before = fetch(&mut tx, id).await?.ok_or(DomainError::NotFound {
        entity: "inbox_item",
        id: id.to_string(),
    })?;
    let after: Option<InboxItem> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "UPDATE inbox_items SET status = ?, linked_entity_type = ?, linked_entity_id = ?, updated_at = ?
         WHERE id = ? AND status = 'new' RETURNING {INBOX_COLS}"
    )))
    .bind(status)
    .bind(link.map(|l| l.0))
    .bind(link.map(|l| l.1))
    .bind(now_utc())
    .bind(id)
    .fetch_optional(&mut *tx)
    .await?;
    let after =
        after.ok_or_else(|| DomainError::Validation("inbox item was already handled".into()))?;
    log_change(
        &mut tx,
        "inbox_item",
        id,
        "update",
        actor,
        Some(&before),
        Some(&after),
        None,
    )
    .await?;
    tx.commit().await?;
    Ok(after)
}

#[async_trait]
impl InboxRepository for SqliteInboxRepository {
    async fn capture(&self, raw_text: &str, actor: Actor) -> DomainResult<InboxItem> {
        let mut tx = self.pool.begin().await?;
        let now = now_utc();
        let item: InboxItem = sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "INSERT INTO inbox_items (id, raw_text, created_at, updated_at) VALUES (?, ?, ?, ?) RETURNING {INBOX_COLS}"
        )))
        .bind(uuid::Uuid::new_v4().to_string())
        .bind(raw_text)
        .bind(&now)
        .bind(&now)
        .fetch_one(&mut *tx)
        .await?;
        log_change::<(), _>(
            &mut tx,
            "inbox_item",
            &item.id,
            "create",
            actor,
            None,
            Some(&item),
            None,
        )
        .await?;
        tx.commit().await?;
        Ok(item)
    }

    async fn get(&self, id: &str) -> DomainResult<Option<InboxItem>> {
        let mut c = self.pool.acquire().await?;
        fetch(&mut c, id).await
    }

    async fn list(&self, status: Option<&str>, limit: i64) -> DomainResult<Vec<InboxItem>> {
        let mut sql = format!("SELECT {INBOX_COLS} FROM inbox_items WHERE deleted_at IS NULL");
        if status.is_some() {
            sql.push_str(" AND status = ?");
        }
        sql.push_str(" ORDER BY created_at DESC, rowid DESC LIMIT ?");
        let mut q = sqlx::query_as::<_, InboxItem>(sqlx::AssertSqlSafe(sql));
        if let Some(s) = status {
            q = q.bind(s.to_string());
        }
        Ok(q.bind(limit).fetch_all(&self.pool).await?)
    }

    async fn count_new(&self) -> DomainResult<i64> {
        let r: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM inbox_items WHERE status = 'new' AND deleted_at IS NULL",
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(r.0)
    }

    async fn mark_triaged(
        &self,
        id: &str,
        entity_type: &str,
        entity_id: &str,
        actor: Actor,
    ) -> DomainResult<InboxItem> {
        transition(
            &self.pool,
            id,
            "triaged",
            Some((entity_type, entity_id)),
            actor,
        )
        .await
    }

    async fn dismiss(&self, id: &str, actor: Actor) -> DomainResult<InboxItem> {
        transition(&self.pool, id, "dismissed", None, actor).await
    }
}
