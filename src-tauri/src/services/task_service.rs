use std::collections::HashSet;
use std::sync::Arc;

use serde::Deserialize;

use crate::domain::activity::Actor;
use crate::domain::events::{DomainEvent, EventBus, MaybeBus, emit};
use crate::domain::kanban::{BoardView, ChecklistItem, NewTask, Task, TaskPatch};
use crate::domain::validation::{clean_optional, clean_text, invalid, normalize_utc, parse_uuid};
use crate::error::{DomainError, DomainResult};
use crate::infrastructure::database::{ProjectRepository, TaskRepository};

#[derive(Debug, Deserialize)]
pub struct CreateTaskInput {
    /// Required unless `parent_id` is set (subtasks live in their parent's column).
    pub column_id: Option<String>,
    pub parent_id: Option<String>,
    pub title: String,
    pub description: Option<String>,
    pub priority: Option<i64>,
    /// RFC 3339 timestamp.
    pub due_at: Option<String>,
    pub position: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateTaskInput {
    pub title: Option<String>,
    /// Empty string clears.
    pub description: Option<String>,
    pub priority: Option<i64>,
    /// Empty string clears.
    pub due_at: Option<String>,
}

pub struct TaskService {
    tasks: Arc<dyn TaskRepository>,
    projects: Arc<dyn ProjectRepository>,
    bus: MaybeBus,
}

pub fn validate_priority(p: i64) -> DomainResult<i64> {
    if (0..=3).contains(&p) {
        Ok(p)
    } else {
        Err(invalid("priority must be between 0 and 3"))
    }
}

impl TaskService {
    pub fn new(tasks: Arc<dyn TaskRepository>, projects: Arc<dyn ProjectRepository>) -> Self {
        Self {
            tasks,
            projects,
            bus: None,
        }
    }

    /// Publish domain events (task created/moved/completed) on this bus after each commit.
    pub fn with_bus(mut self, bus: Arc<EventBus>) -> Self {
        self.bus = Some(bus);
        self
    }

    async fn require_task(&self, id: &str) -> DomainResult<Task> {
        self.tasks.get_task(id).await?.ok_or(DomainError::NotFound {
            entity: "task",
            id: id.to_string(),
        })
    }

    /// Validate a create request and resolve its column.
    pub async fn build_new_task(&self, input: &CreateTaskInput) -> DomainResult<NewTask> {
        let parent_id = input
            .parent_id
            .as_deref()
            .map(|p| parse_uuid(p, "parent id"))
            .transpose()?;
        let column_id = match &parent_id {
            Some(p) => self.require_task(p).await?.column_id,
            None => parse_uuid(
                input
                    .column_id
                    .as_deref()
                    .ok_or_else(|| invalid("column_id is required"))?,
                "column id",
            )?,
        };
        if let Some(pos) = input.position
            && pos < 0
        {
            return Err(invalid("position must be >= 0"));
        }
        Ok(NewTask {
            column_id,
            parent_id,
            title: clean_text(&input.title, "title", 300)?,
            description: input
                .description
                .as_deref()
                .map(|d| clean_optional(d, "description", 10_000))
                .transpose()?
                .flatten(),
            priority: validate_priority(input.priority.unwrap_or(0))?,
            due_at: input
                .due_at
                .as_deref()
                .filter(|d| !d.is_empty())
                .map(|d| normalize_utc(d, "due_at"))
                .transpose()?,
            position: input.position,
            checklist: vec![],
            children: vec![],
        })
    }

    pub async fn create_task(&self, input: CreateTaskInput, actor: Actor) -> DomainResult<Task> {
        let new = self.build_new_task(&input).await?;
        let mut roots = self.tasks.create_task_trees(&[new], actor).await?;
        let task = roots.pop().ok_or_else(|| invalid("nothing created"))?;
        emit(
            &self.bus,
            DomainEvent::TaskCreated {
                task_id: task.id.clone(),
                title: task.title.clone(),
            },
        );
        Ok(task)
    }

    /// Create several validated task trees atomically (used by templates).
    pub async fn create_trees(&self, trees: Vec<NewTask>, actor: Actor) -> DomainResult<Vec<Task>> {
        self.tasks.create_task_trees(&trees, actor).await
    }

    /// A column by id (used to describe where a task would go).
    pub async fn column(&self, id: &str) -> DomainResult<crate::domain::kanban::Column> {
        let id = parse_uuid(id, "column id")?;
        self.projects
            .get_column(&id)
            .await?
            .ok_or(DomainError::NotFound {
                entity: "column",
                id,
            })
    }

    pub async fn get_task(&self, id: &str) -> DomainResult<Task> {
        self.require_task(&parse_uuid(id, "task id")?).await
    }

    pub async fn update_task(
        &self,
        id: &str,
        input: UpdateTaskInput,
        actor: Actor,
    ) -> DomainResult<Task> {
        let id = parse_uuid(id, "task id")?;
        let patch = TaskPatch {
            title: input
                .title
                .as_deref()
                .map(|t| clean_text(t, "title", 300))
                .transpose()?,
            description: input
                .description
                .as_deref()
                .map(|d| clean_optional(d, "description", 10_000))
                .transpose()?,
            priority: input.priority.map(validate_priority).transpose()?,
            due_at: input
                .due_at
                .as_deref()
                .map(|d| {
                    if d.is_empty() {
                        Ok(None)
                    } else {
                        normalize_utc(d, "due_at").map(Some)
                    }
                })
                .transpose()?,
            completed: None,
        };
        self.tasks.update_task(&id, &patch, actor).await
    }

    /// Warn (require `force`) when completing/activating a task that has open subtasks or blockers.
    async fn check_warnings(
        &self,
        task: &Task,
        to_done: bool,
        to_active: bool,
        force: bool,
    ) -> DomainResult<()> {
        if force {
            return Ok(());
        }
        if to_done {
            let open = self.tasks.open_descendant_count(&task.id).await?;
            if open > 0 {
                return Err(DomainError::RequiresConfirmation(format!(
                    "this task still has {open} open subtask(s)"
                )));
            }
        }
        if to_done || to_active {
            let blockers = self.tasks.unresolved_blockers(&task.id).await?;
            if !blockers.is_empty() {
                let names: Vec<_> = blockers.iter().map(|b| b.title.as_str()).collect();
                return Err(DomainError::RequiresConfirmation(format!(
                    "this task is blocked by unfinished task(s): {}",
                    names.join(", ")
                )));
            }
        }
        Ok(())
    }

    /// The warning a move would raise (open subtasks / unfinished blockers), if any. Lets previews
    /// show it up front instead of failing at apply time.
    pub async fn move_warning(&self, id: &str, column_id: &str) -> DomainResult<Option<String>> {
        let task = self.require_task(&parse_uuid(id, "task id")?).await?;
        let target = self.column(column_id).await?;
        if task.column_id == target.id {
            return Ok(None);
        }
        let to_done = target.kind == "done" && task.completed_at.is_none();
        let to_active = target.kind == "active";
        Self::warning_text(self.check_warnings(&task, to_done, to_active, false).await)
    }

    /// The warning completing a task would raise, if any.
    pub async fn complete_warning(&self, id: &str) -> DomainResult<Option<String>> {
        let task = self.require_task(&parse_uuid(id, "task id")?).await?;
        if task.completed_at.is_some() {
            return Ok(None);
        }
        Self::warning_text(self.check_warnings(&task, true, false, false).await)
    }

    fn warning_text(r: DomainResult<()>) -> DomainResult<Option<String>> {
        match r {
            Ok(()) => Ok(None),
            Err(DomainError::RequiresConfirmation(m)) => Ok(Some(m)),
            Err(e) => Err(e),
        }
    }

    pub async fn move_task(
        &self,
        id: &str,
        column_id: &str,
        position: i64,
        force: bool,
        actor: Actor,
    ) -> DomainResult<Task> {
        let id = parse_uuid(id, "task id")?;
        let column_id = parse_uuid(column_id, "column id")?;
        if position < 0 {
            return Err(invalid("position must be >= 0"));
        }
        let task = self.require_task(&id).await?;
        if task.parent_id.is_some() {
            return Err(invalid("subtasks move together with their parent task"));
        }
        let target = self
            .projects
            .get_column(&column_id)
            .await?
            .ok_or(DomainError::NotFound {
                entity: "column",
                id: column_id.clone(),
            })?;
        let current = self.projects.get_column(&task.column_id).await?;
        if current.as_ref().map(|c| &c.board_id) != Some(&target.board_id) {
            return Err(invalid("tasks can only move within their board"));
        }
        let changing = task.column_id != column_id;
        let completing = changing && target.kind == "done" && task.completed_at.is_none();
        if changing {
            let to_done = target.kind == "done" && task.completed_at.is_none();
            let to_active = target.kind == "active";
            self.check_warnings(&task, to_done, to_active, force)
                .await?;
        }
        let moved = self
            .tasks
            .move_task(&id, &column_id, position, actor)
            .await?;
        if changing {
            emit(
                &self.bus,
                DomainEvent::TaskMoved {
                    task_id: moved.id.clone(),
                    title: moved.title.clone(),
                    to_column_id: column_id.clone(),
                    to_column_kind: target.kind.clone(),
                },
            );
            if completing {
                emit(
                    &self.bus,
                    DomainEvent::TaskCompleted {
                        task_id: moved.id.clone(),
                        title: moved.title.clone(),
                    },
                );
            }
        }
        Ok(moved)
    }

    /// Mark a task done/undone. Top-level tasks move to the board's Done (or first open) column.
    pub async fn set_completed(
        &self,
        id: &str,
        completed: bool,
        force: bool,
        actor: Actor,
    ) -> DomainResult<Task> {
        let id = parse_uuid(id, "task id")?;
        let task = self.require_task(&id).await?;
        if task.parent_id.is_some() {
            if completed && task.completed_at.is_none() {
                self.check_warnings(&task, true, false, force).await?;
            }
            let patch = TaskPatch {
                completed: Some(completed),
                ..Default::default()
            };
            let t = self.tasks.update_task(&id, &patch, actor).await?;
            if completed && task.completed_at.is_none() {
                emit(
                    &self.bus,
                    DomainEvent::TaskCompleted {
                        task_id: t.id.clone(),
                        title: t.title.clone(),
                    },
                );
            }
            return Ok(t);
        }
        let col = self
            .projects
            .get_column(&task.column_id)
            .await?
            .ok_or_else(|| invalid("task column missing"))?;
        let columns = self.projects.list_columns(&col.board_id).await?;
        let target = if completed {
            columns.iter().find(|c| c.kind == "done")
        } else {
            columns
                .iter()
                .find(|c| c.kind == "active")
                .or_else(|| columns.iter().find(|c| c.kind != "done"))
        }
        .ok_or_else(|| invalid("board has no suitable column"))?;
        if completed == task.completed_at.is_some() {
            return Ok(task);
        }
        self.move_task(&id, &target.id, 0, force, actor).await
    }

    pub async fn delete_task(&self, id: &str, actor: Actor) -> DomainResult<()> {
        self.tasks
            .delete_task(&parse_uuid(id, "task id")?, actor)
            .await
    }

    /// Reject self edges and any edge that would close a cycle.
    pub async fn add_dependency(
        &self,
        task_id: &str,
        blocked_by_id: &str,
        actor: Actor,
    ) -> DomainResult<()> {
        let task_id = parse_uuid(task_id, "task id")?;
        let blocker = parse_uuid(blocked_by_id, "blocked_by id")?;
        if task_id == blocker {
            return Err(invalid("a task cannot block itself"));
        }
        // New edge: task_id blocked_by blocker. It closes a cycle iff task_id is already
        // reachable from blocker by following blocked_by edges.
        let mut seen = HashSet::new();
        let mut stack = vec![blocker.clone()];
        while let Some(cur) = stack.pop() {
            if cur == task_id {
                return Err(invalid("this dependency would create a cycle"));
            }
            if seen.insert(cur.clone()) {
                stack.extend(self.tasks.blocked_by_ids(&cur).await?);
            }
        }
        self.tasks.add_dependency(&task_id, &blocker, actor).await
    }

    pub async fn remove_dependency(
        &self,
        task_id: &str,
        blocked_by_id: &str,
        actor: Actor,
    ) -> DomainResult<()> {
        self.tasks
            .remove_dependency(
                &parse_uuid(task_id, "task id")?,
                &parse_uuid(blocked_by_id, "blocked_by id")?,
                actor,
            )
            .await
    }

    pub async fn add_checklist_item(
        &self,
        task_id: &str,
        text: &str,
        actor: Actor,
    ) -> DomainResult<ChecklistItem> {
        self.tasks
            .add_checklist_item(
                &parse_uuid(task_id, "task id")?,
                &clean_text(text, "text", 500)?,
                actor,
            )
            .await
    }

    pub async fn update_checklist_item(
        &self,
        id: &str,
        text: Option<&str>,
        done: Option<bool>,
        actor: Actor,
    ) -> DomainResult<ChecklistItem> {
        let text = text.map(|t| clean_text(t, "text", 500)).transpose()?;
        self.tasks
            .update_checklist_item(
                &parse_uuid(id, "checklist item id")?,
                text.as_deref(),
                done,
                actor,
            )
            .await
    }

    pub async fn delete_checklist_item(&self, id: &str, actor: Actor) -> DomainResult<()> {
        self.tasks
            .delete_checklist_item(&parse_uuid(id, "checklist item id")?, actor)
            .await
    }

    pub async fn board_view(&self, board_id: &str) -> DomainResult<BoardView> {
        let board_id = parse_uuid(board_id, "board id")?;
        let board = self
            .projects
            .get_board(&board_id)
            .await?
            .ok_or(DomainError::NotFound {
                entity: "board",
                id: board_id.clone(),
            })?;
        Ok(BoardView {
            columns: self.projects.list_columns(&board_id).await?,
            tasks: self.tasks.list_for_board(&board_id).await?,
            checklist_items: self.tasks.checklist_for_board(&board_id).await?,
            dependencies: self.tasks.dependencies_for_board(&board_id).await?,
            board,
        })
    }
}
