use serde::{Deserialize, Serialize};

use crate::error::{DomainError, DomainResult};

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub color: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Board {
    pub id: String,
    pub project_id: String,
    pub name: String,
    pub position: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ColumnKind {
    Backlog,
    Active,
    Done,
}

impl ColumnKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ColumnKind::Backlog => "backlog",
            ColumnKind::Active => "active",
            ColumnKind::Done => "done",
        }
    }
    pub fn parse(s: &str) -> DomainResult<Self> {
        match s {
            "backlog" => Ok(Self::Backlog),
            "active" => Ok(Self::Active),
            "done" => Ok(Self::Done),
            _ => Err(DomainError::Validation(
                "column kind must be backlog, active or done".into(),
            )),
        }
    }
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Column {
    pub id: String,
    pub board_id: String,
    pub name: String,
    pub kind: String,
    pub position: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Task {
    pub id: String,
    pub column_id: String,
    pub parent_id: Option<String>,
    pub title: String,
    pub description: Option<String>,
    /// 0 none, 1 low, 2 medium, 3 high
    pub priority: i64,
    pub due_at: Option<String>,
    pub position: i64,
    pub completed_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    /// RRULE for recurring tasks; `due_at` is the series start.
    pub recurrence_rule: Option<String>,
    pub recurrence_tz: Option<String>,
}

pub const TASK_COLS: &str = "id, column_id, parent_id, title, description, priority, due_at, position, completed_at, created_at, updated_at, recurrence_rule, recurrence_tz";

/// Validated input for creating a task (and optionally a subtree below it).
#[derive(Debug, Clone)]
pub struct NewTask {
    pub column_id: String,
    pub parent_id: Option<String>,
    pub title: String,
    pub description: Option<String>,
    pub priority: i64,
    pub due_at: Option<String>,
    /// `None` appends at the end.
    pub position: Option<i64>,
    pub checklist: Vec<String>,
    pub children: Vec<NewTask>,
}

/// Validated partial update. `Some(None)` clears a nullable field.
#[derive(Debug, Clone, Default)]
pub struct TaskPatch {
    pub title: Option<String>,
    pub description: Option<Option<String>>,
    pub priority: Option<i64>,
    pub due_at: Option<Option<String>>,
    pub completed: Option<bool>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct ChecklistItem {
    pub id: String,
    pub task_id: String,
    pub text: String,
    pub done: bool,
    pub position: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Dependency {
    pub task_id: String,
    pub blocked_by_id: String,
}

/// Everything the board UI needs in one round trip.
#[derive(Debug, Clone, Serialize)]
pub struct BoardView {
    pub board: Board,
    pub columns: Vec<Column>,
    pub tasks: Vec<Task>,
    pub checklist_items: Vec<ChecklistItem>,
    pub dependencies: Vec<Dependency>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct TaskTemplate {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub tree_json: String,
    pub created_at: String,
    pub updated_at: String,
}

/// A node of a stored template tree.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateNode {
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub priority: i64,
    /// Due date as days from instantiation time.
    #[serde(default)]
    pub due_offset_days: Option<i64>,
    #[serde(default)]
    pub children: Vec<TemplateNode>,
    #[serde(default)]
    pub checklist: Vec<String>,
}
