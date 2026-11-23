use async_trait::async_trait;
use sqlx::SqlitePool;

use crate::domain::search::{HitRow, MARK_CLOSE, MARK_OPEN, SearchHit};
use crate::error::DomainResult;

#[async_trait]
pub trait SearchRepository: Send + Sync {
    /// `expr` must already be a sanitised FTS5 expression. `kinds` are valid kind names.
    async fn search(
        &self,
        expr: &str,
        kinds: &[&'static str],
        limit: i64,
    ) -> DomainResult<Vec<SearchHit>>;
}

pub struct SqliteSearchRepository {
    pool: SqlitePool,
}

impl SqliteSearchRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

fn sql_for(kind: &str) -> String {
    let snip = |fts: &str| format!("snippet({fts}, -1, '{MARK_OPEN}', '{MARK_CLOSE}', '…', 12)");
    match kind {
        "task" => format!(
            "SELECT t.id, t.title, {} AS snippet, bm25(tasks_fts) AS rank, b.project_id AS project_id, b.id AS board_id, NULL AS date
             FROM tasks_fts JOIN tasks t ON t.rowid = tasks_fts.rowid
             JOIN board_columns c ON c.id = t.column_id AND c.deleted_at IS NULL
             JOIN boards b ON b.id = c.board_id AND b.deleted_at IS NULL
             WHERE tasks_fts MATCH ?1 AND t.deleted_at IS NULL ORDER BY rank LIMIT ?2",
            snip("tasks_fts")
        ),
        "event" => format!(
            "SELECT e.id, e.title, {} AS snippet, bm25(events_fts) AS rank, NULL AS project_id, NULL AS board_id, e.start_at AS date
             FROM events_fts JOIN events e ON e.rowid = events_fts.rowid
             WHERE events_fts MATCH ?1 AND e.deleted_at IS NULL ORDER BY rank LIMIT ?2",
            snip("events_fts")
        ),
        "note" => format!(
            "SELECT n.id, n.title, {} AS snippet, bm25(notes_fts) AS rank, NULL AS project_id, NULL AS board_id, n.journal_date AS date
             FROM notes_fts JOIN notes n ON n.rowid = notes_fts.rowid
             WHERE notes_fts MATCH ?1 AND n.deleted_at IS NULL ORDER BY rank LIMIT ?2",
            snip("notes_fts")
        ),
        "project" => format!(
            "SELECT p.id, p.name AS title, {} AS snippet, bm25(projects_fts) AS rank, p.id AS project_id, NULL AS board_id, NULL AS date
             FROM projects_fts JOIN projects p ON p.rowid = projects_fts.rowid
             WHERE projects_fts MATCH ?1 AND p.deleted_at IS NULL ORDER BY rank LIMIT ?2",
            snip("projects_fts")
        ),
        _ => format!(
            "SELECT i.id, substr(i.raw_text, 1, 80) AS title, {} AS snippet, bm25(inbox_fts) AS rank, NULL AS project_id, NULL AS board_id, NULL AS date
             FROM inbox_fts JOIN inbox_items i ON i.rowid = inbox_fts.rowid
             WHERE inbox_fts MATCH ?1 AND i.deleted_at IS NULL ORDER BY rank LIMIT ?2",
            snip("inbox_fts")
        ),
    }
}

#[async_trait]
impl SearchRepository for SqliteSearchRepository {
    async fn search(
        &self,
        expr: &str,
        kinds: &[&'static str],
        limit: i64,
    ) -> DomainResult<Vec<SearchHit>> {
        let mut hits = Vec::new();
        for kind in kinds {
            let rows: Vec<HitRow> = sqlx::query_as(sqlx::AssertSqlSafe(sql_for(kind)))
                .bind(expr)
                .bind(limit)
                .fetch_all(&self.pool)
                .await?;
            hits.extend(rows.into_iter().map(|r| SearchHit {
                kind,
                id: r.id,
                title: r.title,
                snippet: r.snippet,
                score: -r.rank,
                project_id: r.project_id,
                board_id: r.board_id,
                date: r.date,
            }));
        }
        Ok(hits)
    }
}
