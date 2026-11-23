use async_trait::async_trait;
use sqlx::{SqliteConnection, SqlitePool};

use super::activity_repo::log_change;
use super::now_utc;
use crate::domain::activity::Actor;
use crate::domain::kanban::{ChecklistItem, Dependency, NewTask, TASK_COLS, Task, TaskPatch};
use crate::error::{DomainError, DomainResult};

const CHECK_COLS: &str = "id, task_id, text, done, position, created_at, updated_at";

#[async_trait]
pub trait TaskRepository: Send + Sync {
    /// Create a task and its whole subtree (subtasks + checklists) in one transaction,
    /// sharing one batch id in the activity log. Returns the root tasks, in order.
    async fn create_task_trees(&self, news: &[NewTask], actor: Actor) -> DomainResult<Vec<Task>>;
    async fn get_task(&self, id: &str) -> DomainResult<Option<Task>>;
    async fn list_for_board(&self, board_id: &str) -> DomainResult<Vec<Task>>;
    async fn update_task(&self, id: &str, patch: &TaskPatch, actor: Actor) -> DomainResult<Task>;
    /// Move a top-level task (and its subtasks) to a column/position. Completion follows the
    /// target column kind.
    async fn move_task(
        &self,
        id: &str,
        column_id: &str,
        position: i64,
        actor: Actor,
    ) -> DomainResult<Task>;
    /// Soft-delete a task and all its descendants.
    async fn delete_task(&self, id: &str, actor: Actor) -> DomainResult<()>;
    /// Number of non-deleted, non-completed descendants.
    async fn open_descendant_count(&self, id: &str) -> DomainResult<i64>;
    /// Blockers of a task that are not yet completed.
    async fn unresolved_blockers(&self, id: &str) -> DomainResult<Vec<Task>>;
    async fn blocked_by_ids(&self, id: &str) -> DomainResult<Vec<String>>;
    /// Dependency links (either direction) touching this task or its subtasks.
    async fn dependency_links(&self, id: &str) -> DomainResult<i64>;
    /// Open top-level tasks in a column (where a relocated task is appended).
    async fn root_count(&self, column_id: &str, excluding: &str) -> DomainResult<i64>;
    async fn add_dependency(
        &self,
        task_id: &str,
        blocked_by_id: &str,
        actor: Actor,
    ) -> DomainResult<()>;
    async fn remove_dependency(
        &self,
        task_id: &str,
        blocked_by_id: &str,
        actor: Actor,
    ) -> DomainResult<()>;
    async fn dependencies_for_board(&self, board_id: &str) -> DomainResult<Vec<Dependency>>;

    async fn add_checklist_item(
        &self,
        task_id: &str,
        text: &str,
        actor: Actor,
    ) -> DomainResult<ChecklistItem>;
    async fn update_checklist_item(
        &self,
        id: &str,
        text: Option<&str>,
        done: Option<bool>,
        actor: Actor,
    ) -> DomainResult<ChecklistItem>;
    async fn delete_checklist_item(&self, id: &str, actor: Actor) -> DomainResult<()>;
    async fn checklist_for_board(&self, board_id: &str) -> DomainResult<Vec<ChecklistItem>>;
    /// Column ids of a board in order, with their kinds (used for completion moves).
    async fn board_id_of_column(&self, column_id: &str) -> DomainResult<Option<String>>;
}

pub struct SqliteTaskRepository {
    pool: SqlitePool,
}

impl SqliteTaskRepository {
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

async fn fetch_task(conn: &mut SqliteConnection, id: &str) -> DomainResult<Option<Task>> {
    Ok(sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {TASK_COLS} FROM tasks WHERE id = ? AND deleted_at IS NULL"
    )))
    .bind(id)
    .fetch_optional(conn)
    .await?)
}

async fn column_kind(conn: &mut SqliteConnection, column_id: &str) -> DomainResult<String> {
    let row: Option<(String,)> =
        sqlx::query_as("SELECT kind FROM board_columns WHERE id = ? AND deleted_at IS NULL")
            .bind(column_id)
            .fetch_optional(conn)
            .await?;
    row.map(|r| r.0)
        .ok_or_else(|| not_found("column", column_id))
}

/// Make room at `position` among the siblings of a task scope (parent or column roots).
async fn shift_siblings(
    conn: &mut SqliteConnection,
    column_id: &str,
    parent_id: Option<&str>,
    exclude: Option<&str>,
    position: i64,
) -> DomainResult<()> {
    let exclude = exclude.unwrap_or("");
    match parent_id {
        Some(p) => {
            sqlx::query(
                "UPDATE tasks SET position = position + 1
                 WHERE parent_id = ? AND id <> ? AND position >= ? AND deleted_at IS NULL",
            )
            .bind(p)
            .bind(exclude)
            .bind(position)
            .execute(conn)
            .await?;
        }
        None => {
            sqlx::query(
                "UPDATE tasks SET position = position + 1
                 WHERE column_id = ? AND parent_id IS NULL AND id <> ? AND position >= ? AND deleted_at IS NULL",
            )
            .bind(column_id)
            .bind(exclude)
            .bind(position)
            .execute(conn)
            .await?;
        }
    }
    Ok(())
}

async fn next_position(
    conn: &mut SqliteConnection,
    column_id: &str,
    parent_id: Option<&str>,
) -> DomainResult<i64> {
    let row: (i64,) = match parent_id {
        Some(p) => {
            sqlx::query_as(
                "SELECT COALESCE(MAX(position), -1) + 1 FROM tasks WHERE parent_id = ? AND deleted_at IS NULL",
            )
            .bind(p)
            .fetch_one(conn)
            .await?
        }
        None => {
            sqlx::query_as(
                "SELECT COALESCE(MAX(position), -1) + 1 FROM tasks WHERE column_id = ? AND parent_id IS NULL AND deleted_at IS NULL",
            )
            .bind(column_id)
            .fetch_one(conn)
            .await?
        }
    };
    Ok(row.0)
}

/// Iteratively insert a task tree. Uses an explicit stack to avoid async recursion.
async fn insert_tree(
    conn: &mut SqliteConnection,
    root: &NewTask,
    actor: Actor,
    batch: &str,
) -> DomainResult<Task> {
    let now = now_utc();
    let mut stack: Vec<(&NewTask, Option<String>)> = vec![(root, root.parent_id.clone())];
    let mut root_task: Option<Task> = None;
    while let Some((node, parent)) = stack.pop() {
        let position = match node.position {
            Some(p) => {
                shift_siblings(conn, &node.column_id, parent.as_deref(), None, p).await?;
                p
            }
            None => next_position(conn, &node.column_id, parent.as_deref()).await?,
        };
        let kind = column_kind(conn, &node.column_id).await?;
        let completed = (kind == "done").then(|| now.clone());
        let task: Task = sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "INSERT INTO tasks (id, column_id, parent_id, title, description, priority, due_at, position, completed_at, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) RETURNING {TASK_COLS}"
        )))
        .bind(uuid::Uuid::new_v4().to_string())
        .bind(&node.column_id)
        .bind(&parent)
        .bind(&node.title)
        .bind(&node.description)
        .bind(node.priority)
        .bind(&node.due_at)
        .bind(position)
        .bind(&completed)
        .bind(&now)
        .bind(&now)
        .fetch_one(&mut *conn)
        .await?;
        log_change::<(), _>(
            conn,
            "task",
            &task.id,
            "create",
            actor,
            None,
            Some(&task),
            Some(batch),
        )
        .await?;
        for (i, text) in node.checklist.iter().enumerate() {
            let item: ChecklistItem = sqlx::query_as(sqlx::AssertSqlSafe(format!(
                "INSERT INTO checklist_items (id, task_id, text, done, position, created_at, updated_at)
                 VALUES (?, ?, ?, 0, ?, ?, ?) RETURNING {CHECK_COLS}"
            )))
            .bind(uuid::Uuid::new_v4().to_string())
            .bind(&task.id)
            .bind(text)
            .bind(i as i64)
            .bind(&now)
            .bind(&now)
            .fetch_one(&mut *conn)
            .await?;
            log_change::<(), _>(
                conn,
                "checklist_item",
                &item.id,
                "create",
                actor,
                None,
                Some(&item),
                Some(batch),
            )
            .await?;
        }
        // Children are inserted with the new task as parent, in their given order.
        for child in node.children.iter().rev() {
            // Children inherit the parent's column.
            stack.push((child, Some(task.id.clone())));
        }
        if root_task.is_none() {
            root_task = Some(task);
        }
    }
    root_task.ok_or_else(|| DomainError::Validation("empty task tree".into()))
}

#[async_trait]
impl TaskRepository for SqliteTaskRepository {
    async fn create_task_trees(&self, news: &[NewTask], actor: Actor) -> DomainResult<Vec<Task>> {
        let mut tx = self.pool.begin().await?;
        let batch = uuid::Uuid::new_v4().to_string();
        let mut roots = Vec::with_capacity(news.len());
        for new in news {
            if let Some(p) = &new.parent_id {
                fetch_task(&mut tx, p)
                    .await?
                    .ok_or_else(|| not_found("task", p))?;
            }
            roots.push(insert_tree(&mut tx, new, actor, &batch).await?);
        }
        tx.commit().await?;
        Ok(roots)
    }

    async fn get_task(&self, id: &str) -> DomainResult<Option<Task>> {
        let mut c = self.pool.acquire().await?;
        fetch_task(&mut c, id).await
    }

    async fn list_for_board(&self, board_id: &str) -> DomainResult<Vec<Task>> {
        Ok(sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "SELECT {} FROM tasks t WHERE t.deleted_at IS NULL
             AND t.column_id IN (SELECT id FROM board_columns WHERE board_id = ? AND deleted_at IS NULL)
             ORDER BY t.position, t.created_at, t.rowid",
            TASK_COLS
                .split(", ")
                .map(|c| format!("t.{c}"))
                .collect::<Vec<_>>()
                .join(", ")
        )))
        .bind(board_id)
        .fetch_all(&self.pool)
        .await?)
    }

    async fn update_task(&self, id: &str, patch: &TaskPatch, actor: Actor) -> DomainResult<Task> {
        let mut tx = self.pool.begin().await?;
        let before = fetch_task(&mut tx, id)
            .await?
            .ok_or_else(|| not_found("task", id))?;
        let now = now_utc();
        let title = patch.title.as_deref().unwrap_or(&before.title);
        let description = patch
            .description
            .as_ref()
            .map_or(before.description.as_deref(), |d| d.as_deref());
        let due_at = patch
            .due_at
            .as_ref()
            .map_or(before.due_at.as_deref(), |d| d.as_deref());
        let completed_at = match patch.completed {
            Some(true) => Some(before.completed_at.clone().unwrap_or_else(|| now.clone())),
            Some(false) => None,
            None => before.completed_at.clone(),
        };
        let after: Task = sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "UPDATE tasks SET title = ?, description = ?, priority = ?, due_at = ?, completed_at = ?, updated_at = ?
             WHERE id = ? RETURNING {TASK_COLS}"
        )))
        .bind(title)
        .bind(description)
        .bind(patch.priority.unwrap_or(before.priority))
        .bind(due_at)
        .bind(&completed_at)
        .bind(&now)
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
        log_change(
            &mut tx,
            "task",
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

    async fn move_task(
        &self,
        id: &str,
        column_id: &str,
        position: i64,
        actor: Actor,
    ) -> DomainResult<Task> {
        let mut tx = self.pool.begin().await?;
        let before = fetch_task(&mut tx, id)
            .await?
            .ok_or_else(|| not_found("task", id))?;
        let kind = column_kind(&mut tx, column_id).await?;
        let now = now_utc();
        shift_siblings(&mut tx, column_id, None, Some(id), position).await?;
        let completed_at = if kind == "done" {
            Some(before.completed_at.clone().unwrap_or_else(|| now.clone()))
        } else {
            None
        };
        let after: Task = sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "UPDATE tasks SET column_id = ?, position = ?, completed_at = ?, updated_at = ?
             WHERE id = ? RETURNING {TASK_COLS}"
        )))
        .bind(column_id)
        .bind(position)
        .bind(&completed_at)
        .bind(&now)
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
        // Subtasks live in the same column as their parent.
        sqlx::query(
            "WITH RECURSIVE d(id) AS (
                SELECT id FROM tasks WHERE parent_id = ?1 AND deleted_at IS NULL
                UNION ALL SELECT t.id FROM tasks t JOIN d ON t.parent_id = d.id WHERE t.deleted_at IS NULL)
             UPDATE tasks SET column_id = ?2 WHERE id IN (SELECT id FROM d)",
        )
        .bind(id)
        .bind(column_id)
        .execute(&mut *tx)
        .await?;
        log_change(
            &mut tx,
            "task",
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

    async fn delete_task(&self, id: &str, actor: Actor) -> DomainResult<()> {
        let mut tx = self.pool.begin().await?;
        let before = fetch_task(&mut tx, id)
            .await?
            .ok_or_else(|| not_found("task", id))?;
        let now = now_utc();
        sqlx::query(
            "WITH RECURSIVE d(id) AS (
                SELECT ?1 UNION ALL SELECT t.id FROM tasks t JOIN d ON t.parent_id = d.id WHERE t.deleted_at IS NULL)
             UPDATE tasks SET deleted_at = ?2 WHERE id IN (SELECT id FROM d)",
        )
        .bind(id)
        .bind(&now)
        .execute(&mut *tx)
        .await?;
        log_change::<_, ()>(
            &mut tx,
            "task",
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

    async fn open_descendant_count(&self, id: &str) -> DomainResult<i64> {
        let n: (i64,) = sqlx::query_as(
            "WITH RECURSIVE d(id) AS (
                SELECT id FROM tasks WHERE parent_id = ?1 AND deleted_at IS NULL
                UNION ALL SELECT t.id FROM tasks t JOIN d ON t.parent_id = d.id WHERE t.deleted_at IS NULL)
             SELECT COUNT(*) FROM tasks WHERE id IN (SELECT id FROM d) AND completed_at IS NULL",
        )
        .bind(id)
        .fetch_one(&self.pool)
        .await?;
        Ok(n.0)
    }

    async fn unresolved_blockers(&self, id: &str) -> DomainResult<Vec<Task>> {
        Ok(sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "SELECT {} FROM tasks t JOIN task_dependencies d ON d.blocked_by_id = t.id
             WHERE d.task_id = ? AND t.deleted_at IS NULL AND t.completed_at IS NULL",
            TASK_COLS
                .split(", ")
                .map(|c| format!("t.{c}"))
                .collect::<Vec<_>>()
                .join(", ")
        )))
        .bind(id)
        .fetch_all(&self.pool)
        .await?)
    }

    async fn dependency_links(&self, id: &str) -> DomainResult<i64> {
        Ok(sqlx::query_as::<_, (i64,)>(
            "WITH RECURSIVE d(id) AS (
                SELECT ?1 UNION ALL SELECT t.id FROM tasks t JOIN d ON t.parent_id = d.id WHERE t.deleted_at IS NULL)
             SELECT COUNT(*) FROM task_dependencies
             WHERE task_id IN (SELECT id FROM d) OR blocked_by_id IN (SELECT id FROM d)",
        )
        .bind(id)
        .fetch_one(&self.pool)
        .await?
        .0)
    }

    async fn root_count(&self, column_id: &str, excluding: &str) -> DomainResult<i64> {
        Ok(sqlx::query_as::<_, (i64,)>(
            "SELECT COUNT(*) FROM tasks WHERE column_id = ? AND parent_id IS NULL AND deleted_at IS NULL AND id != ?",
        )
        .bind(column_id)
        .bind(excluding)
        .fetch_one(&self.pool)
        .await?
        .0)
    }

    async fn blocked_by_ids(&self, id: &str) -> DomainResult<Vec<String>> {
        let rows: Vec<(String,)> =
            sqlx::query_as("SELECT blocked_by_id FROM task_dependencies WHERE task_id = ?")
                .bind(id)
                .fetch_all(&self.pool)
                .await?;
        Ok(rows.into_iter().map(|r| r.0).collect())
    }

    async fn add_dependency(
        &self,
        task_id: &str,
        blocked_by_id: &str,
        actor: Actor,
    ) -> DomainResult<()> {
        let mut tx = self.pool.begin().await?;
        fetch_task(&mut tx, task_id)
            .await?
            .ok_or_else(|| not_found("task", task_id))?;
        fetch_task(&mut tx, blocked_by_id)
            .await?
            .ok_or_else(|| not_found("task", blocked_by_id))?;
        let dep = Dependency {
            task_id: task_id.to_string(),
            blocked_by_id: blocked_by_id.to_string(),
        };
        let res = sqlx::query(
            "INSERT OR IGNORE INTO task_dependencies (task_id, blocked_by_id, created_at) VALUES (?, ?, ?)",
        )
        .bind(task_id)
        .bind(blocked_by_id)
        .bind(now_utc())
        .execute(&mut *tx)
        .await?;
        if res.rows_affected() > 0 {
            log_change::<(), _>(
                &mut tx,
                "task_dependency",
                &format!("{task_id}:{blocked_by_id}"),
                "create",
                actor,
                None,
                Some(&dep),
                None,
            )
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    async fn remove_dependency(
        &self,
        task_id: &str,
        blocked_by_id: &str,
        actor: Actor,
    ) -> DomainResult<()> {
        let mut tx = self.pool.begin().await?;
        let res =
            sqlx::query("DELETE FROM task_dependencies WHERE task_id = ? AND blocked_by_id = ?")
                .bind(task_id)
                .bind(blocked_by_id)
                .execute(&mut *tx)
                .await?;
        if res.rows_affected() == 0 {
            return Err(not_found(
                "dependency",
                &format!("{task_id}:{blocked_by_id}"),
            ));
        }
        let dep = Dependency {
            task_id: task_id.to_string(),
            blocked_by_id: blocked_by_id.to_string(),
        };
        log_change::<_, ()>(
            &mut tx,
            "task_dependency",
            &format!("{task_id}:{blocked_by_id}"),
            "delete",
            actor,
            Some(&dep),
            None,
            None,
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }

    async fn dependencies_for_board(&self, board_id: &str) -> DomainResult<Vec<Dependency>> {
        Ok(sqlx::query_as(
            "SELECT d.task_id, d.blocked_by_id FROM task_dependencies d
             JOIN tasks t ON t.id = d.task_id AND t.deleted_at IS NULL
             WHERE t.column_id IN (SELECT id FROM board_columns WHERE board_id = ? AND deleted_at IS NULL)",
        )
        .bind(board_id)
        .fetch_all(&self.pool)
        .await?)
    }

    async fn add_checklist_item(
        &self,
        task_id: &str,
        text: &str,
        actor: Actor,
    ) -> DomainResult<ChecklistItem> {
        let mut tx = self.pool.begin().await?;
        fetch_task(&mut tx, task_id)
            .await?
            .ok_or_else(|| not_found("task", task_id))?;
        let now = now_utc();
        let item: ChecklistItem = sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "INSERT INTO checklist_items (id, task_id, text, done, position, created_at, updated_at)
             VALUES (?, ?, ?, 0, (SELECT COALESCE(MAX(position), -1) + 1 FROM checklist_items WHERE task_id = ?), ?, ?)
             RETURNING {CHECK_COLS}"
        )))
        .bind(uuid::Uuid::new_v4().to_string())
        .bind(task_id)
        .bind(text)
        .bind(task_id)
        .bind(&now)
        .bind(&now)
        .fetch_one(&mut *tx)
        .await?;
        log_change::<(), _>(
            &mut tx,
            "checklist_item",
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

    async fn update_checklist_item(
        &self,
        id: &str,
        text: Option<&str>,
        done: Option<bool>,
        actor: Actor,
    ) -> DomainResult<ChecklistItem> {
        let mut tx = self.pool.begin().await?;
        let before: ChecklistItem = sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "SELECT {CHECK_COLS} FROM checklist_items WHERE id = ?"
        )))
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| not_found("checklist_item", id))?;
        let after: ChecklistItem = sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "UPDATE checklist_items SET text = ?, done = ?, updated_at = ? WHERE id = ? RETURNING {CHECK_COLS}"
        )))
        .bind(text.unwrap_or(&before.text))
        .bind(done.unwrap_or(before.done))
        .bind(now_utc())
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
        log_change(
            &mut tx,
            "checklist_item",
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

    async fn delete_checklist_item(&self, id: &str, actor: Actor) -> DomainResult<()> {
        let mut tx = self.pool.begin().await?;
        let before: ChecklistItem = sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "SELECT {CHECK_COLS} FROM checklist_items WHERE id = ?"
        )))
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| not_found("checklist_item", id))?;
        sqlx::query("DELETE FROM checklist_items WHERE id = ?")
            .bind(id)
            .execute(&mut *tx)
            .await?;
        log_change::<_, ()>(
            &mut tx,
            "checklist_item",
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

    async fn checklist_for_board(&self, board_id: &str) -> DomainResult<Vec<ChecklistItem>> {
        Ok(sqlx::query_as(
            "SELECT c.id, c.task_id, c.text, c.done, c.position, c.created_at, c.updated_at
             FROM checklist_items c JOIN tasks t ON t.id = c.task_id AND t.deleted_at IS NULL
             WHERE t.column_id IN (SELECT id FROM board_columns WHERE board_id = ? AND deleted_at IS NULL)
             ORDER BY c.position, c.rowid",
        )
        .bind(board_id)
        .fetch_all(&self.pool)
        .await?)
    }

    async fn board_id_of_column(&self, column_id: &str) -> DomainResult<Option<String>> {
        let r: Option<(String,)> = sqlx::query_as(
            "SELECT board_id FROM board_columns WHERE id = ? AND deleted_at IS NULL",
        )
        .bind(column_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(r.map(|x| x.0))
    }
}
