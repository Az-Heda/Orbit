use async_trait::async_trait;
use sqlx::SqlitePool;

use crate::domain::stats::FocusRow;
use crate::error::DomainResult;

#[async_trait]
pub trait StatsRepository: Send + Sync {
    /// Focus sessions starting in [from, to), with their project (via task -> column -> board).
    async fn focus_rows(&self, from: &str, to: &str) -> DomainResult<Vec<FocusRow>>;
    /// `completed_at` of live tasks completed in [from, to).
    async fn completions(&self, from: &str, to: &str) -> DomainResult<Vec<String>>;
}

pub struct SqliteStatsRepository {
    pool: SqlitePool,
}

impl SqliteStatsRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl StatsRepository for SqliteStatsRepository {
    async fn focus_rows(&self, from: &str, to: &str) -> DomainResult<Vec<FocusRow>> {
        Ok(sqlx::query_as(
            "SELECT s.started_at, s.duration_ms, p.id AS project_id, p.name AS project_name
             FROM focus_sessions s
             LEFT JOIN tasks t ON t.id = s.task_id
             LEFT JOIN board_columns c ON c.id = t.column_id
             LEFT JOIN boards b ON b.id = c.board_id
             LEFT JOIN projects p ON p.id = b.project_id AND p.deleted_at IS NULL
             WHERE s.started_at >= ? AND s.started_at < ?",
        )
        .bind(from)
        .bind(to)
        .fetch_all(&self.pool)
        .await?)
    }

    async fn completions(&self, from: &str, to: &str) -> DomainResult<Vec<String>> {
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT completed_at FROM tasks
             WHERE deleted_at IS NULL AND completed_at IS NOT NULL AND completed_at >= ? AND completed_at < ?",
        )
        .bind(from)
        .bind(to)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|r| r.0).collect())
    }
}
