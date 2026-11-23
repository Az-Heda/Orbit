//! Automation rules: a closed set of triggers and actions. Anything outside this set is rejected.

use chrono::NaiveTime;
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};

use super::events::DomainEvent;
use super::validation::{clean_text, invalid, parse_uuid};
use crate::error::DomainResult;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Trigger {
    TaskCreated,
    TaskMoved {
        /// Only when moved into a column of this kind (backlog | active | done).
        #[serde(default)]
        to_column_kind: Option<String>,
        #[serde(default)]
        to_column_id: Option<String>,
    },
    TaskCompleted,
    TimerEnded,
    InboxAdded,
    /// Daily/weekly at a local wall-clock time. `days`: 1 = Monday .. 7 = Sunday; empty = every day.
    Schedule {
        time: String,
        #[serde(default)]
        days: Vec<u8>,
        timezone: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Action {
    /// `title`/`description` may contain `{{title}}` (the triggering item) and `{{date}}` (UTC date).
    CreateTask {
        column_id: String,
        title: String,
        #[serde(default)]
        description: Option<String>,
    },
    CreateNote {
        title: String,
        #[serde(default)]
        body: String,
    },
    /// Moves the task that triggered the rule.
    MoveTask { column_id: String },
    StartTimer {
        /// pomodoro | countdown
        timer_kind: String,
        minutes: i64,
        #[serde(default)]
        label: Option<String>,
    },
    Notify {
        title: String,
        #[serde(default)]
        body: Option<String>,
    },
}

impl Trigger {
    pub fn kind(&self) -> &'static str {
        match self {
            Trigger::TaskCreated => "task_created",
            Trigger::TaskMoved { .. } => "task_moved",
            Trigger::TaskCompleted => "task_completed",
            Trigger::TimerEnded => "timer_ended",
            Trigger::InboxAdded => "inbox_added",
            Trigger::Schedule { .. } => "schedule",
        }
    }

    pub fn validate(&self) -> DomainResult<()> {
        match self {
            Trigger::TaskMoved {
                to_column_kind,
                to_column_id,
            } => {
                if let Some(k) = to_column_kind
                    && !matches!(k.as_str(), "backlog" | "active" | "done")
                {
                    return Err(invalid("column kind must be backlog, active or done"));
                }
                if let Some(id) = to_column_id {
                    parse_uuid(id, "column id")?;
                }
            }
            Trigger::Schedule {
                time,
                days,
                timezone,
            } => {
                NaiveTime::parse_from_str(time, "%H:%M")
                    .map_err(|_| invalid("time must look like HH:MM"))?;
                if days.iter().any(|d| !(1..=7).contains(d)) {
                    return Err(invalid("days must be between 1 (Mon) and 7 (Sun)"));
                }
                timezone
                    .parse::<Tz>()
                    .map_err(|_| invalid("unknown timezone"))?;
            }
            _ => {}
        }
        Ok(())
    }

    /// Does this trigger fire for the event? (`Schedule` is driven by the scheduler, never by events.)
    pub fn matches(&self, ev: &DomainEvent) -> bool {
        match (self, ev) {
            (Trigger::TaskCreated, DomainEvent::TaskCreated { .. })
            | (Trigger::TaskCompleted, DomainEvent::TaskCompleted { .. })
            | (Trigger::TimerEnded, DomainEvent::TimerEnded { .. })
            | (Trigger::InboxAdded, DomainEvent::InboxAdded { .. }) => true,
            (
                Trigger::TaskMoved {
                    to_column_kind,
                    to_column_id,
                },
                DomainEvent::TaskMoved {
                    to_column_id: id,
                    to_column_kind: kind,
                    ..
                },
            ) => {
                to_column_kind.as_ref().is_none_or(|k| k == kind)
                    && to_column_id.as_ref().is_none_or(|c| c == id)
            }
            _ => false,
        }
    }
}

impl Action {
    pub fn kind(&self) -> &'static str {
        match self {
            Action::CreateTask { .. } => "create_task",
            Action::CreateNote { .. } => "create_note",
            Action::MoveTask { .. } => "move_task",
            Action::StartTimer { .. } => "start_timer",
            Action::Notify { .. } => "notify",
        }
    }

    pub fn validate(&self) -> DomainResult<()> {
        match self {
            Action::CreateTask {
                column_id, title, ..
            } => {
                parse_uuid(column_id, "column id")?;
                clean_text(title, "task title", 300)?;
            }
            Action::CreateNote { title, body } => {
                clean_text(title, "note title", 300)?;
                if body.len() > 20_000 {
                    return Err(invalid("note body template is too long"));
                }
            }
            Action::MoveTask { column_id } => {
                parse_uuid(column_id, "column id")?;
            }
            Action::StartTimer {
                timer_kind,
                minutes,
                ..
            } => {
                if !matches!(timer_kind.as_str(), "pomodoro" | "countdown") {
                    return Err(invalid("timer_kind must be pomodoro or countdown"));
                }
                if !(1..=1440).contains(minutes) {
                    return Err(invalid("minutes must be between 1 and 1440"));
                }
            }
            Action::Notify { title, .. } => {
                clean_text(title, "notification title", 120)?;
            }
        }
        Ok(())
    }
}

/// Raw database row.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct RuleRow {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub trigger_kind: String,
    pub trigger_config: String,
    pub action_kind: String,
    pub action_config: String,
    pub created_at: String,
    pub updated_at: String,
}

pub const RULE_COLS: &str = "id, name, enabled, trigger_kind, trigger_config, action_kind, action_config, created_at, updated_at";

#[derive(Debug, Clone, Serialize)]
pub struct Rule {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub trigger: Trigger,
    pub action: Action,
    pub created_at: String,
    pub updated_at: String,
}

impl RuleRow {
    pub fn into_rule(self) -> DomainResult<Rule> {
        let bad = |what: &str| {
            crate::error::DomainError::Storage(format!("corrupt rule {what}: {}", self.id))
        };
        Ok(Rule {
            trigger: serde_json::from_str(&self.trigger_config).map_err(|_| bad("trigger"))?,
            action: serde_json::from_str(&self.action_config).map_err(|_| bad("action"))?,
            id: self.id,
            name: self.name,
            enabled: self.enabled,
            created_at: self.created_at,
            updated_at: self.updated_at,
        })
    }
}

/// Replace `{{title}}` and `{{date}}` in a template.
pub fn render(template: &str, title: &str, date: &str) -> String {
    template
        .replace("{{title}}", title)
        .replace("{{date}}", date)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_round_trip_and_validation() {
        let t: Trigger =
            serde_json::from_str(r#"{"kind":"task_moved","to_column_kind":"done"}"#).unwrap();
        assert!(t.validate().is_ok());
        assert!(
            serde_json::from_str::<Trigger>(r#"{"kind":"run_shell"}"#).is_err(),
            "closed set"
        );
        let bad = Trigger::Schedule {
            time: "25:00".into(),
            days: vec![],
            timezone: "UTC".into(),
        };
        assert!(bad.validate().is_err());
        let bad = Trigger::Schedule {
            time: "09:00".into(),
            days: vec![8],
            timezone: "UTC".into(),
        };
        assert!(bad.validate().is_err());
        assert!(
            Action::StartTimer {
                timer_kind: "stopwatch".into(),
                minutes: 5,
                label: None
            }
            .validate()
            .is_err()
        );
        assert_eq!(
            render("Done: {{title}} on {{date}}", "X", "2026-01-01"),
            "Done: X on 2026-01-01"
        );
    }
}
