//! Whole-database snapshots (backups, export) and validation of database files before import.
//! SQL stays here; the service decides when and where.

use std::path::Path;
use std::str::FromStr;

use sqlx::SqlitePool;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

use super::MIGRATOR;
use crate::domain::validation::invalid;
use crate::error::{DomainError, DomainResult};

/// Write a consistent copy of the live database to `dest` (which must not exist). Safe while the
/// app is running: `VACUUM INTO` reads a snapshot and also leaves out free pages.
pub async fn snapshot(pool: &SqlitePool, dest: &Path) -> DomainResult<()> {
    let dest = dest
        .to_str()
        .ok_or_else(|| invalid("the path must be valid UTF-8"))?;
    if dest.contains('\0') {
        return Err(invalid("invalid path"));
    }
    // VACUUM does not accept a bound parameter (it would silently vacuum in place), so the path
    // goes in as an escaped string literal.
    let sql = format!("VACUUM INTO '{}'", dest.replace('\'', "''"));
    sqlx::query(sqlx::AssertSqlSafe(sql)).execute(pool).await?;
    if !Path::new(dest).is_file() {
        return Err(DomainError::Storage("the snapshot was not written".into()));
    }
    Ok(())
}

/// Check that `path` is an intact Orbit database this version can open. Returns its schema version.
pub async fn verify(path: &Path) -> DomainResult<i64> {
    let opts = SqliteConnectOptions::from_str(&format!("sqlite://{}", path.display()))
        .map_err(|e| invalid(format!("bad path: {e}")))?
        .read_only(true)
        .create_if_missing(false);
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(opts)
        .await
        .map_err(|_| invalid("that file is not a readable SQLite database"))?;
    let result = verify_open(&pool).await;
    pool.close().await;
    result
}

async fn verify_open(pool: &SqlitePool) -> DomainResult<i64> {
    let check: (String,) = sqlx::query_as("PRAGMA integrity_check")
        .fetch_one(pool)
        .await
        .map_err(|_| invalid("that file is not a readable SQLite database"))?;
    if check.0 != "ok" {
        return Err(invalid(
            "the database file is damaged (integrity check failed)",
        ));
    }
    let version: (Option<i64>,) = sqlx::query_as("SELECT MAX(version) FROM _sqlx_migrations")
        .fetch_one(pool)
        .await
        .map_err(|_| invalid("that file is not an Orbit database"))?;
    let version = version
        .0
        .ok_or_else(|| invalid("that file is not an Orbit database"))?;
    let ours = MIGRATOR.iter().map(|m| m.version).max().unwrap_or(0);
    if version > ours {
        return Err(DomainError::Validation(
            "that database was made by a newer version of Orbit; update Orbit first".into(),
        ));
    }
    Ok(version)
}
