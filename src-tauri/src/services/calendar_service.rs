use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Duration, TimeZone, Utc};
use serde::Deserialize;

use crate::domain::activity::Actor;
use crate::domain::calendar::*;
use crate::domain::ics;
use crate::domain::recurrence::{clean_rule, occurrences, parse_tz, validate};
use crate::domain::validation::{
    clean_optional, clean_text, fmt_utc, invalid, normalize_utc, parse_utc, parse_uuid,
};
use crate::error::{DomainError, DomainResult};
use crate::infrastructure::database::{CalendarRepository, ImportEvent};

const MAX_RANGE_DAYS: i64 = 400;

#[derive(Debug, Deserialize)]
pub struct CreateEventInput {
    pub title: String,
    pub description: Option<String>,
    pub location: Option<String>,
    pub all_day: Option<bool>,
    pub start_at: String,
    pub end_at: String,
    /// IANA zone the event is expressed/expanded in.
    pub timezone: String,
    pub recurrence_rule: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub struct UpdateEventInput {
    pub title: Option<String>,
    /// Empty string clears.
    pub description: Option<String>,
    /// Empty string clears.
    pub location: Option<String>,
    pub all_day: Option<bool>,
    pub start_at: Option<String>,
    pub end_at: Option<String>,
    pub timezone: Option<String>,
    /// Empty string removes the recurrence.
    pub recurrence_rule: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub struct OverrideInput {
    pub title: Option<String>,
    pub description: Option<String>,
    pub location: Option<String>,
    pub start_at: Option<String>,
    pub end_at: Option<String>,
}

pub struct CalendarService {
    repo: Arc<dyn CalendarRepository>,
}

/// All-day events are stored as local midnights in the event's timezone (end exclusive).
fn snap_all_day(
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    tz: chrono_tz::Tz,
) -> DomainResult<(DateTime<Utc>, DateTime<Utc>)> {
    let midnight = |d: DateTime<Utc>| -> DomainResult<DateTime<Utc>> {
        let date = d.with_timezone(&tz).date_naive();
        tz.from_local_datetime(&date.and_time(chrono::NaiveTime::MIN))
            .earliest()
            .map(|x| x.with_timezone(&Utc))
            .ok_or_else(|| invalid("invalid all-day date"))
    };
    let s = midnight(start)?;
    let mut e = midnight(end)?;
    if e <= s {
        let next = s.with_timezone(&tz).date_naive() + Duration::days(1);
        e = tz
            .from_local_datetime(&next.and_time(chrono::NaiveTime::MIN))
            .earliest()
            .map(|x| x.with_timezone(&Utc))
            .ok_or_else(|| invalid("invalid all-day date"))?;
    }
    Ok((s, e))
}

fn opt(s: &Option<String>, label: &str, max: usize) -> DomainResult<Option<Option<String>>> {
    s.as_deref()
        .map(|v| clean_optional(v, label, max))
        .transpose()
}

impl CalendarService {
    pub fn new(repo: Arc<dyn CalendarRepository>) -> Self {
        Self { repo }
    }

    async fn require_event(&self, id: &str) -> DomainResult<Event> {
        self.repo.get_event(id).await?.ok_or(DomainError::NotFound {
            entity: "event",
            id: id.to_string(),
        })
    }

    /// Validate and normalise the timing + recurrence of an event.
    fn resolve_timing(
        all_day: bool,
        start: &str,
        end: &str,
        tz_name: &str,
        rule: Option<&str>,
    ) -> DomainResult<(String, String, String, Option<String>)> {
        let tz = parse_tz(tz_name)?;
        let mut s = parse_utc(&normalize_utc(start, "start_at")?)?;
        let mut e = parse_utc(&normalize_utc(end, "end_at")?)?;
        if all_day {
            (s, e) = snap_all_day(s, e, tz)?;
        }
        if e < s {
            return Err(invalid("end must not be before start"));
        }
        let rule = rule
            .filter(|r| !r.trim().is_empty())
            .map(clean_rule)
            .transpose()?;
        if let Some(r) = &rule {
            validate(r, tz_name, s)?;
        }
        Ok((fmt_utc(s), fmt_utc(e), tz_name.trim().to_string(), rule))
    }

    pub async fn create_event(&self, input: CreateEventInput, actor: Actor) -> DomainResult<Event> {
        let all_day = input.all_day.unwrap_or(false);
        let (start_at, end_at, timezone, recurrence_rule) = Self::resolve_timing(
            all_day,
            &input.start_at,
            &input.end_at,
            &input.timezone,
            input.recurrence_rule.as_deref(),
        )?;
        self.repo
            .create_event(
                &NewEvent {
                    uid: format!("{}@orbit", uuid::Uuid::new_v4()),
                    title: clean_text(&input.title, "title", 300)?,
                    description: opt(&input.description, "description", 10_000)?.flatten(),
                    location: opt(&input.location, "location", 300)?.flatten(),
                    all_day,
                    start_at,
                    end_at,
                    timezone,
                    recurrence_rule,
                },
                actor,
            )
            .await
    }

    /// Validate an event creation without writing anything.
    pub fn check_create(input: &CreateEventInput) -> DomainResult<()> {
        Self::resolve_timing(
            input.all_day.unwrap_or(false),
            &input.start_at,
            &input.end_at,
            &input.timezone,
            input.recurrence_rule.as_deref(),
        )?;
        clean_text(&input.title, "title", 300)?;
        opt(&input.description, "description", 10_000)?;
        opt(&input.location, "location", 300)?;
        Ok(())
    }

    /// The current event and the validated change that `input` would make to it.
    async fn build_update(
        &self,
        id: &str,
        input: &UpdateEventInput,
    ) -> DomainResult<(String, Event, EventUpdate)> {
        let id = parse_uuid(id, "event id")?;
        let cur = self.require_event(&id).await?;
        ensure_editable(&cur)?;
        let all_day = input.all_day.unwrap_or(cur.all_day);
        let rule = match &input.recurrence_rule {
            Some(r) => Some(r.as_str()).filter(|r| !r.trim().is_empty()),
            None => cur.recurrence_rule.as_deref(),
        };
        let (start_at, end_at, timezone, recurrence_rule) = Self::resolve_timing(
            all_day,
            input.start_at.as_deref().unwrap_or(&cur.start_at),
            input.end_at.as_deref().unwrap_or(&cur.end_at),
            input.timezone.as_deref().unwrap_or(&cur.timezone),
            rule,
        )?;
        let update = EventUpdate {
            title: match &input.title {
                Some(t) => clean_text(t, "title", 300)?,
                None => cur.title.clone(),
            },
            description: opt(&input.description, "description", 10_000)?
                .unwrap_or_else(|| cur.description.clone()),
            location: opt(&input.location, "location", 300)?
                .unwrap_or_else(|| cur.location.clone()),
            all_day,
            start_at,
            end_at,
            timezone,
            recurrence_rule,
        };
        Ok((id, cur, update))
    }

    /// What the event would look like after `update_event`, without writing anything.
    pub async fn plan_update(
        &self,
        id: &str,
        input: &UpdateEventInput,
    ) -> DomainResult<(Event, Event)> {
        let (_, cur, u) = self.build_update(id, input).await?;
        let after = Event {
            title: u.title,
            description: u.description,
            location: u.location,
            all_day: u.all_day,
            start_at: u.start_at,
            end_at: u.end_at,
            timezone: u.timezone,
            recurrence_rule: u.recurrence_rule,
            ..cur.clone()
        };
        Ok((cur, after))
    }

    pub async fn update_event(
        &self,
        id: &str,
        input: UpdateEventInput,
        actor: Actor,
    ) -> DomainResult<Event> {
        let (id, _, update) = self.build_update(id, &input).await?;
        self.repo.update_event(&id, &update, actor).await
    }

    pub async fn get_event(&self, id: &str) -> DomainResult<Event> {
        self.require_event(&parse_uuid(id, "event id")?).await
    }

    pub async fn delete_event(&self, id: &str, actor: Actor) -> DomainResult<()> {
        let id = parse_uuid(id, "event id")?;
        ensure_editable(&self.require_event(&id).await?)?;
        self.repo.delete_event(&id, actor).await
    }

    async fn recurring_event(&self, id: &str) -> DomainResult<Event> {
        let ev = self.require_event(&parse_uuid(id, "event id")?).await?;
        ensure_editable(&ev)?;
        if ev.recurrence_rule.is_none() {
            return Err(invalid("event is not recurring"));
        }
        Ok(ev)
    }

    pub async fn skip_occurrence(
        &self,
        event_id: &str,
        original_start: &str,
        actor: Actor,
    ) -> DomainResult<()> {
        let ev = self.recurring_event(event_id).await?;
        let ex = NewException {
            original_start: normalize_utc(original_start, "original_start")?,
            skip: true,
            ..Default::default()
        };
        self.repo.set_exception(&ev.id, &ex, actor).await
    }

    pub async fn override_occurrence(
        &self,
        event_id: &str,
        original_start: &str,
        input: OverrideInput,
        actor: Actor,
    ) -> DomainResult<()> {
        let ev = self.recurring_event(event_id).await?;
        let start = input
            .start_at
            .as_deref()
            .map(|s| normalize_utc(s, "start_at"))
            .transpose()?;
        let end = input
            .end_at
            .as_deref()
            .map(|s| normalize_utc(s, "end_at"))
            .transpose()?;
        if let (Some(s), Some(e)) = (&start, &end)
            && e < s
        {
            return Err(invalid("end must not be before start"));
        }
        let ex = NewException {
            original_start: normalize_utc(original_start, "original_start")?,
            skip: false,
            title: input
                .title
                .as_deref()
                .map(|t| clean_text(t, "title", 300))
                .transpose()?,
            description: opt(&input.description, "description", 10_000)?.flatten(),
            location: opt(&input.location, "location", 300)?.flatten(),
            start_at: start,
            end_at: end,
        };
        self.repo.set_exception(&ev.id, &ex, actor).await
    }

    /// Remove a skip/override so the occurrence follows the series again.
    pub async fn restore_occurrence(
        &self,
        event_id: &str,
        original_start: &str,
        actor: Actor,
    ) -> DomainResult<()> {
        let id = parse_uuid(event_id, "event id")?;
        ensure_editable(&self.require_event(&id).await?)?;
        self.repo
            .clear_exception(
                &id,
                &normalize_utc(original_start, "original_start")?,
                actor,
            )
            .await
    }

    pub async fn set_task_recurrence(
        &self,
        task_id: &str,
        rule: Option<&str>,
        timezone: &str,
        actor: Actor,
    ) -> DomainResult<()> {
        let id = parse_uuid(task_id, "task id")?;
        let task = self
            .repo
            .task_due_and_rule(&id)
            .await?
            .ok_or(DomainError::NotFound {
                entity: "task",
                id: id.clone(),
            })?;
        match rule.filter(|r| !r.trim().is_empty()) {
            None => self.repo.set_task_recurrence(&id, None, None, actor).await,
            Some(r) => {
                let r = clean_rule(r)?;
                validate(&r, timezone, parse_utc(&task.due_at)?)?;
                self.repo
                    .set_task_recurrence(&id, Some(&r), Some(timezone.trim()), actor)
                    .await
            }
        }
    }

    /// Mark one occurrence of a recurring task `done`, `skipped` or `open`.
    pub async fn set_task_occurrence(
        &self,
        task_id: &str,
        original_due: &str,
        status: &str,
        actor: Actor,
    ) -> DomainResult<()> {
        let id = parse_uuid(task_id, "task id")?;
        let task = self
            .repo
            .task_due_and_rule(&id)
            .await?
            .ok_or(DomainError::NotFound {
                entity: "task",
                id: id.clone(),
            })?;
        if task.recurrence_rule.is_none() {
            return Err(invalid("task is not recurring"));
        }
        let status = match status {
            "done" | "skipped" => Some(status),
            "open" => None,
            _ => return Err(invalid("status must be done, skipped or open")),
        };
        self.repo
            .set_task_occurrence(
                &id,
                &normalize_utc(original_due, "original_due")?,
                status,
                actor,
            )
            .await
    }

    /// Everything to draw in `[from, to)`: event occurrences and task deadlines.
    pub async fn list_range(&self, from: &str, to: &str) -> DomainResult<Vec<CalendarItem>> {
        let from_dt = parse_utc(&normalize_utc(from, "from")?)?;
        let to_dt = parse_utc(&normalize_utc(to, "to")?)?;
        if to_dt <= from_dt {
            return Err(invalid("'to' must be after 'from'"));
        }
        if to_dt - from_dt > Duration::days(MAX_RANGE_DAYS) {
            return Err(invalid(format!(
                "range must be at most {MAX_RANGE_DAYS} days"
            )));
        }
        let (from_s, to_s) = (fmt_utc(from_dt), fmt_utc(to_dt));
        let overlaps = |s: DateTime<Utc>, e: DateTime<Utc>| {
            s < to_dt && (e > from_dt || (e == s && s >= from_dt))
        };
        let mut items = vec![];

        let events = self.repo.events_for_range(&from_s, &to_s).await?;
        let ids: Vec<String> = events
            .iter()
            .filter(|e| e.recurrence_rule.is_some())
            .map(|e| e.id.clone())
            .collect();
        let mut exc_by_event: HashMap<String, Vec<EventException>> = HashMap::new();
        for x in self.repo.exceptions_for(&ids).await? {
            exc_by_event.entry(x.event_id.clone()).or_default().push(x);
        }
        let base =
            |ev: &Event, key: String, start: DateTime<Utc>, end: DateTime<Utc>| CalendarItem {
                key,
                kind: "event",
                source_id: ev.id.clone(),
                title: ev.title.clone(),
                description: ev.description.clone(),
                location: ev.location.clone(),
                start_at: fmt_utc(start),
                end_at: fmt_utc(end),
                all_day: ev.all_day,
                timezone: Some(ev.timezone.clone()),
                recurring: ev.recurrence_rule.is_some(),
                occurrence_start: None,
                overridden: false,
                completed: false,
                priority: 0,
                project_id: None,
                board_id: None,
            };
        for ev in &events {
            let (start, end) = (parse_utc(&ev.start_at)?, parse_utc(&ev.end_at)?);
            let Some(rule) = &ev.recurrence_rule else {
                items.push(base(ev, format!("event:{}", ev.id), start, end));
                continue;
            };
            let dur = end - start;
            let excs = exc_by_event.get(&ev.id).map(Vec::as_slice).unwrap_or(&[]);
            let by_orig: HashMap<&str, &EventException> = excs
                .iter()
                .map(|x| (x.original_start.as_str(), x))
                .collect();
            for occ in occurrences(rule, &ev.timezone, start, from_dt - dur, to_dt)? {
                let orig = fmt_utc(occ);
                if by_orig.contains_key(orig.as_str()) || !overlaps(occ, occ + dur) {
                    continue;
                }
                let mut it = base(ev, format!("event:{}:{orig}", ev.id), occ, occ + dur);
                it.occurrence_start = Some(orig);
                items.push(it);
            }
            // Overrides are positioned by their own (possibly moved) times.
            for x in excs.iter().filter(|x| x.kind == "override") {
                let orig = parse_utc(&x.original_start)?;
                let s = x
                    .start_at
                    .as_deref()
                    .map(parse_utc)
                    .transpose()?
                    .unwrap_or(orig);
                let e = x
                    .end_at
                    .as_deref()
                    .map(parse_utc)
                    .transpose()?
                    .unwrap_or(s + dur);
                if !overlaps(s, e) {
                    continue;
                }
                let mut it = base(ev, format!("event:{}:{}", ev.id, x.original_start), s, e);
                it.title = x.title.clone().unwrap_or_else(|| ev.title.clone());
                it.description = x.description.clone().or_else(|| ev.description.clone());
                it.location = x.location.clone().or_else(|| ev.location.clone());
                it.occurrence_start = Some(x.original_start.clone());
                it.overridden = true;
                items.push(it);
            }
        }

        // Task deadlines (one-off), then recurring tasks expanded by their rule.
        let task_item = |t: &TaskDeadline,
                         key: String,
                         due: &str,
                         recurring: bool,
                         completed: bool,
                         occ: Option<String>| CalendarItem {
            key,
            kind: "task",
            source_id: t.id.clone(),
            title: t.title.clone(),
            description: None,
            location: None,
            start_at: due.to_string(),
            end_at: due.to_string(),
            all_day: false,
            timezone: t.recurrence_tz.clone(),
            recurring,
            occurrence_start: occ,
            overridden: false,
            completed,
            priority: t.priority,
            project_id: Some(t.project_id.clone()),
            board_id: Some(t.board_id.clone()),
        };
        for t in self.repo.task_deadlines_in_range(&from_s, &to_s).await? {
            items.push(task_item(
                &t,
                format!("task:{}", t.id),
                &t.due_at,
                false,
                t.completed_at.is_some(),
                None,
            ));
        }
        let recurring = self.repo.recurring_tasks().await?;
        let rec_ids: Vec<String> = recurring.iter().map(|t| t.id.clone()).collect();
        let mut occ_status: HashMap<(String, String), TaskOccurrence> = HashMap::new();
        for o in self.repo.task_occurrences(&rec_ids).await? {
            occ_status.insert((o.task_id.clone(), o.original_due.clone()), o);
        }
        for t in &recurring {
            let (Some(rule), Some(tz)) = (&t.recurrence_rule, &t.recurrence_tz) else {
                continue;
            };
            for occ in occurrences(rule, tz, parse_utc(&t.due_at)?, from_dt, to_dt)? {
                let due = fmt_utc(occ);
                let st = occ_status.get(&(t.id.clone(), due.clone()));
                if st.is_some_and(|o| o.status == "skipped") {
                    continue;
                }
                let done = st.is_some_and(|o| o.status == "done");
                items.push(task_item(
                    t,
                    format!("task:{}:{due}", t.id),
                    &due,
                    true,
                    done,
                    Some(due.clone()),
                ));
            }
        }

        items.sort_by(|a, b| a.start_at.cmp(&b.start_at).then(a.key.cmp(&b.key)));
        Ok(items)
    }

    pub async fn export_ics(&self) -> DomainResult<String> {
        let events = self.repo.all_events().await?;
        let ids: Vec<String> = events
            .iter()
            .filter(|e| e.recurrence_rule.is_some())
            .map(|e| e.id.clone())
            .collect();
        let mut by_event: HashMap<String, Vec<EventException>> = HashMap::new();
        for x in self.repo.exceptions_for(&ids).await? {
            by_event.entry(x.event_id.clone()).or_default().push(x);
        }
        let items: Vec<_> = events
            .into_iter()
            .map(|e| {
                let x = by_event.remove(&e.id).unwrap_or_default();
                (e, x)
            })
            .collect();
        Ok(ics::export(&items))
    }

    /// Import an .ics document; events whose UID already exists are skipped (deduplicated).
    pub async fn import_ics(
        &self,
        text: &str,
        default_timezone: &str,
        actor: Actor,
    ) -> DomainResult<ImportSummary> {
        if text.len() > 20 * 1024 * 1024 {
            return Err(invalid("file too large"));
        }
        let (parsed, mut summary) =
            ics::parse(text, default_timezone).map_err(DomainError::Validation)?;
        let items: Vec<ImportEvent> = parsed
            .into_iter()
            .map(|p| ImportEvent {
                event: p.event,
                exceptions: p.exceptions,
            })
            .collect();
        let (created, skipped) = self.repo.import_events(&items, actor).await?;
        summary.created = created;
        summary.skipped_duplicates = skipped;
        Ok(summary)
    }
}

/// Events from a subscribed feed are read-only for everyone (UI and AI alike).
pub fn ensure_editable(ev: &Event) -> DomainResult<()> {
    if ev.external_calendar_id.is_some() {
        return Err(invalid(
            "this event comes from a subscribed calendar and is read-only",
        ));
    }
    Ok(())
}
