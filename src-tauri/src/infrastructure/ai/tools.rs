//! The only way Claude touches Orbit. Every tool has a JSON schema, validates its arguments
//! server-side (ids, dates, ranges, enums) and goes through the same services as the UI.
//!
//! * Read tools run immediately and return trimmed data, respecting the sharing settings.
//! * Write tools never write. `plan` validates and builds a preview; the change is only applied
//!   by `apply` after the user confirms that exact proposal.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use chrono::{DateTime, Duration, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};

use crate::domain::activity::Actor;
use crate::domain::ai::{SharingSettings, validate_memory_text};
use crate::domain::planning::{RankInput, Slot, free_slots, rank_tasks};
use crate::domain::recurrence::parse_tz;
use crate::domain::validation::{clean_text, fmt_utc, invalid, parse_uuid};
use crate::error::{DomainError, DomainResult};
use crate::infrastructure::database::AiRepository;
use crate::services::{
    AutomationService, CalendarService, CreateEventInput, CreateTaskInput, InboxService,
    NoteService, ProjectService, RuleInput, SearchService, StartTimerInput, TaskService,
    TemplateService, TimerService, UpdateEventInput, UpdateTaskInput,
};

const MAX_RESULT_ITEMS: usize = 100;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Share {
    Tasks,
    Calendar,
    Notes,
    Inbox,
    Memories,
    Focus,
}

impl Share {
    fn allowed(self, s: &SharingSettings) -> bool {
        match self {
            Share::Tasks => s.tasks,
            Share::Calendar => s.calendar,
            Share::Notes => s.notes,
            Share::Inbox => s.inbox,
            Share::Memories => s.memories,
            Share::Focus => s.focus,
        }
    }
    fn label(self) -> &'static str {
        match self {
            Share::Tasks => "tasks",
            Share::Calendar => "calendar",
            Share::Notes => "notes",
            Share::Inbox => "inbox",
            Share::Memories => "memories",
            Share::Focus => "focus sessions",
        }
    }
}

pub struct ToolSpec {
    pub name: &'static str,
    pub description: &'static str,
    pub schema: Value,
    pub write: bool,
    pub share: Option<Share>,
}

/// Per-request facts the tools need.
#[derive(Clone)]
pub struct ToolEnv {
    pub sharing: SharingSettings,
    pub timezone: String,
    pub now: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Change {
    pub label: String,
    pub before: Option<String>,
    pub after: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Preview {
    pub summary: String,
    /// Deletions (and anything that removes data) are flagged so the UI can warn.
    pub destructive: bool,
    pub changes: Vec<Change>,
}

fn change(label: &str, before: Option<String>, after: Option<String>) -> Change {
    Change {
        label: label.into(),
        before,
        after,
    }
}

fn obj(props: Value, required: &[&str]) -> Value {
    json!({ "type": "object", "properties": props, "required": required, "additionalProperties": false })
}

fn args<T: DeserializeOwned>(input: &Value) -> DomainResult<T> {
    serde_json::from_value(input.clone()).map_err(|e| invalid(format!("invalid arguments: {e}")))
}

// ---------- time helpers ----------

/// Accepts RFC 3339 (with offset), `YYYY-MM-DDTHH:MM[:SS]` or `YYYY-MM-DD`; the last two are
/// read in the user's timezone.
pub fn parse_when(s: &str, tz: Tz) -> DomainResult<DateTime<Utc>> {
    let s = s.trim();
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Ok(dt.with_timezone(&Utc));
    }
    let naive = NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S")
        .or_else(|_| NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M"))
        .or_else(|_| {
            NaiveDate::parse_from_str(s, "%Y-%m-%d").map(|d| d.and_time(chrono::NaiveTime::MIN))
        })
        .map_err(|_| {
            invalid(format!(
                "'{s}' is not a valid date/time (use YYYY-MM-DDTHH:MM)"
            ))
        })?;
    tz.from_local_datetime(&naive)
        .earliest()
        .map(|d| d.with_timezone(&Utc))
        .ok_or_else(|| {
            invalid(format!(
                "'{s}' does not exist in {tz} (daylight saving gap)"
            ))
        })
}

fn local(utc: &str, tz: Tz) -> String {
    DateTime::parse_from_rfc3339(utc)
        .map(|d| d.with_timezone(&tz).format("%a %d %b %Y %H:%M").to_string())
        .unwrap_or_else(|_| utc.to_string())
}

fn slot_json(s: &Slot, tz: Tz) -> Value {
    json!({
        "start": s.start.with_timezone(&tz).to_rfc3339(),
        "end": s.end.with_timezone(&tz).to_rfc3339(),
        "minutes": s.minutes(),
    })
}

fn work_window(start: Option<&str>, end: Option<&str>) -> DomainResult<(NaiveTime, NaiveTime)> {
    let parse = |v: Option<&str>, d: &str| {
        NaiveTime::parse_from_str(v.unwrap_or(d), "%H:%M")
            .map_err(|_| invalid("working hours must be HH:MM"))
    };
    let (s, e) = (parse(start, "09:00")?, parse(end, "18:00")?);
    if e <= s {
        return Err(invalid("work_end must be after work_start"));
    }
    Ok((s, e))
}

/// `[start, end)` of a local calendar day (default: today in the user's timezone).
fn day_bounds(
    date: Option<&str>,
    now: DateTime<Utc>,
    tz: Tz,
) -> DomainResult<(DateTime<Utc>, DateTime<Utc>, NaiveDate)> {
    let day = match date {
        Some(d) => NaiveDate::parse_from_str(d.trim(), "%Y-%m-%d")
            .map_err(|_| invalid("date must be YYYY-MM-DD"))?,
        None => now.with_timezone(&tz).date_naive(),
    };
    let at = |d: NaiveDate| {
        tz.from_local_datetime(&d.and_hms_opt(0, 0, 0).unwrap_or_default())
            .earliest()
            .map(|x| x.with_timezone(&Utc))
            .ok_or_else(|| invalid("that date does not exist in the timezone"))
    };
    Ok((at(day)?, at(day + Duration::days(1))?, day))
}

fn local_iso(utc: &str, tz: Tz) -> String {
    DateTime::parse_from_rfc3339(utc)
        .map(|d| d.with_timezone(&tz).to_rfc3339())
        .unwrap_or_else(|_| utc.to_string())
}

fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(max).collect::<String>())
    }
}

fn strip_marks(s: &str) -> String {
    s.replace(['⟦', '⟧'], "")
}

// ---------- argument structs ----------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GetSchedule {
    from: String,
    to: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GetTasks {
    board_id: Option<String>,
    include_done: Option<bool>,
    limit: Option<usize>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct QueryArgs {
    query: String,
    kinds: Option<Vec<String>>,
    limit: Option<i64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NoteId {
    note_id: String,
    max_chars: Option<usize>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Limit {
    limit: Option<i64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateTaskArgs {
    column_id: String,
    title: String,
    description: Option<String>,
    priority: Option<i64>,
    due: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateSubtaskArgs {
    parent_id: String,
    title: String,
    description: Option<String>,
    priority: Option<i64>,
    due: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateTaskArgs {
    task_id: String,
    title: Option<String>,
    description: Option<String>,
    priority: Option<i64>,
    /// Empty string clears the due date.
    due: Option<String>,
    completed: Option<bool>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MoveTaskArgs {
    task_id: String,
    column_id: String,
    position: Option<i64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TaskIdArgs {
    task_id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateEventArgs {
    title: String,
    description: Option<String>,
    location: Option<String>,
    all_day: Option<bool>,
    start: String,
    end: Option<String>,
    timezone: Option<String>,
    recurrence_rule: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateEventArgs {
    event_id: String,
    title: Option<String>,
    description: Option<String>,
    location: Option<String>,
    all_day: Option<bool>,
    start: Option<String>,
    end: Option<String>,
    timezone: Option<String>,
    recurrence_rule: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EventIdArgs {
    event_id: String,
}

#[derive(Deserialize, Clone, Copy, PartialEq)]
#[serde(rename_all = "snake_case")]
enum TimerKind {
    Pomodoro,
    Countdown,
    Stopwatch,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StartTimerArgs {
    kind: TimerKind,
    minutes: Option<i64>,
    label: Option<String>,
    task_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateNoteArgs {
    title: String,
    body: String,
}

#[derive(Deserialize, Clone, Copy, PartialEq)]
#[serde(rename_all = "snake_case")]
enum TriageAs {
    Task,
    Event,
    Dismiss,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TriageArgs {
    item_id: String,
    #[serde(rename = "as")]
    kind: TriageAs,
    column_id: Option<String>,
    title: Option<String>,
    due: Option<String>,
    start: Option<String>,
    end: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DependencyArgs {
    task_id: String,
    blocked_by_id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FromTemplateArgs {
    template_id: String,
    column_id: String,
    parent_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RememberArgs {
    text: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ForgetArgs {
    memory_id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DraftRuleArgs {
    name: String,
    trigger: Value,
    action: Value,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FreeSlotsArgs {
    from: String,
    to: String,
    duration_minutes: i64,
    work_start: Option<String>,
    work_end: Option<String>,
    limit: Option<usize>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DayArgs {
    date: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RangeArgs {
    from: String,
    to: String,
    limit: Option<usize>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SubtaskItem {
    title: String,
    description: Option<String>,
    priority: Option<i64>,
    due: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateSubtasksArgs {
    parent_id: String,
    subtasks: Vec<SubtaskItem>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MoveTasksArgs {
    task_ids: Vec<String>,
    column_id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LinkSessionArgs {
    session_id: String,
    task_id: Option<String>,
}

// ---------- planned actions ----------

enum Action {
    CreateTask(CreateTaskInput),
    UpdateTask {
        id: String,
        input: UpdateTaskInput,
        completed: Option<bool>,
    },
    MoveTask {
        id: String,
        column_id: String,
        position: i64,
    },
    DeleteTask(String),
    CreateEvent(CreateEventInput),
    UpdateEvent(String, UpdateEventInput),
    DeleteEvent(String),
    StartTimer(StartTimerInput),
    CreateNote(String, String),
    TriageTask {
        id: String,
        column_id: String,
        title: Option<String>,
        due: Option<String>,
    },
    TriageEvent {
        id: String,
        title: Option<String>,
        start: String,
        end: String,
        tz: String,
    },
    Dismiss(String),
    AddDependency(String, String),
    FromTemplate {
        template_id: String,
        column_id: String,
        parent_id: Option<String>,
    },
    Remember(String),
    Forget(String),
    Rule(RuleInput),
    CreateSubtasks(String, Vec<CreateTaskInput>),
    MoveTasks {
        ids: Vec<String>,
        column_id: String,
    },
    LinkSession(String, Option<String>),
}

struct Plan {
    preview: Preview,
    action: Action,
}

pub struct ToolDeps {
    pub tasks: Arc<TaskService>,
    pub projects: Arc<ProjectService>,
    pub templates: Arc<TemplateService>,
    pub calendar: Arc<CalendarService>,
    pub notes: Arc<NoteService>,
    pub search: Arc<SearchService>,
    pub timers: Arc<TimerService>,
    pub inbox: Arc<InboxService>,
    pub rules: Arc<AutomationService>,
    pub ai: Arc<dyn AiRepository>,
}

pub struct Tools {
    d: ToolDeps,
    specs: Vec<ToolSpec>,
}

fn spec(
    name: &'static str,
    description: &'static str,
    schema: Value,
    write: bool,
    share: Option<Share>,
) -> ToolSpec {
    ToolSpec {
        name,
        description,
        schema,
        write,
        share,
    }
}

fn catalog() -> Vec<ToolSpec> {
    let id = |d: &str| json!({ "type": "string", "description": d });
    let when = |d: &str| json!({ "type": "string", "description": d });
    let prio = json!({ "type": "integer", "minimum": 0, "maximum": 3, "description": "0 none, 1 low, 2 medium, 3 high" });
    let t = Some(Share::Tasks);
    let c = Some(Share::Calendar);
    let n = Some(Share::Notes);
    let i = Some(Share::Inbox);
    vec![
        // ----- read -----
        spec(
            "get_schedule",
            "List calendar events and task deadlines between two moments. Dates without an offset are read in the user's timezone.",
            obj(
                json!({ "from": when("Start, e.g. 2026-03-10 or 2026-03-10T09:00"), "to": when("End (exclusive); at most 400 days after from") }),
                &["from", "to"],
            ),
            false,
            c,
        ),
        spec(
            "list_projects",
            "List projects with their boards and columns (ids you need for creating or moving tasks).",
            obj(json!({}), &[]),
            false,
            t,
        ),
        spec(
            "get_tasks",
            "List tasks, optionally for one board. Each task says whether it is blocked by an unfinished task; do not schedule or recommend blocked tasks as ready to start.",
            obj(
                json!({ "board_id": id("Board id; omit for all boards"), "include_done": { "type": "boolean", "description": "Include completed tasks (default false)" }, "limit": { "type": "integer", "minimum": 1, "maximum": 200 } }),
                &[],
            ),
            false,
            t,
        ),
        spec(
            "list_templates",
            "List task templates (ids for create_task_from_template).",
            obj(json!({}), &[]),
            false,
            t,
        ),
        spec(
            "search_notes",
            "Search the user's notes. Returns short snippets, not full notes; use get_note for the text.",
            obj(
                json!({ "query": { "type": "string" }, "limit": { "type": "integer", "minimum": 1, "maximum": 20 } }),
                &["query"],
            ),
            false,
            n,
        ),
        spec(
            "get_note",
            "Read one note (truncated to max_chars, default 4000).",
            obj(
                json!({ "note_id": id("Note id"), "max_chars": { "type": "integer", "minimum": 200, "maximum": 20000 } }),
                &["note_id"],
            ),
            false,
            n,
        ),
        spec(
            "get_inbox",
            "List captured inbox items waiting to be triaged.",
            obj(
                json!({ "limit": { "type": "integer", "minimum": 1, "maximum": 50 } }),
                &[],
            ),
            false,
            i,
        ),
        spec(
            "search_all",
            "Search tasks, events, notes, projects and inbox at once. Returns snippets with ids.",
            obj(
                json!({ "query": { "type": "string" }, "kinds": { "type": "array", "items": { "type": "string", "enum": ["task", "event", "note", "project", "inbox"] } }, "limit": { "type": "integer", "minimum": 1, "maximum": 20 } }),
                &["query"],
            ),
            false,
            None,
        ),
        // ----- write (proposals) -----
        spec(
            "create_task",
            "Propose creating a task. Nothing is created until the user confirms.",
            obj(
                json!({ "column_id": id("Column id from list_projects"), "title": { "type": "string" }, "description": { "type": "string" }, "priority": prio.clone(), "due": when("Due date/time") }),
                &["column_id", "title"],
            ),
            true,
            t,
        ),
        spec(
            "create_subtask",
            "Propose creating a subtask under an existing task.",
            obj(
                json!({ "parent_id": id("Parent task id"), "title": { "type": "string" }, "description": { "type": "string" }, "priority": prio.clone(), "due": when("Due date/time") }),
                &["parent_id", "title"],
            ),
            true,
            t,
        ),
        spec(
            "update_task",
            "Propose changing a task's title, description, priority or due date, or marking it done/not done.",
            obj(
                json!({ "task_id": id("Task id"), "title": { "type": "string" }, "description": { "type": "string", "description": "Empty string clears" }, "priority": prio, "due": when("New due date/time; empty string clears"), "completed": { "type": "boolean" } }),
                &["task_id"],
            ),
            true,
            t,
        ),
        spec(
            "move_task",
            "Propose moving a task to another column of the same board.",
            obj(
                json!({ "task_id": id("Task id"), "column_id": id("Target column id"), "position": { "type": "integer", "minimum": 0, "description": "Index in the column; omit for the end" } }),
                &["task_id", "column_id"],
            ),
            true,
            t,
        ),
        spec(
            "delete_task",
            "Propose deleting a task (and its subtasks). Always needs the user's confirmation.",
            obj(json!({ "task_id": id("Task id") }), &["task_id"]),
            true,
            t,
        ),
        spec(
            "add_dependency",
            "Propose that a task is blocked by another task. Cycles are rejected.",
            obj(
                json!({ "task_id": id("The blocked task"), "blocked_by_id": id("The task that must finish first") }),
                &["task_id", "blocked_by_id"],
            ),
            true,
            t,
        ),
        spec(
            "create_task_from_template",
            "Propose creating tasks from a template into a column.",
            obj(
                json!({ "template_id": id("Template id from list_templates"), "column_id": id("Column id"), "parent_id": id("Optional parent task") }),
                &["template_id", "column_id"],
            ),
            true,
            t,
        ),
        spec(
            "create_event",
            "Propose creating a calendar event. Times without an offset are read in the user's timezone. For recurring events give an RFC 5545 RRULE value such as FREQ=WEEKLY;BYDAY=MO.",
            obj(
                json!({ "title": { "type": "string" }, "description": { "type": "string" }, "location": { "type": "string" }, "all_day": { "type": "boolean" }, "start": when("Start"), "end": when("End; defaults to one hour after start"), "timezone": { "type": "string", "description": "IANA zone; defaults to the user's" }, "recurrence_rule": { "type": "string" } }),
                &["title", "start"],
            ),
            true,
            c,
        ),
        spec(
            "update_event",
            "Propose changing an event.",
            obj(
                json!({ "event_id": id("Event id"), "title": { "type": "string" }, "description": { "type": "string" }, "location": { "type": "string" }, "all_day": { "type": "boolean" }, "start": when("New start"), "end": when("New end"), "timezone": { "type": "string" }, "recurrence_rule": { "type": "string", "description": "Empty string removes recurrence" } }),
                &["event_id"],
            ),
            true,
            c,
        ),
        spec(
            "delete_event",
            "Propose deleting an event. Always needs the user's confirmation.",
            obj(json!({ "event_id": id("Event id") }), &["event_id"]),
            true,
            c,
        ),
        spec(
            "start_timer",
            "Propose starting a focus timer (only one can run at a time).",
            obj(
                json!({ "kind": { "type": "string", "enum": ["pomodoro", "countdown", "stopwatch"] }, "minutes": { "type": "integer", "minimum": 1, "maximum": 1440, "description": "Required for pomodoro and countdown" }, "label": { "type": "string" }, "task_id": id("Task to link the focus session to") }),
                &["kind"],
            ),
            true,
            None,
        ),
        spec(
            "create_note",
            "Propose creating a markdown note.",
            obj(
                json!({ "title": { "type": "string" }, "body": { "type": "string", "description": "Markdown" } }),
                &["title", "body"],
            ),
            true,
            n,
        ),
        spec(
            "triage_inbox_item",
            "Propose filing an inbox item as a task or an event, or dismissing it. The original text is always kept.",
            obj(
                json!({ "item_id": id("Inbox item id"), "as": { "type": "string", "enum": ["task", "event", "dismiss"] }, "column_id": id("Required for task"), "title": { "type": "string" }, "due": when("Task due"), "start": when("Required for event"), "end": when("Event end; defaults to one hour after start") }),
                &["item_id", "as"],
            ),
            true,
            i,
        ),
        spec(
            "remember",
            "Propose saving a short, durable preference or fact about the user (plain text, never secrets or sensitive personal data). The user must approve it.",
            obj(
                json!({ "text": { "type": "string", "maxLength": 300 } }),
                &["text"],
            ),
            true,
            Some(Share::Memories),
        ),
        spec(
            "forget",
            "Propose deleting one saved memory.",
            obj(
                json!({ "memory_id": id("Memory id from the memory list") }),
                &["memory_id"],
            ),
            true,
            Some(Share::Memories),
        ),
        spec(
            "draft_automation_rule",
            "Propose an automation rule. It is only created if the user confirms. Triggers: task_created, task_moved {to_column_kind?: backlog|active|done, to_column_id?}, task_completed, timer_ended, inbox_added, schedule {time: HH:MM, days: [1-7], timezone}. Actions: create_task {column_id, title, description?}, create_note {title, body}, move_task {column_id}, start_timer {timer_kind: pomodoro|countdown, minutes, label?}, notify {title, body?}.",
            obj(
                json!({ "name": { "type": "string" }, "trigger": { "type": "object" }, "action": { "type": "object" } }),
                &["name", "trigger", "action"],
            ),
            true,
            None,
        ),
        // ----- planning (read) -----
        spec(
            "find_free_slots",
            "Find free time between two moments for something that needs `duration_minutes`, inside daily working hours (default 09:00-18:00 in the user's timezone). Use it before proposing a calendar block (e.g. \"block 2 hours tomorrow for studying\"), then call create_event with one of the slots.",
            obj(
                json!({ "from": when("Start, e.g. 2026-03-10 or 2026-03-10T09:00"), "to": when("End; at most 60 days after from"), "duration_minutes": { "type": "integer", "minimum": 5, "maximum": 480 }, "work_start": { "type": "string", "description": "HH:MM, default 09:00" }, "work_end": { "type": "string", "description": "HH:MM, default 18:00" }, "limit": { "type": "integer", "minimum": 1, "maximum": 30 } }),
                &["from", "to", "duration_minutes"],
            ),
            false,
            c,
        ),
        spec(
            "suggest_next_tasks",
            "Rank the user's open, unblocked tasks by what deserves attention now (overdue, due soon, priority, already in progress). Use it for \"what should I work on next?\". Blocked tasks are excluded and counted.",
            obj(
                json!({ "limit": { "type": "integer", "minimum": 1, "maximum": 20 } }),
                &[],
            ),
            false,
            t,
        ),
        spec(
            "plan_day",
            "Gather what is needed to plan one day: its events, the ranked unblocked tasks, overdue count and free slots (working hours). Then propose time blocks with create_event if the user wants them.",
            obj(
                json!({ "date": { "type": "string", "description": "YYYY-MM-DD in the user's timezone; default today" } }),
                &[],
            ),
            false,
            None,
        ),
        spec(
            "daily_summary",
            "Summarise a day: tasks completed, tasks still open and due, events, and (if shared) focus time.",
            obj(
                json!({ "date": { "type": "string", "description": "YYYY-MM-DD in the user's timezone; default today" } }),
                &[],
            ),
            false,
            None,
        ),
        spec(
            "get_focus_sessions",
            "List recorded focus sessions between two moments, with the task each is linked to (or none).",
            obj(
                json!({ "from": when("Start"), "to": when("End; at most 60 days after from"), "limit": { "type": "integer", "minimum": 1, "maximum": 100 } }),
                &["from", "to"],
            ),
            false,
            Some(Share::Focus),
        ),
        // ----- planning (write) -----
        spec(
            "create_subtasks",
            "Propose breaking a task into several subtasks at once (1-20). Nothing is created until the user confirms.",
            obj(
                json!({ "parent_id": id("Task to break down"), "subtasks": { "type": "array", "minItems": 1, "maxItems": 20, "items": { "type": "object", "properties": { "title": { "type": "string" }, "description": { "type": "string" }, "priority": prio.clone(), "due": when("Due date/time") }, "required": ["title"], "additionalProperties": false } } }),
                &["parent_id", "subtasks"],
            ),
            true,
            t,
        ),
        spec(
            "move_tasks",
            "Propose moving several top-level tasks (up to 30) to one column, including a column on a different board or project (e.g. gather all Rust tasks into the Rust project). Tasks with dependencies cannot leave their board. Always needs the user's confirmation.",
            obj(
                json!({ "task_ids": { "type": "array", "minItems": 1, "maxItems": 30, "items": { "type": "string" } }, "column_id": id("Target column id from list_projects") }),
                &["task_ids", "column_id"],
            ),
            true,
            t,
        ),
        spec(
            "link_focus_session",
            "Propose linking a recorded focus session to a task (or unlinking it with task_id omitted).",
            obj(
                json!({ "session_id": id("Focus session id from get_focus_sessions"), "task_id": id("Task id; omit to unlink") }),
                &["session_id"],
            ),
            true,
            Some(Share::Focus),
        ),
    ]
}

impl Tools {
    pub fn new(d: ToolDeps) -> Self {
        Self {
            d,
            specs: catalog(),
        }
    }

    fn find(&self, name: &str) -> DomainResult<&ToolSpec> {
        self.specs
            .iter()
            .find(|s| s.name == name)
            .ok_or_else(|| invalid(format!("unknown tool '{name}'")))
    }

    pub fn is_write(&self, name: &str) -> Option<bool> {
        self.specs.iter().find(|s| s.name == name).map(|s| s.write)
    }

    /// Tool definitions for the API, in a fixed order. The list never depends on settings: a
    /// thinking block's signature covers the tool set, so changing it mid-conversation would
    /// invalidate earlier reasoning. Sharing is enforced when a tool runs (`gate`) and the
    /// per-message context says which categories are off.
    pub fn definitions(&self) -> Vec<Value> {
        self.specs
            .iter()
            .map(|s| json!({ "name": s.name, "description": s.description, "input_schema": s.schema }))
            .collect()
    }

    fn gate(&self, spec: &ToolSpec, env: &ToolEnv) -> DomainResult<()> {
        match spec.share {
            Some(g) if !g.allowed(&env.sharing) => Err(invalid(format!(
                "the user has not allowed sharing {} with the assistant",
                g.label()
            ))),
            _ => Ok(()),
        }
    }

    fn tz(env: &ToolEnv) -> DomainResult<Tz> {
        parse_tz(&env.timezone)
    }

    // ---------- read tools ----------

    pub async fn read(&self, name: &str, input: &Value, env: &ToolEnv) -> DomainResult<Value> {
        let spec = self.find(name)?;
        if spec.write {
            return Err(invalid("that tool changes data and must be proposed"));
        }
        self.gate(spec, env)?;
        let tz = Self::tz(env)?;
        match name {
            "get_schedule" => {
                let a: GetSchedule = args(input)?;
                let (from, to) = (parse_when(&a.from, tz)?, parse_when(&a.to, tz)?);
                if from >= to {
                    return Err(invalid("from must be before to"));
                }
                let items = self
                    .d
                    .calendar
                    .list_range(&fmt_utc(from), &fmt_utc(to))
                    .await?;
                let shown: Vec<Value> = items
                    .iter()
                    .filter(|i| i.kind != "task" || env.sharing.tasks)
                    .take(MAX_RESULT_ITEMS)
                    .map(|i| {
                        json!({
                            "kind": i.kind, "id": i.source_id, "title": i.title,
                            "start": local_iso(&i.start_at, tz), "end": local_iso(&i.end_at, tz),
                            "all_day": i.all_day, "recurring": i.recurring, "completed": i.completed,
                            "location": i.location, "priority": i.priority, "read_only": i.external,
                        })
                    })
                    .collect();
                Ok(json!({ "timezone": env.timezone, "count": shown.len(), "items": shown }))
            }
            "list_projects" => {
                let mut out = vec![];
                for p in self.d.projects.list_projects().await? {
                    let mut boards = vec![];
                    for b in self.d.projects.list_boards(&p.id).await? {
                        let view = self.d.tasks.board_view(&b.id).await?;
                        let cols: Vec<Value> = view
                            .columns
                            .iter()
                            .map(|c| json!({ "id": c.id, "name": c.name, "kind": c.kind }))
                            .collect();
                        boards.push(json!({ "id": b.id, "name": b.name, "columns": cols }));
                    }
                    out.push(json!({ "id": p.id, "name": p.name, "boards": boards }));
                }
                Ok(json!({ "projects": out }))
            }
            "get_tasks" => {
                let a: GetTasks = args(input)?;
                let limit = a.limit.unwrap_or(50).clamp(1, 200);
                let include_done = a.include_done.unwrap_or(false);
                let mut boards = vec![];
                match a.board_id {
                    Some(b) => boards.push(parse_uuid(&b, "board id")?),
                    None => {
                        for p in self.d.projects.list_projects().await? {
                            for b in self.d.projects.list_boards(&p.id).await? {
                                boards.push(b.id);
                            }
                        }
                    }
                }
                let mut tasks = vec![];
                for b in boards {
                    let v = self.d.tasks.board_view(&b).await?;
                    let cols: HashMap<_, _> = v.columns.iter().map(|c| (c.id.clone(), c)).collect();
                    let open: HashSet<&str> = v
                        .tasks
                        .iter()
                        .filter(|t| t.completed_at.is_none())
                        .map(|t| t.id.as_str())
                        .collect();
                    for t in &v.tasks {
                        let done = t.completed_at.is_some();
                        if done && !include_done {
                            continue;
                        }
                        let blockers: Vec<&str> = v
                            .dependencies
                            .iter()
                            .filter(|d| d.task_id == t.id)
                            .map(|d| d.blocked_by_id.as_str())
                            .collect();
                        let blocked = blockers.iter().any(|b| open.contains(b));
                        let col = cols.get(&t.column_id);
                        tasks.push(json!({
                            "id": t.id, "title": t.title, "board_id": b,
                            "column": col.map(|c| c.name.clone()), "column_id": t.column_id,
                            "column_kind": col.map(|c| c.kind.clone()),
                            "parent_id": t.parent_id, "priority": t.priority,
                            "due": t.due_at.as_deref().map(|d| local_iso(d, tz)),
                            "completed": done, "blocked": blocked, "blocked_by": blockers,
                            "description": t.description.as_deref().map(|d| clip(d, 200)),
                        }));
                    }
                }
                let total = tasks.len();
                tasks.truncate(limit);
                Ok(json!({ "total": total, "returned": tasks.len(), "tasks": tasks }))
            }
            "find_free_slots" => {
                let a: FreeSlotsArgs = args(input)?;
                let (from, to) = (parse_when(&a.from, tz)?, parse_when(&a.to, tz)?);
                if from >= to || to - from > Duration::days(60) {
                    return Err(invalid("from must be before to, at most 60 days apart"));
                }
                if !(5..=480).contains(&a.duration_minutes) {
                    return Err(invalid("duration_minutes must be between 5 and 480"));
                }
                let (ws, we) = work_window(a.work_start.as_deref(), a.work_end.as_deref())?;
                let slots = self.free(from, to, a.duration_minutes, tz, ws, we).await?;
                let limit = a.limit.unwrap_or(10).clamp(1, 30);
                Ok(json!({
                    "timezone": env.timezone,
                    "slots": slots.iter().take(limit).map(|x| slot_json(x, tz)).collect::<Vec<_>>(),
                    "total_slots": slots.len(),
                }))
            }
            "suggest_next_tasks" => {
                let a: Limit = args(input)?;
                let (ranked, blocked, overdue) = self.ranked_open(env.now, tz).await?;
                Ok(json!({
                    "now": local_iso(&fmt_utc(env.now), tz),
                    "blocked_excluded": blocked, "overdue": overdue,
                    "tasks": ranked.into_iter().take(a.limit.unwrap_or(5).clamp(1, 20) as usize).collect::<Vec<_>>(),
                }))
            }
            "plan_day" => {
                let a: DayArgs = args(input)?;
                let (start, end, date) = day_bounds(a.date.as_deref(), env.now, tz)?;
                let mut out = json!({ "date": date.to_string(), "timezone": env.timezone });
                let mut busy = vec![];
                if env.sharing.calendar {
                    let items = self
                        .d
                        .calendar
                        .list_range(&fmt_utc(start), &fmt_utc(end))
                        .await?;
                    let events: Vec<Value> = items
                        .iter()
                        .filter(|i| i.kind == "event")
                        .take(MAX_RESULT_ITEMS)
                        .map(|i| json!({ "title": i.title, "start": local_iso(&i.start_at, tz), "end": local_iso(&i.end_at, tz), "all_day": i.all_day, "read_only": i.external }))
                        .collect();
                    busy = self.busy(start, end).await?;
                    out["events"] = json!(events);
                    let (ws, we) = work_window(None, None)?;
                    out["free_slots"] = json!(
                        free_slots(&busy, start.max(env.now.min(end)), end, 30, tz, ws, we)
                            .iter()
                            .take(12)
                            .map(|x| slot_json(x, tz))
                            .collect::<Vec<_>>()
                    );
                } else {
                    out["events"] = json!("not shared");
                }
                let _ = busy;
                if env.sharing.tasks {
                    let (ranked, blocked, overdue) = self.ranked_open(env.now, tz).await?;
                    out["suggested_tasks"] = json!(ranked.into_iter().take(8).collect::<Vec<_>>());
                    out["blocked_excluded"] = json!(blocked);
                    out["overdue"] = json!(overdue);
                } else {
                    out["suggested_tasks"] = json!("not shared");
                }
                Ok(out)
            }
            "daily_summary" => {
                let a: DayArgs = args(input)?;
                let (start, end, date) = day_bounds(a.date.as_deref(), env.now, tz)?;
                let mut out = json!({ "date": date.to_string(), "timezone": env.timezone });
                if env.sharing.tasks {
                    let (mut done, mut open_due) = (vec![], vec![]);
                    for p in self.d.projects.list_projects().await? {
                        for b in self.d.projects.list_boards(&p.id).await? {
                            for t in self.d.tasks.board_view(&b.id).await?.tasks {
                                let at = |x: &Option<String>| {
                                    x.as_deref()
                                        .and_then(|v| crate::domain::validation::parse_utc(v).ok())
                                };
                                if let Some(c) = at(&t.completed_at) {
                                    if c >= start && c < end {
                                        done.push(json!({ "id": t.id, "title": t.title, "project": p.name }));
                                    }
                                } else if at(&t.due_at).is_some_and(|d| d < end) {
                                    open_due.push(json!({ "id": t.id, "title": t.title, "project": p.name, "due": t.due_at.as_deref().map(|d| local_iso(d, tz)) }));
                                }
                            }
                        }
                    }
                    done.truncate(MAX_RESULT_ITEMS);
                    open_due.truncate(MAX_RESULT_ITEMS);
                    out["completed"] = json!(done);
                    out["unfinished_due_or_overdue"] = json!(open_due);
                } else {
                    out["completed"] = json!("not shared");
                }
                if env.sharing.calendar {
                    let items = self
                        .d
                        .calendar
                        .list_range(&fmt_utc(start), &fmt_utc(end))
                        .await?;
                    out["events"] = json!(
                        items
                            .iter()
                            .filter(|i| i.kind == "event")
                            .take(MAX_RESULT_ITEMS)
                            .map(
                                |i| json!({ "title": i.title, "start": local_iso(&i.start_at, tz) })
                            )
                            .collect::<Vec<_>>()
                    );
                }
                if env.sharing.focus {
                    let sessions = self
                        .d
                        .timers
                        .sessions(&fmt_utc(start), &fmt_utc(end), None)
                        .await?;
                    out["focus_minutes"] =
                        json!(sessions.iter().map(|x| x.duration_ms).sum::<i64>() / 60_000);
                    out["focus_sessions"] = json!(sessions.len());
                }
                Ok(out)
            }
            "get_focus_sessions" => {
                let a: RangeArgs = args(input)?;
                let (from, to) = (parse_when(&a.from, tz)?, parse_when(&a.to, tz)?);
                if to - from > Duration::days(60) {
                    return Err(invalid("range must be at most 60 days"));
                }
                let sessions = self
                    .d
                    .timers
                    .sessions(&fmt_utc(from), &fmt_utc(to), None)
                    .await?;
                let rows: Vec<Value> = sessions
                    .iter()
                    .take(a.limit.unwrap_or(30).clamp(1, 100))
                    .map(|x| json!({ "id": x.id, "task_id": x.task_id, "start": local_iso(&x.started_at, tz), "minutes": x.duration_ms / 60_000 }))
                    .collect();
                Ok(json!({ "count": sessions.len(), "sessions": rows }))
            }
            "list_templates" => {
                let t: Vec<Value> = self
                    .d
                    .templates
                    .list()
                    .await?
                    .iter()
                    .map(|t| json!({ "id": t.id, "name": t.name, "description": t.description }))
                    .collect();
                Ok(json!({ "templates": t }))
            }
            "search_notes" => {
                let a: QueryArgs = args(input)?;
                self.search(&a.query, Some(vec!["note".into()]), a.limit, env)
                    .await
            }
            "search_all" => {
                let a: QueryArgs = args(input)?;
                self.search(&a.query, a.kinds, a.limit, env).await
            }
            "get_note" => {
                let a: NoteId = args(input)?;
                let n = self.d.notes.get(&a.note_id).await?;
                let max = a.max_chars.unwrap_or(4000).clamp(200, 20_000);
                Ok(json!({
                    "id": n.note.id, "title": n.note.title, "kind": n.note.kind,
                    "body": clip(&n.note.body, max),
                    "truncated": n.note.body.chars().count() > max,
                    "updated_at": local_iso(&n.note.updated_at, tz),
                }))
            }
            "get_inbox" => {
                let a: Limit = args(input)?;
                let items: Vec<Value> = self
                    .d
                    .inbox
                    .list(Some("new"), a.limit.unwrap_or(20).clamp(1, 50))
                    .await?
                    .iter()
                    .map(|i| json!({ "id": i.id, "text": clip(&i.raw_text, 300), "captured": local_iso(&i.created_at, tz) }))
                    .collect();
                Ok(json!({ "items": items }))
            }
            other => Err(invalid(format!("unknown tool '{other}'"))),
        }
    }

    async fn search(
        &self,
        query: &str,
        kinds: Option<Vec<String>>,
        limit: Option<i64>,
        env: &ToolEnv,
    ) -> DomainResult<Value> {
        let s = &env.sharing;
        let allowed = |k: &str| match k {
            "task" | "project" => s.tasks,
            "event" => s.calendar,
            "note" => s.notes,
            "inbox" => s.inbox,
            _ => false,
        };
        let wanted: Vec<String> = match kinds.filter(|k| !k.is_empty()) {
            Some(k) => k,
            None => ["task", "event", "note", "project", "inbox"]
                .map(String::from)
                .to_vec(),
        };
        let kinds: Vec<String> = wanted.into_iter().filter(|k| allowed(k)).collect();
        if kinds.is_empty() {
            return Err(invalid("none of those kinds is shared with the assistant"));
        }
        let hits = self
            .d
            .search
            .search(query, Some(kinds), Some(limit.unwrap_or(10).clamp(1, 20)))
            .await?;
        let out: Vec<Value> = hits
            .iter()
            .map(|h| json!({ "kind": h.kind, "id": h.id, "title": h.title, "snippet": clip(&strip_marks(&h.snippet), 240), "date": h.date }))
            .collect();
        Ok(json!({ "results": out }))
    }

    // ---------- write tools ----------

    /// Validate a write call and describe what it would do. Writes nothing.
    pub async fn propose(&self, name: &str, input: &Value, env: &ToolEnv) -> DomainResult<Preview> {
        Ok(self.plan(name, input, env).await?.preview)
    }

    /// Apply a confirmed proposal. Re-validates against the current state first.
    pub async fn apply(&self, name: &str, input: &Value, env: &ToolEnv) -> DomainResult<Value> {
        let plan = self.plan(name, input, env).await?;
        let a = Actor::Ai;
        let d = &self.d;
        let done = |kind: &str, id: &str, title: &str| json!({ "ok": true, "kind": kind, "id": id, "title": title });
        Ok(match plan.action {
            Action::CreateTask(i) => {
                let t = d.tasks.create_task(i, a).await?;
                done("task", &t.id, &t.title)
            }
            Action::UpdateTask {
                id,
                input,
                completed,
            } => {
                let has_fields = input.title.is_some()
                    || input.description.is_some()
                    || input.priority.is_some()
                    || input.due_at.is_some();
                let mut t = if has_fields {
                    Some(d.tasks.update_task(&id, input, a).await?)
                } else {
                    None
                };
                if let Some(c) = completed {
                    // The user confirmed a preview that showed any warning, so force past it.
                    t = Some(d.tasks.set_completed(&id, c, true, a).await?);
                }
                let t = match t {
                    Some(t) => t,
                    None => d.tasks.get_task(&id).await?,
                };
                done("task", &t.id, &t.title)
            }
            Action::MoveTask {
                id,
                column_id,
                position,
            } => {
                let t = d
                    .tasks
                    .move_task(&id, &column_id, position, true, a)
                    .await?;
                done("task", &t.id, &t.title)
            }
            Action::DeleteTask(id) => {
                d.tasks.delete_task(&id, a).await?;
                done("task", &id, "deleted")
            }
            Action::CreateEvent(i) => {
                let e = d.calendar.create_event(i, a).await?;
                done("event", &e.id, &e.title)
            }
            Action::UpdateEvent(id, i) => {
                let e = d.calendar.update_event(&id, i, a).await?;
                done("event", &e.id, &e.title)
            }
            Action::DeleteEvent(id) => {
                d.calendar.delete_event(&id, a).await?;
                done("event", &id, "deleted")
            }
            Action::StartTimer(i) => {
                let t = d.timers.start(i, a).await?;
                json!({ "ok": true, "kind": "timer", "id": t.timer.id, "title": t.timer.label })
            }
            Action::CreateNote(title, body) => {
                let n = d.notes.create(&title, &body, a).await?;
                done("note", &n.id, &n.title)
            }
            Action::TriageTask {
                id,
                column_id,
                title,
                due,
            } => {
                let (_, t) = d
                    .inbox
                    .triage_to_task(&id, &column_id, title.as_deref(), due.as_deref(), a)
                    .await?;
                done("task", &t.id, &t.title)
            }
            Action::TriageEvent {
                id,
                title,
                start,
                end,
                tz,
            } => {
                let (_, e) = d
                    .inbox
                    .triage_to_event(&id, title.as_deref(), &start, &end, &tz, a)
                    .await?;
                done("event", &e.id, &e.title)
            }
            Action::Dismiss(id) => {
                d.inbox.dismiss(&id, a).await?;
                done("inbox_item", &id, "dismissed")
            }
            Action::AddDependency(t, b) => {
                d.tasks.add_dependency(&t, &b, a).await?;
                done("task_dependency", &format!("{t}:{b}"), "dependency added")
            }
            Action::FromTemplate {
                template_id,
                column_id,
                parent_id,
            } => {
                let tasks = d
                    .templates
                    .instantiate(&template_id, &column_id, parent_id.as_deref(), a)
                    .await?;
                done(
                    "task",
                    &tasks.first().map(|t| t.id.clone()).unwrap_or_default(),
                    &format!("{} task(s) created", tasks.len()),
                )
            }
            Action::Remember(text) => {
                let m = d.ai.create_memory(&text, a).await?;
                done("memory", &m.id, &m.text)
            }
            Action::Forget(id) => {
                d.ai.delete_memory(&id, a).await?;
                done("memory", &id, "forgotten")
            }
            Action::CreateSubtasks(parent, inputs) => {
                let made = d.tasks.create_subtasks(&parent, inputs, a).await?;
                done(
                    "task",
                    &parent,
                    &format!("{} subtask(s) created", made.len()),
                )
            }
            Action::MoveTasks { ids, column_id } => {
                for id in &ids {
                    d.tasks.relocate_task(id, &column_id, true, a).await?;
                }
                done(
                    "task",
                    ids.first().map(String::as_str).unwrap_or_default(),
                    &format!("{} task(s) moved", ids.len()),
                )
            }
            Action::LinkSession(session, task) => {
                let s = d
                    .timers
                    .set_session_task(&session, task.as_deref(), a)
                    .await?;
                done("focus_session", &s.id, "focus session linked")
            }
            Action::Rule(r) => {
                let r = d
                    .rules
                    .create(
                        RuleInput {
                            enabled: Some(true),
                            ..r
                        },
                        a,
                    )
                    .await?;
                done("automation_rule", &r.id, &r.name)
            }
        })
    }

    /// Merged busy intervals (events only; tasks are not busy time) in `[from, to)`.
    async fn busy(
        &self,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    ) -> DomainResult<Vec<(DateTime<Utc>, DateTime<Utc>)>> {
        let mut out = vec![];
        let mut cursor = from;
        while cursor < to {
            let next = (cursor + Duration::days(60)).min(to);
            for i in self
                .d
                .calendar
                .list_range(&fmt_utc(cursor), &fmt_utc(next))
                .await?
            {
                if i.kind != "event" || i.all_day {
                    continue;
                }
                if let (Ok(s), Ok(e)) = (
                    crate::domain::validation::parse_utc(&i.start_at),
                    crate::domain::validation::parse_utc(&i.end_at),
                ) {
                    out.push((s, e));
                }
            }
            cursor = next;
        }
        Ok(out)
    }

    async fn free(
        &self,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
        minutes: i64,
        tz: Tz,
        ws: NaiveTime,
        we: NaiveTime,
    ) -> DomainResult<Vec<Slot>> {
        let busy = self.busy(from, to).await?;
        Ok(free_slots(&busy, from, to, minutes, tz, ws, we))
    }

    /// Open, unblocked tasks ranked for attention; returns (ranked json, blocked count, overdue count).
    async fn ranked_open(
        &self,
        now: DateTime<Utc>,
        tz: Tz,
    ) -> DomainResult<(Vec<Value>, usize, usize)> {
        let mut inputs = vec![];
        let mut info: HashMap<String, Value> = HashMap::new();
        let mut blocked_n = 0;
        for p in self.d.projects.list_projects().await? {
            for b in self.d.projects.list_boards(&p.id).await? {
                let v = self.d.tasks.board_view(&b.id).await?;
                let cols: HashMap<_, _> = v.columns.iter().map(|c| (c.id.clone(), c)).collect();
                let open: HashSet<&str> = v
                    .tasks
                    .iter()
                    .filter(|t| t.completed_at.is_none())
                    .map(|t| t.id.as_str())
                    .collect();
                for t in v.tasks.iter().filter(|t| t.completed_at.is_none()) {
                    let col = cols.get(&t.column_id);
                    let kind = col.map(|c| c.kind.as_str()).unwrap_or("");
                    if kind == "done" {
                        continue;
                    }
                    if v.dependencies
                        .iter()
                        .any(|d| d.task_id == t.id && open.contains(d.blocked_by_id.as_str()))
                    {
                        blocked_n += 1;
                        continue;
                    }
                    inputs.push(RankInput {
                        id: t.id.clone(),
                        priority: t.priority,
                        due_at: t
                            .due_at
                            .as_deref()
                            .and_then(|d| crate::domain::validation::parse_utc(d).ok()),
                        in_progress: kind == "active",
                    });
                    info.insert(
                        t.id.clone(),
                        json!({
                            "id": t.id, "title": t.title, "project": p.name,
                            "column": col.map(|c| c.name.clone()), "priority": t.priority,
                            "due": t.due_at.as_deref().map(|d| local_iso(d, tz)),
                        }),
                    );
                }
            }
        }
        let ranked = rank_tasks(&inputs, now, tz);
        let overdue = ranked
            .iter()
            .filter(|r| r.reasons.iter().any(|x| x.starts_with("overdue")))
            .count();
        let out = ranked
            .into_iter()
            .filter_map(|r| {
                let mut v = info.remove(&r.id)?;
                v["score"] = json!(r.score);
                v["reasons"] = json!(r.reasons);
                Some(v)
            })
            .collect();
        Ok((out, blocked_n, overdue))
    }

    async fn task_summary(&self, id: &str) -> DomainResult<(crate::domain::kanban::Task, String)> {
        let t = self.d.tasks.get_task(id).await?;
        let col = self.d.tasks.column(&t.column_id).await?;
        Ok((t, col.name))
    }

    async fn plan(&self, name: &str, input: &Value, env: &ToolEnv) -> DomainResult<Plan> {
        let spec = self.find(name)?;
        if !spec.write {
            return Err(invalid("that tool only reads data"));
        }
        self.gate(spec, env)?;
        let tz = Self::tz(env)?;
        let due = |s: &Option<String>| -> DomainResult<Option<String>> {
            match s.as_deref().map(str::trim) {
                None => Ok(None),
                Some("") => Ok(Some(String::new())),
                Some(v) => Ok(Some(fmt_utc(parse_when(v, tz)?))),
            }
        };
        let show_due =
            |u: &Option<String>| u.as_deref().filter(|d| !d.is_empty()).map(|d| local(d, tz));
        match name {
            "create_task" => {
                let a: CreateTaskArgs = args(input)?;
                let col = self.d.tasks.column(&a.column_id).await?;
                let due_at = due(&a.due)?;
                let i = CreateTaskInput {
                    column_id: Some(col.id.clone()),
                    parent_id: None,
                    title: a.title,
                    description: a.description,
                    priority: a.priority,
                    due_at: due_at.clone().filter(|d| !d.is_empty()),
                    position: None,
                };
                let n = self.d.tasks.build_new_task(&i).await?;
                let mut changes = vec![
                    change("Title", None, Some(n.title.clone())),
                    change("Column", None, Some(col.name.clone())),
                ];
                if n.priority > 0 {
                    changes.push(change("Priority", None, Some(n.priority.to_string())));
                }
                if let Some(d) = n.due_at.as_deref() {
                    changes.push(change("Due", None, Some(local(d, tz))));
                }
                if let Some(d) = &n.description {
                    changes.push(change("Description", None, Some(clip(d, 200))));
                }
                Ok(Plan {
                    preview: Preview {
                        summary: format!("Create task “{}” in {}", n.title, col.name),
                        destructive: false,
                        changes,
                    },
                    action: Action::CreateTask(i),
                })
            }
            "create_subtask" => {
                let a: CreateSubtaskArgs = args(input)?;
                let (parent, col) = self
                    .task_summary(&parse_uuid(&a.parent_id, "parent id")?)
                    .await?;
                let i = CreateTaskInput {
                    column_id: None,
                    parent_id: Some(parent.id.clone()),
                    title: a.title,
                    description: a.description,
                    priority: a.priority,
                    due_at: due(&a.due)?.filter(|d| !d.is_empty()),
                    position: None,
                };
                let n = self.d.tasks.build_new_task(&i).await?;
                let mut changes = vec![
                    change("Title", None, Some(n.title.clone())),
                    change("Parent", None, Some(parent.title.clone())),
                ];
                if let Some(d) = n.due_at.as_deref() {
                    changes.push(change("Due", None, Some(local(d, tz))));
                }
                Ok(Plan {
                    preview: Preview {
                        summary: format!("Add subtask “{}” to “{}” ({col})", n.title, parent.title),
                        destructive: false,
                        changes,
                    },
                    action: Action::CreateTask(i),
                })
            }
            "create_subtasks" => {
                let a: CreateSubtasksArgs = args(input)?;
                if a.subtasks.is_empty() || a.subtasks.len() > 20 {
                    return Err(invalid("provide between 1 and 20 subtasks"));
                }
                let (parent, col) = self
                    .task_summary(&parse_uuid(&a.parent_id, "parent id")?)
                    .await?;
                let mut inputs = vec![];
                let mut changes = vec![];
                for st in a.subtasks {
                    let i = CreateTaskInput {
                        column_id: None,
                        parent_id: Some(parent.id.clone()),
                        title: st.title,
                        description: st.description,
                        priority: st.priority,
                        due_at: due(&st.due)?.filter(|d| !d.is_empty()),
                        position: None,
                    };
                    let n = self.d.tasks.build_new_task(&i).await?;
                    changes.push(change(
                        "Subtask",
                        None,
                        Some(match n.due_at.as_deref() {
                            Some(d) => format!("{} (due {})", n.title, local(d, tz)),
                            None => n.title.clone(),
                        }),
                    ));
                    inputs.push(i);
                }
                Ok(Plan {
                    preview: Preview {
                        summary: format!(
                            "Break “{}” ({col}) into {} subtasks",
                            parent.title,
                            inputs.len()
                        ),
                        destructive: false,
                        changes,
                    },
                    action: Action::CreateSubtasks(parent.id, inputs),
                })
            }
            "move_tasks" => {
                let a: MoveTasksArgs = args(input)?;
                if a.task_ids.is_empty() || a.task_ids.len() > 30 {
                    return Err(invalid("provide between 1 and 30 task ids"));
                }
                let mut seen = HashSet::new();
                let target = self.d.tasks.column(&a.column_id).await?;
                let mut changes = vec![];
                for raw in &a.task_ids {
                    let id = parse_uuid(raw, "task id")?;
                    if !seen.insert(id.clone()) {
                        return Err(invalid("duplicate task id"));
                    }
                    if let Some(why) = self.d.tasks.relocate_blocker(&id, &target.id).await? {
                        let (t, _) = self.task_summary(&id).await?;
                        return Err(invalid(format!("cannot move “{}”: {why}", t.title)));
                    }
                    let (t, from) = self.task_summary(&id).await?;
                    changes.push(change(&t.title, Some(from), Some(target.name.clone())));
                    if let Some(w) = self.d.tasks.move_warning(&id, &target.id).await? {
                        changes.push(change("Warning", Some(t.title.clone()), Some(w)));
                    }
                }
                Ok(Plan {
                    preview: Preview {
                        summary: format!("Move {} task(s) to “{}”", a.task_ids.len(), target.name),
                        // Bulk operations always need an explicit, prominent confirmation.
                        destructive: true,
                        changes,
                    },
                    action: Action::MoveTasks {
                        ids: a.task_ids,
                        column_id: target.id,
                    },
                })
            }
            "link_focus_session" => {
                let a: LinkSessionArgs = args(input)?;
                let session = self.d.timers.get_session(&a.session_id).await?;
                let (label, after) = match &a.task_id {
                    Some(t) => {
                        let (task, _) = self.task_summary(&parse_uuid(t, "task id")?).await?;
                        (task.title.clone(), Some(task.title))
                    }
                    None => ("(none)".to_string(), None),
                };
                let before = match &session.task_id {
                    Some(t) => self.task_summary(t).await.ok().map(|x| x.0.title),
                    None => None,
                };
                Ok(Plan {
                    preview: Preview {
                        summary: format!(
                            "Link the {} min focus session of {} to “{label}”",
                            session.duration_ms / 60_000,
                            local(&session.started_at, tz)
                        ),
                        destructive: false,
                        changes: vec![change("Task", before, after)],
                    },
                    action: Action::LinkSession(session.id, a.task_id),
                })
            }
            "update_task" => {
                let a: UpdateTaskArgs = args(input)?;
                let id = parse_uuid(&a.task_id, "task id")?;
                let (t, _) = self.task_summary(&id).await?;
                let due_at = due(&a.due)?;
                let upd = UpdateTaskInput {
                    title: a.title.clone(),
                    description: a.description.clone(),
                    priority: a.priority,
                    due_at: due_at.clone(),
                };
                if upd.title.is_none()
                    && upd.description.is_none()
                    && upd.priority.is_none()
                    && upd.due_at.is_none()
                    && a.completed.is_none()
                {
                    return Err(invalid("nothing to change"));
                }
                // Dry-run the field validation (the write itself happens on apply).
                if let Some(title) = &a.title {
                    clean_text(title, "title", 300)?;
                }
                if let Some(p) = a.priority {
                    crate::services::validate_priority(p)?;
                }
                let mut changes = vec![];
                if let Some(v) = &a.title {
                    changes.push(change(
                        "Title",
                        Some(t.title.clone()),
                        Some(v.trim().to_string()),
                    ));
                }
                if let Some(v) = &a.description {
                    changes.push(change(
                        "Description",
                        t.description.as_deref().map(|d| clip(d, 200)),
                        Some(clip(v, 200)),
                    ));
                }
                if let Some(p) = a.priority {
                    changes.push(change(
                        "Priority",
                        Some(t.priority.to_string()),
                        Some(p.to_string()),
                    ));
                }
                if due_at.is_some() {
                    changes.push(change(
                        "Due",
                        t.due_at.as_deref().map(|d| local(d, tz)),
                        show_due(&due_at).or(Some("none".into())),
                    ));
                }
                if a.completed == Some(true)
                    && let Some(w) = self.d.tasks.complete_warning(&id).await?
                {
                    changes.push(change("Warning", None, Some(w)));
                }
                if let Some(c) = a.completed {
                    changes.push(change(
                        "Status",
                        Some(
                            if t.completed_at.is_some() {
                                "done"
                            } else {
                                "open"
                            }
                            .into(),
                        ),
                        Some(if c { "done" } else { "open" }.into()),
                    ));
                }
                Ok(Plan {
                    preview: Preview {
                        summary: format!("Update task “{}”", t.title),
                        destructive: false,
                        changes,
                    },
                    action: Action::UpdateTask {
                        id,
                        input: upd,
                        completed: a.completed,
                    },
                })
            }
            "move_task" => {
                let a: MoveTaskArgs = args(input)?;
                let id = parse_uuid(&a.task_id, "task id")?;
                let (t, from) = self.task_summary(&id).await?;
                let target = self.d.tasks.column(&a.column_id).await?;
                let position = match a.position {
                    Some(p) if p >= 0 => p,
                    Some(_) => return Err(invalid("position must be >= 0")),
                    None => {
                        let v = self.d.tasks.board_view(&target.board_id).await?;
                        v.tasks
                            .iter()
                            .filter(|x| {
                                x.column_id == target.id && x.parent_id.is_none() && x.id != t.id
                            })
                            .count() as i64
                    }
                };
                let mut changes = vec![change("Column", Some(from), Some(target.name.clone()))];
                if let Some(w) = self.d.tasks.move_warning(&id, &target.id).await? {
                    changes.push(change("Warning", None, Some(w)));
                }
                Ok(Plan {
                    preview: Preview {
                        summary: format!("Move “{}” to {}", t.title, target.name),
                        destructive: false,
                        changes,
                    },
                    action: Action::MoveTask {
                        id,
                        column_id: target.id,
                        position,
                    },
                })
            }
            "delete_task" => {
                let a: TaskIdArgs = args(input)?;
                let id = parse_uuid(&a.task_id, "task id")?;
                let (t, col) = self.task_summary(&id).await?;
                Ok(Plan {
                    preview: Preview {
                        summary: format!("Delete task “{}” and its subtasks", t.title),
                        destructive: true,
                        changes: vec![change(
                            "Task",
                            Some(format!("{} ({col})", t.title)),
                            Some("deleted".into()),
                        )],
                    },
                    action: Action::DeleteTask(id),
                })
            }
            "add_dependency" => {
                let a: DependencyArgs = args(input)?;
                let (t, b) = (
                    parse_uuid(&a.task_id, "task id")?,
                    parse_uuid(&a.blocked_by_id, "blocked_by id")?,
                );
                if t == b {
                    return Err(invalid("a task cannot block itself"));
                }
                let (task, _) = self.task_summary(&t).await?;
                let (blocker, _) = self.task_summary(&b).await?;
                Ok(Plan {
                    preview: Preview {
                        summary: format!("“{}” will be blocked by “{}”", task.title, blocker.title),
                        destructive: false,
                        changes: vec![change("Blocked by", None, Some(blocker.title))],
                    },
                    action: Action::AddDependency(t, b),
                })
            }
            "create_task_from_template" => {
                let a: FromTemplateArgs = args(input)?;
                let tid = parse_uuid(&a.template_id, "template id")?;
                let tpl = self
                    .d
                    .templates
                    .list()
                    .await?
                    .into_iter()
                    .find(|t| t.id == tid)
                    .ok_or(DomainError::NotFound {
                        entity: "task_template",
                        id: tid.clone(),
                    })?;
                let col = self.d.tasks.column(&a.column_id).await?;
                let parent = a
                    .parent_id
                    .as_deref()
                    .map(|p| parse_uuid(p, "parent id"))
                    .transpose()?;
                if let Some(p) = &parent {
                    self.d.tasks.get_task(p).await?;
                }
                Ok(Plan {
                    preview: Preview {
                        summary: format!(
                            "Create tasks from template “{}” in {}",
                            tpl.name, col.name
                        ),
                        destructive: false,
                        changes: vec![
                            change("Template", None, Some(tpl.name)),
                            change("Column", None, Some(col.name)),
                        ],
                    },
                    action: Action::FromTemplate {
                        template_id: tid,
                        column_id: col.id,
                        parent_id: parent,
                    },
                })
            }
            "create_event" => {
                let a: CreateEventArgs = args(input)?;
                let zone = a.timezone.clone().unwrap_or_else(|| env.timezone.clone());
                let ztz = parse_tz(&zone)?;
                let start = parse_when(&a.start, ztz)?;
                let end = match a.end.as_deref() {
                    Some(e) => parse_when(e, ztz)?,
                    None if a.all_day.unwrap_or(false) => start + Duration::days(1),
                    None => start + Duration::hours(1),
                };
                let i = CreateEventInput {
                    title: a.title,
                    description: a.description,
                    location: a.location,
                    all_day: a.all_day,
                    start_at: fmt_utc(start),
                    end_at: fmt_utc(end),
                    timezone: zone.clone(),
                    recurrence_rule: a.recurrence_rule,
                };
                CalendarService::check_create(&i)?;
                let mut changes = vec![
                    change("Title", None, Some(i.title.trim().to_string())),
                    change(
                        "When",
                        None,
                        Some(if i.all_day == Some(true) {
                            format!(
                                "{} (all day)",
                                local(&i.start_at, ztz)
                                    .split(' ')
                                    .take(4)
                                    .collect::<Vec<_>>()
                                    .join(" ")
                            )
                        } else {
                            format!(
                                "{} – {}",
                                local(&i.start_at, ztz),
                                local(&i.end_at, ztz).split(' ').skip(4).collect::<String>()
                            )
                        }),
                    ),
                ];
                if let Some(r) = i
                    .recurrence_rule
                    .as_deref()
                    .filter(|r| !r.trim().is_empty())
                {
                    changes.push(change("Repeats", None, Some(r.to_string())));
                }
                if let Some(l) = i.location.as_deref().filter(|l| !l.trim().is_empty()) {
                    changes.push(change("Location", None, Some(l.to_string())));
                }
                Ok(Plan {
                    preview: Preview {
                        summary: format!("Create event “{}”", i.title.trim()),
                        destructive: false,
                        changes,
                    },
                    action: Action::CreateEvent(i),
                })
            }
            "update_event" => {
                let a: UpdateEventArgs = args(input)?;
                let ztz = parse_tz(a.timezone.as_deref().unwrap_or(&env.timezone))?;
                let conv = |s: &Option<String>| -> DomainResult<Option<String>> {
                    s.as_deref()
                        .map(|v| parse_when(v, ztz).map(fmt_utc))
                        .transpose()
                };
                let upd = UpdateEventInput {
                    title: a.title,
                    description: a.description,
                    location: a.location,
                    all_day: a.all_day,
                    start_at: conv(&a.start)?,
                    end_at: conv(&a.end)?,
                    timezone: a.timezone,
                    recurrence_rule: a.recurrence_rule,
                };
                let (before, after) = self.d.calendar.plan_update(&a.event_id, &upd).await?;
                let tzb = parse_tz(&before.timezone)?;
                let mut changes = vec![];
                let mut diff = |label: &str, b: String, n: String| {
                    if b != n {
                        changes.push(change(label, Some(b), Some(n)));
                    }
                };
                diff("Title", before.title.clone(), after.title.clone());
                diff(
                    "Start",
                    local(&before.start_at, tzb),
                    local(&after.start_at, parse_tz(&after.timezone)?),
                );
                diff(
                    "End",
                    local(&before.end_at, tzb),
                    local(&after.end_at, parse_tz(&after.timezone)?),
                );
                diff(
                    "Location",
                    before.location.clone().unwrap_or_default(),
                    after.location.clone().unwrap_or_default(),
                );
                diff(
                    "Description",
                    clip(&before.description.clone().unwrap_or_default(), 200),
                    clip(&after.description.clone().unwrap_or_default(), 200),
                );
                diff(
                    "Repeats",
                    before.recurrence_rule.clone().unwrap_or_default(),
                    after.recurrence_rule.clone().unwrap_or_default(),
                );
                diff("Timezone", before.timezone.clone(), after.timezone.clone());
                diff(
                    "All day",
                    before.all_day.to_string(),
                    after.all_day.to_string(),
                );
                if changes.is_empty() {
                    return Err(invalid("nothing to change"));
                }
                Ok(Plan {
                    preview: Preview {
                        summary: format!("Update event “{}”", before.title),
                        destructive: false,
                        changes,
                    },
                    action: Action::UpdateEvent(before.id, upd),
                })
            }
            "delete_event" => {
                let a: EventIdArgs = args(input)?;
                let e = self.d.calendar.get_event(&a.event_id).await?;
                crate::services::ensure_editable(&e)?;
                let tze = parse_tz(&e.timezone)?;
                Ok(Plan {
                    preview: Preview {
                        summary: format!(
                            "Delete event “{}”{}",
                            e.title,
                            if e.recurrence_rule.is_some() {
                                " (the whole series)"
                            } else {
                                ""
                            }
                        ),
                        destructive: true,
                        changes: vec![change(
                            "Event",
                            Some(format!("{} · {}", e.title, local(&e.start_at, tze))),
                            Some("deleted".into()),
                        )],
                    },
                    action: Action::DeleteEvent(e.id),
                })
            }
            "start_timer" => {
                let a: StartTimerArgs = args(input)?;
                let duration_ms = match (a.kind, a.minutes) {
                    (TimerKind::Stopwatch, None) => None,
                    (TimerKind::Stopwatch, Some(_)) => {
                        return Err(invalid("a stopwatch has no duration"));
                    }
                    (_, Some(m)) if (1..=1440).contains(&m) => Some(m * 60_000),
                    (_, Some(_)) => return Err(invalid("minutes must be between 1 and 1440")),
                    (_, None) => return Err(invalid("minutes is required for this timer")),
                };
                let task_id = match a.task_id.as_deref() {
                    Some(t) => {
                        let t = parse_uuid(t, "task id")?;
                        self.d.tasks.get_task(&t).await?;
                        Some(t)
                    }
                    None => None,
                };
                if !self.d.timers.active().await?.is_empty() {
                    return Err(invalid("another timer is already running; stop it first"));
                }
                let kind = match a.kind {
                    TimerKind::Pomodoro => "pomodoro",
                    TimerKind::Countdown => "countdown",
                    TimerKind::Stopwatch => "stopwatch",
                };
                let what = match a.minutes {
                    Some(m) => format!("{m} min {kind}"),
                    None => kind.to_string(),
                };
                Ok(Plan {
                    preview: Preview {
                        summary: format!("Start a {what} timer"),
                        destructive: false,
                        changes: vec![change("Timer", None, Some(what))],
                    },
                    action: Action::StartTimer(StartTimerInput {
                        kind: kind.into(),
                        label: a.label,
                        task_id,
                        duration_ms,
                        phase: None,
                    }),
                })
            }
            "create_note" => {
                let a: CreateNoteArgs = args(input)?;
                let title = clean_text(&a.title, "title", 300)?;
                if a.body.len() > 100_000 {
                    return Err(invalid("note body is too long"));
                }
                Ok(Plan {
                    preview: Preview {
                        summary: format!("Create note “{title}”"),
                        destructive: false,
                        changes: vec![
                            change("Title", None, Some(title.clone())),
                            change("Body", None, Some(clip(&a.body, 300))),
                        ],
                    },
                    action: Action::CreateNote(title, a.body),
                })
            }
            "triage_inbox_item" => {
                let a: TriageArgs = args(input)?;
                let id = parse_uuid(&a.item_id, "item id")?;
                let item = self
                    .d
                    .inbox
                    .list(Some("new"), 200)
                    .await?
                    .into_iter()
                    .find(|i| i.id == id)
                    .ok_or(DomainError::NotFound {
                        entity: "inbox_item",
                        id: id.clone(),
                    })?;
                let from = change("Inbox item", Some(clip(&item.raw_text, 120)), None);
                match a.kind {
                    TriageAs::Dismiss => Ok(Plan {
                        preview: Preview {
                            summary: "Dismiss inbox item".into(),
                            destructive: false,
                            changes: vec![Change {
                                after: Some("dismissed".into()),
                                ..from
                            }],
                        },
                        action: Action::Dismiss(id),
                    }),
                    TriageAs::Task => {
                        let col =
                            self.d
                                .tasks
                                .column(a.column_id.as_deref().ok_or_else(|| {
                                    invalid("column_id is required to file a task")
                                })?)
                                .await?;
                        let due_at = due(&a.due)?.filter(|d| !d.is_empty());
                        if let Some(t) = &a.title {
                            clean_text(t, "title", 300)?;
                        }
                        let mut changes = vec![Change {
                            after: Some(format!("task in {}", col.name)),
                            ..from
                        }];
                        if let Some(t) = &a.title {
                            changes.push(change("Title", None, Some(t.trim().to_string())));
                        }
                        if let Some(d) = &due_at {
                            changes.push(change("Due", None, Some(local(d, tz))));
                        }
                        Ok(Plan {
                            preview: Preview {
                                summary: format!("File inbox item as a task in {}", col.name),
                                destructive: false,
                                changes,
                            },
                            action: Action::TriageTask {
                                id,
                                column_id: col.id,
                                title: a.title,
                                due: due_at,
                            },
                        })
                    }
                    TriageAs::Event => {
                        let start = parse_when(
                            a.start
                                .as_deref()
                                .ok_or_else(|| invalid("start is required to file an event"))?,
                            tz,
                        )?;
                        let end = match a.end.as_deref() {
                            Some(e) => parse_when(e, tz)?,
                            None => start + Duration::hours(1),
                        };
                        let probe = CreateEventInput {
                            title: a.title.clone().unwrap_or_else(|| "x".into()),
                            description: None,
                            location: None,
                            all_day: None,
                            start_at: fmt_utc(start),
                            end_at: fmt_utc(end),
                            timezone: env.timezone.clone(),
                            recurrence_rule: None,
                        };
                        CalendarService::check_create(&probe)?;
                        let changes = vec![
                            Change {
                                after: Some("event".into()),
                                ..from
                            },
                            change("When", None, Some(local(&probe.start_at, tz))),
                        ];
                        Ok(Plan {
                            preview: Preview {
                                summary: "File inbox item as an event".into(),
                                destructive: false,
                                changes,
                            },
                            action: Action::TriageEvent {
                                id,
                                title: a.title,
                                start: fmt_utc(start),
                                end: fmt_utc(end),
                                tz: env.timezone.clone(),
                            },
                        })
                    }
                }
            }
            "remember" => {
                let a: RememberArgs = args(input)?;
                let text = validate_memory_text(&a.text)?;
                Ok(Plan {
                    preview: Preview {
                        summary: "Remember this about you".into(),
                        destructive: false,
                        changes: vec![change("Memory", None, Some(text.clone()))],
                    },
                    action: Action::Remember(text),
                })
            }
            "forget" => {
                let a: ForgetArgs = args(input)?;
                let id = parse_uuid(&a.memory_id, "memory id")?;
                let m = self
                    .d
                    .ai
                    .get_memory(&id)
                    .await?
                    .ok_or(DomainError::NotFound {
                        entity: "memory",
                        id: id.clone(),
                    })?;
                Ok(Plan {
                    preview: Preview {
                        summary: "Forget a saved memory".into(),
                        destructive: true,
                        changes: vec![change("Memory", Some(m.text), Some("deleted".into()))],
                    },
                    action: Action::Forget(id),
                })
            }
            "draft_automation_rule" => {
                let a: DraftRuleArgs = args(input)?;
                let rule = RuleInput {
                    name: a.name,
                    enabled: Some(true),
                    trigger: serde_json::from_value(a.trigger.clone())
                        .map_err(|e| invalid(format!("invalid trigger: {e}")))?,
                    action: serde_json::from_value(a.action.clone())
                        .map_err(|e| invalid(format!("invalid action: {e}")))?,
                };
                AutomationService::validate(&rule)?;
                let show = |v: &Value| serde_json::to_string(v).unwrap_or_default();
                Ok(Plan {
                    preview: Preview {
                        summary: format!(
                            "Create automation rule “{}” (enabled once confirmed)",
                            rule.name.trim()
                        ),
                        destructive: false,
                        changes: vec![
                            change("Trigger", None, Some(show(&a.trigger))),
                            change("Action", None, Some(show(&a.action))),
                        ],
                    },
                    action: Action::Rule(rule),
                })
            }
            other => Err(invalid(format!("unknown tool '{other}'"))),
        }
    }
}
