use serde::Serialize;

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Event {
    pub id: String,
    pub uid: String,
    pub title: String,
    pub description: Option<String>,
    pub location: Option<String>,
    pub all_day: bool,
    pub start_at: String,
    pub end_at: String,
    pub timezone: String,
    pub recurrence_rule: Option<String>,
    /// Set for events that come from a subscribed feed: read-only, never editable (also by the AI).
    pub external_calendar_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

pub const EVENT_COLS: &str = "id, uid, title, description, location, all_day, start_at, end_at, timezone, recurrence_rule, external_calendar_id, created_at, updated_at";

#[derive(Debug, Clone)]
pub struct NewEvent {
    pub uid: String,
    pub title: String,
    pub description: Option<String>,
    pub location: Option<String>,
    pub all_day: bool,
    pub start_at: String,
    pub end_at: String,
    pub timezone: String,
    pub recurrence_rule: Option<String>,
}

/// Fully-resolved update (the service merges patches before calling the repository).
#[derive(Debug, Clone)]
pub struct EventUpdate {
    pub title: String,
    pub description: Option<String>,
    pub location: Option<String>,
    pub all_day: bool,
    pub start_at: String,
    pub end_at: String,
    pub timezone: String,
    pub recurrence_rule: Option<String>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct EventException {
    pub id: String,
    pub event_id: String,
    pub original_start: String,
    /// "skip" or "override"
    pub kind: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub location: Option<String>,
    pub start_at: Option<String>,
    pub end_at: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Default)]
pub struct NewException {
    pub original_start: String,
    pub skip: bool,
    pub title: Option<String>,
    pub description: Option<String>,
    pub location: Option<String>,
    pub start_at: Option<String>,
    pub end_at: Option<String>,
}

/// A task with a due date, plus where it lives (for linking from the calendar).
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct TaskDeadline {
    pub id: String,
    pub title: String,
    pub priority: i64,
    pub due_at: String,
    pub completed_at: Option<String>,
    pub recurrence_rule: Option<String>,
    pub recurrence_tz: Option<String>,
    pub board_id: String,
    pub project_id: String,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct TaskOccurrence {
    pub task_id: String,
    pub original_due: String,
    pub status: String,
    pub completed_at: Option<String>,
}

/// One thing to draw on the calendar: an event occurrence or a task deadline.
#[derive(Debug, Clone, Serialize)]
pub struct CalendarItem {
    /// Unique and stable per occurrence.
    pub key: String,
    /// "event" or "task"
    pub kind: &'static str,
    pub source_id: String,
    pub title: String,
    pub description: Option<String>,
    pub location: Option<String>,
    pub start_at: String,
    pub end_at: String,
    pub all_day: bool,
    pub timezone: Option<String>,
    pub recurring: bool,
    /// Original start of this occurrence (key for skip/override/complete).
    pub occurrence_start: Option<String>,
    pub overridden: bool,
    pub completed: bool,
    pub priority: i64,
    pub project_id: Option<String>,
    pub board_id: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ImportSummary {
    pub created: usize,
    pub skipped_duplicates: usize,
    pub warnings: Vec<String>,
}
