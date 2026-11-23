//! SQLite access. All SQL lives in this module tree (repositories).

mod activity_repo;
mod ai_repo;
mod automation_repo;
mod calendar_repo;
mod external_repo;
mod inbox_repo;
mod note_repo;
mod project_repo;
mod search_repo;
mod settings_repo;
mod stats_repo;
mod task_repo;
mod template_repo;
mod timer_repo;

use std::path::Path;
use std::str::FromStr;
use std::time::Duration;

use sqlx::SqlitePool;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};

pub use activity_repo::{ActivityRepository, SqliteActivityRepository};
pub use ai_repo::{AiRepository, SqliteAiRepository, UndoReport};
pub use automation_repo::{
    AutomationRepository, FiredRepository, RuleWrite, SqliteAutomationRepository,
    SqliteFiredRepository,
};
pub use calendar_repo::{CalendarRepository, ImportEvent, SqliteCalendarRepository};
pub use external_repo::{ExternalCalendarRepository, SqliteExternalCalendarRepository};
pub use inbox_repo::{InboxRepository, SqliteInboxRepository};
pub use note_repo::{NoteRepository, SqliteNoteRepository};
pub use project_repo::{ProjectRepository, SqliteProjectRepository};
pub use search_repo::{SearchRepository, SqliteSearchRepository};
pub use settings_repo::{SettingsRepository, SqliteSettingsRepository};
pub use stats_repo::{SqliteStatsRepository, StatsRepository};
pub use task_repo::{SqliteTaskRepository, TaskRepository};
pub use template_repo::{SqliteTemplateRepository, TemplateRepository};
pub use timer_repo::{SqliteTimerRepository, TimerRepository};

use crate::error::DomainResult;

pub static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

fn base_options(opts: SqliteConnectOptions) -> SqliteConnectOptions {
    opts.foreign_keys(true).busy_timeout(Duration::from_secs(5))
}

/// Open (creating if needed) the on-disk database in WAL mode with FKs on, and migrate it.
pub async fn open(path: &Path) -> DomainResult<SqlitePool> {
    let opts = base_options(
        SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true),
    )
    .journal_mode(SqliteJournalMode::Wal)
    .synchronous(SqliteSynchronous::Normal);
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(opts)
        .await?;
    MIGRATOR
        .run(&pool)
        .await
        .map_err(|e| crate::error::DomainError::Storage(e.to_string()))?;
    Ok(pool)
}

/// Single-connection in-memory database (each pooled connection would otherwise get its own DB).
pub async fn open_in_memory() -> DomainResult<SqlitePool> {
    let opts = base_options(SqliteConnectOptions::from_str("sqlite::memory:")?);
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(opts)
        .await?;
    MIGRATOR
        .run(&pool)
        .await
        .map_err(|e| crate::error::DomainError::Storage(e.to_string()))?;
    Ok(pool)
}

pub(crate) fn now_utc() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn on_disk_db_uses_wal_and_migrates() {
        let dir = std::env::temp_dir().join(format!("orbit-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let pool = open(&dir.join("t.db")).await.unwrap();
        let mode: (String,) = sqlx::query_as("PRAGMA journal_mode")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(mode.0, "wal");
        let fk: (i64,) = sqlx::query_as("PRAGMA foreign_keys")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(fk.0, 1);
        pool.close().await;
        let _ = std::fs::remove_dir_all(dir);
    }
}
