use std::sync::{Arc, Mutex};

use chrono::{DateTime, Duration, TimeZone, Utc};

use super::test_support::*;
use super::*;
use crate::domain::activity::Actor;
use crate::error::DomainError;
use crate::infrastructure::database::SqliteTimerRepository;

const U: Actor = Actor::User;

struct Fixture {
    c: Ctx,
    timers: TimerService,
    now: Arc<Mutex<DateTime<Utc>>>,
}
impl Fixture {
    fn advance(&self, d: Duration) {
        *self.now.lock().unwrap() += d;
    }
}

async fn fx() -> Fixture {
    let c = ctx().await;
    let now = Arc::new(Mutex::new(
        Utc.with_ymd_and_hms(2026, 3, 10, 9, 0, 0).unwrap(),
    ));
    let n = now.clone();
    let timers = TimerService::with_clock(
        Arc::new(SqliteTimerRepository::new(c.pool.clone())),
        Arc::new(move || *n.lock().unwrap()),
    );
    Fixture { c, timers, now }
}

fn countdown(ms: i64) -> StartTimerInput {
    StartTimerInput {
        kind: "countdown".into(),
        label: None,
        task_id: None,
        duration_ms: Some(ms),
        phase: None,
    }
}

#[tokio::test]
async fn pause_resume_accumulates_and_stop_records_session() {
    let f = fx().await;
    let p =
        f.c.projects
            .create_project("P", None, None, U)
            .await
            .unwrap();
    let b = f.c.projects.list_boards(&p.id).await.unwrap().remove(0);
    let col = f.c.tasks.board_view(&b.id).await.unwrap().columns.remove(0);
    let task =
        f.c.tasks
            .create_task(new_task(&col.id, "Write"), U)
            .await
            .unwrap();

    let mut input = countdown(25 * 60_000);
    input.task_id = Some(task.id.clone());
    let t = f.timers.start(input, U).await.unwrap();
    assert!(
        f.timers.start(countdown(1000), U).await.is_err(),
        "only one active timer"
    );

    f.advance(Duration::minutes(10));
    let paused = f.timers.pause(&t.timer.id).await.unwrap();
    assert_eq!(paused.elapsed_ms, 600_000);
    f.advance(Duration::minutes(30)); // paused time does not count
    assert_eq!(f.timers.active().await.unwrap()[0].elapsed_ms, 600_000);
    f.timers.resume(&t.timer.id).await.unwrap();
    f.advance(Duration::minutes(5));
    let v = f.timers.active().await.unwrap().remove(0);
    assert_eq!((v.elapsed_ms, v.remaining_ms), (900_000, Some(600_000)));

    let stopped = f.timers.stop(&t.timer.id).await.unwrap();
    assert_eq!(stopped.timer.state, "cancelled");
    let from = "2026-03-10T00:00:00Z";
    let s = f
        .timers
        .sessions(from, "2026-03-11T00:00:00Z", Some(&task.id))
        .await
        .unwrap();
    assert_eq!(s.len(), 1);
    assert_eq!(s[0].duration_ms, 900_000);
    assert!(f.timers.active().await.unwrap().is_empty());
    assert!(
        f.timers.stop(&t.timer.id).await.is_ok(),
        "stop is idempotent"
    );
    assert_eq!(
        f.timers
            .sessions(from, "2026-03-11T00:00:00Z", None)
            .await
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn timer_that_ran_out_during_sleep_finishes_at_its_due_time() {
    let f = fx().await;
    let t = f.timers.start(countdown(25 * 60_000), U).await.unwrap();
    f.advance(Duration::hours(8)); // laptop slept, or the app was closed
    assert!(f.timers.active().await.unwrap().is_empty());
    let h = f.timers.history(10).await.unwrap();
    assert_eq!(h[0].timer.id, t.timer.id);
    assert_eq!(h[0].timer.state, "finished");
    assert_eq!(
        h[0].timer.ended_at.as_deref(),
        Some("2026-03-10T09:25:00.000Z")
    );
    let s = f
        .timers
        .sessions("2026-03-10T00:00:00Z", "2026-03-11T00:00:00Z", None)
        .await
        .unwrap();
    assert_eq!(
        (s.len(), s[0].duration_ms),
        (1, 25 * 60_000),
        "session is the planned length, not 8h"
    );
    // a second sync must not duplicate the session
    f.timers.active().await.unwrap();
    assert_eq!(
        f.timers
            .sessions("2026-03-10T00:00:00Z", "2026-03-11T00:00:00Z", None)
            .await
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn breaks_stopwatch_and_validation() {
    let f = fx().await;
    let brk = StartTimerInput {
        kind: "pomodoro".into(),
        label: None,
        task_id: None,
        duration_ms: Some(300_000),
        phase: Some("break".into()),
    };
    f.timers.start(brk, U).await.unwrap();
    f.advance(Duration::minutes(6));
    assert!(f.timers.active().await.unwrap().is_empty());
    assert!(
        f.timers
            .sessions("2026-03-10T00:00:00Z", "2026-03-11T00:00:00Z", None)
            .await
            .unwrap()
            .is_empty(),
        "breaks are not focus time"
    );

    let sw = StartTimerInput {
        kind: "stopwatch".into(),
        label: Some("deep work".into()),
        task_id: None,
        duration_ms: None,
        phase: None,
    };
    let t = f.timers.start(sw, U).await.unwrap();
    f.advance(Duration::minutes(42));
    let done = f.timers.stop(&t.timer.id).await.unwrap();
    assert_eq!(done.timer.state, "finished");
    assert_eq!(done.elapsed_ms, 42 * 60_000);

    let bad = |k: &str, d: Option<i64>| StartTimerInput {
        kind: k.into(),
        label: None,
        task_id: None,
        duration_ms: d,
        phase: None,
    };
    for (k, d) in [
        ("nope", Some(1000)),
        ("countdown", None),
        ("countdown", Some(0)),
        ("countdown", Some(99 * 3_600_000)),
        ("stopwatch", Some(5000)),
    ] {
        assert!(
            matches!(
                f.timers.start(bad(k, d), U).await,
                Err(DomainError::Validation(_))
            ),
            "{k} {d:?}"
        );
    }
    let mut ghost = countdown(5000);
    ghost.task_id = Some(uuid::Uuid::new_v4().to_string());
    assert!(matches!(
        f.timers.start(ghost, U).await,
        Err(DomainError::NotFound { .. })
    ));
    assert!(
        f.timers
            .sessions("2026-03-11T00:00:00Z", "2026-03-10T00:00:00Z", None)
            .await
            .is_err()
    );
}
