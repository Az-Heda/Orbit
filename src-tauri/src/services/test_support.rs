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

// ---------- Phase 2 fixture: bus, mock clock, recording notifier, full service graph ----------
use std::sync::Mutex;

use chrono::{DateTime, Duration, TimeZone, Utc};
use tokio::sync::broadcast;

use crate::domain::activity::Actor;
use crate::domain::events::{Envelope, EventBus};

#[derive(Default)]
pub struct RecordingNotifier(pub Mutex<Vec<(String, String)>>);
impl Notifier for RecordingNotifier {
    fn notify(&self, title: &str, body: &str) {
        self.0
            .lock()
            .unwrap()
            .push((title.to_string(), body.to_string()));
    }
}
impl RecordingNotifier {
    pub fn titles(&self) -> Vec<String> {
        self.0.lock().unwrap().iter().map(|n| n.0.clone()).collect()
    }
}

/// Records remote messages instead of sending them; can be told to fail.
#[derive(Default)]
pub struct FakeSender {
    pub sent: Mutex<Vec<(String, String)>>,
    pub fail_with: Mutex<Option<String>>,
}
#[async_trait::async_trait]
impl crate::infrastructure::channels::ChannelSender for FakeSender {
    async fn send(
        &self,
        target: &crate::infrastructure::channels::Target,
        text: &str,
    ) -> crate::error::DomainResult<()> {
        use crate::infrastructure::channels::Target;
        if let Some(e) = self.fail_with.lock().unwrap().clone() {
            return Err(crate::error::DomainError::External(e));
        }
        let to = match target {
            Target::Telegram { token, chat_id } => format!("telegram:{chat_id}:{token}"),
            Target::Discord { webhook_url } => format!("discord:{webhook_url}"),
        };
        self.sent.lock().unwrap().push((to, text.to_string()));
        Ok(())
    }
}
impl FakeSender {
    pub fn texts(&self) -> Vec<String> {
        self.sent
            .lock()
            .unwrap()
            .iter()
            .map(|m| m.1.clone())
            .collect()
    }
}

pub struct P2 {
    pub pool: SqlitePool,
    pub now: Arc<Mutex<DateTime<Utc>>>,
    pub rx: Mutex<broadcast::Receiver<Envelope>>,
    pub projects: ProjectService,
    pub tasks: Arc<TaskService>,
    pub notes: Arc<NoteService>,
    pub calendar: Arc<CalendarService>,
    pub timers: Arc<TimerService>,
    pub inbox: Arc<InboxService>,
    pub rules: Arc<AutomationService>,
    pub channels: Arc<ChannelService>,
    pub secrets: Arc<crate::infrastructure::channels::MemoryChannelSecrets>,
    pub sender: Arc<FakeSender>,
    pub engine: Arc<AutomationEngine>,
    pub scheduler: Scheduler,
    pub notifier: Arc<RecordingNotifier>,
    pub search: SearchService,
}

impl P2 {
    pub fn advance(&self, d: Duration) {
        *self.now.lock().unwrap() += d;
    }

    /// Deliver every pending (and resulting) event to the automation engine. Returns how many
    /// events were handled.
    pub async fn drain(&self) -> usize {
        let mut n = 0;
        loop {
            let env = self.rx.lock().unwrap().try_recv();
            match env {
                Ok(env) => {
                    self.engine.handle(env).await;
                    n += 1;
                }
                Err(_) => return n,
            }
        }
    }

    /// A project with its default columns: (backlog, active, done) ids.
    pub async fn board(&self, name: &str) -> (String, String, String) {
        let p = self
            .projects
            .create_project(name, None, None, Actor::User)
            .await
            .unwrap();
        let b = self.projects.list_boards(&p.id).await.unwrap().remove(0);
        let cols = self.tasks.board_view(&b.id).await.unwrap().columns;
        let of = |k: &str| cols.iter().find(|c| c.kind == k).unwrap().id.clone();
        (of("backlog"), of("active"), of("done"))
    }
}

pub async fn p2() -> P2 {
    let pool = open_in_memory().await.unwrap();
    let now = Arc::new(Mutex::new(
        Utc.with_ymd_and_hms(2026, 3, 10, 9, 0, 0).unwrap(),
    ));
    let clock: super::timer_service::Clock = {
        let n = now.clone();
        Arc::new(move || *n.lock().unwrap())
    };
    let bus = Arc::new(EventBus::new());
    let rx = Mutex::new(bus.subscribe());
    let notifier = Arc::new(RecordingNotifier::default());

    let project_repo: Arc<dyn ProjectRepository> =
        Arc::new(SqliteProjectRepository::new(pool.clone()));
    let task_repo: Arc<dyn TaskRepository> = Arc::new(SqliteTaskRepository::new(pool.clone()));
    let tasks = Arc::new(TaskService::new(task_repo, project_repo.clone()).with_bus(bus.clone()));
    let notes = Arc::new(NoteService::new(Arc::new(SqliteNoteRepository::new(
        pool.clone(),
    ))));
    let calendar = Arc::new(CalendarService::new(Arc::new(
        SqliteCalendarRepository::new(pool.clone()),
    )));
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
    let secrets = Arc::new(crate::infrastructure::channels::MemoryChannelSecrets::default());
    let sender = Arc::new(FakeSender::default());
    let channels = Arc::new(ChannelService::new(
        Arc::new(SqliteChannelRepository::new(pool.clone())),
        secrets.clone(),
        sender.clone(),
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
    let settings = Arc::new(SettingsService::new(Arc::new(
        SqliteSettingsRepository::new(pool.clone()),
    )));
    let scheduler = Scheduler::new(
        calendar.clone(),
        timers.clone(),
        rules.clone(),
        engine.clone(),
        settings,
        notifier.clone(),
        Arc::new(SqliteFiredRepository::new(pool.clone())),
        clock,
    );
    P2 {
        search: SearchService::new(Arc::new(SqliteSearchRepository::new(pool.clone()))),
        projects: ProjectService::new(project_repo),
        pool,
        now,
        rx,
        tasks,
        notes,
        calendar,
        timers,
        inbox,
        rules,
        channels,
        secrets,
        sender,
        engine,
        scheduler,
        notifier,
    }
}
