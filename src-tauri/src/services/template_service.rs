use std::sync::Arc;

use chrono::{Duration, Utc};

use crate::domain::activity::Actor;
use crate::domain::kanban::{NewTask, Task, TaskTemplate, TemplateNode};
use crate::domain::validation::{clean_optional, clean_text, fmt_utc, invalid, parse_uuid};
use crate::error::{DomainError, DomainResult};
use crate::infrastructure::database::{ProjectRepository, TaskRepository, TemplateRepository};
use crate::services::task_service::{CreateTaskInput, TaskService, validate_priority};

const MAX_NODES: usize = 200;
const MAX_DEPTH: usize = 6;

pub struct TemplateService {
    repo: Arc<dyn TemplateRepository>,
    tasks_repo: Arc<dyn TaskRepository>,
    projects: Arc<dyn ProjectRepository>,
    task_service: Arc<TaskService>,
}

fn sanitize(
    nodes: Vec<TemplateNode>,
    depth: usize,
    count: &mut usize,
) -> DomainResult<Vec<TemplateNode>> {
    if depth > MAX_DEPTH {
        return Err(invalid("template is nested too deeply"));
    }
    nodes
        .into_iter()
        .map(|n| {
            *count += 1;
            if *count > MAX_NODES {
                return Err(invalid("template has too many tasks"));
            }
            Ok(TemplateNode {
                title: clean_text(&n.title, "title", 300)?,
                description: n
                    .description
                    .as_deref()
                    .map(|d| clean_optional(d, "description", 10_000))
                    .transpose()?
                    .flatten(),
                priority: validate_priority(n.priority)?,
                due_offset_days: match n.due_offset_days {
                    Some(d) if !(0..=3650).contains(&d) => {
                        return Err(invalid("due_offset_days out of range"));
                    }
                    other => other,
                },
                checklist: n
                    .checklist
                    .iter()
                    .map(|c| clean_text(c, "checklist item", 500))
                    .collect::<DomainResult<_>>()?,
                children: sanitize(n.children, depth + 1, count)?,
            })
        })
        .collect()
}

fn to_new_tasks(
    nodes: &[TemplateNode],
    column_id: &str,
    parent: Option<&str>,
    now: chrono::DateTime<Utc>,
) -> Vec<NewTask> {
    nodes
        .iter()
        .map(|n| NewTask {
            column_id: column_id.to_string(),
            parent_id: parent.map(str::to_string),
            title: n.title.clone(),
            description: n.description.clone(),
            priority: n.priority,
            due_at: n.due_offset_days.map(|d| fmt_utc(now + Duration::days(d))),
            position: None,
            checklist: n.checklist.clone(),
            // children inherit the column; their parent id is assigned by the repository
            children: to_new_tasks(&n.children, column_id, None, now),
        })
        .collect()
}

impl TemplateService {
    pub fn new(
        repo: Arc<dyn TemplateRepository>,
        tasks_repo: Arc<dyn TaskRepository>,
        projects: Arc<dyn ProjectRepository>,
        task_service: Arc<TaskService>,
    ) -> Self {
        Self {
            repo,
            tasks_repo,
            projects,
            task_service,
        }
    }

    pub async fn list(&self) -> DomainResult<Vec<TaskTemplate>> {
        self.repo.list().await
    }

    pub async fn create(
        &self,
        name: &str,
        description: Option<&str>,
        tree: Vec<TemplateNode>,
        actor: Actor,
    ) -> DomainResult<TaskTemplate> {
        let name = clean_text(name, "name", 120)?;
        let description = description
            .map(|d| clean_optional(d, "description", 2000))
            .transpose()?
            .flatten();
        if tree.is_empty() {
            return Err(invalid("template needs at least one task"));
        }
        let tree = sanitize(tree, 0, &mut 0)?;
        let json = serde_json::to_string(&tree).map_err(|e| DomainError::Storage(e.to_string()))?;
        self.repo
            .create(&name, description.as_deref(), &json, actor)
            .await
    }

    pub async fn delete(&self, id: &str, actor: Actor) -> DomainResult<()> {
        self.repo
            .delete(&parse_uuid(id, "template id")?, actor)
            .await
    }

    /// Instantiate through the normal task service so validation and activity logging apply.
    pub async fn instantiate(
        &self,
        template_id: &str,
        column_id: &str,
        parent_id: Option<&str>,
        actor: Actor,
    ) -> DomainResult<Vec<Task>> {
        let template_id = parse_uuid(template_id, "template id")?;
        let t = self
            .repo
            .get(&template_id)
            .await?
            .ok_or(DomainError::NotFound {
                entity: "task_template",
                id: template_id,
            })?;
        let nodes: Vec<TemplateNode> = serde_json::from_str(&t.tree_json)
            .map_err(|e| DomainError::Storage(format!("corrupt template: {e}")))?;
        let nodes = sanitize(nodes, 0, &mut 0)?;
        // Resolve the destination column (validates ids / parent) via the task service.
        let probe = self
            .task_service
            .build_new_task(&CreateTaskInput {
                column_id: Some(column_id.to_string()),
                parent_id: parent_id.map(str::to_string),
                title: "template".into(),
                description: None,
                priority: None,
                due_at: None,
                position: None,
            })
            .await?;
        let mut trees = to_new_tasks(
            &nodes,
            &probe.column_id,
            probe.parent_id.as_deref(),
            Utc::now(),
        );
        for tree in &mut trees {
            tree.parent_id = probe.parent_id.clone();
        }
        self.task_service.create_trees(trees, actor).await
    }

    /// Build a template from an existing task and its subtasks (checklists included).
    pub async fn save_task_as_template(
        &self,
        task_id: &str,
        name: &str,
        actor: Actor,
    ) -> DomainResult<TaskTemplate> {
        let task_id = parse_uuid(task_id, "task id")?;
        let task = self.task_service.get_task(&task_id).await?;
        let board_id = self
            .tasks_repo
            .board_id_of_column(&task.column_id)
            .await?
            .ok_or_else(|| invalid("task column missing"))?;
        let _ = &self.projects;
        let all = self.tasks_repo.list_for_board(&board_id).await?;
        let items = self.tasks_repo.checklist_for_board(&board_id).await?;
        let now = Utc::now();
        fn build(
            t: &Task,
            all: &[Task],
            items: &[crate::domain::kanban::ChecklistItem],
            now: chrono::DateTime<Utc>,
            depth: usize,
        ) -> TemplateNode {
            let due_offset_days = t
                .due_at
                .as_deref()
                .and_then(|d| crate::domain::validation::parse_utc(d).ok())
                .map(|d| (d - now).num_days().max(0));
            TemplateNode {
                title: t.title.clone(),
                description: t.description.clone(),
                priority: t.priority,
                due_offset_days,
                checklist: items
                    .iter()
                    .filter(|i| i.task_id == t.id)
                    .map(|i| i.text.clone())
                    .collect(),
                children: if depth >= MAX_DEPTH {
                    vec![]
                } else {
                    all.iter()
                        .filter(|c| c.parent_id.as_deref() == Some(&t.id))
                        .map(|c| build(c, all, items, now, depth + 1))
                        .collect()
                },
            }
        }
        let node = build(&task, &all, &items, now, 0);
        self.create(name, None, vec![node], actor).await
    }
}
