use async_trait::async_trait;
use sqlx::{SqliteConnection, SqlitePool};

use super::activity_repo::log_change;
use super::now_utc;
use crate::domain::activity::Actor;
use crate::domain::notes::{NOTE_COLS, Note, NoteLink};
use crate::error::{DomainError, DomainResult};

#[async_trait]
pub trait NoteRepository: Send + Sync {
    async fn create(&self, title: &str, body: &str, actor: Actor) -> DomainResult<Note>;
    async fn get(&self, id: &str) -> DomainResult<Option<Note>>;
    async fn update(
        &self,
        id: &str,
        title: Option<&str>,
        body: Option<&str>,
        actor: Actor,
    ) -> DomainResult<Note>;
    async fn delete(&self, id: &str, actor: Actor) -> DomainResult<()>;
    /// Most recently updated first. Optionally only notes linked to an entity, or only a kind.
    async fn list(
        &self,
        kind: Option<&str>,
        entity: Option<(&str, &str)>,
        limit: i64,
    ) -> DomainResult<Vec<Note>>;
    /// Get or create the journal entry for a local date (yyyy-mm-dd).
    async fn journal(&self, date: &str, actor: Actor) -> DomainResult<Note>;
    async fn add_link(
        &self,
        note_id: &str,
        entity_type: &str,
        entity_id: &str,
        actor: Actor,
    ) -> DomainResult<()>;
    async fn remove_link(
        &self,
        note_id: &str,
        entity_type: &str,
        entity_id: &str,
        actor: Actor,
    ) -> DomainResult<()>;
    async fn links(&self, note_id: &str) -> DomainResult<Vec<NoteLink>>;
}

pub struct SqliteNoteRepository {
    pool: SqlitePool,
}

impl SqliteNoteRepository {
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

async fn fetch(conn: &mut SqliteConnection, id: &str) -> DomainResult<Option<Note>> {
    Ok(sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {NOTE_COLS} FROM notes WHERE id = ? AND deleted_at IS NULL"
    )))
    .bind(id)
    .fetch_optional(conn)
    .await?)
}

async fn insert(
    conn: &mut SqliteConnection,
    title: &str,
    body: &str,
    kind: &str,
    journal_date: Option<&str>,
    actor: Actor,
) -> DomainResult<Note> {
    let now = now_utc();
    let note: Note = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "INSERT INTO notes (id, title, body, kind, journal_date, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?) RETURNING {NOTE_COLS}"
    )))
    .bind(uuid::Uuid::new_v4().to_string())
    .bind(title)
    .bind(body)
    .bind(kind)
    .bind(journal_date)
    .bind(&now)
    .bind(&now)
    .fetch_one(&mut *conn)
    .await?;
    log_change::<(), _>(
        conn,
        "note",
        &note.id,
        "create",
        actor,
        None,
        Some(&note),
        None,
    )
    .await?;
    Ok(note)
}

fn table_for(entity_type: &str) -> DomainResult<&'static str> {
    match entity_type {
        "task" => Ok("tasks"),
        "event" => Ok("events"),
        "project" => Ok("projects"),
        _ => Err(DomainError::Validation("unknown entity type".into())),
    }
}

#[async_trait]
impl NoteRepository for SqliteNoteRepository {
    async fn create(&self, title: &str, body: &str, actor: Actor) -> DomainResult<Note> {
        let mut tx = self.pool.begin().await?;
        let n = insert(&mut tx, title, body, "note", None, actor).await?;
        tx.commit().await?;
        Ok(n)
    }

    async fn get(&self, id: &str) -> DomainResult<Option<Note>> {
        let mut c = self.pool.acquire().await?;
        fetch(&mut c, id).await
    }

    async fn update(
        &self,
        id: &str,
        title: Option<&str>,
        body: Option<&str>,
        actor: Actor,
    ) -> DomainResult<Note> {
        let mut tx = self.pool.begin().await?;
        let before = fetch(&mut tx, id)
            .await?
            .ok_or_else(|| not_found("note", id))?;
        let after: Note = sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "UPDATE notes SET title = ?, body = ?, updated_at = ? WHERE id = ? RETURNING {NOTE_COLS}"
        )))
        .bind(title.unwrap_or(&before.title))
        .bind(body.unwrap_or(&before.body))
        .bind(now_utc())
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
        log_change(
            &mut tx,
            "note",
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

    async fn delete(&self, id: &str, actor: Actor) -> DomainResult<()> {
        let mut tx = self.pool.begin().await?;
        let before = fetch(&mut tx, id)
            .await?
            .ok_or_else(|| not_found("note", id))?;
        let now = now_utc();
        sqlx::query("UPDATE notes SET deleted_at = ?, updated_at = ? WHERE id = ?")
            .bind(&now)
            .bind(&now)
            .bind(id)
            .execute(&mut *tx)
            .await?;
        log_change::<_, ()>(
            &mut tx,
            "note",
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

    async fn list(
        &self,
        kind: Option<&str>,
        entity: Option<(&str, &str)>,
        limit: i64,
    ) -> DomainResult<Vec<Note>> {
        let mut sql = format!("SELECT {NOTE_COLS} FROM notes n WHERE n.deleted_at IS NULL");
        if kind.is_some() {
            sql.push_str(" AND n.kind = ?");
        }
        if entity.is_some() {
            sql.push_str(" AND n.id IN (SELECT note_id FROM note_links WHERE entity_type = ? AND entity_id = ?)");
        }
        sql.push_str(" ORDER BY n.updated_at DESC, n.rowid DESC LIMIT ?");
        let mut q = sqlx::query_as::<_, Note>(sqlx::AssertSqlSafe(sql));
        if let Some(k) = kind {
            q = q.bind(k.to_string());
        }
        if let Some((t, i)) = entity {
            q = q.bind(t.to_string()).bind(i.to_string());
        }
        Ok(q.bind(limit).fetch_all(&self.pool).await?)
    }

    async fn journal(&self, date: &str, actor: Actor) -> DomainResult<Note> {
        let mut tx = self.pool.begin().await?;
        let existing: Option<Note> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "SELECT {NOTE_COLS} FROM notes WHERE kind = 'journal' AND journal_date = ? AND deleted_at IS NULL"
        )))
        .bind(date)
        .fetch_optional(&mut *tx)
        .await?;
        let note = match existing {
            Some(n) => n,
            None => insert(&mut tx, date, "", "journal", Some(date), actor).await?,
        };
        tx.commit().await?;
        Ok(note)
    }

    async fn add_link(
        &self,
        note_id: &str,
        entity_type: &str,
        entity_id: &str,
        actor: Actor,
    ) -> DomainResult<()> {
        let table = table_for(entity_type)?;
        let mut tx = self.pool.begin().await?;
        fetch(&mut tx, note_id)
            .await?
            .ok_or_else(|| not_found("note", note_id))?;
        let exists: Option<(i64,)> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "SELECT 1 FROM {table} WHERE id = ? AND deleted_at IS NULL"
        )))
        .bind(entity_id)
        .fetch_optional(&mut *tx)
        .await?;
        if exists.is_none() {
            return Err(not_found("link target", entity_id));
        }
        let res = sqlx::query(
            "INSERT OR IGNORE INTO note_links (note_id, entity_type, entity_id, created_at) VALUES (?, ?, ?, ?)",
        )
        .bind(note_id)
        .bind(entity_type)
        .bind(entity_id)
        .bind(now_utc())
        .execute(&mut *tx)
        .await?;
        if res.rows_affected() > 0 {
            let link = serde_json::json!({"note_id": note_id, "entity_type": entity_type, "entity_id": entity_id});
            log_change::<(), _>(
                &mut tx,
                "note_link",
                note_id,
                "create",
                actor,
                None,
                Some(&link),
                None,
            )
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    async fn remove_link(
        &self,
        note_id: &str,
        entity_type: &str,
        entity_id: &str,
        actor: Actor,
    ) -> DomainResult<()> {
        let mut tx = self.pool.begin().await?;
        let res = sqlx::query(
            "DELETE FROM note_links WHERE note_id = ? AND entity_type = ? AND entity_id = ?",
        )
        .bind(note_id)
        .bind(entity_type)
        .bind(entity_id)
        .execute(&mut *tx)
        .await?;
        if res.rows_affected() == 0 {
            return Err(not_found("note_link", entity_id));
        }
        let link = serde_json::json!({"note_id": note_id, "entity_type": entity_type, "entity_id": entity_id});
        log_change::<_, ()>(
            &mut tx,
            "note_link",
            note_id,
            "delete",
            actor,
            Some(&link),
            None,
            None,
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }

    async fn links(&self, note_id: &str) -> DomainResult<Vec<NoteLink>> {
        Ok(sqlx::query_as(
            "SELECT l.entity_type, l.entity_id, COALESCE(t.title, e.title, p.name) AS title
             FROM note_links l
             LEFT JOIN tasks t ON l.entity_type = 'task' AND t.id = l.entity_id AND t.deleted_at IS NULL
             LEFT JOIN events e ON l.entity_type = 'event' AND e.id = l.entity_id AND e.deleted_at IS NULL
             LEFT JOIN projects p ON l.entity_type = 'project' AND p.id = l.entity_id AND p.deleted_at IS NULL
             WHERE l.note_id = ? ORDER BY l.created_at, l.rowid",
        )
        .bind(note_id)
        .fetch_all(&self.pool)
        .await?)
    }
}
