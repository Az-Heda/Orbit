use std::sync::Arc;

use sqlx::SqlitePool;

use crate::infrastructure::database::*;
use crate::services::*;

/// Application state managed by Tauri. Wires repositories into services.
pub struct AppState {
    pub settings: SettingsService,
    pub activity: ActivityService,
    pub projects: ProjectService,
    pub tasks: Arc<TaskService>,
    pub templates: TemplateService,
    pub calendar: CalendarService,
    pub notes: NoteService,
    pub search: SearchService,
    pub timers: TimerService,
    pub stats: StatsService,
}

impl AppState {
    pub fn new(pool: SqlitePool) -> Self {
        let project_repo: Arc<dyn ProjectRepository> =
            Arc::new(SqliteProjectRepository::new(pool.clone()));
        let task_repo: Arc<dyn TaskRepository> = Arc::new(SqliteTaskRepository::new(pool.clone()));
        let tasks = Arc::new(TaskService::new(task_repo.clone(), project_repo.clone()));
        Self {
            notes: NoteService::new(Arc::new(SqliteNoteRepository::new(pool.clone()))),
            search: SearchService::new(Arc::new(SqliteSearchRepository::new(pool.clone()))),
            timers: TimerService::new(Arc::new(SqliteTimerRepository::new(pool.clone()))),
            stats: StatsService::new(Arc::new(SqliteStatsRepository::new(pool.clone()))),
            calendar: CalendarService::new(Arc::new(SqliteCalendarRepository::new(pool.clone()))),
            settings: SettingsService::new(Arc::new(SqliteSettingsRepository::new(pool.clone()))),
            activity: ActivityService::new(Arc::new(SqliteActivityRepository::new(pool.clone()))),
            projects: ProjectService::new(project_repo.clone()),
            templates: TemplateService::new(
                Arc::new(SqliteTemplateRepository::new(pool)),
                task_repo,
                project_repo,
                tasks.clone(),
            ),
            tasks,
        }
    }
}
