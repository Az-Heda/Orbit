use async_trait::async_trait;
use serde::Serialize;
use serde_json::Value;
use sqlx::{SqliteConnection, SqlitePool};

use super::activity_repo::{log_change, record};
use super::now_utc;
use crate::domain::activity::{Actor, NewActivity};
use crate::domain::ai::{Conversation, Memory, MessageRow, Proposal, ProposalRow, StoredMessage};
use crate::error::{DomainError, DomainResult};

#[derive(Debug, Clone, Serialize, Default, PartialEq)]
pub struct UndoReport {
    pub reverted: usize,
}

#[async_trait]
pub trait AiRepository: Send + Sync {
    async fn create_conversation(&self, title: &str) -> DomainResult<Conversation>;
    async fn list_conversations(&self, limit: i64) -> DomainResult<Vec<Conversation>>;
    async fn get_conversation(&self, id: &str) -> DomainResult<Option<Conversation>>;
    async fn delete_conversation(&self, id: &str) -> DomainResult<()>;
    async fn append_message(
        &self,
        conversation_id: &str,
        role: &str,
        content_json: &str,
    ) -> DomainResult<StoredMessage>;
    async fn messages(&self, conversation_id: &str) -> DomainResult<Vec<StoredMessage>>;

    async fn create_proposal(
        &self,
        conversation_id: &str,
        tool: &str,
        input_json: &str,
        preview_json: &str,
    ) -> DomainResult<Proposal>;
    async fn get_proposal(&self, id: &str) -> DomainResult<Option<Proposal>>;
    async fn proposals(&self, conversation_id: &str) -> DomainResult<Vec<Proposal>>;
    /// Move a proposal between states only if it is currently in `from` (guards double-apply).
    async fn transition_proposal(
        &self,
        id: &str,
        from: &str,
        to: &str,
        result_json: Option<&str>,
    ) -> DomainResult<bool>;
    /// Resolved proposals the model has not been told about yet; marks them as reported.
    async fn take_unreported(&self, conversation_id: &str) -> DomainResult<Vec<Proposal>>;

    async fn list_memories(&self) -> DomainResult<Vec<Memory>>;
    async fn get_memory(&self, id: &str) -> DomainResult<Option<Memory>>;
    async fn create_memory(&self, text: &str, actor: Actor) -> DomainResult<Memory>;
    async fn update_memory(&self, id: &str, text: &str, actor: Actor) -> DomainResult<Memory>;
    async fn delete_memory(&self, id: &str, actor: Actor) -> DomainResult<()>;

    /// Revert every change recorded under `batch_id`, all or nothing.
    async fn undo_batch(&self, batch_id: &str) -> DomainResult<UndoReport>;
}

pub struct SqliteAiRepository {
    pool: SqlitePool,
}

impl SqliteAiRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

fn not_found(entity: &'static str, id: &str) -> DomainError {
    DomainError::NotFound {
        entity,
        id: id.to_string(),
    }
}

const PROPOSAL_COLS: &str = "id, conversation_id, tool_name, input_json, preview_json, status, result_json, created_at, resolved_at";

#[async_trait]
impl AiRepository for SqliteAiRepository {
    async fn create_conversation(&self, title: &str) -> DomainResult<Conversation> {
        let now = now_utc();
        let c = Conversation {
            id: uuid::Uuid::new_v4().to_string(),
            title: title.to_string(),
            created_at: now.clone(),
            updated_at: now,
        };
        sqlx::query(
            "INSERT INTO ai_conversations (id, title, created_at, updated_at) VALUES (?, ?, ?, ?)",
        )
        .bind(&c.id)
        .bind(&c.title)
        .bind(&c.created_at)
        .bind(&c.updated_at)
        .execute(&self.pool)
        .await?;
        Ok(c)
    }

    async fn list_conversations(&self, limit: i64) -> DomainResult<Vec<Conversation>> {
        Ok(sqlx::query_as::<_, Conversation>(
            "SELECT id, title, created_at, updated_at FROM ai_conversations
             WHERE deleted_at IS NULL ORDER BY updated_at DESC, rowid DESC LIMIT ?",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?)
    }

    async fn get_conversation(&self, id: &str) -> DomainResult<Option<Conversation>> {
        Ok(sqlx::query_as::<_, Conversation>(
            "SELECT id, title, created_at, updated_at FROM ai_conversations
             WHERE id = ? AND deleted_at IS NULL",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?)
    }

    async fn delete_conversation(&self, id: &str) -> DomainResult<()> {
        let r = sqlx::query(
            "UPDATE ai_conversations SET deleted_at = ? WHERE id = ? AND deleted_at IS NULL",
        )
        .bind(now_utc())
        .bind(id)
        .execute(&self.pool)
        .await?;
        if r.rows_affected() == 0 {
            return Err(not_found("conversation", id));
        }
        Ok(())
    }

    async fn append_message(
        &self,
        conversation_id: &str,
        role: &str,
        content_json: &str,
    ) -> DomainResult<StoredMessage> {
        let mut tx = self.pool.begin().await?;
        let (seq,): (i64,) = sqlx::query_as(
            "SELECT COALESCE(MAX(seq), 0) + 1 FROM ai_messages WHERE conversation_id = ?",
        )
        .bind(conversation_id)
        .fetch_one(&mut *tx)
        .await?;
        let id = uuid::Uuid::new_v4().to_string();
        let now = now_utc();
        sqlx::query(
            "INSERT INTO ai_messages (id, conversation_id, seq, role, content_json, created_at)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(conversation_id)
        .bind(seq)
        .bind(role)
        .bind(content_json)
        .bind(&now)
        .execute(&mut *tx)
        .await?;
        sqlx::query("UPDATE ai_conversations SET updated_at = ? WHERE id = ?")
            .bind(&now)
            .bind(conversation_id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(StoredMessage {
            id,
            seq,
            role: role.to_string(),
            content: serde_json::from_str(content_json).unwrap_or(Value::Null),
            created_at: now,
        })
    }

    async fn messages(&self, conversation_id: &str) -> DomainResult<Vec<StoredMessage>> {
        Ok(sqlx::query_as::<_, MessageRow>(
            "SELECT id, seq, role, content_json, created_at FROM ai_messages
             WHERE conversation_id = ? ORDER BY seq",
        )
        .bind(conversation_id)
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(MessageRow::into_message)
        .collect())
    }

    async fn create_proposal(
        &self,
        conversation_id: &str,
        tool: &str,
        input_json: &str,
        preview_json: &str,
    ) -> DomainResult<Proposal> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = now_utc();
        sqlx::query(
            "INSERT INTO ai_proposals (id, conversation_id, tool_name, input_json, preview_json, status, created_at)
             VALUES (?, ?, ?, ?, ?, 'pending', ?)",
        )
        .bind(&id)
        .bind(conversation_id)
        .bind(tool)
        .bind(input_json)
        .bind(preview_json)
        .bind(&now)
        .execute(&self.pool)
        .await?;
        self.get_proposal(&id)
            .await?
            .ok_or_else(|| not_found("proposal", &id))
    }

    async fn get_proposal(&self, id: &str) -> DomainResult<Option<Proposal>> {
        Ok(
            sqlx::query_as::<_, ProposalRow>(sqlx::AssertSqlSafe(format!(
                "SELECT {PROPOSAL_COLS} FROM ai_proposals WHERE id = ?"
            )))
            .bind(id)
            .fetch_optional(&self.pool)
            .await?
            .map(ProposalRow::into_proposal),
        )
    }

    async fn proposals(&self, conversation_id: &str) -> DomainResult<Vec<Proposal>> {
        Ok(sqlx::query_as::<_, ProposalRow>(sqlx::AssertSqlSafe(format!(
            "SELECT {PROPOSAL_COLS} FROM ai_proposals WHERE conversation_id = ? ORDER BY created_at, rowid"
        )))
        .bind(conversation_id)
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(ProposalRow::into_proposal)
        .collect())
    }

    async fn transition_proposal(
        &self,
        id: &str,
        from: &str,
        to: &str,
        result_json: Option<&str>,
    ) -> DomainResult<bool> {
        let r = sqlx::query(
            "UPDATE ai_proposals SET status = ?, result_json = COALESCE(?, result_json),
                    resolved_at = ?, reported = 0
             WHERE id = ? AND status = ?",
        )
        .bind(to)
        .bind(result_json)
        .bind(now_utc())
        .bind(id)
        .bind(from)
        .execute(&self.pool)
        .await?;
        Ok(r.rows_affected() == 1)
    }

    async fn take_unreported(&self, conversation_id: &str) -> DomainResult<Vec<Proposal>> {
        let mut tx = self.pool.begin().await?;
        let rows = sqlx::query_as::<_, ProposalRow>(sqlx::AssertSqlSafe(format!(
            "SELECT {PROPOSAL_COLS} FROM ai_proposals
             WHERE conversation_id = ? AND status != 'pending' AND reported = 0
             ORDER BY resolved_at, rowid"
        )))
        .bind(conversation_id)
        .fetch_all(&mut *tx)
        .await?;
        sqlx::query(
            "UPDATE ai_proposals SET reported = 1
             WHERE conversation_id = ? AND status != 'pending' AND reported = 0",
        )
        .bind(conversation_id)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(rows.into_iter().map(ProposalRow::into_proposal).collect())
    }

    async fn list_memories(&self) -> DomainResult<Vec<Memory>> {
        Ok(sqlx::query_as::<_, Memory>(
            "SELECT id, text, created_at, updated_at FROM ai_memories
             WHERE deleted_at IS NULL ORDER BY created_at, rowid",
        )
        .fetch_all(&self.pool)
        .await?)
    }

    async fn get_memory(&self, id: &str) -> DomainResult<Option<Memory>> {
        Ok(sqlx::query_as::<_, Memory>(
            "SELECT id, text, created_at, updated_at FROM ai_memories WHERE id = ? AND deleted_at IS NULL",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?)
    }

    async fn create_memory(&self, text: &str, actor: Actor) -> DomainResult<Memory> {
        let now = now_utc();
        let m = Memory {
            id: uuid::Uuid::new_v4().to_string(),
            text: text.to_string(),
            created_at: now.clone(),
            updated_at: now,
        };
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "INSERT INTO ai_memories (id, text, created_at, updated_at) VALUES (?, ?, ?, ?)",
        )
        .bind(&m.id)
        .bind(&m.text)
        .bind(&m.created_at)
        .bind(&m.updated_at)
        .execute(&mut *tx)
        .await?;
        log_change::<(), _>(
            &mut tx,
            "ai_memory",
            &m.id,
            "create",
            actor,
            None,
            Some(&m),
            None,
        )
        .await?;
        tx.commit().await?;
        Ok(m)
    }

    async fn update_memory(&self, id: &str, text: &str, actor: Actor) -> DomainResult<Memory> {
        let mut tx = self.pool.begin().await?;
        let before = sqlx::query_as::<_, Memory>(
            "SELECT id, text, created_at, updated_at FROM ai_memories WHERE id = ? AND deleted_at IS NULL",
        )
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| not_found("memory", id))?;
        let now = now_utc();
        sqlx::query("UPDATE ai_memories SET text = ?, updated_at = ? WHERE id = ?")
            .bind(text)
            .bind(&now)
            .bind(id)
            .execute(&mut *tx)
            .await?;
        let after = Memory {
            text: text.to_string(),
            updated_at: now,
            ..before.clone()
        };
        log_change(
            &mut tx,
            "ai_memory",
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

    async fn delete_memory(&self, id: &str, actor: Actor) -> DomainResult<()> {
        let mut tx = self.pool.begin().await?;
        let before = sqlx::query_as::<_, Memory>(
            "SELECT id, text, created_at, updated_at FROM ai_memories WHERE id = ? AND deleted_at IS NULL",
        )
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| not_found("memory", id))?;
        sqlx::query("UPDATE ai_memories SET deleted_at = ? WHERE id = ?")
            .bind(now_utc())
            .bind(id)
            .execute(&mut *tx)
            .await?;
        log_change::<_, ()>(
            &mut tx,
            "ai_memory",
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

    async fn undo_batch(&self, batch_id: &str) -> DomainResult<UndoReport> {
        let mut tx = self.pool.begin().await?;
        let rows: Vec<(String, String, String, Option<String>, Option<String>)> = sqlx::query_as(
            "SELECT entity_type, entity_id, action, before_json, after_json FROM activity_log
             WHERE batch_id = ? ORDER BY created_at DESC, rowid DESC",
        )
        .bind(batch_id)
        .fetch_all(&mut *tx)
        .await?;
        if rows.is_empty() {
            return Err(DomainError::Validation(
                "nothing to undo for this action".into(),
            ));
        }
        let mut reverted = 0;
        for (entity, id, action, before, after) in rows {
            let parse = |s: &Option<String>| -> Value {
                s.as_deref()
                    .and_then(|j| serde_json::from_str(j).ok())
                    .unwrap_or(Value::Null)
            };
            revert_one(
                &mut tx,
                &entity,
                &id,
                &action,
                &parse(&before),
                &parse(&after),
            )
            .await?;
            reverted += 1;
        }
        record(
            &mut tx,
            &NewActivity {
                entity_type: "ai_batch",
                entity_id: batch_id.to_string(),
                action: "undo",
                actor: Actor::User,
                before_json: None,
                after_json: None,
                batch_id: None,
            },
        )
        .await?;
        tx.commit().await?;
        Ok(UndoReport { reverted })
    }
}

fn conflict(entity: &str, id: &str) -> DomainError {
    DomainError::Validation(format!(
        "cannot undo: {entity} {id} was changed again after the AI action"
    ))
}

fn s(v: &Value, k: &str) -> Option<String> {
    v.get(k).and_then(Value::as_str).map(str::to_string)
}
fn i(v: &Value, k: &str) -> Option<i64> {
    v.get(k).and_then(Value::as_i64)
}
fn b(v: &Value, k: &str) -> Option<bool> {
    v.get(k).and_then(Value::as_bool)
}

async fn current_updated_at(
    tx: &mut SqliteConnection,
    sql: &'static str,
    id: &str,
) -> DomainResult<Option<String>> {
    Ok(sqlx::query_as::<_, (String,)>(sql)
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?
        .map(|r| r.0))
}

/// Revert one activity row. Updates and creates are only undone while the entity is still exactly
/// as the AI left it; otherwise the whole undo is refused rather than clobbering later edits.
async fn revert_one(
    tx: &mut SqliteConnection,
    entity: &str,
    id: &str,
    action: &str,
    before: &Value,
    after: &Value,
) -> DomainResult<()> {
    let unchanged =
        |current: Option<String>| current.is_some() && current == s(after, "updated_at");
    match (entity, action) {
        ("task", "create") => {
            let cur = current_updated_at(
                tx,
                "SELECT updated_at FROM tasks WHERE id = ? AND deleted_at IS NULL",
                id,
            )
            .await?;
            if !unchanged(cur) {
                return Err(conflict(entity, id));
            }
            sqlx::query(
                "WITH RECURSIVE d(id) AS (
                    SELECT ?1 UNION ALL SELECT t.id FROM tasks t JOIN d ON t.parent_id = d.id WHERE t.deleted_at IS NULL)
                 UPDATE tasks SET deleted_at = ?2 WHERE id IN (SELECT id FROM d)",
            )
            .bind(id)
            .bind(now_utc())
            .execute(&mut *tx)
            .await?;
        }
        ("task", "update") => {
            let cur = current_updated_at(
                tx,
                "SELECT updated_at FROM tasks WHERE id = ? AND deleted_at IS NULL",
                id,
            )
            .await?;
            if !unchanged(cur) {
                return Err(conflict(entity, id));
            }
            sqlx::query(
                "UPDATE tasks SET column_id = ?, parent_id = ?, title = ?, description = ?, priority = ?,
                        due_at = ?, position = ?, completed_at = ?, updated_at = ? WHERE id = ?",
            )
            .bind(s(before, "column_id"))
            .bind(s(before, "parent_id"))
            .bind(s(before, "title"))
            .bind(s(before, "description"))
            .bind(i(before, "priority"))
            .bind(s(before, "due_at"))
            .bind(i(before, "position"))
            .bind(s(before, "completed_at"))
            .bind(s(before, "updated_at"))
            .bind(id)
            .execute(&mut *tx)
            .await?;
        }
        ("task", "delete") => {
            sqlx::query(
                "UPDATE tasks SET deleted_at = NULL
                 WHERE deleted_at IS NOT NULL AND deleted_at = (SELECT deleted_at FROM tasks WHERE id = ?)",
            )
            .bind(id)
            .execute(&mut *tx)
            .await?;
        }
        ("event", "create") => {
            let cur = current_updated_at(
                tx,
                "SELECT updated_at FROM events WHERE id = ? AND deleted_at IS NULL",
                id,
            )
            .await?;
            if !unchanged(cur) {
                return Err(conflict(entity, id));
            }
            sqlx::query("UPDATE events SET deleted_at = ? WHERE id = ?")
                .bind(now_utc())
                .bind(id)
                .execute(&mut *tx)
                .await?;
        }
        ("event", "update") => {
            let cur = current_updated_at(
                tx,
                "SELECT updated_at FROM events WHERE id = ? AND deleted_at IS NULL",
                id,
            )
            .await?;
            if !unchanged(cur) {
                return Err(conflict(entity, id));
            }
            sqlx::query(
                "UPDATE events SET title = ?, description = ?, location = ?, all_day = ?, start_at = ?,
                        end_at = ?, timezone = ?, recurrence_rule = ?, updated_at = ? WHERE id = ?",
            )
            .bind(s(before, "title"))
            .bind(s(before, "description"))
            .bind(s(before, "location"))
            .bind(b(before, "all_day").unwrap_or(false))
            .bind(s(before, "start_at"))
            .bind(s(before, "end_at"))
            .bind(s(before, "timezone"))
            .bind(s(before, "recurrence_rule"))
            .bind(s(before, "updated_at"))
            .bind(id)
            .execute(&mut *tx)
            .await?;
        }
        ("event", "delete") => {
            sqlx::query("UPDATE events SET deleted_at = NULL WHERE id = ?")
                .bind(id)
                .execute(&mut *tx)
                .await?;
        }
        ("note", "create") => {
            let cur = current_updated_at(
                tx,
                "SELECT updated_at FROM notes WHERE id = ? AND deleted_at IS NULL",
                id,
            )
            .await?;
            if !unchanged(cur) {
                return Err(conflict(entity, id));
            }
            sqlx::query("UPDATE notes SET deleted_at = ? WHERE id = ?")
                .bind(now_utc())
                .bind(id)
                .execute(&mut *tx)
                .await?;
        }
        ("note", "update") => {
            let cur = current_updated_at(
                tx,
                "SELECT updated_at FROM notes WHERE id = ? AND deleted_at IS NULL",
                id,
            )
            .await?;
            if !unchanged(cur) {
                return Err(conflict(entity, id));
            }
            sqlx::query("UPDATE notes SET title = ?, body = ?, updated_at = ? WHERE id = ?")
                .bind(s(before, "title"))
                .bind(s(before, "body"))
                .bind(s(before, "updated_at"))
                .bind(id)
                .execute(&mut *tx)
                .await?;
        }
        ("note", "delete") => {
            sqlx::query("UPDATE notes SET deleted_at = NULL WHERE id = ?")
                .bind(id)
                .execute(&mut *tx)
                .await?;
        }
        ("inbox_item", "update") => {
            let cur = current_updated_at(
                tx,
                "SELECT updated_at FROM inbox_items WHERE id = ? AND deleted_at IS NULL",
                id,
            )
            .await?;
            if !unchanged(cur) {
                return Err(conflict(entity, id));
            }
            sqlx::query(
                "UPDATE inbox_items SET status = ?, linked_entity_type = ?, linked_entity_id = ?, updated_at = ? WHERE id = ?",
            )
            .bind(s(before, "status"))
            .bind(s(before, "linked_entity_type"))
            .bind(s(before, "linked_entity_id"))
            .bind(s(before, "updated_at"))
            .bind(id)
            .execute(&mut *tx)
            .await?;
        }
        ("ai_memory", "create") => {
            sqlx::query("UPDATE ai_memories SET deleted_at = ? WHERE id = ?")
                .bind(now_utc())
                .bind(id)
                .execute(&mut *tx)
                .await?;
        }
        ("ai_memory", "delete") => {
            sqlx::query("UPDATE ai_memories SET deleted_at = NULL WHERE id = ?")
                .bind(id)
                .execute(&mut *tx)
                .await?;
        }
        ("ai_memory", "update") => {
            sqlx::query("UPDATE ai_memories SET text = ?, updated_at = ? WHERE id = ?")
                .bind(s(before, "text"))
                .bind(s(before, "updated_at"))
                .bind(id)
                .execute(&mut *tx)
                .await?;
        }
        ("automation_rule", "create") => {
            sqlx::query("UPDATE automation_rules SET deleted_at = ? WHERE id = ?")
                .bind(now_utc())
                .bind(id)
                .execute(&mut *tx)
                .await?;
        }
        ("task_dependency", "create") | ("task_dependency", "delete") => {
            let (task_id, blocker) = id
                .split_once(':')
                .ok_or_else(|| DomainError::Validation("bad dependency id".into()))?;
            let sql = if action == "create" {
                "DELETE FROM task_dependencies WHERE task_id = ? AND blocked_by_id = ?"
            } else {
                "INSERT OR IGNORE INTO task_dependencies (task_id, blocked_by_id, created_at) VALUES (?, ?, ?)"
            };
            let q = sqlx::query(sql).bind(task_id).bind(blocker);
            let q = if action == "create" {
                q
            } else {
                q.bind(now_utc())
            };
            q.execute(&mut *tx).await?;
        }
        _ => {
            return Err(DomainError::Validation(format!(
                "this action changed a {entity} ({action}), which cannot be undone automatically"
            )));
        }
    }
    Ok(())
}
