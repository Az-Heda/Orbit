use std::collections::HashSet;

use async_trait::async_trait;
use sqlx::SqlitePool;

use super::activity_repo::log_change;
use super::calendar_repo::ImportEvent;
use super::now_utc;
use crate::domain::activity::Actor;
use crate::domain::external::{ExternalCalendar, SyncReport};
use crate::error::{DomainError, DomainResult};

#[async_trait]
pub trait ExternalCalendarRepository: Send + Sync {
    async fn create(&self, name: &str) -> DomainResult<ExternalCalendar>;
    async fn list(&self) -> DomainResult<Vec<ExternalCalendar>>;
    async fn get(&self, id: &str) -> DomainResult<Option<ExternalCalendar>>;
    /// Make the calendar's events match the feed: add new UIDs, refresh known ones, drop the rest.
    /// Never touches events that are not tagged with this calendar.
    async fn replace_events(&self, id: &str, items: &[ImportEvent]) -> DomainResult<SyncReport>;
    async fn set_status(&self, id: &str, error: Option<&str>) -> DomainResult<()>;
    /// Remove the calendar and its events (soft delete).
    async fn delete(&self, id: &str) -> DomainResult<()>;
}

pub struct SqliteExternalCalendarRepository {
    pool: SqlitePool,
}

impl SqliteExternalCalendarRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

const SELECT: &str = "SELECT c.id, c.name, c.created_at, c.last_synced_at, c.last_error,
        (SELECT COUNT(*) FROM events e WHERE e.external_calendar_id = c.id AND e.deleted_at IS NULL) AS event_count
     FROM external_calendars c WHERE c.deleted_at IS NULL";

#[async_trait]
impl ExternalCalendarRepository for SqliteExternalCalendarRepository {
    async fn create(&self, name: &str) -> DomainResult<ExternalCalendar> {
        let mut tx = self.pool.begin().await?;
        let id = uuid::Uuid::new_v4().to_string();
        sqlx::query("INSERT INTO external_calendars (id, name, created_at) VALUES (?, ?, ?)")
            .bind(&id)
            .bind(name)
            .bind(now_utc())
            .execute(&mut *tx)
            .await?;
        log_change::<(), _>(
            &mut tx,
            "external_calendar",
            &id,
            "create",
            Actor::User,
            None,
            Some(&name),
            None,
        )
        .await?;
        tx.commit().await?;
        self.get(&id).await?.ok_or(DomainError::NotFound {
            entity: "external_calendar",
            id,
        })
    }

    async fn list(&self) -> DomainResult<Vec<ExternalCalendar>> {
        Ok(sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "{SELECT} ORDER BY c.created_at"
        )))
        .fetch_all(&self.pool)
        .await?)
    }

    async fn get(&self, id: &str) -> DomainResult<Option<ExternalCalendar>> {
        Ok(
            sqlx::query_as(sqlx::AssertSqlSafe(format!("{SELECT} AND c.id = ?")))
                .bind(id)
                .fetch_optional(&self.pool)
                .await?,
        )
    }

    async fn replace_events(&self, id: &str, items: &[ImportEvent]) -> DomainResult<SyncReport> {
        let mut tx = self.pool.begin().await?;
        let mut report = SyncReport::default();
        let now = now_utc();
        let mut seen: HashSet<&str> = HashSet::new();
        for item in items {
            let e = &item.event;
            if !seen.insert(e.uid.as_str()) {
                continue; // duplicate UID inside the feed
            }
            let existing: Option<(String, Option<String>)> = sqlx::query_as(
                "SELECT id, external_calendar_id FROM events WHERE uid = ? AND deleted_at IS NULL",
            )
            .bind(&e.uid)
            .fetch_optional(&mut *tx)
            .await?;
            let event_id = match existing {
                Some((_, owner)) if owner.as_deref() != Some(id) => {
                    report.skipped += 1;
                    continue;
                }
                Some((event_id, _)) => {
                    sqlx::query(
                        "UPDATE events SET title = ?, description = ?, location = ?, all_day = ?, start_at = ?,
                                end_at = ?, timezone = ?, recurrence_rule = ?, updated_at = ? WHERE id = ?",
                    )
                    .bind(&e.title).bind(&e.description).bind(&e.location).bind(e.all_day)
                    .bind(&e.start_at).bind(&e.end_at).bind(&e.timezone).bind(&e.recurrence_rule)
                    .bind(&now).bind(&event_id)
                    .execute(&mut *tx)
                    .await?;
                    sqlx::query("DELETE FROM event_exceptions WHERE event_id = ?")
                        .bind(&event_id)
                        .execute(&mut *tx)
                        .await?;
                    report.updated += 1;
                    event_id
                }
                None => {
                    let event_id = uuid::Uuid::new_v4().to_string();
                    sqlx::query(
                        "INSERT INTO events (id, uid, title, description, location, all_day, start_at, end_at, timezone,
                                recurrence_rule, external_calendar_id, created_at, updated_at)
                         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                    )
                    .bind(&event_id).bind(&e.uid).bind(&e.title).bind(&e.description).bind(&e.location)
                    .bind(e.all_day).bind(&e.start_at).bind(&e.end_at).bind(&e.timezone)
                    .bind(&e.recurrence_rule).bind(id).bind(&now).bind(&now)
                    .execute(&mut *tx)
                    .await?;
                    report.created += 1;
                    event_id
                }
            };
            for ex in &item.exceptions {
                sqlx::query(
                    "INSERT OR REPLACE INTO event_exceptions (id, event_id, original_start, kind, title, description, location, start_at, end_at, created_at)
                     VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                )
                .bind(uuid::Uuid::new_v4().to_string()).bind(&event_id).bind(&ex.original_start)
                .bind(if ex.skip { "skip" } else { "override" })
                .bind(&ex.title).bind(&ex.description).bind(&ex.location).bind(&ex.start_at).bind(&ex.end_at)
                .bind(&now)
                .execute(&mut *tx)
                .await?;
            }
        }
        let current: Vec<(String, String)> = sqlx::query_as(
            "SELECT id, uid FROM events WHERE external_calendar_id = ? AND deleted_at IS NULL",
        )
        .bind(id)
        .fetch_all(&mut *tx)
        .await?;
        for (event_id, uid) in current {
            if !seen.contains(uid.as_str()) {
                sqlx::query("UPDATE events SET deleted_at = ? WHERE id = ?")
                    .bind(&now)
                    .bind(event_id)
                    .execute(&mut *tx)
                    .await?;
                report.removed += 1;
            }
        }
        sqlx::query(
            "UPDATE external_calendars SET last_synced_at = ?, last_error = NULL WHERE id = ?",
        )
        .bind(&now)
        .bind(id)
        .execute(&mut *tx)
        .await?;
        // One summary row per sync instead of one per event (feeds can hold thousands).
        log_change::<(), _>(
            &mut tx,
            "external_calendar",
            id,
            "sync",
            Actor::User,
            None,
            Some(&(
                report.created,
                report.updated,
                report.removed,
                report.skipped,
            )),
            None,
        )
        .await?;
        tx.commit().await?;
        Ok(report)
    }

    async fn set_status(&self, id: &str, error: Option<&str>) -> DomainResult<()> {
        sqlx::query("UPDATE external_calendars SET last_error = ? WHERE id = ?")
            .bind(error)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn delete(&self, id: &str) -> DomainResult<()> {
        let mut tx = self.pool.begin().await?;
        let now = now_utc();
        sqlx::query("UPDATE events SET deleted_at = ? WHERE external_calendar_id = ? AND deleted_at IS NULL")
            .bind(&now)
            .bind(id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("UPDATE external_calendars SET deleted_at = ? WHERE id = ?")
            .bind(&now)
            .bind(id)
            .execute(&mut *tx)
            .await?;
        log_change::<(), ()>(
            &mut tx,
            "external_calendar",
            id,
            "delete",
            Actor::User,
            None,
            None,
            None,
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }
}
