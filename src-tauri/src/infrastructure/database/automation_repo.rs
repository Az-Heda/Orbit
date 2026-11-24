use async_trait::async_trait;
use sqlx::SqlitePool;

use super::activity_repo::{log_change, record};
use super::now_utc;
use crate::domain::activity::{Actor, NewActivity};
use crate::domain::automation::{RULE_COLS, RuleRow};
use crate::error::{DomainError, DomainResult};

pub struct RuleWrite<'a> {
    pub name: &'a str,
    pub enabled: bool,
    pub trigger_kind: &'a str,
    pub trigger_config: &'a str,
    /// JSON array of actions (the chain).
    pub actions: &'a str,
}

#[async_trait]
pub trait AutomationRepository: Send + Sync {
    async fn create(&self, r: &RuleWrite<'_>, actor: Actor) -> DomainResult<RuleRow>;
    async fn get(&self, id: &str) -> DomainResult<Option<RuleRow>>;
    async fn update(&self, id: &str, r: &RuleWrite<'_>, actor: Actor) -> DomainResult<RuleRow>;
    async fn delete(&self, id: &str, actor: Actor) -> DomainResult<()>;
    async fn list(&self, only_enabled: bool) -> DomainResult<Vec<RuleRow>>;
    /// Record that a step of a rule failed (the chain stopped there) in `activity_log`.
    async fn log_step_failure(
        &self,
        rule_id: &str,
        step: usize,
        action_kind: &str,
        error: &str,
    ) -> DomainResult<()>;
}

/// Idempotency keys and tiny markers (e.g. last scheduler tick) that must survive restarts.
#[async_trait]
pub trait FiredRepository: Send + Sync {
    /// Record `key`; returns true only the first time (so the caller fires exactly once).
    async fn try_fire(&self, key: &str, at: &str, missed: bool) -> DomainResult<bool>;
    async fn marker(&self, key: &str) -> DomainResult<Option<String>>;
    async fn set_marker(&self, key: &str, value: &str) -> DomainResult<()>;
    /// Delete keys fired before `before` (markers excluded).
    async fn prune(&self, before: &str) -> DomainResult<u64>;
}

pub struct SqliteAutomationRepository {
    pool: SqlitePool,
}

impl SqliteAutomationRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl AutomationRepository for SqliteAutomationRepository {
    async fn create(&self, r: &RuleWrite<'_>, actor: Actor) -> DomainResult<RuleRow> {
        let mut tx = self.pool.begin().await?;
        let now = now_utc();
        let row: RuleRow = sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "INSERT INTO automation_rules (id, name, enabled, trigger_kind, trigger_config, actions, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?) RETURNING {RULE_COLS}"
        )))
        .bind(uuid::Uuid::new_v4().to_string())
        .bind(r.name)
        .bind(r.enabled)
        .bind(r.trigger_kind)
        .bind(r.trigger_config)
        .bind(r.actions)
        .bind(&now)
        .bind(&now)
        .fetch_one(&mut *tx)
        .await?;
        log_change::<(), _>(
            &mut tx,
            "automation_rule",
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

    async fn get(&self, id: &str) -> DomainResult<Option<RuleRow>> {
        Ok(sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "SELECT {RULE_COLS} FROM automation_rules WHERE id = ? AND deleted_at IS NULL"
        )))
        .bind(id)
        .fetch_optional(&self.pool)
        .await?)
    }

    async fn update(&self, id: &str, r: &RuleWrite<'_>, actor: Actor) -> DomainResult<RuleRow> {
        let mut tx = self.pool.begin().await?;
        let before: RuleRow = sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "SELECT {RULE_COLS} FROM automation_rules WHERE id = ? AND deleted_at IS NULL"
        )))
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(DomainError::NotFound {
            entity: "automation_rule",
            id: id.to_string(),
        })?;
        let after: RuleRow = sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "UPDATE automation_rules SET name = ?, enabled = ?, trigger_kind = ?, trigger_config = ?,
             actions = ?, updated_at = ? WHERE id = ? RETURNING {RULE_COLS}"
        )))
        .bind(r.name)
        .bind(r.enabled)
        .bind(r.trigger_kind)
        .bind(r.trigger_config)
        .bind(r.actions)
        .bind(now_utc())
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
        log_change(
            &mut tx,
            "automation_rule",
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
        let before: RuleRow = sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "SELECT {RULE_COLS} FROM automation_rules WHERE id = ? AND deleted_at IS NULL"
        )))
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(DomainError::NotFound {
            entity: "automation_rule",
            id: id.to_string(),
        })?;
        let now = now_utc();
        sqlx::query("UPDATE automation_rules SET deleted_at = ?, updated_at = ? WHERE id = ?")
            .bind(&now)
            .bind(&now)
            .bind(id)
            .execute(&mut *tx)
            .await?;
        log_change::<_, ()>(
            &mut tx,
            "automation_rule",
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

    async fn list(&self, only_enabled: bool) -> DomainResult<Vec<RuleRow>> {
        let sql = format!(
            "SELECT {RULE_COLS} FROM automation_rules WHERE deleted_at IS NULL{} ORDER BY created_at, rowid",
            if only_enabled { " AND enabled = 1" } else { "" }
        );
        Ok(sqlx::query_as(sqlx::AssertSqlSafe(sql))
            .fetch_all(&self.pool)
            .await?)
    }

    async fn log_step_failure(
        &self,
        rule_id: &str,
        step: usize,
        action_kind: &str,
        error: &str,
    ) -> DomainResult<()> {
        let mut conn = self.pool.acquire().await?;
        record(
            &mut conn,
            &NewActivity {
                entity_type: "automation_rule",
                entity_id: rule_id.to_string(),
                action: "step_failed",
                actor: Actor::Automation,
                before_json: None,
                after_json: Some(
                    serde_json::json!({ "step": step + 1, "action": action_kind, "error": error })
                        .to_string(),
                ),
                batch_id: None,
            },
        )
        .await
    }
}

fn row_json(r: &RuleRow) -> serde_json::Value {
    serde_json::json!({
        "id": r.id, "name": r.name, "enabled": r.enabled,
        "trigger": serde_json::from_str::<serde_json::Value>(&r.trigger_config).ok(),
        "actions": serde_json::from_str::<serde_json::Value>(&r.actions).ok(),
    })
}

pub struct SqliteFiredRepository {
    pool: SqlitePool,
}

impl SqliteFiredRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl FiredRepository for SqliteFiredRepository {
    async fn try_fire(&self, key: &str, at: &str, missed: bool) -> DomainResult<bool> {
        let r = sqlx::query(
            "INSERT OR IGNORE INTO fired_keys (key, fired_at, missed) VALUES (?, ?, ?)",
        )
        .bind(key)
        .bind(at)
        .bind(missed)
        .execute(&self.pool)
        .await?;
        Ok(r.rows_affected() > 0)
    }

    async fn marker(&self, key: &str) -> DomainResult<Option<String>> {
        let r: Option<(String,)> = sqlx::query_as("SELECT fired_at FROM fired_keys WHERE key = ?")
            .bind(format!("marker:{key}"))
            .fetch_optional(&self.pool)
            .await?;
        Ok(r.map(|r| r.0))
    }

    async fn set_marker(&self, key: &str, value: &str) -> DomainResult<()> {
        sqlx::query(
            "INSERT INTO fired_keys (key, fired_at) VALUES (?, ?)
             ON CONFLICT(key) DO UPDATE SET fired_at = excluded.fired_at",
        )
        .bind(format!("marker:{key}"))
        .bind(value)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn prune(&self, before: &str) -> DomainResult<u64> {
        let r =
            sqlx::query("DELETE FROM fired_keys WHERE fired_at < ? AND key NOT LIKE 'marker:%'")
                .bind(before)
                .execute(&self.pool)
                .await?;
        Ok(r.rows_affected())
    }
}
