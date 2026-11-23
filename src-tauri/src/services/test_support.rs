//! Test fixtures: all services wired over a single in-memory SQLite database.
use std::sync::Arc;

use sqlx::SqlitePool;

use super::*;
use crate::infrastructure::database::*;

pub struct Ctx {
    pub pool: SqlitePool,
    pub projects: ProjectService,
    pub tasks: Arc<TaskService>,
    pub templates: TemplateService,
    pub calendar: CalendarService,
    pub notes: NoteService,
    pub search: SearchService,
}

pub async fn ctx() -> Ctx {
    let pool = open_in_memory().await.unwrap();
    let project_repo: Arc<dyn ProjectRepository> =
        Arc::new(SqliteProjectRepository::new(pool.clone()));
    let task_repo: Arc<dyn TaskRepository> = Arc::new(SqliteTaskRepository::new(pool.clone()));
    let tasks = Arc::new(TaskService::new(task_repo.clone(), project_repo.clone()));
    Ctx {
        notes: NoteService::new(Arc::new(SqliteNoteRepository::new(pool.clone()))),
        search: SearchService::new(Arc::new(SqliteSearchRepository::new(pool.clone()))),
        calendar: CalendarService::new(Arc::new(SqliteCalendarRepository::new(pool.clone()))),
        projects: ProjectService::new(project_repo.clone()),
        templates: TemplateService::new(
            Arc::new(SqliteTemplateRepository::new(pool.clone())),
            task_repo,
            project_repo,
            tasks.clone(),
        ),
        tasks,
        pool,
    }
}

pub fn new_task(column_id: &str, title: &str) -> CreateTaskInput {
    CreateTaskInput {
        column_id: Some(column_id.into()),
        parent_id: None,
        title: title.into(),
        description: None,
        priority: None,
        due_at: None,
        position: None,
    }
}

pub fn sub_task(parent_id: &str, title: &str) -> CreateTaskInput {
    CreateTaskInput {
        column_id: None,
        parent_id: Some(parent_id.into()),
        title: title.into(),
        description: None,
        priority: None,
        due_at: None,
        position: None,
    }
}
