//! Phase 6: chained automation actions and notification channels.
use std::str::FromStr;
use std::time::Duration;

use super::test_support::*;
use super::*;
use crate::domain::activity::Actor;
use crate::domain::automation::{Action, Trigger};
use crate::domain::channels::{ChannelInput, ChannelKind};
use crate::error::DomainError;
use crate::infrastructure::channels::ChannelSecrets;
use crate::infrastructure::database::{AutomationRepository, MIGRATOR, SqliteAutomationRepository};

const U: Actor = Actor::User;
const TOKEN: &str = "123456:SECRET-token_xyz";
const HOOK: &str = "https://discord.com/api/webhooks/42/SECRETHOOK";

fn chain(name: &str, trigger: Trigger, actions: Vec<Action>) -> RuleInput {
    RuleInput {
        name: name.into(),
        enabled: None,
        trigger,
        actions,
    }
}

fn note(title: &str) -> Action {
    Action::CreateNote {
        title: title.into(),
        body: String::new(),
    }
}

fn windows(name: &str, template: &str) -> ChannelInput {
    ChannelInput {
        name: name.into(),
        kind: ChannelKind::Windows,
        template: template.into(),
        chat_id: None,
        secret: None,
    }
}

fn telegram(name: &str, template: &str) -> ChannelInput {
    ChannelInput {
        name: name.into(),
        kind: ChannelKind::Telegram,
        template: template.into(),
        chat_id: Some("42".into()),
        secret: Some(TOKEN.into()),
    }
}

fn discord(name: &str, template: &str) -> ChannelInput {
    ChannelInput {
        name: name.into(),
        kind: ChannelKind::Discord,
        template: template.into(),
        chat_id: None,
        secret: Some(HOOK.into()),
    }
}

/// Delivery is detached from the engine, so wait (briefly) for a condition.
async fn settle(mut ok: impl FnMut() -> bool) {
    for _ in 0..400 {
        if ok() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    panic!("condition not reached");
}

async fn note_titles(c: &P2) -> Vec<String> {
    let mut v: Vec<String> = c
        .notes
        .list(None, None, 50)
        .await
        .unwrap()
        .into_iter()
        .map(|n| n.title)
        .collect();
    v.sort();
    v
}

async fn log_rows(
    c: &P2,
    entity_type: &str,
    action: &str,
) -> Vec<(String, String, Option<String>)> {
    sqlx::query_as(
        "SELECT entity_id, actor, after_json FROM activity_log
         WHERE entity_type = ? AND action = ? ORDER BY created_at, rowid",
    )
    .bind(entity_type)
    .bind(action)
    .fetch_all(&c.pool)
    .await
    .unwrap()
}

async fn finish_a_task(c: &P2, title: &str) {
    let (backlog, _, done) = c.board("P").await;
    let t = c
        .tasks
        .create_task(new_task(&backlog, title), U)
        .await
        .unwrap();
    c.tasks.move_task(&t.id, &done, 0, false, U).await.unwrap();
    c.drain().await;
}

// ---------------- migration ----------------

#[tokio::test]
async fn existing_single_action_rules_become_one_step_chains() {
    let opts = sqlx::sqlite::SqliteConnectOptions::from_str("sqlite::memory:")
        .unwrap()
        .foreign_keys(true);
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(opts)
        .await
        .unwrap();
    MIGRATOR.run_to(9, &pool).await.unwrap();
    sqlx::query(
        "INSERT INTO automation_rules (id, name, enabled, trigger_kind, trigger_config, action_kind, action_config, created_at, updated_at)
         VALUES ('11111111-1111-4111-8111-111111111111', 'Old rule', 1, 'task_completed', '{\"kind\":\"task_completed\"}',
                 'notify', '{\"kind\":\"notify\",\"title\":\"Hi\"}', '2026-01-01T00:00:00.000Z', '2026-01-01T00:00:00.000Z')",
    )
    .execute(&pool)
    .await
    .unwrap();
    MIGRATOR.run(&pool).await.unwrap();

    let repo = SqliteAutomationRepository::new(pool);
    let rules = repo.list(false).await.unwrap();
    assert_eq!(rules.len(), 1);
    let rule = rules.into_iter().next().unwrap().into_rule().unwrap();
    assert_eq!(rule.name, "Old rule");
    assert_eq!(
        rule.actions,
        vec![Action::Notify {
            title: "Hi".into(),
            body: None
        }]
    );
}

// ---------------- chained actions ----------------

#[tokio::test]
async fn steps_run_in_order_with_context_placeholders() {
    let c = p2().await;
    c.rules
        .create(
            chain(
                "Wrap up",
                Trigger::TaskCompleted,
                vec![
                    note("1 {{task.title}}"),
                    note("2 {{rule.name}} / {{trigger}}"),
                    note("3 {{date}}"),
                ],
            ),
            U,
        )
        .await
        .unwrap();
    finish_a_task(&c, "Ship").await;
    assert_eq!(
        note_titles(&c).await,
        ["1 Ship", "2 Wrap up / task_completed", "3 2026-03-10"]
    );
    // Every step is a normal, logged mutation by the automation actor.
    let rows: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM activity_log WHERE entity_type = 'note' AND actor = 'automation'",
    )
    .fetch_one(&c.pool)
    .await
    .unwrap();
    assert_eq!(rows, 3);
}

#[tokio::test]
async fn a_failing_step_is_logged_and_stops_the_chain() {
    let c = p2().await;
    let missing = "22222222-2222-4222-8222-222222222222";
    let r = c
        .rules
        .create(
            chain(
                "Fragile",
                Trigger::TaskCompleted,
                vec![
                    note("before"),
                    Action::MoveTask {
                        column_id: missing.into(),
                    },
                    note("never"),
                ],
            ),
            U,
        )
        .await
        .unwrap();
    finish_a_task(&c, "Ship").await;
    assert_eq!(note_titles(&c).await, ["before"], "step 3 must not run");
    let rows = log_rows(&c, "automation_rule", "step_failed").await;
    assert_eq!(rows.len(), 1);
    assert_eq!(
        (rows[0].0.as_str(), rows[0].1.as_str()),
        (r.id.as_str(), "automation")
    );
    let detail: serde_json::Value = serde_json::from_str(rows[0].2.as_ref().unwrap()).unwrap();
    assert_eq!(detail["step"], 2);
    assert_eq!(detail["action"], "move_task");
}

#[tokio::test]
async fn chain_validation_rejects_empty_oversized_and_bad_steps() {
    let c = p2().await;
    let bad = |actions| chain("x", Trigger::TaskCreated, actions);
    for actions in [
        vec![],
        vec![note("n"); 11],
        vec![
            note("ok"),
            Action::SendNotification {
                channel_id: "nope".into(),
            },
        ],
    ] {
        assert!(matches!(
            c.rules.create(bad(actions), U).await,
            Err(DomainError::Validation(_))
        ));
    }
    assert!(c.rules.create(bad(vec![note("n"); 10]), U).await.is_ok());
}

#[tokio::test]
async fn a_chain_counts_as_one_run_for_the_rate_limit_and_never_retriggers_itself() {
    let c = p2().await;
    let (backlog, _, _) = c.board("P").await;
    // Creates a task on every task_created: its own creations must not loop.
    c.rules
        .create(
            chain(
                "Echo",
                Trigger::TaskCreated,
                vec![
                    Action::CreateTask {
                        column_id: backlog.clone(),
                        title: "echo 1".into(),
                        description: None,
                    },
                    Action::CreateTask {
                        column_id: backlog.clone(),
                        title: "echo 2".into(),
                        description: None,
                    },
                ],
            ),
            U,
        )
        .await
        .unwrap();
    c.tasks
        .create_task(new_task(&backlog, "seed"), U)
        .await
        .unwrap();
    c.drain().await;
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM tasks WHERE title LIKE 'echo%'")
        .fetch_one(&c.pool)
        .await
        .unwrap();
    assert_eq!(n, 2, "one run, two steps, no re-trigger");
}

// ---------------- channels ----------------

#[tokio::test]
async fn channel_crud_keeps_secrets_out_of_the_database_and_the_view() {
    let c = p2().await;
    let tg = c
        .channels
        .create(telegram("Phone", "{{title}}"), U)
        .await
        .unwrap();
    let dc = c
        .channels
        .create(discord("Team", "{{title}}"), U)
        .await
        .unwrap();
    assert!(tg.has_secret && dc.has_secret);
    assert_eq!(tg.chat_id.as_deref(), Some("42"));
    assert_eq!(c.secrets.get(&tg.id).unwrap().as_deref(), Some(TOKEN));

    // Nothing the frontend or the database sees contains a secret.
    let view = serde_json::to_string(&c.channels.list().await.unwrap()).unwrap();
    assert!(!view.contains("SECRET"), "{view}");
    for sql in [
        "SELECT COUNT(*) FROM notification_channels WHERE name || config || template LIKE '%SECRET%'",
        "SELECT COUNT(*) FROM activity_log WHERE coalesce(before_json, '') || coalesce(after_json, '') LIKE '%SECRET%'",
        "SELECT COUNT(*) FROM settings WHERE value LIKE '%SECRET%'",
    ] {
        let n: i64 = sqlx::query_scalar(sql).fetch_one(&c.pool).await.unwrap();
        assert_eq!(n, 0, "secret leaked: {sql}");
    }

    // Updating without a secret keeps the stored one; a new one replaces it.
    let tg2 = c
        .channels
        .update(
            &tg.id,
            ChannelInput {
                secret: None,
                ..telegram("Phone 2", "x")
            },
            U,
        )
        .await
        .unwrap();
    assert_eq!(tg2.name, "Phone 2");
    assert_eq!(c.secrets.get(&tg.id).unwrap().as_deref(), Some(TOKEN));

    // Names are unique (case-insensitive); remote kinds need a secret; a kind change needs a new one.
    assert!(matches!(
        c.channels.create(windows("team", "x"), U).await,
        Err(DomainError::Validation(_))
    ));
    assert!(matches!(
        c.channels
            .create(
                ChannelInput {
                    secret: None,
                    ..discord("NoHook", "x")
                },
                U
            )
            .await,
        Err(DomainError::Validation(_))
    ));
    assert!(matches!(
        c.channels
            .update(
                &dc.id,
                ChannelInput {
                    secret: None,
                    ..telegram("Team", "x")
                },
                U
            )
            .await,
        Err(DomainError::Validation(_))
    ));

    // Deleting removes the secret too.
    c.channels.delete(&tg.id, U).await.unwrap();
    assert_eq!(c.secrets.get(&tg.id).unwrap(), None);
    assert_eq!(c.channels.list().await.unwrap().len(), 1);
    assert!(c.channels.find_by_name("TEAM").await.unwrap().is_some());
    assert!(c.channels.find_by_name("Phone 2").await.unwrap().is_none());
}

#[tokio::test]
async fn send_test_reports_the_outcome() {
    let c = p2().await;
    let ch = c
        .channels
        .create(telegram("Phone", "{{task.title}} / {{rule.name}}"), U)
        .await
        .unwrap();
    c.channels.send_test(&ch.id).await.unwrap();
    assert_eq!(c.sender.texts(), ["Sample task / Test rule"]);
    assert_eq!(
        c.sender.sent.lock().unwrap()[0].0,
        format!("telegram:42:{TOKEN}")
    );

    *c.sender.fail_with.lock().unwrap() = Some("Telegram rejected the message (HTTP 401)".into());
    let e = c.channels.send_test(&ch.id).await.unwrap_err().to_string();
    assert!(e.contains("HTTP 401"), "{e}");

    let w = c
        .channels
        .create(windows("Desk", "hello {{title}}"), U)
        .await
        .unwrap();
    c.channels.send_test(&w.id).await.unwrap();
    assert_eq!(
        c.notifier.0.lock().unwrap().last().unwrap(),
        &("Orbit test".to_string(), "hello Sample item".to_string())
    );
}

#[tokio::test]
async fn send_notification_step_renders_the_channel_template() {
    let c = p2().await;
    let desk = c
        .channels
        .create(windows("Desk", "“{{task.title}}” done ({{rule.name}})"), U)
        .await
        .unwrap();
    let phone = c
        .channels
        .create(telegram("Phone", "Done: {{task.title}}"), U)
        .await
        .unwrap();
    c.rules
        .create(
            chain(
                "Celebrate",
                Trigger::TaskCompleted,
                vec![
                    Action::SendNotification {
                        channel_id: desk.id.clone(),
                    },
                    note("logged"),
                    Action::SendNotification {
                        channel_id: phone.id.clone(),
                    },
                ],
            ),
            U,
        )
        .await
        .unwrap();
    finish_a_task(&c, "Ship v1").await;
    settle(|| !c.sender.texts().is_empty() && !c.notifier.titles().is_empty()).await;
    assert_eq!(c.sender.texts(), ["Done: Ship v1"]);
    let toast = c.notifier.0.lock().unwrap()[0].clone();
    assert_eq!(
        toast,
        (
            "Celebrate".to_string(),
            "“Ship v1” done (Celebrate)".to_string()
        )
    );
    assert_eq!(
        note_titles(&c).await,
        ["logged"],
        "steps around a notification still run"
    );
}

#[tokio::test]
async fn delivery_failure_is_logged_without_secrets_and_does_not_stop_the_rule() {
    let c = p2().await;
    let ch = c
        .channels
        .create(discord("Team", "{{title}}"), U)
        .await
        .unwrap();
    *c.sender.fail_with.lock().unwrap() =
        Some("Discord rejected the message (HTTP 404): Unknown Webhook".into());
    let rule = c
        .rules
        .create(
            chain(
                "Ping",
                Trigger::TaskCompleted,
                vec![
                    Action::SendNotification {
                        channel_id: ch.id.clone(),
                    },
                    note("after"),
                ],
            ),
            U,
        )
        .await
        .unwrap();
    finish_a_task(&c, "Ship").await;
    for _ in 0..400 {
        if !log_rows(&c, "notification_channel", "delivery_failed")
            .await
            .is_empty()
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    let rows = log_rows(&c, "notification_channel", "delivery_failed").await;
    assert_eq!(rows.len(), 1);
    assert_eq!(
        (rows[0].0.as_str(), rows[0].1.as_str()),
        (ch.id.as_str(), "automation")
    );
    let detail = rows[0].2.clone().unwrap();
    assert!(
        detail.contains("Unknown Webhook") && detail.contains(&rule.name),
        "{detail}"
    );
    assert!(!detail.contains("SECRET"));
    assert_eq!(note_titles(&c).await, ["after"], "the chain carried on");
}

#[tokio::test]
async fn a_deleted_channel_fails_the_step_not_the_engine() {
    let c = p2().await;
    let ch = c.channels.create(windows("Desk", "x"), U).await.unwrap();
    let rule = c
        .rules
        .create(
            chain(
                "Orphan",
                Trigger::TaskCompleted,
                vec![
                    Action::SendNotification {
                        channel_id: ch.id.clone(),
                    },
                    note("never"),
                ],
            ),
            U,
        )
        .await
        .unwrap();
    c.channels.delete(&ch.id, U).await.unwrap();
    finish_a_task(&c, "Ship").await;
    assert!(note_titles(&c).await.is_empty());
    let rows = log_rows(&c, "automation_rule", "step_failed").await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].0, rule.id);
    assert!(rows[0].2.as_ref().unwrap().contains("send_notification"));
}

#[tokio::test]
async fn deliveries_are_rate_limited_per_channel() {
    let c = p2().await;
    let ch = c
        .channels
        .create(discord("Team", "{{title}}"), U)
        .await
        .unwrap();
    let ctx = || crate::domain::automation::RenderCtx {
        title: "t".into(),
        rule_name: "Loop".into(),
        ..Default::default()
    };
    for _ in 0..30 {
        c.channels.dispatch(&ch.id, ctx()).await.unwrap();
    }
    settle(|| c.sender.texts().len() >= crate::services::channel_service::DELIVERY_RATE_LIMIT)
        .await;
    // Give the remaining (refused) deliveries time to be handled.
    for _ in 0..400 {
        if !log_rows(&c, "notification_channel", "rate_limited")
            .await
            .is_empty()
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(
        c.sender.texts().len(),
        crate::services::channel_service::DELIVERY_RATE_LIMIT
    );
    assert_eq!(
        log_rows(&c, "notification_channel", "rate_limited")
            .await
            .len(),
        1,
        "one log row per minute, not one per refused message"
    );
    // The window moves on: sending works again.
    c.advance(chrono::Duration::minutes(2));
    c.channels.dispatch(&ch.id, ctx()).await.unwrap();
    settle(|| c.sender.texts().len() == crate::services::channel_service::DELIVERY_RATE_LIMIT + 1)
        .await;
}
