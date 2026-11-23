use async_trait::async_trait;
use sqlx::{SqliteConnection, SqlitePool};

use super::activity_repo::log_change;
use super::now_utc;
use crate::domain::activity::Actor;
use crate::domain::calendar::*;
use crate::error::{DomainError, DomainResult};

const EXC_COLS: &str = "id, event_id, original_start, kind, title, description, location, start_at, end_at, created_at";

pub struct ImportEvent {
    pub event: NewEvent,
    pub exceptions: Vec<NewException>,
}

#[async_trait]
pub trait CalendarRepository: Send + Sync {
    async fn create_event(&self, e: &NewEvent, actor: Actor) -> DomainResult<Event>;
    async fn get_event(&self, id: &str) -> DomainResult<Option<Event>>;
    /// Updating a recurring series' timing or rule clears its exceptions (they would be orphaned).
    async fn update_event(&self, id: &str, u: &EventUpdate, actor: Actor) -> DomainResult<Event>;
    async fn delete_event(&self, id: &str, actor: Actor) -> DomainResult<()>;
    /// Non-recurring events overlapping `[from, to)` and recurring series starting before `to`.
    async fn events_for_range(&self, from: &str, to: &str) -> DomainResult<Vec<Event>>;
    async fn all_events(&self) -> DomainResult<Vec<Event>>;
    async fn exceptions_for(&self, event_ids: &[String]) -> DomainResult<Vec<EventException>>;
    async fn set_exception(
        &self,
        event_id: &str,
        ex: &NewException,
        actor: Actor,
    ) -> DomainResult<()>;
    async fn clear_exception(
        &self,
        event_id: &str,
        original_start: &str,
        actor: Actor,
    ) -> DomainResult<()>;
    /// Insert events whose UID is not yet present, in one transaction with a shared batch id.
    /// Returns (created, skipped_duplicates).
    async fn import_events(
        &self,
        items: &[ImportEvent],
        actor: Actor,
    ) -> DomainResult<(usize, usize)>;

    async fn task_deadlines_in_range(
        &self,
        from: &str,
        to: &str,
    ) -> DomainResult<Vec<TaskDeadline>>;
    async fn recurring_tasks(&self) -> DomainResult<Vec<TaskDeadline>>;
    async fn task_occurrences(&self, task_ids: &[String]) -> DomainResult<Vec<TaskOccurrence>>;
    async fn set_task_recurrence(
        &self,
        task_id: &str,
        rule: Option<&str>,
        tz: Option<&str>,
        actor: Actor,
    ) -> DomainResult<()>;
    /// `status = None` reopens the occurrence.
    async fn set_task_occurrence(
        &self,
        task_id: &str,
        original_due: &str,
        status: Option<&str>,
        actor: Actor,
    ) -> DomainResult<()>;
    async fn task_due_and_rule(&self, task_id: &str) -> DomainResult<Option<TaskDeadline>>;
}

pub struct SqliteCalendarRepository {
    pool: SqlitePool,
}

impl SqliteCalendarRepository {
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

async fn fetch_event(conn: &mut SqliteConnection, id: &str) -> DomainResult<Option<Event>> {
    Ok(sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {EVENT_COLS} FROM events WHERE id = ? AND deleted_at IS NULL"
    )))
    .bind(id)
    .fetch_optional(conn)
    .await?)
}

async fn insert_event(
    conn: &mut SqliteConnection,
    e: &NewEvent,
    actor: Actor,
    batch: Option<&str>,
) -> DomainResult<Event> {
    let now = now_utc();
    let ev: Event = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "INSERT INTO events (id, uid, title, description, location, all_day, start_at, end_at, timezone, recurrence_rule, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) RETURNING {EVENT_COLS}"
    )))
    .bind(uuid::Uuid::new_v4().to_string())
    .bind(&e.uid)
    .bind(&e.title)
    .bind(&e.description)
    .bind(&e.location)
    .bind(e.all_day)
    .bind(&e.start_at)
    .bind(&e.end_at)
    .bind(&e.timezone)
    .bind(&e.recurrence_rule)
    .bind(&now)
    .bind(&now)
    .fetch_one(&mut *conn)
    .await?;
    log_change::<(), _>(
        conn,
        "event",
        &ev.id,
        "create",
        actor,
        None,
        Some(&ev),
        batch,
    )
    .await?;
    Ok(ev)
}

async fn upsert_exception(
    conn: &mut SqliteConnection,
    event_id: &str,
    ex: &NewException,
    actor: Actor,
    batch: Option<&str>,
) -> DomainResult<()> {
    let before: Option<EventException> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {EXC_COLS} FROM event_exceptions WHERE event_id = ? AND original_start = ?"
    )))
    .bind(event_id)
    .bind(&ex.original_start)
    .fetch_optional(&mut *conn)
    .await?;
    let after: EventException = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "INSERT INTO event_exceptions (id, event_id, original_start, kind, title, description, location, start_at, end_at, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT(event_id, original_start) DO UPDATE SET kind = excluded.kind, title = excluded.title,
           description = excluded.description, location = excluded.location,
           start_at = excluded.start_at, end_at = excluded.end_at
         RETURNING {EXC_COLS}"
    )))
    .bind(uuid::Uuid::new_v4().to_string())
    .bind(event_id)
    .bind(&ex.original_start)
    .bind(if ex.skip { "skip" } else { "override" })
    .bind(&ex.title)
    .bind(&ex.description)
    .bind(&ex.location)
    .bind(&ex.start_at)
    .bind(&ex.end_at)
    .bind(now_utc())
    .fetch_one(&mut *conn)
    .await?;
    log_change(
        conn,
        "event_exception",
        &after.id,
        if before.is_some() { "update" } else { "create" },
        actor,
        before.as_ref(),
        Some(&after),
        batch,
    )
    .await
}

fn deadline_select(extra: &str) -> String {
    format!(
        "SELECT t.id, t.title, t.priority, t.due_at, t.completed_at, t.recurrence_rule, t.recurrence_tz,
                b.id AS board_id, b.project_id AS project_id
         FROM tasks t
         JOIN board_columns c ON c.id = t.column_id AND c.deleted_at IS NULL
         JOIN boards b ON b.id = c.board_id AND b.deleted_at IS NULL
         WHERE t.deleted_at IS NULL AND t.due_at IS NOT NULL {extra}"
    )
}

fn in_list(n: usize) -> String {
    vec!["?"; n].join(",")
}

#[async_trait]
impl CalendarRepository for SqliteCalendarRepository {
    async fn create_event(&self, e: &NewEvent, actor: Actor) -> DomainResult<Event> {
        let mut tx = self.pool.begin().await?;
        let dup: Option<(String,)> =
            sqlx::query_as("SELECT id FROM events WHERE uid = ? AND deleted_at IS NULL")
                .bind(&e.uid)
                .fetch_optional(&mut *tx)
                .await?;
        if dup.is_some() {
            return Err(DomainError::Validation(
                "an event with this UID already exists".into(),
            ));
        }
        let ev = insert_event(&mut tx, e, actor, None).await?;
        tx.commit().await?;
        Ok(ev)
    }

    async fn get_event(&self, id: &str) -> DomainResult<Option<Event>> {
        let mut c = self.pool.acquire().await?;
        fetch_event(&mut c, id).await
    }

    async fn update_event(&self, id: &str, u: &EventUpdate, actor: Actor) -> DomainResult<Event> {
        let mut tx = self.pool.begin().await?;
        let before = fetch_event(&mut tx, id)
            .await?
            .ok_or_else(|| not_found("event", id))?;
        let after: Event = sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "UPDATE events SET title = ?, description = ?, location = ?, all_day = ?, start_at = ?, end_at = ?,
                timezone = ?, recurrence_rule = ?, updated_at = ? WHERE id = ? RETURNING {EVENT_COLS}"
        )))
        .bind(&u.title)
        .bind(&u.description)
        .bind(&u.location)
        .bind(u.all_day)
        .bind(&u.start_at)
        .bind(&u.end_at)
        .bind(&u.timezone)
        .bind(&u.recurrence_rule)
        .bind(now_utc())
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
        let series_changed = before.start_at != after.start_at
            || before.timezone != after.timezone
            || before.recurrence_rule != after.recurrence_rule;
        if series_changed {
            sqlx::query("DELETE FROM event_exceptions WHERE event_id = ?")
                .bind(id)
                .execute(&mut *tx)
                .await?;
        }
        log_change(
            &mut tx,
            "event",
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

    async fn delete_event(&self, id: &str, actor: Actor) -> DomainResult<()> {
        let mut tx = self.pool.begin().await?;
        let before = fetch_event(&mut tx, id)
            .await?
            .ok_or_else(|| not_found("event", id))?;
        let now = now_utc();
        sqlx::query("UPDATE events SET deleted_at = ?, updated_at = ? WHERE id = ?")
            .bind(&now)
            .bind(&now)
            .bind(id)
            .execute(&mut *tx)
            .await?;
        log_change::<_, ()>(
            &mut tx,
            "event",
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

    async fn events_for_range(&self, from: &str, to: &str) -> DomainResult<Vec<Event>> {
        Ok(sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "SELECT {EVENT_COLS} FROM events WHERE deleted_at IS NULL AND (
               (recurrence_rule IS NULL AND start_at < ?1 AND (end_at > ?2 OR (end_at = start_at AND start_at >= ?2)))
               OR (recurrence_rule IS NOT NULL AND start_at < ?1))
             ORDER BY start_at"
        )))
        .bind(to)
        .bind(from)
        .fetch_all(&self.pool)
        .await?)
    }

    async fn all_events(&self) -> DomainResult<Vec<Event>> {
        Ok(sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "SELECT {EVENT_COLS} FROM events WHERE deleted_at IS NULL ORDER BY start_at"
        )))
        .fetch_all(&self.pool)
        .await?)
    }

    async fn exceptions_for(&self, event_ids: &[String]) -> DomainResult<Vec<EventException>> {
        if event_ids.is_empty() {
            return Ok(vec![]);
        }
        let sql = format!(
            "SELECT {EXC_COLS} FROM event_exceptions WHERE event_id IN ({}) ORDER BY original_start",
            in_list(event_ids.len())
        );
        let mut q = sqlx::query_as::<_, EventException>(sqlx::AssertSqlSafe(sql));
        for id in event_ids {
            q = q.bind(id);
        }
        Ok(q.fetch_all(&self.pool).await?)
    }

    async fn set_exception(
        &self,
        event_id: &str,
        ex: &NewException,
        actor: Actor,
    ) -> DomainResult<()> {
        let mut tx = self.pool.begin().await?;
        fetch_event(&mut tx, event_id)
            .await?
            .ok_or_else(|| not_found("event", event_id))?;
        upsert_exception(&mut tx, event_id, ex, actor, None).await?;
        tx.commit().await?;
        Ok(())
    }

    async fn clear_exception(
        &self,
        event_id: &str,
        original_start: &str,
        actor: Actor,
    ) -> DomainResult<()> {
        let mut tx = self.pool.begin().await?;
        let before: EventException = sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "SELECT {EXC_COLS} FROM event_exceptions WHERE event_id = ? AND original_start = ?"
        )))
        .bind(event_id)
        .bind(original_start)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| not_found("event_exception", original_start))?;
        sqlx::query("DELETE FROM event_exceptions WHERE id = ?")
            .bind(&before.id)
            .execute(&mut *tx)
            .await?;
        log_change::<_, ()>(
            &mut tx,
            "event_exception",
            &before.id,
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

    async fn import_events(
        &self,
        items: &[ImportEvent],
        actor: Actor,
    ) -> DomainResult<(usize, usize)> {
        let mut tx = self.pool.begin().await?;
        let batch = uuid::Uuid::new_v4().to_string();
        let (mut created, mut skipped) = (0, 0);
        for item in items {
            let dup: Option<(String,)> =
                sqlx::query_as("SELECT id FROM events WHERE uid = ? AND deleted_at IS NULL")
                    .bind(&item.event.uid)
                    .fetch_optional(&mut *tx)
                    .await?;
            if dup.is_some() {
                skipped += 1;
                continue;
            }
            let ev = insert_event(&mut tx, &item.event, actor, Some(&batch)).await?;
            for ex in &item.exceptions {
                upsert_exception(&mut tx, &ev.id, ex, actor, Some(&batch)).await?;
            }
            created += 1;
        }
        tx.commit().await?;
        Ok((created, skipped))
    }

    async fn task_deadlines_in_range(
        &self,
        from: &str,
        to: &str,
    ) -> DomainResult<Vec<TaskDeadline>> {
        Ok(sqlx::query_as(sqlx::AssertSqlSafe(deadline_select(
            "AND t.recurrence_rule IS NULL AND t.due_at >= ? AND t.due_at < ? ORDER BY t.due_at",
        )))
        .bind(from)
        .bind(to)
        .fetch_all(&self.pool)
        .await?)
    }

    async fn recurring_tasks(&self) -> DomainResult<Vec<TaskDeadline>> {
        Ok(sqlx::query_as(sqlx::AssertSqlSafe(deadline_select(
            "AND t.recurrence_rule IS NOT NULL",
        )))
        .fetch_all(&self.pool)
        .await?)
    }

    async fn task_occurrences(&self, task_ids: &[String]) -> DomainResult<Vec<TaskOccurrence>> {
        if task_ids.is_empty() {
            return Ok(vec![]);
        }
        let sql = format!(
            "SELECT task_id, original_due, status, completed_at FROM task_occurrences WHERE task_id IN ({})",
            in_list(task_ids.len())
        );
        let mut q = sqlx::query_as::<_, TaskOccurrence>(sqlx::AssertSqlSafe(sql));
        for id in task_ids {
            q = q.bind(id);
        }
        Ok(q.fetch_all(&self.pool).await?)
    }

    async fn set_task_recurrence(
        &self,
        task_id: &str,
        rule: Option<&str>,
        tz: Option<&str>,
        actor: Actor,
    ) -> DomainResult<()> {
        let mut tx = self.pool.begin().await?;
        let before: (Option<String>, Option<String>) = sqlx::query_as(
            "SELECT recurrence_rule, recurrence_tz FROM tasks WHERE id = ? AND deleted_at IS NULL",
        )
        .bind(task_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| not_found("task", task_id))?;
        sqlx::query(
            "UPDATE tasks SET recurrence_rule = ?, recurrence_tz = ?, updated_at = ? WHERE id = ?",
        )
        .bind(rule)
        .bind(tz)
        .bind(now_utc())
        .bind(task_id)
        .execute(&mut *tx)
        .await?;
        sqlx::query("DELETE FROM task_occurrences WHERE task_id = ?")
            .bind(task_id)
            .execute(&mut *tx)
            .await?;
        log_change(
            &mut tx,
            "task_recurrence",
            task_id,
            "update",
            actor,
            Some(&before),
            Some(&(rule, tz)),
            None,
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }

    async fn set_task_occurrence(
        &self,
        task_id: &str,
        original_due: &str,
        status: Option<&str>,
        actor: Actor,
    ) -> DomainResult<()> {
        let mut tx = self.pool.begin().await?;
        let before: Option<TaskOccurrence> = sqlx::query_as(
            "SELECT task_id, original_due, status, completed_at FROM task_occurrences WHERE task_id = ? AND original_due = ?",
        )
        .bind(task_id)
        .bind(original_due)
        .fetch_optional(&mut *tx)
        .await?;
        let after: Option<TaskOccurrence> = match status {
            Some(s) => Some(
                sqlx::query_as(
                    "INSERT INTO task_occurrences (task_id, original_due, status, completed_at) VALUES (?, ?, ?, ?)
                     ON CONFLICT(task_id, original_due) DO UPDATE SET status = excluded.status, completed_at = excluded.completed_at
                     RETURNING task_id, original_due, status, completed_at",
                )
                .bind(task_id)
                .bind(original_due)
                .bind(s)
                .bind((s == "done").then(now_utc))
                .fetch_one(&mut *tx)
                .await?,
            ),
            None => {
                sqlx::query("DELETE FROM task_occurrences WHERE task_id = ? AND original_due = ?")
                    .bind(task_id)
                    .bind(original_due)
                    .execute(&mut *tx)
                    .await?;
                None
            }
        };
        log_change(
            &mut tx,
            "task_occurrence",
            &format!("{task_id}:{original_due}"),
            "update",
            actor,
            before.as_ref(),
            after.as_ref(),
            None,
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }

    async fn task_due_and_rule(&self, task_id: &str) -> DomainResult<Option<TaskDeadline>> {
        Ok(
            sqlx::query_as(sqlx::AssertSqlSafe(deadline_select("AND t.id = ?")))
                .bind(task_id)
                .fetch_optional(&self.pool)
                .await?,
        )
    }
}
