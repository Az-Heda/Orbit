use async_trait::async_trait;
use sqlx::{SqliteConnection, SqlitePool};

use super::activity_repo::log_change;
use super::now_utc;
use crate::domain::activity::Actor;
use crate::domain::kanban::{Board, Column, ColumnKind, Project};
use crate::error::{DomainError, DomainResult};

const PROJECT_COLS: &str = "id, name, description, color, created_at, updated_at";
const BOARD_COLS: &str = "id, project_id, name, position, created_at, updated_at";
const COLUMN_COLS: &str = "id, board_id, name, kind, position, created_at, updated_at";

#[async_trait]
pub trait ProjectRepository: Send + Sync {
    /// Creates the project plus a default board with Backlog / In progress / Done columns.
    async fn create_project(
        &self,
        name: &str,
        description: Option<&str>,
        color: Option<&str>,
        actor: Actor,
    ) -> DomainResult<Project>;
    async fn get_project(&self, id: &str) -> DomainResult<Option<Project>>;
    async fn list_projects(&self) -> DomainResult<Vec<Project>>;
    async fn update_project(
        &self,
        id: &str,
        name: Option<&str>,
        description: Option<Option<&str>>,
        color: Option<Option<&str>>,
        actor: Actor,
    ) -> DomainResult<Project>;
    async fn delete_project(&self, id: &str, actor: Actor) -> DomainResult<()>;

    async fn create_board(&self, project_id: &str, name: &str, actor: Actor)
    -> DomainResult<Board>;
    async fn get_board(&self, id: &str) -> DomainResult<Option<Board>>;
    async fn list_boards(&self, project_id: &str) -> DomainResult<Vec<Board>>;
    async fn update_board(&self, id: &str, name: &str, actor: Actor) -> DomainResult<Board>;
    async fn delete_board(&self, id: &str, actor: Actor) -> DomainResult<()>;

    async fn create_column(
        &self,
        board_id: &str,
        name: &str,
        kind: ColumnKind,
        actor: Actor,
    ) -> DomainResult<Column>;
    async fn get_column(&self, id: &str) -> DomainResult<Option<Column>>;
    async fn list_columns(&self, board_id: &str) -> DomainResult<Vec<Column>>;
    async fn update_column(
        &self,
        id: &str,
        name: Option<&str>,
        kind: Option<ColumnKind>,
        actor: Actor,
    ) -> DomainResult<Column>;
    async fn move_column(&self, id: &str, position: i64, actor: Actor) -> DomainResult<Column>;
    async fn delete_column(&self, id: &str, actor: Actor) -> DomainResult<()>;
}

pub struct SqliteProjectRepository {
    pool: SqlitePool,
}

impl SqliteProjectRepository {
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

async fn fetch_project(conn: &mut SqliteConnection, id: &str) -> DomainResult<Option<Project>> {
    Ok(sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {PROJECT_COLS} FROM projects WHERE id = ? AND deleted_at IS NULL"
    )))
    .bind(id)
    .fetch_optional(conn)
    .await?)
}

async fn fetch_board(conn: &mut SqliteConnection, id: &str) -> DomainResult<Option<Board>> {
    Ok(sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {BOARD_COLS} FROM boards WHERE id = ? AND deleted_at IS NULL"
    )))
    .bind(id)
    .fetch_optional(conn)
    .await?)
}

async fn fetch_column(conn: &mut SqliteConnection, id: &str) -> DomainResult<Option<Column>> {
    Ok(sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLUMN_COLS} FROM board_columns WHERE id = ? AND deleted_at IS NULL"
    )))
    .bind(id)
    .fetch_optional(conn)
    .await?)
}

async fn insert_board(
    conn: &mut SqliteConnection,
    project_id: &str,
    name: &str,
    actor: Actor,
) -> DomainResult<Board> {
    let now = now_utc();
    let pos: (i64,) = sqlx::query_as(
        "SELECT COALESCE(MAX(position), -1) + 1 FROM boards WHERE project_id = ? AND deleted_at IS NULL",
    )
    .bind(project_id)
    .fetch_one(&mut *conn)
    .await?;
    let board: Board = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "INSERT INTO boards (id, project_id, name, position, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?) RETURNING {BOARD_COLS}"
    )))
    .bind(uuid::Uuid::new_v4().to_string())
    .bind(project_id)
    .bind(name)
    .bind(pos.0)
    .bind(&now)
    .bind(&now)
    .fetch_one(&mut *conn)
    .await?;
    log_change::<(), _>(
        conn,
        "board",
        &board.id,
        "create",
        actor,
        None,
        Some(&board),
        None,
    )
    .await?;
    for (i, (cname, kind)) in [
        ("Backlog", ColumnKind::Backlog),
        ("In progress", ColumnKind::Active),
        ("Done", ColumnKind::Done),
    ]
    .into_iter()
    .enumerate()
    {
        insert_column(conn, &board.id, cname, kind, Some(i as i64), actor).await?;
    }
    Ok(board)
}

async fn insert_column(
    conn: &mut SqliteConnection,
    board_id: &str,
    name: &str,
    kind: ColumnKind,
    position: Option<i64>,
    actor: Actor,
) -> DomainResult<Column> {
    let now = now_utc();
    let pos = match position {
        Some(p) => p,
        None => {
            sqlx::query_as::<_, (i64,)>(
                "SELECT COALESCE(MAX(position), -1) + 1 FROM board_columns WHERE board_id = ? AND deleted_at IS NULL",
            )
            .bind(board_id)
            .fetch_one(&mut *conn)
            .await?
            .0
        }
    };
    let col: Column = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "INSERT INTO board_columns (id, board_id, name, kind, position, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?) RETURNING {COLUMN_COLS}"
    )))
    .bind(uuid::Uuid::new_v4().to_string())
    .bind(board_id)
    .bind(name)
    .bind(kind.as_str())
    .bind(pos)
    .bind(&now)
    .bind(&now)
    .fetch_one(&mut *conn)
    .await?;
    log_change::<(), _>(
        conn,
        "column",
        &col.id,
        "create",
        actor,
        None,
        Some(&col),
        None,
    )
    .await?;
    Ok(col)
}

/// Soft-delete a board's columns and tasks (used by board and project deletion).
async fn cascade_board(conn: &mut SqliteConnection, board_id: &str, now: &str) -> DomainResult<()> {
    sqlx::query(
        "UPDATE tasks SET deleted_at = ? WHERE deleted_at IS NULL
         AND column_id IN (SELECT id FROM board_columns WHERE board_id = ?)",
    )
    .bind(now)
    .bind(board_id)
    .execute(&mut *conn)
    .await?;
    sqlx::query(
        "UPDATE board_columns SET deleted_at = ? WHERE board_id = ? AND deleted_at IS NULL",
    )
    .bind(now)
    .bind(board_id)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

#[async_trait]
impl ProjectRepository for SqliteProjectRepository {
    async fn create_project(
        &self,
        name: &str,
        description: Option<&str>,
        color: Option<&str>,
        actor: Actor,
    ) -> DomainResult<Project> {
        let mut tx = self.pool.begin().await?;
        let now = now_utc();
        let project: Project = sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "INSERT INTO projects (id, name, description, color, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?) RETURNING {PROJECT_COLS}"
        )))
        .bind(uuid::Uuid::new_v4().to_string())
        .bind(name)
        .bind(description)
        .bind(color)
        .bind(&now)
        .bind(&now)
        .fetch_one(&mut *tx)
        .await?;
        log_change::<(), _>(
            &mut tx,
            "project",
            &project.id,
            "create",
            actor,
            None,
            Some(&project),
            None,
        )
        .await?;
        insert_board(&mut tx, &project.id, "Main", actor).await?;
        tx.commit().await?;
        Ok(project)
    }

    async fn get_project(&self, id: &str) -> DomainResult<Option<Project>> {
        let mut c = self.pool.acquire().await?;
        fetch_project(&mut c, id).await
    }

    async fn list_projects(&self) -> DomainResult<Vec<Project>> {
        Ok(sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "SELECT {PROJECT_COLS} FROM projects WHERE deleted_at IS NULL ORDER BY created_at, rowid"
        )))
        .fetch_all(&self.pool)
        .await?)
    }

    async fn update_project(
        &self,
        id: &str,
        name: Option<&str>,
        description: Option<Option<&str>>,
        color: Option<Option<&str>>,
        actor: Actor,
    ) -> DomainResult<Project> {
        let mut tx = self.pool.begin().await?;
        let before = fetch_project(&mut tx, id)
            .await?
            .ok_or_else(|| not_found("project", id))?;
        let name = name.unwrap_or(&before.name);
        let description = description.unwrap_or(before.description.as_deref());
        let color = color.unwrap_or(before.color.as_deref());
        let after: Project = sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "UPDATE projects SET name = ?, description = ?, color = ?, updated_at = ?
             WHERE id = ? RETURNING {PROJECT_COLS}"
        )))
        .bind(name)
        .bind(description)
        .bind(color)
        .bind(now_utc())
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
        log_change(
            &mut tx,
            "project",
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

    async fn delete_project(&self, id: &str, actor: Actor) -> DomainResult<()> {
        let mut tx = self.pool.begin().await?;
        let before = fetch_project(&mut tx, id)
            .await?
            .ok_or_else(|| not_found("project", id))?;
        let now = now_utc();
        let boards: Vec<(String,)> =
            sqlx::query_as("SELECT id FROM boards WHERE project_id = ? AND deleted_at IS NULL")
                .bind(id)
                .fetch_all(&mut *tx)
                .await?;
        for (bid,) in boards {
            cascade_board(&mut tx, &bid, &now).await?;
        }
        sqlx::query("UPDATE boards SET deleted_at = ? WHERE project_id = ? AND deleted_at IS NULL")
            .bind(&now)
            .bind(id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("UPDATE projects SET deleted_at = ?, updated_at = ? WHERE id = ?")
            .bind(&now)
            .bind(&now)
            .bind(id)
            .execute(&mut *tx)
            .await?;
        log_change::<_, ()>(
            &mut tx,
            "project",
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

    async fn create_board(
        &self,
        project_id: &str,
        name: &str,
        actor: Actor,
    ) -> DomainResult<Board> {
        let mut tx = self.pool.begin().await?;
        fetch_project(&mut tx, project_id)
            .await?
            .ok_or_else(|| not_found("project", project_id))?;
        let board = insert_board(&mut tx, project_id, name, actor).await?;
        tx.commit().await?;
        Ok(board)
    }

    async fn get_board(&self, id: &str) -> DomainResult<Option<Board>> {
        let mut c = self.pool.acquire().await?;
        fetch_board(&mut c, id).await
    }

    async fn list_boards(&self, project_id: &str) -> DomainResult<Vec<Board>> {
        Ok(sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "SELECT {BOARD_COLS} FROM boards WHERE project_id = ? AND deleted_at IS NULL ORDER BY position, rowid"
        )))
        .bind(project_id)
        .fetch_all(&self.pool)
        .await?)
    }

    async fn update_board(&self, id: &str, name: &str, actor: Actor) -> DomainResult<Board> {
        let mut tx = self.pool.begin().await?;
        let before = fetch_board(&mut tx, id)
            .await?
            .ok_or_else(|| not_found("board", id))?;
        let after: Board = sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "UPDATE boards SET name = ?, updated_at = ? WHERE id = ? RETURNING {BOARD_COLS}"
        )))
        .bind(name)
        .bind(now_utc())
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
        log_change(
            &mut tx,
            "board",
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

    async fn delete_board(&self, id: &str, actor: Actor) -> DomainResult<()> {
        let mut tx = self.pool.begin().await?;
        let before = fetch_board(&mut tx, id)
            .await?
            .ok_or_else(|| not_found("board", id))?;
        let now = now_utc();
        cascade_board(&mut tx, id, &now).await?;
        sqlx::query("UPDATE boards SET deleted_at = ?, updated_at = ? WHERE id = ?")
            .bind(&now)
            .bind(&now)
            .bind(id)
            .execute(&mut *tx)
            .await?;
        log_change::<_, ()>(
            &mut tx,
            "board",
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

    async fn create_column(
        &self,
        board_id: &str,
        name: &str,
        kind: ColumnKind,
        actor: Actor,
    ) -> DomainResult<Column> {
        let mut tx = self.pool.begin().await?;
        fetch_board(&mut tx, board_id)
            .await?
            .ok_or_else(|| not_found("board", board_id))?;
        let col = insert_column(&mut tx, board_id, name, kind, None, actor).await?;
        tx.commit().await?;
        Ok(col)
    }

    async fn get_column(&self, id: &str) -> DomainResult<Option<Column>> {
        let mut c = self.pool.acquire().await?;
        fetch_column(&mut c, id).await
    }

    async fn list_columns(&self, board_id: &str) -> DomainResult<Vec<Column>> {
        Ok(sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "SELECT {COLUMN_COLS} FROM board_columns WHERE board_id = ? AND deleted_at IS NULL ORDER BY position, rowid"
        )))
        .bind(board_id)
        .fetch_all(&self.pool)
        .await?)
    }

    async fn update_column(
        &self,
        id: &str,
        name: Option<&str>,
        kind: Option<ColumnKind>,
        actor: Actor,
    ) -> DomainResult<Column> {
        let mut tx = self.pool.begin().await?;
        let before = fetch_column(&mut tx, id)
            .await?
            .ok_or_else(|| not_found("column", id))?;
        let new_kind: String = kind.map_or_else(|| before.kind.clone(), |k| k.as_str().to_string());
        let after: Column = sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "UPDATE board_columns SET name = ?, kind = ?, updated_at = ? WHERE id = ? RETURNING {COLUMN_COLS}"
        )))
        .bind(name.unwrap_or(&before.name))
        .bind(new_kind)
        .bind(now_utc())
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
        // Keep completed_at consistent if the column's meaning changed.
        if before.kind != after.kind {
            if after.kind == "done" {
                sqlx::query("UPDATE tasks SET completed_at = ? WHERE column_id = ? AND completed_at IS NULL AND deleted_at IS NULL")
                    .bind(now_utc())
                    .bind(id)
                    .execute(&mut *tx)
                    .await?;
            } else if before.kind == "done" {
                sqlx::query("UPDATE tasks SET completed_at = NULL WHERE column_id = ? AND deleted_at IS NULL")
                    .bind(id)
                    .execute(&mut *tx)
                    .await?;
            }
        }
        log_change(
            &mut tx,
            "column",
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

    async fn move_column(&self, id: &str, position: i64, actor: Actor) -> DomainResult<Column> {
        let mut tx = self.pool.begin().await?;
        let before = fetch_column(&mut tx, id)
            .await?
            .ok_or_else(|| not_found("column", id))?;
        sqlx::query(
            "UPDATE board_columns SET position = position + 1
             WHERE board_id = ? AND id <> ? AND position >= ? AND deleted_at IS NULL",
        )
        .bind(&before.board_id)
        .bind(id)
        .bind(position)
        .execute(&mut *tx)
        .await?;
        let after: Column = sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "UPDATE board_columns SET position = ?, updated_at = ? WHERE id = ? RETURNING {COLUMN_COLS}"
        )))
        .bind(position)
        .bind(now_utc())
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
        log_change(
            &mut tx,
            "column",
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

    async fn delete_column(&self, id: &str, actor: Actor) -> DomainResult<()> {
        let mut tx = self.pool.begin().await?;
        let before = fetch_column(&mut tx, id)
            .await?
            .ok_or_else(|| not_found("column", id))?;
        let n: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM tasks WHERE column_id = ? AND deleted_at IS NULL")
                .bind(id)
                .fetch_one(&mut *tx)
                .await?;
        if n.0 > 0 {
            return Err(DomainError::Validation(
                "column still contains tasks; move or delete them first".into(),
            ));
        }
        let now = now_utc();
        sqlx::query("UPDATE board_columns SET deleted_at = ?, updated_at = ? WHERE id = ?")
            .bind(&now)
            .bind(&now)
            .bind(id)
            .execute(&mut *tx)
            .await?;
        log_change::<_, ()>(
            &mut tx,
            "column",
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
