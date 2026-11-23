use async_trait::async_trait;
use sqlx::SqlitePool;

use super::activity_repo::log_change;
use super::now_utc;
use crate::domain::activity::Actor;
use crate::domain::kanban::TaskTemplate;
use crate::error::{DomainError, DomainResult};

const COLS: &str = "id, name, description, tree_json, created_at, updated_at";

#[async_trait]
pub trait TemplateRepository: Send + Sync {
    async fn create(
        &self,
        name: &str,
        description: Option<&str>,
        tree_json: &str,
        actor: Actor,
    ) -> DomainResult<TaskTemplate>;
    async fn get(&self, id: &str) -> DomainResult<Option<TaskTemplate>>;
    async fn list(&self) -> DomainResult<Vec<TaskTemplate>>;
    async fn delete(&self, id: &str, actor: Actor) -> DomainResult<()>;
}

pub struct SqliteTemplateRepository {
    pool: SqlitePool,
}

impl SqliteTemplateRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl TemplateRepository for SqliteTemplateRepository {
    async fn create(
        &self,
        name: &str,
        description: Option<&str>,
        tree_json: &str,
        actor: Actor,
    ) -> DomainResult<TaskTemplate> {
        let mut tx = self.pool.begin().await?;
        let now = now_utc();
        let t: TaskTemplate = sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "INSERT INTO task_templates (id, name, description, tree_json, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?) RETURNING {COLS}"
        )))
        .bind(uuid::Uuid::new_v4().to_string())
        .bind(name)
        .bind(description)
        .bind(tree_json)
        .bind(&now)
        .bind(&now)
        .fetch_one(&mut *tx)
        .await?;
        log_change::<(), _>(
            &mut tx,
            "task_template",
            &t.id,
            "create",
            actor,
            None,
            Some(&t),
            None,
        )
        .await?;
        tx.commit().await?;
        Ok(t)
    }

    async fn get(&self, id: &str) -> DomainResult<Option<TaskTemplate>> {
        Ok(sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "SELECT {COLS} FROM task_templates WHERE id = ? AND deleted_at IS NULL"
        )))
        .bind(id)
        .fetch_optional(&self.pool)
        .await?)
    }

    async fn list(&self) -> DomainResult<Vec<TaskTemplate>> {
        Ok(sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "SELECT {COLS} FROM task_templates WHERE deleted_at IS NULL ORDER BY name, rowid"
        )))
        .fetch_all(&self.pool)
        .await?)
    }

    async fn delete(&self, id: &str, actor: Actor) -> DomainResult<()> {
        let mut tx = self.pool.begin().await?;
        let before: TaskTemplate = sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "SELECT {COLS} FROM task_templates WHERE id = ? AND deleted_at IS NULL"
        )))
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| DomainError::NotFound {
            entity: "task_template",
            id: id.to_string(),
        })?;
        let now = now_utc();
        sqlx::query("UPDATE task_templates SET deleted_at = ?, updated_at = ? WHERE id = ?")
            .bind(&now)
            .bind(&now)
            .bind(id)
            .execute(&mut *tx)
            .await?;
        log_change::<_, ()>(
            &mut tx,
            "task_template",
            id,
            "delete",
            actor,
            Some(&before),
            None,
            None,
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }
}
