use async_trait::async_trait;
use sqlx::{SqliteConnection, SqlitePool};

use super::activity_repo::log_change;
use super::now_utc;
use crate::domain::activity::Actor;
use crate::domain::timers::{FocusSession, NewSession, NewTimer, TIMER_COLS, Timer, TimerChange};
use crate::error::{DomainError, DomainResult};

const SESSION_COLS: &str = "id, timer_id, task_id, started_at, ended_at, duration_ms, created_at";

#[async_trait]
pub trait TimerRepository: Send + Sync {
    async fn create(&self, t: &NewTimer, actor: Actor) -> DomainResult<Timer>;
    async fn get(&self, id: &str) -> DomainResult<Option<Timer>>;
    /// Apply a transition guarded by `expected_state`; optionally record a focus session in the
    /// same transaction. Fails with a validation error if the timer is no longer in that state.
    async fn transition(
        &self,
        id: &str,
        change: &TimerChange,
        session: Option<&NewSession>,
        actor: Actor,
    ) -> DomainResult<Timer>;
    async fn list_by_states(&self, states: &[&str]) -> DomainResult<Vec<Timer>>;
    async fn recent_finished(&self, limit: i64) -> DomainResult<Vec<Timer>>;
    async fn sessions(
        &self,
        from: &str,
        to: &str,
        task_id: Option<&str>,
    ) -> DomainResult<Vec<FocusSession>>;
    async fn set_session_task(
        &self,
        session_id: &str,
        task_id: Option<&str>,
        actor: Actor,
    ) -> DomainResult<FocusSession>;
    async fn task_exists(&self, task_id: &str) -> DomainResult<bool>;
}

pub struct SqliteTimerRepository {
    pool: SqlitePool,
}

impl SqliteTimerRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

async fn fetch(conn: &mut SqliteConnection, id: &str) -> DomainResult<Option<Timer>> {
    Ok(sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {TIMER_COLS} FROM timers WHERE id = ?"
    )))
    .bind(id)
    .fetch_optional(conn)
    .await?)
}

#[async_trait]
impl TimerRepository for SqliteTimerRepository {
    async fn create(&self, t: &NewTimer, actor: Actor) -> DomainResult<Timer> {
        let mut tx = self.pool.begin().await?;
        let now = now_utc();
        let timer: Timer = sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "INSERT INTO timers (id, kind, label, task_id, duration_ms, phase, state, started_at, segment_started_at, accumulated_ms, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, 'running', ?, ?, 0, ?, ?) RETURNING {TIMER_COLS}"
        )))
        .bind(uuid::Uuid::new_v4().to_string())
        .bind(&t.kind)
        .bind(&t.label)
        .bind(&t.task_id)
        .bind(t.duration_ms)
        .bind(&t.phase)
        .bind(&t.started_at)
        .bind(&t.started_at)
        .bind(&now)
        .bind(&now)
        .fetch_one(&mut *tx)
        .await?;
        log_change::<(), _>(
            &mut tx,
            "timer",
            &timer.id,
            "create",
            actor,
            None,
            Some(&timer),
            None,
        )
        .await?;
        tx.commit().await?;
        Ok(timer)
    }

    async fn get(&self, id: &str) -> DomainResult<Option<Timer>> {
        let mut c = self.pool.acquire().await?;
        fetch(&mut c, id).await
    }

    async fn transition(
        &self,
        id: &str,
        change: &TimerChange,
        session: Option<&NewSession>,
        actor: Actor,
    ) -> DomainResult<Timer> {
        let mut tx = self.pool.begin().await?;
        let before = fetch(&mut tx, id).await?.ok_or(DomainError::NotFound {
            entity: "timer",
            id: id.to_string(),
        })?;
        let after: Option<Timer> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "UPDATE timers SET state = ?, segment_started_at = ?, accumulated_ms = ?, ended_at = ?, updated_at = ?
             WHERE id = ? AND state = ? RETURNING {TIMER_COLS}"
        )))
        .bind(change.state)
        .bind(&change.segment_started_at)
        .bind(change.accumulated_ms)
        .bind(&change.ended_at)
        .bind(now_utc())
        .bind(id)
        .bind(change.expected_state)
        .fetch_optional(&mut *tx)
        .await?;
        let after = after.ok_or_else(|| {
            DomainError::Validation(format!("timer is no longer {}", change.expected_state))
        })?;
        log_change(
            &mut tx,
            "timer",
            id,
            "update",
            actor,
            Some(&before),
            Some(&after),
            None,
        )
        .await?;
        if let Some(s) = session {
            let fs: FocusSession = sqlx::query_as(sqlx::AssertSqlSafe(format!(
                "INSERT INTO focus_sessions (id, timer_id, task_id, started_at, ended_at, duration_ms, created_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?) RETURNING {SESSION_COLS}"
            )))
            .bind(uuid::Uuid::new_v4().to_string())
            .bind(id)
            .bind(&s.task_id)
            .bind(&s.started_at)
            .bind(&s.ended_at)
            .bind(s.duration_ms)
            .bind(now_utc())
            .fetch_one(&mut *tx)
            .await?;
            log_change::<(), _>(
                &mut tx,
                "focus_session",
                &fs.id,
                "create",
                actor,
                None,
                Some(&fs),
                None,
            )
            .await?;
        }
        tx.commit().await?;
        Ok(after)
    }

    async fn list_by_states(&self, states: &[&str]) -> DomainResult<Vec<Timer>> {
        let sql = format!(
            "SELECT {TIMER_COLS} FROM timers WHERE state IN ({}) ORDER BY started_at, rowid",
            vec!["?"; states.len()].join(",")
        );
        let mut q = sqlx::query_as::<_, Timer>(sqlx::AssertSqlSafe(sql));
        for s in states {
            q = q.bind(s.to_string());
        }
        Ok(q.fetch_all(&self.pool).await?)
    }

    async fn recent_finished(&self, limit: i64) -> DomainResult<Vec<Timer>> {
        Ok(sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "SELECT {TIMER_COLS} FROM timers WHERE state = 'finished' ORDER BY ended_at DESC, rowid DESC LIMIT ?"
        )))
        .bind(limit)
        .fetch_all(&self.pool)
        .await?)
    }

    async fn sessions(
        &self,
        from: &str,
        to: &str,
        task_id: Option<&str>,
    ) -> DomainResult<Vec<FocusSession>> {
        let mut sql = format!(
            "SELECT {SESSION_COLS} FROM focus_sessions WHERE started_at >= ? AND started_at < ?"
        );
        if task_id.is_some() {
            sql.push_str(" AND task_id = ?");
        }
        sql.push_str(" ORDER BY started_at, rowid");
        let mut q = sqlx::query_as::<_, FocusSession>(sqlx::AssertSqlSafe(sql))
            .bind(from)
            .bind(to);
        if let Some(t) = task_id {
            q = q.bind(t.to_string());
        }
        Ok(q.fetch_all(&self.pool).await?)
    }

    async fn set_session_task(
        &self,
        session_id: &str,
        task_id: Option<&str>,
        actor: Actor,
    ) -> DomainResult<FocusSession> {
        let mut tx = self.pool.begin().await?;
        let before: FocusSession = sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "SELECT {SESSION_COLS} FROM focus_sessions WHERE id = ?"
        )))
        .bind(session_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(DomainError::NotFound {
            entity: "focus_session",
            id: session_id.to_string(),
        })?;
        let after: FocusSession = sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "UPDATE focus_sessions SET task_id = ? WHERE id = ? RETURNING {SESSION_COLS}"
        )))
        .bind(task_id)
        .bind(session_id)
        .fetch_one(&mut *tx)
        .await?;
        log_change(
            &mut tx,
            "focus_session",
            session_id,
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

    async fn task_exists(&self, task_id: &str) -> DomainResult<bool> {
        let r: Option<(i64,)> =
            sqlx::query_as("SELECT 1 FROM tasks WHERE id = ? AND deleted_at IS NULL")
                .bind(task_id)
                .fetch_optional(&self.pool)
                .await?;
        Ok(r.is_some())
    }
}
