use std::sync::Arc;

use sqlx::SqlitePool;

use crate::domain::events::EventBus;
use crate::infrastructure::database::*;
use crate::services::*;

/// Application state managed by Tauri. Wires repositories into services.
pub struct AppState {
    pub settings: Arc<SettingsService>,
    pub activity: ActivityService,
    pub projects: ProjectService,
    pub tasks: Arc<TaskService>,
    pub templates: TemplateService,
    pub calendar: Arc<CalendarService>,
    pub notes: Arc<NoteService>,
    pub search: SearchService,
    pub timers: Arc<TimerService>,
    pub stats: StatsService,
    pub inbox: Arc<InboxService>,
    pub rules: Arc<AutomationService>,
    pub engine: Arc<AutomationEngine>,
    pub scheduler: Arc<Scheduler>,
    pub bus: Arc<EventBus>,
}

impl AppState {
    pub fn new(pool: SqlitePool, notifier: Arc<dyn Notifier>) -> Self {
        let bus = Arc::new(EventBus::new());
        let clock: crate::services::Clock = Arc::new(chrono::Utc::now);

        let project_repo: Arc<dyn ProjectRepository> =
            Arc::new(SqliteProjectRepository::new(pool.clone()));
        let task_repo: Arc<dyn TaskRepository> = Arc::new(SqliteTaskRepository::new(pool.clone()));
        let tasks = Arc::new(
            TaskService::new(task_repo.clone(), project_repo.clone()).with_bus(bus.clone()),
        );
        let settings = Arc::new(SettingsService::new(Arc::new(
            SqliteSettingsRepository::new(pool.clone()),
        )));
        let calendar = Arc::new(CalendarService::new(Arc::new(
            SqliteCalendarRepository::new(pool.clone()),
        )));
        let notes = Arc::new(NoteService::new(Arc::new(SqliteNoteRepository::new(
            pool.clone(),
        ))));
        let timers = Arc::new(
            TimerService::with_clock(
                Arc::new(SqliteTimerRepository::new(pool.clone())),
                clock.clone(),
            )
            .with_bus(bus.clone()),
        );
        let inbox = Arc::new(
            InboxService::new(
                Arc::new(SqliteInboxRepository::new(pool.clone())),
                tasks.clone(),
                calendar.clone(),
            )
            .with_bus(bus.clone()),
        );
        let rules = Arc::new(AutomationService::new(Arc::new(
            SqliteAutomationRepository::new(pool.clone()),
        )));
        let engine = Arc::new(AutomationEngine::new(
            rules.clone(),
            tasks.clone(),
            notes.clone(),
            timers.clone(),
            notifier.clone(),
            clock.clone(),
        ));
        let scheduler = Arc::new(Scheduler::new(
            calendar.clone(),
            timers.clone(),
            rules.clone(),
            engine.clone(),
            settings.clone(),
            notifier,
            Arc::new(SqliteFiredRepository::new(pool.clone())),
            clock,
        ));
        Self {
            search: SearchService::new(Arc::new(SqliteSearchRepository::new(pool.clone()))),
            stats: StatsService::new(Arc::new(SqliteStatsRepository::new(pool.clone()))),
            activity: ActivityService::new(Arc::new(SqliteActivityRepository::new(pool.clone()))),
            projects: ProjectService::new(project_repo.clone()),
            templates: TemplateService::new(
                Arc::new(SqliteTemplateRepository::new(pool)),
                task_repo,
                project_repo,
                tasks.clone(),
            ),
            settings,
            tasks,
            calendar,
            notes,
            timers,
            inbox,
            rules,
            engine,
            scheduler,
            bus,
        }
    }
}
