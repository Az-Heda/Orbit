use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::{DateTime, Duration, Utc};
use serde::Serialize;
use sqlx::SqlitePool;

use super::{Clock, SettingsService};
use crate::domain::validation::invalid;
use crate::error::{DomainError, DomainResult};
use crate::infrastructure::database::backup;

const PENDING: &str = "import-pending.db";
const DEFAULT_KEEP: usize = 7;
/// Automatic backups are taken at most this often.
const AUTO_EVERY_HOURS: i64 = 24;

#[derive(Debug, Clone, Serialize)]
pub struct BackupInfo {
    pub name: String,
    /// auto | manual | pre-import | pre-update
    pub kind: String,
    pub size_bytes: u64,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct BackupStatus {
    pub enabled: bool,
    pub keep: usize,
    /// A database import is staged and will replace the data on the next start.
    pub import_pending: bool,
    pub folder: String,
}

/// Local backups, export and import of the whole database. Everything is a SQLite file:
/// a backup is a consistent snapshot, export writes one to a path of the user's choice, and
/// import (or restoring a backup) stages a verified file that replaces the database at the next
/// start, after a safety backup of the current data.
pub struct BackupService {
    pool: SqlitePool,
    dir: PathBuf,
    settings: Arc<SettingsService>,
    clock: Clock,
}

const KINDS: [&str; 4] = ["auto", "manual", "pre-import", "pre-update"];

impl BackupService {
    pub fn new(
        pool: SqlitePool,
        dir: PathBuf,
        settings: Arc<SettingsService>,
        clock: Clock,
    ) -> Self {
        Self {
            pool,
            dir,
            settings,
            clock,
        }
    }

    fn folder(&self) -> PathBuf {
        self.dir.join("backups")
    }

    async fn setting<T: serde::de::DeserializeOwned>(&self, key: &str, default: T) -> T {
        match self.settings.get(key).await {
            Ok(Some(s)) => serde_json::from_str(&s.value).unwrap_or(default),
            _ => default,
        }
    }

    pub async fn status(&self) -> BackupStatus {
        BackupStatus {
            enabled: self.setting("backup.enabled", true).await,
            keep: self.keep().await,
            import_pending: self.dir.join(PENDING).exists(),
            folder: self.folder().display().to_string(),
        }
    }

    async fn keep(&self) -> usize {
        self.setting("backup.keep", DEFAULT_KEEP).await.clamp(1, 60)
    }

    /// Take a snapshot now. `kind` must be one of the known kinds.
    pub async fn backup_now(&self, kind: &str) -> DomainResult<BackupInfo> {
        if !KINDS.contains(&kind) {
            return Err(invalid("unknown backup kind"));
        }
        let folder = self.folder();
        std::fs::create_dir_all(&folder).map_err(io)?;
        let stamp = (self.clock)().format("%Y%m%d-%H%M%S");
        let mut name = format!("orbit-{stamp}-{kind}.db");
        let mut n = 1;
        while folder.join(&name).exists() {
            n += 1;
            name = format!("orbit-{stamp}-{kind}-{n}.db");
        }
        backup::snapshot(&self.pool, &folder.join(&name)).await?;
        self.info(&name)
    }

    fn info(&self, name: &str) -> DomainResult<BackupInfo> {
        let meta = std::fs::metadata(self.folder().join(name)).map_err(io)?;
        // The time is part of the file name (UTC), so ordering and the daily rule do not depend
        // on file times, which copies and restores can change.
        let created: DateTime<Utc> = name
            .get(6..21)
            .and_then(|t| chrono::NaiveDateTime::parse_from_str(t, "%Y%m%d-%H%M%S").ok())
            .map(|t| t.and_utc())
            .map_or_else(|| meta.modified().map(Into::into).map_err(io), Ok)?;
        Ok(BackupInfo {
            name: name.to_string(),
            kind: kind_of(name),
            size_bytes: meta.len(),
            created_at: created.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        })
    }

    /// Newest first.
    pub fn list(&self) -> DomainResult<Vec<BackupInfo>> {
        let folder = self.folder();
        let mut out = vec![];
        let Ok(rd) = std::fs::read_dir(&folder) else {
            return Ok(out);
        };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with("orbit-") && name.ends_with(".db") {
                out.push(self.info(&name)?);
            }
        }
        out.sort_by(|a, b| b.created_at.cmp(&a.created_at).then(b.name.cmp(&a.name)));
        Ok(out)
    }

    /// Delete the oldest automatic backups beyond `keep`. Manual and safety backups stay.
    pub fn prune(&self, keep: usize) -> DomainResult<usize> {
        let autos: Vec<_> = self
            .list()?
            .into_iter()
            .filter(|b| b.kind == "auto")
            .collect();
        let mut removed = 0;
        for old in autos.into_iter().skip(keep) {
            std::fs::remove_file(self.folder().join(&old.name)).map_err(io)?;
            removed += 1;
        }
        Ok(removed)
    }

    /// Called periodically: take an automatic backup if enabled and the last one is old enough.
    pub async fn run_due(&self) -> DomainResult<Option<BackupInfo>> {
        if !self.setting("backup.enabled", true).await {
            return Ok(None);
        }
        let now = (self.clock)();
        let last = self.list()?.into_iter().find(|b| b.kind == "auto");
        if let Some(l) = last
            && DateTime::parse_from_rfc3339(&l.created_at)
                .is_ok_and(|t| now - t.with_timezone(&Utc) < Duration::hours(AUTO_EVERY_HOURS))
        {
            return Ok(None);
        }
        let info = self.backup_now("auto").await?;
        self.prune(self.keep().await)?;
        Ok(Some(info))
    }

    /// Write a snapshot to a path chosen by the user (overwriting a regular file there).
    pub async fn export(&self, dest: &str) -> DomainResult<()> {
        let dest = Path::new(dest);
        if dest.is_dir() || dest.extension().is_none_or(|e| e != "db") {
            return Err(invalid("choose a file name ending in .db"));
        }
        if dest.exists() {
            std::fs::remove_file(dest).map_err(io)?;
        }
        backup::snapshot(&self.pool, dest).await
    }

    /// Verify a database file and stage it to replace the current data at the next start.
    /// A safety backup of the current data is taken first.
    pub async fn stage_import(&self, src: &str) -> DomainResult<()> {
        let src = Path::new(src);
        if !src.is_file() {
            return Err(invalid("that file does not exist"));
        }
        backup::verify(src).await?;
        self.backup_now("pre-import").await?;
        let tmp = self.dir.join(format!("{PENDING}.part"));
        std::fs::copy(src, &tmp).map_err(io)?;
        std::fs::rename(&tmp, self.dir.join(PENDING)).map_err(io)?;
        Ok(())
    }

    /// Stage one of the listed backups (by name) for restore.
    pub async fn stage_restore(&self, name: &str) -> DomainResult<()> {
        if !self.list()?.iter().any(|b| b.name == name) {
            return Err(DomainError::NotFound {
                entity: "backup",
                id: name.to_string(),
            });
        }
        let path = self.folder().join(name);
        self.stage_import(&path.display().to_string()).await
    }

    pub fn cancel_import(&self) -> DomainResult<()> {
        match std::fs::remove_file(self.dir.join(PENDING)) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(io(e)),
        }
    }
}

/// Before the database is opened: swap in a staged import. Returns true when data was replaced.
/// A staged file that no longer verifies is discarded and the current data is kept.
pub async fn apply_pending_import(dir: &Path) -> DomainResult<bool> {
    let pending = dir.join(PENDING);
    if !pending.exists() {
        return Ok(false);
    }
    if backup::verify(&pending).await.is_err() {
        let _ = std::fs::remove_file(&pending);
        return Ok(false);
    }
    let db = dir.join("orbit.db");
    for suffix in ["-wal", "-shm"] {
        let _ = std::fs::remove_file(dir.join(format!("orbit.db{suffix}")));
    }
    std::fs::rename(&pending, &db).map_err(io)?;
    Ok(true)
}

fn kind_of(name: &str) -> String {
    KINDS
        .iter()
        .rev() // "pre-import" before "import"-like prefixes; none overlap, but keep order explicit
        .find(|k| name.contains(&format!("-{k}")))
        .copied()
        .unwrap_or("manual")
        .to_string()
}

fn io(e: std::io::Error) -> DomainError {
    DomainError::Storage(format!("file error: {e}"))
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use std::str::FromStr;

    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

    use super::*;
    use crate::domain::activity::Actor;
    use crate::infrastructure::database::{SqliteSettingsRepository, open};

    struct Env {
        svc: BackupService,
        dir: PathBuf,
        now: Arc<Mutex<DateTime<Utc>>>,
    }

    async fn setup() -> Env {
        let dir = std::env::temp_dir().join(format!("orbit-backup-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let pool = open(&dir.join("orbit.db")).await.unwrap();
        let settings = Arc::new(SettingsService::new(Arc::new(
            SqliteSettingsRepository::new(pool.clone()),
        )));
        let now = Arc::new(Mutex::new(
            "2027-01-10T08:00:00Z".parse::<DateTime<Utc>>().unwrap(),
        ));
        let n = now.clone();
        let clock: Clock = Arc::new(move || *n.lock().unwrap());
        Env {
            svc: BackupService::new(pool, dir.clone(), settings, clock),
            dir,
            now,
        }
    }

    async fn note_count(path: &Path) -> i64 {
        let pool = open(path).await.unwrap();
        let n: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM settings")
            .fetch_one(&pool)
            .await
            .unwrap();
        pool.close().await;
        n.0
    }

    #[tokio::test]
    async fn snapshot_is_an_openable_copy_and_auto_backups_are_rate_limited_and_pruned() {
        let e = setup().await;
        let first = e.svc.run_due().await.unwrap().expect("first auto backup");
        assert_eq!(first.kind, "auto");
        assert!(
            e.svc.run_due().await.unwrap().is_none(),
            "not again within a day"
        );
        assert_eq!(
            note_count(&e.dir.join("backups").join(&first.name)).await,
            0
        );

        for day in 1..=9 {
            *e.now.lock().unwrap() += Duration::hours(25);
            let _ = day;
            e.svc.run_due().await.unwrap().unwrap();
        }
        let autos = e
            .svc
            .list()
            .unwrap()
            .into_iter()
            .filter(|b| b.kind == "auto")
            .count();
        assert_eq!(autos, DEFAULT_KEEP);
        e.svc.backup_now("manual").await.unwrap();
        e.svc.prune(1).unwrap();
        assert!(
            e.svc.list().unwrap().iter().any(|b| b.kind == "manual"),
            "manual backups are never pruned"
        );
        let _ = std::fs::remove_dir_all(&e.dir);
    }

    #[tokio::test]
    async fn disabled_backups_do_nothing() {
        let e = setup().await;
        e.svc
            .settings
            .set("backup.enabled", "false", Actor::User)
            .await
            .unwrap();
        assert!(e.svc.run_due().await.unwrap().is_none());
        assert!(e.svc.list().unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&e.dir);
    }

    #[tokio::test]
    async fn import_rejects_foreign_damaged_and_newer_files_and_swaps_a_good_one() {
        let e = setup().await;
        // Not a database / not Orbit's.
        let junk = e.dir.join("junk.db");
        std::fs::write(&junk, b"hello").unwrap();
        assert!(
            e.svc
                .stage_import(&junk.display().to_string())
                .await
                .is_err()
        );
        let other = e.dir.join("other.db");
        {
            let opts = SqliteConnectOptions::from_str(&format!("sqlite://{}", other.display()))
                .unwrap()
                .create_if_missing(true);
            let p = SqlitePoolOptions::new()
                .max_connections(1)
                .connect_with(opts)
                .await
                .unwrap();
            sqlx::query("CREATE TABLE t (x INTEGER)")
                .execute(&p)
                .await
                .unwrap();
            p.close().await;
        }
        assert!(
            e.svc
                .stage_import(&other.display().to_string())
                .await
                .is_err()
        );
        assert!(!e.dir.join(PENDING).exists());

        // A good export from a database holding one extra setting.
        let src = e.dir.join("src.db");
        {
            let p = open(&src).await.unwrap();
            sqlx::query("INSERT INTO settings (key, value, updated_at) VALUES ('x', '1', 't')")
                .execute(&p)
                .await
                .unwrap();
            // A database from the future is refused.
            let future = e.dir.join("future.db");
            backup::snapshot(&p, &future).await.unwrap();
            p.close().await;
            let opts =
                SqliteConnectOptions::from_str(&format!("sqlite://{}", future.display())).unwrap();
            let fp = SqlitePoolOptions::new()
                .max_connections(1)
                .connect_with(opts)
                .await
                .unwrap();
            sqlx::query("INSERT INTO _sqlx_migrations (version, description, installed_on, success, checksum, execution_time) VALUES (999999, 'x', CURRENT_TIMESTAMP, 1, x'00', 0)")
                .execute(&fp)
                .await
                .unwrap();
            fp.close().await;
            assert!(
                e.svc
                    .stage_import(&future.display().to_string())
                    .await
                    .is_err()
            );
        }
        e.svc
            .stage_import(&src.display().to_string())
            .await
            .unwrap();
        assert!(e.svc.status().await.import_pending);
        assert!(
            e.svc.list().unwrap().iter().any(|b| b.kind == "pre-import"),
            "safety backup taken"
        );

        // Simulate the next start: the staged file replaces orbit.db.
        let live = open(&e.dir.join("orbit.db")).await.unwrap();
        live.close().await;
        assert!(apply_pending_import(&e.dir).await.unwrap());
        assert_eq!(note_count(&e.dir.join("orbit.db")).await, 1);
        assert!(!e.dir.join(PENDING).exists());
        assert!(
            !apply_pending_import(&e.dir).await.unwrap(),
            "nothing left to apply"
        );
        let _ = std::fs::remove_dir_all(&e.dir);
    }

    #[tokio::test]
    async fn export_requires_a_db_file_name_and_restore_only_accepts_listed_backups() {
        let e = setup().await;
        let out = e.dir.join("export.db");
        e.svc.export(&out.display().to_string()).await.unwrap();
        assert!(backup::verify(&out).await.is_ok());
        assert!(e.svc.export(&e.dir.display().to_string()).await.is_err());
        assert!(
            e.svc
                .export(&e.dir.join("x.txt").display().to_string())
                .await
                .is_err()
        );
        assert!(e.svc.stage_restore("../orbit.db").await.is_err());
        let b = e.svc.backup_now("manual").await.unwrap();
        e.svc.stage_restore(&b.name).await.unwrap();
        e.svc.cancel_import().unwrap();
        assert!(!e.svc.status().await.import_pending);
        let _ = std::fs::remove_dir_all(&e.dir);
    }
}
