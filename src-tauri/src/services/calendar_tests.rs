use super::test_support::*;
use crate::domain::activity::Actor;
use crate::error::DomainError;
use crate::services::{CreateEventInput, OverrideInput, UpdateEventInput};

const U: Actor = Actor::User;

fn ev(title: &str, start: &str, end: &str, tz: &str, rule: Option<&str>) -> CreateEventInput {
    CreateEventInput {
        title: title.into(),
        description: None,
        location: None,
        all_day: None,
        start_at: start.into(),
        end_at: end.into(),
        timezone: tz.into(),
        recurrence_rule: rule.map(str::to_string),
    }
}

#[tokio::test]
async fn event_crud_and_validation() {
    let c = ctx().await;
    let e = c
        .calendar
        .create_event(
            ev(
                "Dentist",
                "2026-11-24T10:00:00+01:00",
                "2026-11-24T11:00:00+01:00",
                "Europe/Rome",
                None,
            ),
            U,
        )
        .await
        .unwrap();
    assert_eq!(e.start_at, "2026-11-24T09:00:00.000Z");
    assert!(e.uid.ends_with("@orbit"));

    let items = c
        .calendar
        .list_range("2026-11-24T00:00:00Z", "2026-11-25T00:00:00Z")
        .await
        .unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].title, "Dentist");

    let e = c
        .calendar
        .update_event(
            &e.id,
            UpdateEventInput {
                title: Some("Dentist!".into()),
                location: Some("Clinic".into()),
                ..Default::default()
            },
            U,
        )
        .await
        .unwrap();
    assert_eq!(
        (e.title.as_str(), e.location.as_deref()),
        ("Dentist!", Some("Clinic"))
    );

    for bad in [
        ev(
            "x",
            "2026-11-24T11:00:00Z",
            "2026-11-24T10:00:00Z",
            "UTC",
            None,
        ),
        ev("x", "nope", "2026-11-24T10:00:00Z", "UTC", None),
        ev(
            "x",
            "2026-11-24T10:00:00Z",
            "2026-11-24T11:00:00Z",
            "Nowhere/City",
            None,
        ),
        ev(
            "x",
            "2026-11-24T10:00:00Z",
            "2026-11-24T11:00:00Z",
            "UTC",
            Some("FREQ=SOMETIMES"),
        ),
        ev(
            " ",
            "2026-11-24T10:00:00Z",
            "2026-11-24T11:00:00Z",
            "UTC",
            None,
        ),
    ] {
        assert!(matches!(
            c.calendar.create_event(bad, U).await,
            Err(DomainError::Validation(_))
        ));
    }
    assert!(matches!(
        c.calendar
            .list_range("2026-01-01T00:00:00Z", "2028-01-01T00:00:00Z")
            .await,
        Err(DomainError::Validation(_))
    ));
    assert!(matches!(
        c.calendar
            .list_range("2026-02-01T00:00:00Z", "2026-01-01T00:00:00Z")
            .await,
        Err(DomainError::Validation(_))
    ));

    c.calendar.delete_event(&e.id, U).await.unwrap();
    assert!(
        c.calendar
            .list_range("2026-11-24T00:00:00Z", "2026-11-25T00:00:00Z")
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn all_day_events_snap_to_local_midnight() {
    let c = ctx().await;
    let mut input = ev(
        "Holiday",
        "2026-12-25T15:30:00+01:00",
        "2026-12-25T18:00:00+01:00",
        "Europe/Rome",
        None,
    );
    input.all_day = Some(true);
    let e = c.calendar.create_event(input, U).await.unwrap();
    assert_eq!(e.start_at, "2026-12-24T23:00:00.000Z");
    assert_eq!(e.end_at, "2026-12-25T23:00:00.000Z");
}

#[tokio::test]
async fn recurring_event_expands_across_dst_with_skip_and_override() {
    let c = ctx().await;
    // Mondays 09:00 Rome; DST ends 2026-10-25.
    let e = c
        .calendar
        .create_event(
            ev(
                "Standup",
                "2026-10-19T09:00:00+02:00",
                "2026-10-19T09:30:00+02:00",
                "Europe/Rome",
                Some("FREQ=WEEKLY;BYDAY=MO"),
            ),
            U,
        )
        .await
        .unwrap();
    let list = |from: &'static str, to: &'static str| {
        let c = &c;
        async move { c.calendar.list_range(from, to).await.unwrap() }
    };
    let items = list("2026-10-19T00:00:00Z", "2026-11-10T00:00:00Z").await;
    let starts: Vec<_> = items.iter().map(|i| i.start_at.as_str()).collect();
    assert_eq!(
        starts,
        [
            "2026-10-19T07:00:00.000Z",
            "2026-10-26T08:00:00.000Z",
            "2026-11-02T08:00:00.000Z",
            "2026-11-09T08:00:00.000Z"
        ]
    );
    assert!(
        items
            .iter()
            .all(|i| i.recurring && i.occurrence_start.is_some())
    );

    c.calendar
        .skip_occurrence(&e.id, "2026-10-26T08:00:00.000Z", U)
        .await
        .unwrap();
    c.calendar
        .override_occurrence(
            &e.id,
            "2026-11-02T08:00:00.000Z",
            OverrideInput {
                title: Some("Retro".into()),
                start_at: Some("2026-11-03T15:00:00Z".into()),
                end_at: Some("2026-11-03T16:00:00Z".into()),
                ..Default::default()
            },
            U,
        )
        .await
        .unwrap();
    let items = list("2026-10-19T00:00:00Z", "2026-11-10T00:00:00Z").await;
    let seen: Vec<_> = items
        .iter()
        .map(|i| (i.title.as_str(), i.start_at.as_str()))
        .collect();
    assert_eq!(
        seen,
        [
            ("Standup", "2026-10-19T07:00:00.000Z"),
            ("Retro", "2026-11-03T15:00:00.000Z"),
            ("Standup", "2026-11-09T08:00:00.000Z")
        ]
    );
    assert!(items[1].overridden);

    // restoring brings the series occurrences back
    c.calendar
        .restore_occurrence(&e.id, "2026-10-26T08:00:00.000Z", U)
        .await
        .unwrap();
    c.calendar
        .restore_occurrence(&e.id, "2026-11-02T08:00:00.000Z", U)
        .await
        .unwrap();
    assert_eq!(
        list("2026-10-19T00:00:00Z", "2026-11-10T00:00:00Z")
            .await
            .len(),
        4
    );

    // changing the rule resets exceptions
    c.calendar
        .skip_occurrence(&e.id, "2026-10-26T08:00:00.000Z", U)
        .await
        .unwrap();
    c.calendar
        .update_event(
            &e.id,
            UpdateEventInput {
                recurrence_rule: Some("FREQ=WEEKLY;BYDAY=MO;COUNT=2".into()),
                ..Default::default()
            },
            U,
        )
        .await
        .unwrap();
    assert_eq!(
        list("2026-10-19T00:00:00Z", "2026-11-10T00:00:00Z")
            .await
            .len(),
        2
    );

    // a one-off event cannot be skipped
    let one = c
        .calendar
        .create_event(
            ev(
                "One",
                "2026-11-05T10:00:00Z",
                "2026-11-05T11:00:00Z",
                "UTC",
                None,
            ),
            U,
        )
        .await
        .unwrap();
    assert!(matches!(
        c.calendar
            .skip_occurrence(&one.id, "2026-11-05T10:00:00Z", U)
            .await,
        Err(DomainError::Validation(_))
    ));
}

#[tokio::test]
async fn occurrences_that_started_earlier_still_show_when_overlapping_range() {
    let c = ctx().await;
    c.calendar
        .create_event(
            ev(
                "Overnight",
                "2026-11-01T22:00:00Z",
                "2026-11-02T06:00:00Z",
                "UTC",
                Some("FREQ=DAILY"),
            ),
            U,
        )
        .await
        .unwrap();
    let items = c
        .calendar
        .list_range("2026-11-05T00:00:00Z", "2026-11-05T12:00:00Z")
        .await
        .unwrap();
    // the 4 Nov 22:00 occurrence runs into the range; 5 Nov 22:00 starts after it
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].start_at, "2026-11-04T22:00:00.000Z");
}

#[tokio::test]
async fn task_deadlines_and_recurring_tasks_appear_in_calendar() {
    let c = ctx().await;
    let p = c.projects.create_project("P", None, None, U).await.unwrap();
    let b = c.projects.list_boards(&p.id).await.unwrap().remove(0);
    let col = c.tasks.board_view(&b.id).await.unwrap().columns.remove(0);

    let mut once = new_task(&col.id, "Submit report");
    once.due_at = Some("2026-11-26T12:00:00Z".into());
    let once = c.tasks.create_task(once, U).await.unwrap();
    let mut weekly = new_task(&col.id, "Water plants");
    weekly.due_at = Some("2026-11-02T08:00:00Z".into());
    let weekly = c.tasks.create_task(weekly, U).await.unwrap();
    c.calendar
        .set_task_recurrence(&weekly.id, Some("FREQ=WEEKLY"), "Europe/Rome", U)
        .await
        .unwrap();

    let items = c
        .calendar
        .list_range("2026-11-01T00:00:00Z", "2026-11-30T00:00:00Z")
        .await
        .unwrap();
    let tasks: Vec<_> = items.iter().filter(|i| i.kind == "task").collect();
    assert_eq!(tasks.len(), 5); // 1 one-off + weekly on Nov 2, 9, 16, 23 (Nov 30 is outside the range)
    let one = tasks.iter().find(|i| i.source_id == once.id).unwrap();
    assert_eq!(one.board_id.as_deref(), Some(b.id.as_str()));
    assert_eq!(one.project_id.as_deref(), Some(p.id.as_str()));
    assert!(!one.recurring);

    // complete one occurrence, skip another
    c.calendar
        .set_task_occurrence(&weekly.id, "2026-11-09T08:00:00.000Z", "done", U)
        .await
        .unwrap();
    c.calendar
        .set_task_occurrence(&weekly.id, "2026-11-16T08:00:00.000Z", "skipped", U)
        .await
        .unwrap();
    let items = c
        .calendar
        .list_range("2026-11-01T00:00:00Z", "2026-11-30T00:00:00Z")
        .await
        .unwrap();
    let rec: Vec<_> = items.iter().filter(|i| i.source_id == weekly.id).collect();
    assert_eq!(rec.len(), 3);
    assert!(
        rec.iter()
            .find(|i| i.start_at.starts_with("2026-11-09"))
            .unwrap()
            .completed
    );
    assert!(!rec.iter().any(|i| i.start_at.starts_with("2026-11-16")));
    c.calendar
        .set_task_occurrence(&weekly.id, "2026-11-09T08:00:00.000Z", "open", U)
        .await
        .unwrap();

    assert!(matches!(
        c.calendar
            .set_task_occurrence(&once.id, "2026-11-26T12:00:00Z", "done", U)
            .await,
        Err(DomainError::Validation(_))
    ));
    assert!(matches!(
        c.calendar
            .set_task_recurrence(&once.id, Some("FREQ=NOPE"), "UTC", U)
            .await,
        Err(DomainError::Validation(_))
    ));
    // removing the recurrence turns it back into a plain deadline
    c.calendar
        .set_task_recurrence(&weekly.id, None, "UTC", U)
        .await
        .unwrap();
    let items = c
        .calendar
        .list_range("2026-11-01T00:00:00Z", "2026-11-30T00:00:00Z")
        .await
        .unwrap();
    assert_eq!(items.iter().filter(|i| i.source_id == weekly.id).count(), 1);
}

#[tokio::test]
async fn ics_export_import_roundtrip_dedupes_by_uid() {
    let c = ctx().await;
    c.calendar
        .create_event(
            ev(
                "A",
                "2026-11-24T10:00:00Z",
                "2026-11-24T11:00:00Z",
                "UTC",
                None,
            ),
            U,
        )
        .await
        .unwrap();
    let rec = c
        .calendar
        .create_event(
            ev(
                "B",
                "2026-10-19T09:00:00+02:00",
                "2026-10-19T10:00:00+02:00",
                "Europe/Rome",
                Some("FREQ=WEEKLY"),
            ),
            U,
        )
        .await
        .unwrap();
    c.calendar
        .skip_occurrence(&rec.id, "2026-10-26T08:00:00.000Z", U)
        .await
        .unwrap();
    let ics = c.calendar.export_ics().await.unwrap();

    // importing into the same database: everything is a duplicate
    let s = c.calendar.import_ics(&ics, "UTC", U).await.unwrap();
    assert_eq!((s.created, s.skipped_duplicates), (0, 2));

    // importing into a fresh database recreates both, including the skipped occurrence
    let d = ctx().await;
    let s = d.calendar.import_ics(&ics, "UTC", U).await.unwrap();
    assert_eq!((s.created, s.skipped_duplicates), (2, 0));
    let items = d
        .calendar
        .list_range("2026-10-19T00:00:00Z", "2026-11-10T00:00:00Z")
        .await
        .unwrap();
    let b: Vec<_> = items
        .iter()
        .filter(|i| i.title == "B")
        .map(|i| i.start_at.as_str())
        .collect();
    assert_eq!(
        b,
        [
            "2026-10-19T07:00:00.000Z",
            "2026-11-02T08:00:00.000Z",
            "2026-11-09T08:00:00.000Z"
        ]
    );
    // second import is fully deduplicated
    let s = d.calendar.import_ics(&ics, "UTC", U).await.unwrap();
    assert_eq!((s.created, s.skipped_duplicates), (0, 2));
    // the first import shares one batch id; the deduplicated second import logs nothing
    let n: i64 = sqlx::query_as::<_, (i64,)>(
        "SELECT COUNT(DISTINCT batch_id) FROM activity_log WHERE batch_id IS NOT NULL",
    )
    .fetch_one(&d.pool)
    .await
    .unwrap()
    .0;
    assert_eq!(n, 1);
    assert!(
        d.calendar
            .import_ics("not a calendar", "UTC", U)
            .await
            .is_err()
    );
}
