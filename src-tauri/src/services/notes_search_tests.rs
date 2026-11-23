use super::test_support::*;
use crate::domain::activity::Actor;
use crate::error::DomainError;

const U: Actor = Actor::User;

async fn search_ids(c: &Ctx, q: &str) -> Vec<(&'static str, String)> {
    c.search
        .search(q, None, None)
        .await
        .unwrap()
        .into_iter()
        .map(|h| (h.kind, h.title))
        .collect()
}

#[tokio::test]
async fn notes_crud_journal_and_links() {
    let c = ctx().await;
    let p = c
        .projects
        .create_project("Website", None, None, U)
        .await
        .unwrap();
    let n = c
        .notes
        .create("Auth decision", "We chose **OAuth** with PKCE", U)
        .await
        .unwrap();
    assert_eq!(n.kind, "note");

    c.notes.add_link(&n.id, "project", &p.id, U).await.unwrap();
    c.notes.add_link(&n.id, "project", &p.id, U).await.unwrap(); // idempotent
    let d = c.notes.get(&n.id).await.unwrap();
    assert_eq!(d.links.len(), 1);
    assert_eq!(d.links[0].title.as_deref(), Some("Website"));
    let linked = c
        .notes
        .list(None, Some(("project", &p.id)), 50)
        .await
        .unwrap();
    assert_eq!(linked.len(), 1);
    // link targets must exist and types are validated
    assert!(matches!(
        c.notes
            .add_link(&n.id, "project", &uuid::Uuid::new_v4().to_string(), U)
            .await,
        Err(DomainError::NotFound { .. })
    ));
    assert!(matches!(
        c.notes.add_link(&n.id, "banana", &p.id, U).await,
        Err(DomainError::Validation(_))
    ));
    c.notes
        .remove_link(&n.id, "project", &p.id, U)
        .await
        .unwrap();
    assert!(
        c.notes
            .list(None, Some(("project", &p.id)), 50)
            .await
            .unwrap()
            .is_empty()
    );

    // journal: one entry per date, get-or-create
    let j1 = c.notes.journal("2026-11-23", U).await.unwrap();
    let j2 = c.notes.journal("2026-11-23", U).await.unwrap();
    assert_eq!(j1.id, j2.id);
    assert_eq!(j1.kind, "journal");
    assert!(matches!(
        c.notes.journal("23/11/2026", U).await,
        Err(DomainError::Validation(_))
    ));
    assert_eq!(
        c.notes.list(Some("journal"), None, 10).await.unwrap().len(),
        1
    );

    let u = c.notes.update(&n.id, Some("Auth"), None, U).await.unwrap();
    assert_eq!(
        (u.title.as_str(), u.body.as_str()),
        ("Auth", "We chose **OAuth** with PKCE")
    );
    assert!(matches!(
        c.notes.create("  ", "x", U).await,
        Err(DomainError::Validation(_))
    ));
    c.notes.delete(&n.id, U).await.unwrap();
    assert!(matches!(
        c.notes.get(&n.id).await,
        Err(DomainError::NotFound { .. })
    ));
    let logged: i64 =
        sqlx::query_as::<_, (i64,)>("SELECT COUNT(*) FROM activity_log WHERE entity_type = 'note'")
            .fetch_one(&c.pool)
            .await
            .unwrap()
            .0;
    assert_eq!(logged, 4); // note create + update + delete, journal create
}

#[tokio::test]
async fn fts_stays_in_sync_through_update_and_delete() {
    let c = ctx().await;
    let n = c
        .notes
        .create("Groceries", "milk and eggs", U)
        .await
        .unwrap();
    assert_eq!(
        search_ids(&c, "eggs").await,
        [("note", "Groceries".to_string())]
    );

    c.notes
        .update(&n.id, None, Some("bread and butter"), U)
        .await
        .unwrap();
    assert!(search_ids(&c, "eggs").await.is_empty());
    assert_eq!(search_ids(&c, "butter").await.len(), 1);
    // prefix matching
    assert_eq!(search_ids(&c, "gro").await.len(), 1);

    c.notes.delete(&n.id, U).await.unwrap();
    assert!(search_ids(&c, "butter").await.is_empty());
    assert!(search_ids(&c, "groceries").await.is_empty());
}

#[tokio::test]
async fn global_search_spans_all_kinds_and_respects_deletion() {
    let c = ctx().await;
    let p = c
        .projects
        .create_project("Rustacean app", Some("about rust"), None, U)
        .await
        .unwrap();
    let b = c.projects.list_boards(&p.id).await.unwrap().remove(0);
    let col = c.tasks.board_view(&b.id).await.unwrap().columns.remove(0);
    let mut t = new_task(&col.id, "Learn rust lifetimes");
    t.description = Some("borrow checker".into());
    let t = c.tasks.create_task(t, U).await.unwrap();
    c.calendar
        .create_event(
            crate::services::CreateEventInput {
                title: "Rust meetup".into(),
                description: None,
                location: Some("Milan".into()),
                all_day: None,
                start_at: "2026-12-01T18:00:00Z".into(),
                end_at: "2026-12-01T20:00:00Z".into(),
                timezone: "UTC".into(),
                recurrence_rule: None,
            },
            U,
        )
        .await
        .unwrap();
    c.notes
        .create("Rust notes", "ownership model", U)
        .await
        .unwrap();
    sqlx::query("INSERT INTO inbox_items (id, raw_text, created_at, updated_at) VALUES ('i1', 'read rust book later', 'x', 'x')")
        .execute(&c.pool).await.unwrap();

    let hits = c.search.search("rust", None, None).await.unwrap();
    let mut kinds: Vec<_> = hits.iter().map(|h| h.kind).collect();
    kinds.sort();
    assert_eq!(kinds, ["event", "inbox", "note", "project", "task"]);
    assert!(
        hits.windows(2).all(|w| w[0].score >= w[1].score),
        "sorted by score"
    );
    let task_hit = hits.iter().find(|h| h.kind == "task").unwrap();
    assert_eq!(task_hit.project_id.as_deref(), Some(p.id.as_str()));
    assert_eq!(task_hit.board_id.as_deref(), Some(b.id.as_str()));
    assert!(task_hit.snippet.contains('⟦'));

    // kind filter, location/description columns, limit
    assert_eq!(
        c.search
            .search("rust", Some(vec!["event".into()]), None)
            .await
            .unwrap()
            .len(),
        1
    );
    assert_eq!(c.search.search("milan", None, None).await.unwrap().len(), 1);
    assert_eq!(
        c.search.search("borrow", None, None).await.unwrap().len(),
        1
    );
    assert_eq!(
        c.search.search("rust", None, Some(2)).await.unwrap().len(),
        2
    );
    assert!(matches!(
        c.search.search("rust", None, Some(0)).await,
        Err(DomainError::Validation(_))
    ));
    assert!(matches!(
        c.search
            .search("rust", Some(vec!["nope".into()]), None)
            .await,
        Err(DomainError::Validation(_))
    ));

    // soft-deleted tasks and projects disappear from results
    c.tasks.delete_task(&t.id, U).await.unwrap();
    assert!(
        c.search
            .search("lifetimes", None, None)
            .await
            .unwrap()
            .is_empty()
    );
    c.projects.delete_project(&p.id, U).await.unwrap();
    assert!(
        c.search
            .search("rustacean", None, None)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn hostile_queries_never_error() {
    let c = ctx().await;
    c.notes.create("Hello world", "text", U).await.unwrap();
    for q in [
        "",
        "   ",
        "\"",
        "foo-",
        "a AND",
        "NEAR(",
        "*",
        "(((",
        "title:",
        "hello OR",
        "\"hello",
        "-hello",
        "hello*",
        "'; DROP TABLE notes;--",
        "col:val",
        "^hello",
    ] {
        assert!(
            c.search.search(q, None, None).await.is_ok(),
            "query {q:?} failed"
        );
    }
    assert_eq!(
        c.search.search("hello-", None, None).await.unwrap().len(),
        1
    );
    assert_eq!(
        c.search
            .search("hello world", None, None)
            .await
            .unwrap()
            .len(),
        1
    );
}
