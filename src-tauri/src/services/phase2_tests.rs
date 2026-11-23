use chrono::Duration;

use super::test_support::*;
use super::*;
use crate::domain::activity::Actor;
use crate::domain::automation::{Action, Trigger};
use crate::error::DomainError;

const U: Actor = Actor::User;

fn event(title: &str, start: &str, end: &str) -> CreateEventInput {
    CreateEventInput {
        title: title.into(),
        description: None,
        location: None,
        all_day: None,
        start_at: start.into(),
        end_at: end.into(),
        timezone: "UTC".into(),
        recurrence_rule: None,
    }
}

fn rule(name: &str, trigger: Trigger, action: Action) -> RuleInput {
    RuleInput {
        name: name.into(),
        enabled: None,
        trigger,
        action,
    }
}

async fn count_tasks(c: &P2, title_like: &str) -> i64 {
    sqlx::query_as::<_, (i64,)>(
        "SELECT COUNT(*) FROM tasks WHERE title LIKE ? AND deleted_at IS NULL",
    )
    .bind(title_like)
    .fetch_one(&c.pool)
    .await
    .unwrap()
    .0
}

// ---------------- inbox ----------------

#[tokio::test]
async fn inbox_capture_triage_keeps_original_and_links_back() {
    let c = p2().await;
    let (backlog, _, _) = c.board("Home").await;
    let raw = "  buy milk\nand eggs, ask about the 2% one  ";
    let item = c.inbox.capture(raw, U).await.unwrap();
    assert_eq!(item.raw_text, raw, "stored byte-for-byte");
    assert_eq!(c.inbox.count_new().await.unwrap(), 1);
    assert!(matches!(
        c.inbox.capture("   ", U).await,
        Err(DomainError::Validation(_))
    ));

    let (done, task) = c
        .inbox
        .triage_to_task(&item.id, &backlog, None, None, U)
        .await
        .unwrap();
    assert_eq!(task.title, "buy milk");
    assert_eq!(done.status, "triaged");
    assert_eq!(
        (
            done.linked_entity_type.as_deref(),
            done.linked_entity_id.as_deref()
        ),
        (Some("task"), Some(task.id.as_str()))
    );
    assert_eq!(done.raw_text, raw, "original never discarded");
    // the task description is the (trimmed) text; the inbox row keeps the untouched original
    assert_eq!(
        c.tasks
            .get_task(&task.id)
            .await
            .unwrap()
            .description
            .as_deref(),
        Some(raw.trim())
    );
    assert_eq!(c.inbox.count_new().await.unwrap(), 0);
    assert!(
        matches!(
            c.inbox
                .triage_to_task(&item.id, &backlog, None, None, U)
                .await,
            Err(DomainError::Validation(_))
        ),
        "no double triage"
    );
    assert!(matches!(
        c.inbox.dismiss(&item.id, U).await,
        Err(DomainError::Validation(_))
    ));
    // still searchable after triage
    assert_eq!(
        c.search
            .search("eggs", Some(vec!["inbox".into()]), None)
            .await
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn inbox_event_triage_and_dismiss() {
    let c = p2().await;
    let a = c.inbox.capture("dentist friday 3pm", U).await.unwrap();
    let (item, ev) = c
        .inbox
        .triage_to_event(
            &a.id,
            None,
            "2026-03-13T15:00:00Z",
            "2026-03-13T16:00:00Z",
            "UTC",
            U,
        )
        .await
        .unwrap();
    assert_eq!(
        (item.linked_entity_type.as_deref(), ev.title.as_str()),
        (Some("event"), "dentist friday 3pm")
    );
    let b = c.inbox.capture("random thought", U).await.unwrap();
    assert_eq!(c.inbox.dismiss(&b.id, U).await.unwrap().status, "dismissed");
    assert_eq!(c.inbox.list(Some("new"), 50).await.unwrap().len(), 0);
    assert_eq!(c.inbox.list(None, 50).await.unwrap().len(), 2);
    assert!(c.inbox.list(Some("bogus"), 50).await.is_err());
}

// ---------------- automation ----------------

#[tokio::test]
async fn rule_runs_through_services_with_automation_actor() {
    let c = p2().await;
    let (backlog, _, done) = c.board("P").await;
    c.rules
        .create(
            rule(
                "Journal done tasks",
                Trigger::TaskMoved {
                    to_column_kind: Some("done".into()),
                    to_column_id: None,
                },
                Action::CreateNote {
                    title: "Done: {{title}}".into(),
                    body: "finished on {{date}}".into(),
                },
            ),
            U,
        )
        .await
        .unwrap();
    let t = c
        .tasks
        .create_task(new_task(&backlog, "Ship it"), U)
        .await
        .unwrap();
    c.drain().await;
    assert!(
        c.notes.list(None, None, 10).await.unwrap().is_empty(),
        "creating is not moving to done"
    );

    c.tasks.move_task(&t.id, &done, 0, false, U).await.unwrap();
    c.drain().await;
    let notes = c.notes.list(None, None, 10).await.unwrap();
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].title, "Done: Ship it");
    assert_eq!(notes[0].body, "finished on 2026-03-10");
    let actor: (String,) = sqlx::query_as(
        "SELECT actor FROM activity_log WHERE entity_type = 'note' AND action = 'create'",
    )
    .fetch_one(&c.pool)
    .await
    .unwrap();
    assert_eq!(actor.0, "automation");
}

#[tokio::test]
async fn disabled_rules_do_not_fire_and_rules_are_validated() {
    let c = p2().await;
    let (backlog, _, _) = c.board("P").await;
    let r = c
        .rules
        .create(
            rule(
                "Ping",
                Trigger::TaskCreated,
                Action::Notify {
                    title: "New: {{title}}".into(),
                    body: None,
                },
            ),
            U,
        )
        .await
        .unwrap();
    c.rules.set_enabled(&r.id, false, U).await.unwrap();
    c.tasks
        .create_task(new_task(&backlog, "A"), U)
        .await
        .unwrap();
    c.drain().await;
    assert!(c.notifier.titles().is_empty());
    c.rules.set_enabled(&r.id, true, U).await.unwrap();
    c.tasks
        .create_task(new_task(&backlog, "B"), U)
        .await
        .unwrap();
    c.drain().await;
    assert_eq!(c.notifier.titles(), ["New: B"]);

    let bad = |t, a| c.rules.create(rule("x", t, a), U);
    assert!(
        bad(
            Trigger::TaskCreated,
            Action::MoveTask {
                column_id: "not-a-uuid".into()
            }
        )
        .await
        .is_err()
    );
    assert!(
        bad(
            Trigger::Schedule {
                time: "9am".into(),
                days: vec![],
                timezone: "UTC".into()
            },
            Action::Notify {
                title: "x".into(),
                body: None
            }
        )
        .await
        .is_err()
    );
    assert!(
        bad(
            Trigger::TaskCreated,
            Action::StartTimer {
                timer_kind: "countdown".into(),
                minutes: 0,
                label: None
            }
        )
        .await
        .is_err()
    );
    assert_eq!(c.rules.list().await.unwrap().len(), 1);
    c.rules.delete(&r.id, U).await.unwrap();
    assert!(c.rules.list().await.unwrap().is_empty());
}

#[tokio::test]
async fn a_rule_never_retriggers_itself() {
    let c = p2().await;
    let (backlog, _, _) = c.board("P").await;
    // "when a task is created, create a follow-up task" would recurse forever without a guard.
    c.rules
        .create(
            rule(
                "Follow up",
                Trigger::TaskCreated,
                Action::CreateTask {
                    column_id: backlog.clone(),
                    title: "Follow up: {{title}}".into(),
                    description: None,
                },
            ),
            U,
        )
        .await
        .unwrap();
    c.tasks
        .create_task(new_task(&backlog, "Call Anna"), U)
        .await
        .unwrap();
    c.drain().await;
    assert_eq!(
        count_tasks(&c, "%").await,
        2,
        "original + exactly one follow-up"
    );
}

#[tokio::test]
async fn two_rules_ping_ponging_terminate() {
    let c = p2().await;
    let (backlog, active, _) = c.board("P").await;
    // A: task moved anywhere -> create a task.  B: task created -> move it to "active".
    // A's task triggers B, B's move triggers A, ... until the depth limit stops it.
    c.rules
        .create(
            rule(
                "A",
                Trigger::TaskMoved {
                    to_column_kind: None,
                    to_column_id: None,
                },
                Action::CreateTask {
                    column_id: backlog.clone(),
                    title: "from A".into(),
                    description: None,
                },
            ),
            U,
        )
        .await
        .unwrap();
    c.rules
        .create(
            rule(
                "B",
                Trigger::TaskCreated,
                Action::MoveTask {
                    column_id: active.clone(),
                },
            ),
            U,
        )
        .await
        .unwrap();
    c.tasks
        .create_task(new_task(&backlog, "seed"), U)
        .await
        .unwrap();
    let handled = c.drain().await;
    assert!(handled < 20, "event storm: {handled}");
    let n = count_tasks(&c, "%").await;
    assert!((2..=6).contains(&n), "bounded task count, got {n}");
}

// ---------------- scheduler ----------------

#[tokio::test]
async fn reminder_fires_once_and_survives_repeated_ticks() {
    let c = p2().await;
    c.calendar
        .create_event(
            event("Standup", "2026-03-10T09:08:00Z", "2026-03-10T09:30:00Z"),
            U,
        )
        .await
        .unwrap();
    let r = c.scheduler.tick().await.unwrap();
    assert_eq!((r.reminders, r.missed), (1, 0));
    assert_eq!(
        c.notifier.0.lock().unwrap()[0],
        ("Standup".to_string(), "Starts in 8 min".to_string())
    );
    assert_eq!(
        c.scheduler.tick().await.unwrap().reminders,
        0,
        "no duplicate"
    );
    c.advance(Duration::minutes(5));
    assert_eq!(c.scheduler.tick().await.unwrap().reminders, 0);
    assert_eq!(c.notifier.titles().len(), 1);
}

#[tokio::test]
async fn future_events_outside_lead_time_wait() {
    let c = p2().await;
    c.calendar
        .create_event(
            event("Later", "2026-03-10T11:00:00Z", "2026-03-10T12:00:00Z"),
            U,
        )
        .await
        .unwrap();
    assert_eq!(c.scheduler.tick().await.unwrap().reminders, 0);
    c.advance(Duration::minutes(111)); // 10:51 -> within 10 min lead
    assert_eq!(c.scheduler.tick().await.unwrap().reminders, 1);
}

#[tokio::test]
async fn reminders_after_sleep_are_marked_missed_once_or_dropped() {
    let c = p2().await;
    c.calendar
        .create_event(
            event("Recent", "2026-03-10T07:00:00Z", "2026-03-10T07:30:00Z"),
            U,
        )
        .await
        .unwrap();
    c.calendar
        .create_event(
            event("Ancient", "2026-03-10T01:00:00Z", "2026-03-10T01:30:00Z"),
            U,
        )
        .await
        .unwrap();
    c.advance(Duration::hours(-4)); // 05:00: the app is running and ticking
    assert_eq!(c.scheduler.tick().await.unwrap(), TickReport::default());
    c.advance(Duration::hours(4)); // the PC slept until 09:00
    let r = c.scheduler.tick().await.unwrap();
    assert_eq!(
        (r.reminders, r.missed),
        (1, 1),
        "Recent is missed, Ancient is outside the window"
    );
    assert_eq!(
        c.notifier.0.lock().unwrap()[0],
        (
            "Missed: Recent".to_string(),
            "Started 120 min ago".to_string()
        )
    );
    assert_eq!(c.scheduler.tick().await.unwrap().missed, 0, "only once");
}

#[tokio::test]
async fn items_that_merely_appear_in_the_past_are_not_missed() {
    let c = p2().await;
    c.scheduler.tick().await.unwrap(); // running normally, marker set
    c.advance(Duration::seconds(20));
    // logged/imported after the fact: started an hour ago
    c.calendar
        .create_event(
            event(
                "Logged late",
                "2026-03-10T08:00:00Z",
                "2026-03-10T08:30:00Z",
            ),
            U,
        )
        .await
        .unwrap();
    assert_eq!(c.scheduler.tick().await.unwrap(), TickReport::default());
    assert!(c.notifier.titles().is_empty());
}

#[tokio::test]
async fn completed_tasks_and_all_day_items_do_not_remind() {
    let c = p2().await;
    let (backlog, _, _) = c.board("P").await;
    let mut t = new_task(&backlog, "Pay rent");
    t.due_at = Some("2026-03-10T09:05:00Z".into());
    let t = c.tasks.create_task(t, U).await.unwrap();
    assert_eq!(
        c.scheduler.tick().await.unwrap().reminders,
        1,
        "task deadline reminds"
    );
    let mut t2 = new_task(&backlog, "Done already");
    t2.due_at = Some("2026-03-10T09:06:00Z".into());
    let t2 = c.tasks.create_task(t2, U).await.unwrap();
    c.tasks.set_completed(&t2.id, true, true, U).await.unwrap();
    c.advance(Duration::minutes(1));
    assert_eq!(c.scheduler.tick().await.unwrap().reminders, 0);
    let _ = t;
}

#[tokio::test]
async fn scheduled_rule_catches_up_once_after_a_long_sleep() {
    let c = p2().await;
    c.rules
        .create(
            rule(
                "Review",
                Trigger::Schedule {
                    time: "09:00".into(),
                    days: vec![],
                    timezone: "UTC".into(),
                },
                Action::Notify {
                    title: "Daily review".into(),
                    body: None,
                },
            ),
            U,
        )
        .await
        .unwrap();
    c.advance(Duration::hours(-1)); // 08:00
    assert_eq!(c.scheduler.tick().await.unwrap().rules, 0);
    c.advance(Duration::hours(2)); // 10:00, 09:00 passed
    assert_eq!(c.scheduler.tick().await.unwrap().rules, 1);
    assert_eq!(c.scheduler.tick().await.unwrap().rules, 0, "once per day");
    c.advance(Duration::days(3)); // slept three days: one catch-up, not three
    assert_eq!(c.scheduler.tick().await.unwrap().rules, 1);
    assert_eq!(
        c.notifier
            .titles()
            .iter()
            .filter(|t| *t == "Daily review")
            .count(),
        2
    );
}

#[tokio::test]
async fn weekday_filter_and_disabled_scheduled_rules() {
    let c = p2().await; // 2026-03-10 is a Tuesday (2)
    let r = c
        .rules
        .create(
            rule(
                "Mondays",
                Trigger::Schedule {
                    time: "09:30".into(),
                    days: vec![1],
                    timezone: "UTC".into(),
                },
                Action::Notify {
                    title: "Monday!".into(),
                    body: None,
                },
            ),
            U,
        )
        .await
        .unwrap();
    c.scheduler.tick().await.unwrap();
    c.advance(Duration::hours(1)); // Tue 10:00
    assert_eq!(
        c.scheduler.tick().await.unwrap().rules,
        0,
        "Tuesday is not Monday"
    );
    c.rules.set_enabled(&r.id, false, U).await.unwrap();
    c.advance(Duration::days(7));
    assert_eq!(c.scheduler.tick().await.unwrap().rules, 0, "disabled");
}

#[tokio::test]
async fn timer_finishing_notifies_even_if_app_was_asleep() {
    let c = p2().await;
    c.timers
        .start(
            StartTimerInput {
                kind: "countdown".into(),
                label: Some("Tea".into()),
                task_id: None,
                duration_ms: Some(180_000),
                phase: None,
            },
            U,
        )
        .await
        .unwrap();
    c.advance(Duration::hours(2));
    c.scheduler.tick().await.unwrap();
    c.drain().await;
    assert_eq!(c.notifier.titles(), ["Timer finished"]);
    c.scheduler.tick().await.unwrap();
    c.drain().await;
    assert_eq!(c.notifier.titles().len(), 1, "notified exactly once");
}

#[tokio::test]
async fn timer_ended_rule_can_start_a_break() {
    let c = p2().await;
    c.rules
        .create(
            rule(
                "Auto break",
                Trigger::TimerEnded,
                Action::StartTimer {
                    timer_kind: "pomodoro".into(),
                    minutes: 5,
                    label: Some("Break".into()),
                },
            ),
            U,
        )
        .await
        .unwrap();
    c.timers
        .start(
            StartTimerInput {
                kind: "countdown".into(),
                label: None,
                task_id: None,
                duration_ms: Some(60_000),
                phase: None,
            },
            U,
        )
        .await
        .unwrap();
    c.advance(Duration::minutes(2));
    c.scheduler.tick().await.unwrap();
    c.drain().await;
    let active = c.timers.active().await.unwrap();
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].timer.label.as_deref(), Some("Break"));
}
