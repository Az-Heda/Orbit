use std::sync::Arc;

use sqlx::SqlitePool;

use crate::domain::events::EventBus;
use crate::infrastructure::ai::client::{ClaudeClient, ModelClient};
use crate::infrastructure::ai::secrets::{SecretStore, default_store};
use crate::infrastructure::ai::tools::{ToolDeps, Tools};
use crate::infrastructure::database::*;
use crate::services::*;

/// Application state managed by Tauri. Wires repositories into services.
pub struct AppState {
    pub settings: Arc<SettingsService>,
    pub activity: ActivityService,
    pub projects: Arc<ProjectService>,
    pub tasks: Arc<TaskService>,
    pub templates: Arc<TemplateService>,
    pub calendar: Arc<CalendarService>,
    pub external: Arc<ExternalCalendarService>,
    pub notes: Arc<NoteService>,
    pub search: Arc<SearchService>,
    pub timers: Arc<TimerService>,
    pub stats: StatsService,
    pub inbox: Arc<InboxService>,
    pub rules: Arc<AutomationService>,
    pub channels: Arc<ChannelService>,
    pub engine: Arc<AutomationEngine>,
    pub scheduler: Arc<Scheduler>,
    pub bus: Arc<EventBus>,
    pub ai: Arc<AiService>,
}

impl AppState {
    pub fn new(pool: SqlitePool, notifier: Arc<dyn Notifier>) -> Self {
        let secrets = default_store();
        let client: Arc<dyn ModelClient> = Arc::new(ClaudeClient::new(secrets.clone()));
        Self::with_ai(pool, notifier, secrets, client)
    }

    /// Like `new`, with the secret store and model client supplied (tests use fakes).
    pub fn with_ai(
        pool: SqlitePool,
        notifier: Arc<dyn Notifier>,
        secrets: Arc<dyn SecretStore>,
        client: Arc<dyn ModelClient>,
    ) -> Self {
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
            .with_bus(bus.clone())
            .with_settings(settings.clone()),
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
        let channels = Arc::new(ChannelService::new(
            Arc::new(SqliteChannelRepository::new(pool.clone())),
            crate::infrastructure::channels::default_channel_secrets(),
            Arc::new(crate::infrastructure::channels::HttpChannelSender::new()),
            notifier.clone(),
            clock.clone(),
        ));
        let engine = Arc::new(AutomationEngine::new(
            rules.clone(),
            tasks.clone(),
            notes.clone(),
            timers.clone(),
            channels.clone(),
            notifier.clone(),
            clock.clone(),
        ));
        let clock_for_ai = clock.clone();
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
        let search = Arc::new(SearchService::new(Arc::new(SqliteSearchRepository::new(
            pool.clone(),
        ))));
        let projects = Arc::new(ProjectService::new(project_repo.clone()));
        let templates = Arc::new(TemplateService::new(
            Arc::new(SqliteTemplateRepository::new(pool.clone())),
            task_repo,
            project_repo,
            tasks.clone(),
        ));
        let external = Arc::new(ExternalCalendarService::new(
            Arc::new(SqliteExternalCalendarRepository::new(pool.clone())),
            crate::infrastructure::feeds::default_feed_secrets(),
            Arc::new(crate::infrastructure::feeds::HttpFeedFetcher::new()),
        ));
        let ai_repo: Arc<dyn AiRepository> = Arc::new(SqliteAiRepository::new(pool.clone()));
        let tools = Tools::new(ToolDeps {
            tasks: tasks.clone(),
            projects: projects.clone(),
            templates: templates.clone(),
            calendar: calendar.clone(),
            notes: notes.clone(),
            search: search.clone(),
            timers: timers.clone(),
            inbox: inbox.clone(),
            rules: rules.clone(),
            channels: channels.clone(),
            ai: ai_repo.clone(),
        });
        let ai = Arc::new(AiService::new(
            ai_repo,
            tools,
            settings.clone(),
            client,
            secrets,
            clock_for_ai,
        ));
        Self {
            search,
            stats: StatsService::new(Arc::new(SqliteStatsRepository::new(pool.clone()))),
            activity: ActivityService::new(Arc::new(SqliteActivityRepository::new(pool))),
            projects,
            templates,
            ai,
            settings,
            tasks,
            calendar,
            external,
            notes,
            timers,
            inbox,
            rules,
            channels,
            engine,
            scheduler,
            bus,
        }
    }
}
