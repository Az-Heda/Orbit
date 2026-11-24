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
    /// Sends the channel's message template, rendered with the trigger's data. The channel is
    /// referenced by id (it survives renames); its token/webhook never appears in a rule.
    SendNotification { channel_id: String },
}

/// A rule runs at most this many steps (a chain).
pub const MAX_STEPS: usize = 10;

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
            Action::SendNotification { .. } => "send_notification",
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
            Action::SendNotification { channel_id } => {
                parse_uuid(channel_id, "channel id")?;
            }
        }
        Ok(())
    }

    /// Validate an ordered chain: 1..=MAX_STEPS steps, each valid.
    pub fn validate_chain(actions: &[Action]) -> DomainResult<()> {
        if actions.is_empty() {
            return Err(invalid("a rule needs at least one step"));
        }
        if actions.len() > MAX_STEPS {
            return Err(invalid(format!(
                "a rule can have at most {MAX_STEPS} steps"
            )));
        }
        for (i, a) in actions.iter().enumerate() {
            a.validate()
                .map_err(|e| invalid(format!("step {}: {e}", i + 1)))?;
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
    /// JSON array of actions (the chain, in order).
    pub actions: String,
    pub created_at: String,
    pub updated_at: String,
}

pub const RULE_COLS: &str =
    "id, name, enabled, trigger_kind, trigger_config, actions, created_at, updated_at";

#[derive(Debug, Clone, Serialize)]
pub struct Rule {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub trigger: Trigger,
    /// The chain: steps run in order; a failing step is logged and stops the chain.
    pub actions: Vec<Action>,
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
            actions: serde_json::from_str(&self.actions).map_err(|_| bad("actions"))?,
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

/// Everything a template can refer to. Unknown placeholders are left as they are; known ones with
/// no value (e.g. `{{task.title}}` on a timer trigger) become empty text.
#[derive(Debug, Clone, Default)]
pub struct RenderCtx {
    /// The triggering item's headline (task title, timer label, first line of an inbox item).
    pub title: String,
    /// UTC date, `YYYY-MM-DD`.
    pub date: String,
    pub rule_name: String,
    /// Trigger kind, e.g. `task_moved`.
    pub trigger: String,
    pub task_title: String,
    pub timer_label: String,
    pub inbox_text: String,
}

/// Placeholders understood by `RenderCtx::render` (shown in the UI).
pub const PLACEHOLDERS: &[&str] = &[
    "{{title}}",
    "{{date}}",
    "{{rule.name}}",
    "{{trigger}}",
    "{{task.title}}",
    "{{timer.label}}",
    "{{inbox.text}}",
];

impl RenderCtx {
    /// One pass over the template, so a value that itself contains `{{...}}` is never re-expanded.
    pub fn render(&self, template: &str) -> String {
        let mut out = String::with_capacity(template.len());
        let mut rest = template;
        while let Some(start) = rest.find("{{") {
            out.push_str(&rest[..start]);
            let after = &rest[start..];
            let Some(end) = after.find("}}") else {
                out.push_str(after);
                return out;
            };
            let key = &after[2..end];
            match self.value(key.trim()) {
                Some(v) => out.push_str(v),
                None => out.push_str(&after[..end + 2]),
            }
            rest = &after[end + 2..];
        }
        out.push_str(rest);
        out
    }

    fn value(&self, key: &str) -> Option<&str> {
        Some(match key {
            "title" => &self.title,
            "date" => &self.date,
            "rule.name" => &self.rule_name,
            "trigger" => &self.trigger,
            "task.title" => &self.task_title,
            "timer.label" => &self.timer_label,
            "inbox.text" => &self.inbox_text,
            _ => return None,
        })
    }
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

    #[test]
    fn chain_validation() {
        let note = Action::CreateNote {
            title: "t".into(),
            body: String::new(),
        };
        assert!(Action::validate_chain(&[]).is_err(), "empty chain");
        assert!(Action::validate_chain(std::slice::from_ref(&note)).is_ok());
        assert!(Action::validate_chain(&vec![note.clone(); MAX_STEPS + 1]).is_err());
        let bad = Action::MoveTask {
            column_id: "nope".into(),
        };
        let e = Action::validate_chain(&[note, bad])
            .unwrap_err()
            .to_string();
        assert!(e.contains("step 2"), "{e}");
        assert!(
            Action::SendNotification {
                channel_id: "nope".into()
            }
            .validate()
            .is_err()
        );
    }

    #[test]
    fn render_context_placeholders() {
        let ctx = RenderCtx {
            title: "Ship {{rule.name}}".into(),
            date: "2026-01-01".into(),
            rule_name: "Daily".into(),
            trigger: "task_moved".into(),
            task_title: "Write docs".into(),
            ..Default::default()
        };
        assert_eq!(
            ctx.render(
                "[{{rule.name}}] {{ task.title }} / {{timer.label}} / {{unknown}} / {{title}}"
            ),
            "[Daily] Write docs /  / {{unknown}} / Ship {{rule.name}}",
            "values are not re-expanded, unknown placeholders stay"
        );
        assert_eq!(ctx.render("open {{brace"), "open {{brace");
    }
}
