use std::sync::Arc;

use chrono::{TimeZone, Utc};

use super::test_support::*;
use super::*;
use crate::domain::activity::Actor;
use crate::infrastructure::database::SqliteStatsRepository;

const U: Actor = Actor::User;

fn stats_at(c: &Ctx, y: i32, m: u32, d: u32, h: u32) -> StatsService {
    let now = Utc.with_ymd_and_hms(y, m, d, h, 0, 0).unwrap();
    StatsService::with_clock(
        Arc::new(SqliteStatsRepository::new(c.pool.clone())),
        Arc::new(move || now),
    )
}

async fn session(c: &Ctx, task: Option<&str>, started: &str, ms: i64) {
    sqlx::query("INSERT INTO focus_sessions (id, task_id, started_at, ended_at, duration_ms, created_at) VALUES (?, ?, ?, ?, ?, ?)")
        .bind(uuid::Uuid::new_v4().to_string())
        .bind(task)
        .bind(started)
        .bind(started)
        .bind(ms)
        .bind(started)
        .execute(&c.pool)
        .await
        .unwrap();
}

#[tokio::test]
async fn buckets_by_local_day_streaks_and_projects() {
    let c = ctx().await;
    let p = c
        .projects
        .create_project("Thesis", None, None, U)
        .await
        .unwrap();
    let b = c.projects.list_boards(&p.id).await.unwrap().remove(0);
    let cols = c.tasks.board_view(&b.id).await.unwrap().columns;
    let t = c
        .tasks
        .create_task(new_task(&cols[0].id, "Draft"), U)
        .await
        .unwrap();

    // "Now" is 2026-03-10 09:00 UTC = 10:00 in Rome (CET).
    // 23:30 UTC on the 8th is 00:30 on the 9th in Rome: it must land on the 9th, not the 8th.
    session(&c, Some(&t.id), "2026-03-08T23:30:00.000Z", 1_800_000).await;
    session(&c, None, "2026-03-10T08:00:00.000Z", 600_000).await;
    session(&c, Some(&t.id), "2026-03-06T12:00:00.000Z", 3_600_000).await;

    let s = stats_at(&c, 2026, 3, 10, 9)
        .overview("Europe/Rome", 7)
        .await
        .unwrap();
    assert_eq!(
        (s.from.as_str(), s.to.as_str()),
        ("2026-03-04", "2026-03-10")
    );
    assert_eq!(s.days.len(), 7);
    let day = |d: &str| s.days.iter().find(|x| x.date == d).unwrap();
    assert_eq!(day("2026-03-09").focus_ms, 1_800_000);
    assert_eq!(day("2026-03-08").focus_ms, 0);
    assert_eq!(day("2026-03-10").sessions, 1);
    assert_eq!(s.total_focus_ms, 6_000_000);
    // active: 6th, 9th, 10th -> current streak 2 (9th, 10th), longest 2
    assert_eq!((s.current_streak, s.longest_streak), (2, 2));
    assert_eq!(s.by_project[0].name, "Thesis");
    assert_eq!(s.by_project[0].focus_ms, 5_400_000);
    assert_eq!(s.by_project[1].name, "No project");

    // Same data in UTC puts the 23:30 session on the 8th.
    let u = stats_at(&c, 2026, 3, 10, 9)
        .overview("UTC", 7)
        .await
        .unwrap();
    assert_eq!(
        u.days
            .iter()
            .find(|x| x.date == "2026-03-08")
            .unwrap()
            .focus_ms,
        1_800_000
    );
}

#[tokio::test]
async fn completions_count_and_empty_today_keeps_streak() {
    let c = ctx().await;
    let p = c.projects.create_project("P", None, None, U).await.unwrap();
    let b = c.projects.list_boards(&p.id).await.unwrap().remove(0);
    let cols = c.tasks.board_view(&b.id).await.unwrap().columns;
    let t = c
        .tasks
        .create_task(new_task(&cols[0].id, "x"), U)
        .await
        .unwrap();
    c.tasks.set_completed(&t.id, true, false, U).await.unwrap();
    let real_today = Utc::now().date_naive().to_string();

    let s = StatsService::new(Arc::new(SqliteStatsRepository::new(c.pool.clone())))
        .overview("UTC", 30)
        .await
        .unwrap();
    assert_eq!(s.days.last().unwrap().date, real_today);
    assert_eq!(s.total_completed, 1);
    assert_eq!(s.current_streak, 1);

    // Yesterday active, today empty: streak is preserved.
    session(&c, None, "2026-03-09T10:00:00.000Z", 60_000).await;
    let s = stats_at(&c, 2026, 3, 10, 9)
        .overview("UTC", 5)
        .await
        .unwrap();
    assert_eq!((s.current_streak, s.longest_streak), (1, 1));

    assert!(
        stats_at(&c, 2026, 3, 10, 9)
            .overview("Mars/Base", 5)
            .await
            .is_err()
    );
    assert!(
        stats_at(&c, 2026, 3, 10, 9)
            .overview("UTC", 0)
            .await
            .is_err()
    );
    assert!(
        stats_at(&c, 2026, 3, 10, 9)
            .overview("UTC", 400)
            .await
            .is_err()
    );
}
