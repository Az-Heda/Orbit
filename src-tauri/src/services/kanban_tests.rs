use super::test_support::*;
use crate::domain::activity::Actor;
use crate::error::DomainError;
use crate::services::UpdateTaskInput;

const U: Actor = Actor::User;

/// Create a project and return (board_id, [backlog, active, done] column ids).
async fn board(c: &Ctx) -> (String, Vec<String>) {
    let p = c.projects.create_project("P", None, None, U).await.unwrap();
    let b = c.projects.list_boards(&p.id).await.unwrap().remove(0);
    let v = c.tasks.board_view(&b.id).await.unwrap();
    (b.id, v.columns.into_iter().map(|c| c.id).collect())
}

async fn log_count(c: &Ctx, entity: &str, action: &str) -> i64 {
    sqlx::query_as::<_, (i64,)>(
        "SELECT COUNT(*) FROM activity_log WHERE entity_type = ? AND action = ?",
    )
    .bind(entity)
    .bind(action)
    .fetch_one(&c.pool)
    .await
    .unwrap()
    .0
}

#[tokio::test]
async fn project_gets_default_board_and_columns() {
    let c = ctx().await;
    let (board_id, cols) = board(&c).await;
    assert_eq!(cols.len(), 3);
    let v = c.tasks.board_view(&board_id).await.unwrap();
    let kinds: Vec<_> = v.columns.iter().map(|c| c.kind.as_str()).collect();
    assert_eq!(kinds, ["backlog", "active", "done"]);
    assert_eq!(log_count(&c, "project", "create").await, 1);
    assert_eq!(log_count(&c, "column", "create").await, 3);
}

#[tokio::test]
async fn validation_rejects_bad_input() {
    let c = ctx().await;
    let (_, cols) = board(&c).await;
    assert!(matches!(
        c.projects.create_project("  ", None, None, U).await,
        Err(DomainError::Validation(_))
    ));
    assert!(matches!(
        c.tasks.create_task(new_task("not-a-uuid", "x"), U).await,
        Err(DomainError::Validation(_))
    ));
    let mut t = new_task(&cols[0], "x");
    t.priority = Some(9);
    assert!(matches!(
        c.tasks.create_task(t, U).await,
        Err(DomainError::Validation(_))
    ));
    let mut t = new_task(&cols[0], "x");
    t.due_at = Some("tomorrow".into());
    assert!(matches!(
        c.tasks.create_task(t, U).await,
        Err(DomainError::Validation(_))
    ));
    assert!(matches!(
        c.tasks
            .create_task(new_task(&uuid::Uuid::new_v4().to_string(), "x"), U)
            .await,
        Err(DomainError::NotFound { .. })
    ));
}

#[tokio::test]
async fn task_crud_priority_due_and_activity() {
    let c = ctx().await;
    let (board_id, cols) = board(&c).await;
    let mut input = new_task(&cols[0], "Write docs");
    input.priority = Some(2);
    input.due_at = Some("2026-12-01T10:00:00+01:00".into());
    let t = c.tasks.create_task(input, U).await.unwrap();
    assert_eq!(t.priority, 2);
    assert_eq!(t.due_at.as_deref(), Some("2026-12-01T09:00:00.000Z"));

    let t = c
        .tasks
        .update_task(
            &t.id,
            UpdateTaskInput {
                title: Some("Docs".into()),
                description: Some("d".into()),
                priority: Some(3),
                due_at: Some("".into()),
            },
            U,
        )
        .await
        .unwrap();
    assert_eq!(
        (t.title.as_str(), t.priority, t.due_at.clone()),
        ("Docs", 3, None)
    );
    assert_eq!(t.description.as_deref(), Some("d"));

    c.tasks.delete_task(&t.id, U).await.unwrap();
    assert!(
        c.tasks
            .board_view(&board_id)
            .await
            .unwrap()
            .tasks
            .is_empty()
    );
    assert!(matches!(
        c.tasks.get_task(&t.id).await,
        Err(DomainError::NotFound { .. })
    ));
    assert_eq!(log_count(&c, "task", "create").await, 1);
    assert_eq!(log_count(&c, "task", "update").await, 1);
    assert_eq!(log_count(&c, "task", "delete").await, 1);
}

#[tokio::test]
async fn moving_reorders_and_sets_completion() {
    let c = ctx().await;
    let (board_id, cols) = board(&c).await;
    let a = c
        .tasks
        .create_task(new_task(&cols[0], "a"), U)
        .await
        .unwrap();
    let b = c
        .tasks
        .create_task(new_task(&cols[0], "b"), U)
        .await
        .unwrap();
    let d = c
        .tasks
        .create_task(new_task(&cols[0], "d"), U)
        .await
        .unwrap();
    // move d to the front
    c.tasks
        .move_task(&d.id, &cols[0], 0, false, U)
        .await
        .unwrap();
    let v = c.tasks.board_view(&board_id).await.unwrap();
    let order: Vec<_> = v.tasks.iter().map(|t| t.title.as_str()).collect();
    assert_eq!(order, ["d", "a", "b"]);

    let done = c
        .tasks
        .move_task(&a.id, &cols[2], 0, false, U)
        .await
        .unwrap();
    assert!(done.completed_at.is_some());
    let back = c
        .tasks
        .move_task(&a.id, &cols[1], 0, false, U)
        .await
        .unwrap();
    assert!(back.completed_at.is_none());
    // cross-board moves are rejected
    let (_, other) = board(&c).await;
    assert!(matches!(
        c.tasks.move_task(&b.id, &other[0], 0, false, U).await,
        Err(DomainError::Validation(_))
    ));
}

#[tokio::test]
async fn subtasks_checklist_and_parent_completion_warning() {
    let c = ctx().await;
    let (board_id, cols) = board(&c).await;
    let parent = c
        .tasks
        .create_task(new_task(&cols[1], "parent"), U)
        .await
        .unwrap();
    let s1 = c
        .tasks
        .create_task(sub_task(&parent.id, "s1"), U)
        .await
        .unwrap();
    let s2 = c
        .tasks
        .create_task(sub_task(&s1.id, "s1.1"), U)
        .await
        .unwrap();
    assert_eq!(s1.column_id, parent.column_id);
    assert_eq!(s2.parent_id.as_deref(), Some(s1.id.as_str()));
    c.tasks
        .add_checklist_item(&parent.id, "step", U)
        .await
        .unwrap();

    // subtasks cannot be dragged on their own
    assert!(matches!(
        c.tasks.move_task(&s1.id, &cols[2], 0, false, U).await,
        Err(DomainError::Validation(_))
    ));

    // completing the parent warns, force succeeds
    assert!(matches!(
        c.tasks.set_completed(&parent.id, true, false, U).await,
        Err(DomainError::RequiresConfirmation(_))
    ));
    assert!(matches!(
        c.tasks.move_task(&parent.id, &cols[2], 0, false, U).await,
        Err(DomainError::RequiresConfirmation(_))
    ));
    let done = c
        .tasks
        .set_completed(&parent.id, true, true, U)
        .await
        .unwrap();
    assert!(done.completed_at.is_some());
    assert_eq!(done.column_id, cols[2]);

    // subtasks followed their parent
    let v = c.tasks.board_view(&board_id).await.unwrap();
    assert!(v.tasks.iter().all(|t| t.column_id == cols[2]));
    assert_eq!(v.checklist_items.len(), 1);

    // deleting the parent deletes descendants
    c.tasks.delete_task(&parent.id, U).await.unwrap();
    assert!(
        c.tasks
            .board_view(&board_id)
            .await
            .unwrap()
            .tasks
            .is_empty()
    );
}

#[tokio::test]
async fn subtask_completion_and_checklist_toggle() {
    let c = ctx().await;
    let (_, cols) = board(&c).await;
    let parent = c
        .tasks
        .create_task(new_task(&cols[1], "parent"), U)
        .await
        .unwrap();
    let s = c
        .tasks
        .create_task(sub_task(&parent.id, "s"), U)
        .await
        .unwrap();
    let s = c.tasks.set_completed(&s.id, true, false, U).await.unwrap();
    assert!(s.completed_at.is_some());
    // no open subtasks left, so the parent completes without a warning
    c.tasks
        .set_completed(&parent.id, true, false, U)
        .await
        .unwrap();

    let item = c
        .tasks
        .add_checklist_item(&parent.id, "x", U)
        .await
        .unwrap();
    let item = c
        .tasks
        .update_checklist_item(&item.id, None, Some(true), U)
        .await
        .unwrap();
    assert!(item.done);
    c.tasks.delete_checklist_item(&item.id, U).await.unwrap();
}

#[tokio::test]
async fn dependencies_reject_cycles_and_warn_when_blocked() {
    let c = ctx().await;
    let (_, cols) = board(&c).await;
    let a = c
        .tasks
        .create_task(new_task(&cols[0], "a"), U)
        .await
        .unwrap();
    let b = c
        .tasks
        .create_task(new_task(&cols[0], "b"), U)
        .await
        .unwrap();
    let d = c
        .tasks
        .create_task(new_task(&cols[0], "d"), U)
        .await
        .unwrap();

    // b is blocked by a, d is blocked by b
    c.tasks.add_dependency(&b.id, &a.id, U).await.unwrap();
    c.tasks.add_dependency(&d.id, &b.id, U).await.unwrap();
    // self edge, direct cycle, transitive cycle
    assert!(matches!(
        c.tasks.add_dependency(&a.id, &a.id, U).await,
        Err(DomainError::Validation(_))
    ));
    assert!(matches!(
        c.tasks.add_dependency(&a.id, &b.id, U).await,
        Err(DomainError::Validation(_))
    ));
    assert!(matches!(
        c.tasks.add_dependency(&a.id, &d.id, U).await,
        Err(DomainError::Validation(_))
    ));
    // duplicate add is idempotent and logs once
    c.tasks.add_dependency(&b.id, &a.id, U).await.unwrap();
    assert_eq!(log_count(&c, "task_dependency", "create").await, 2);

    // blocked task cannot start or finish silently
    assert!(matches!(
        c.tasks.move_task(&b.id, &cols[1], 0, false, U).await,
        Err(DomainError::RequiresConfirmation(_))
    ));
    assert!(matches!(
        c.tasks.move_task(&b.id, &cols[2], 0, false, U).await,
        Err(DomainError::RequiresConfirmation(_))
    ));
    // moving within backlog is fine; force overrides
    c.tasks
        .move_task(&b.id, &cols[0], 0, false, U)
        .await
        .unwrap();
    c.tasks
        .move_task(&b.id, &cols[1], 0, true, U)
        .await
        .unwrap();

    // finishing the blocker unblocks the dependent
    c.tasks
        .move_task(&a.id, &cols[2], 0, false, U)
        .await
        .unwrap();
    c.tasks
        .move_task(&d.id, &cols[0], 0, false, U)
        .await
        .unwrap();
    c.tasks.remove_dependency(&b.id, &a.id, U).await.unwrap();
    assert!(matches!(
        c.tasks.remove_dependency(&b.id, &a.id, U).await,
        Err(DomainError::NotFound { .. })
    ));
}

#[tokio::test]
async fn templates_instantiate_atomically_with_batch_log() {
    let c = ctx().await;
    let (board_id, cols) = board(&c).await;
    let list = c.templates.list().await.unwrap();
    let new_project = list
        .iter()
        .find(|t| t.name == "New project")
        .expect("seeded template");
    let created = c
        .templates
        .instantiate(&new_project.id, &cols[0], None, U)
        .await
        .unwrap();
    assert_eq!(created.len(), 4);
    let v = c.tasks.board_view(&board_id).await.unwrap();
    assert_eq!(v.tasks.len(), 4);
    assert!(v.tasks.iter().any(|t| t.due_at.is_some()));
    assert_eq!(v.checklist_items.len(), 2);
    // one batch id for the whole instantiation
    let batches: i64 = sqlx::query_as::<_, (i64,)>(
        "SELECT COUNT(DISTINCT batch_id) FROM activity_log WHERE batch_id IS NOT NULL",
    )
    .fetch_one(&c.pool)
    .await
    .unwrap()
    .0;
    assert_eq!(batches, 1);

    // save a task tree as a template and instantiate it elsewhere
    let parent = c
        .tasks
        .create_task(new_task(&cols[0], "Release"), U)
        .await
        .unwrap();
    c.tasks
        .create_task(sub_task(&parent.id, "Tag"), U)
        .await
        .unwrap();
    c.tasks
        .add_checklist_item(&parent.id, "notes", U)
        .await
        .unwrap();
    let tpl = c
        .templates
        .save_task_as_template(&parent.id, "Release flow", U)
        .await
        .unwrap();
    let made = c
        .templates
        .instantiate(&tpl.id, &cols[1], None, U)
        .await
        .unwrap();
    assert_eq!(made.len(), 1);
    let v = c.tasks.board_view(&board_id).await.unwrap();
    let sub = v
        .tasks
        .iter()
        .find(|t| t.parent_id.as_deref() == Some(made[0].id.as_str()))
        .expect("subtask");
    assert_eq!(sub.title, "Tag");
    assert_eq!(sub.column_id, cols[1]);

    // invalid templates are rejected
    assert!(matches!(
        c.templates.create("x", None, vec![], U).await,
        Err(DomainError::Validation(_))
    ));
}

#[tokio::test]
async fn deleting_column_requires_empty_and_project_delete_cascades() {
    let c = ctx().await;
    let (board_id, cols) = board(&c).await;
    c.tasks
        .create_task(new_task(&cols[0], "a"), U)
        .await
        .unwrap();
    assert!(matches!(
        c.projects.delete_column(&cols[0], U).await,
        Err(DomainError::Validation(_))
    ));
    c.projects.delete_column(&cols[1], U).await.unwrap();
    assert_eq!(
        c.tasks.board_view(&board_id).await.unwrap().columns.len(),
        2
    );

    let p = c.projects.list_projects().await.unwrap().remove(0);
    c.projects.delete_project(&p.id, U).await.unwrap();
    assert!(c.projects.list_projects().await.unwrap().is_empty());
    assert!(matches!(
        c.tasks.board_view(&board_id).await,
        Err(DomainError::NotFound { .. })
    ));
    let live: i64 =
        sqlx::query_as::<_, (i64,)>("SELECT COUNT(*) FROM tasks WHERE deleted_at IS NULL")
            .fetch_one(&c.pool)
            .await
            .unwrap()
            .0;
    assert_eq!(live, 0);
}
