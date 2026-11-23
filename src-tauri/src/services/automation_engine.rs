use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

use chrono::Duration;
use tokio::sync::broadcast;

use super::notifier::Notifier;
use super::timer_service::Clock;
use super::{
    AutomationService, CreateTaskInput, NoteService, StartTimerInput, TaskService, TimerService,
};
use crate::domain::activity::Actor;
use crate::domain::automation::{Action, Rule, render};
use crate::domain::events::{CAUSE, DomainEvent, Envelope};
use crate::error::DomainResult;

/// Rule-triggered changes more than this many hops from a user action are dropped.
pub const MAX_DEPTH: u8 = 3;
/// At most this many runs per rule per minute.
pub const RATE_LIMIT: usize = 10;

/// Runs enabled rules against domain events. Actions go through the same services as the UI,
/// with `Actor::Automation`, so validation and `activity_log` apply.
///
/// Loop protection: (1) a rule ignores events its own action caused, (2) events carry a hop
/// depth and are dropped at `MAX_DEPTH`, (3) a per-rule rate limit.
pub struct AutomationEngine {
    rules: Arc<AutomationService>,
    tasks: Arc<TaskService>,
    notes: Arc<NoteService>,
    timers: Arc<TimerService>,
    notifier: Arc<dyn Notifier>,
    clock: Clock,
    recent: Mutex<HashMap<String, VecDeque<chrono::DateTime<chrono::Utc>>>>,
}

fn event_title(ev: &DomainEvent) -> String {
    match ev {
        DomainEvent::TaskCreated { title, .. }
        | DomainEvent::TaskMoved { title, .. }
        | DomainEvent::TaskCompleted { title, .. } => title.clone(),
        DomainEvent::TimerEnded { label, .. } => label.clone().unwrap_or_else(|| "Timer".into()),
        DomainEvent::InboxAdded { text, .. } => {
            text.lines().next().unwrap_or("").trim().to_string()
        }
    }
}

fn event_task_id(ev: &DomainEvent) -> Option<&str> {
    match ev {
        DomainEvent::TaskCreated { task_id, .. }
        | DomainEvent::TaskMoved { task_id, .. }
        | DomainEvent::TaskCompleted { task_id, .. } => Some(task_id),
        _ => None,
    }
}

impl AutomationEngine {
    pub fn new(
        rules: Arc<AutomationService>,
        tasks: Arc<TaskService>,
        notes: Arc<NoteService>,
        timers: Arc<TimerService>,
        notifier: Arc<dyn Notifier>,
        clock: Clock,
    ) -> Self {
        Self {
            rules,
            tasks,
            notes,
            timers,
            notifier,
            clock,
            recent: Mutex::new(HashMap::new()),
        }
    }

    /// Consume events until the bus closes.
    pub async fn run(self: Arc<Self>, mut rx: broadcast::Receiver<Envelope>) {
        loop {
            match rx.recv().await {
                Ok(env) => {
                    self.handle(env).await;
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!("automation lagged by {n} events")
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    }

    /// Handle one event; returns the ids of the rules that ran.
    pub async fn handle(&self, env: Envelope) -> Vec<String> {
        // Built-in: tell the user when a timer ends (not a rule, so it cannot be disabled by one).
        if let DomainEvent::TimerEnded { label, .. } = &env.event {
            self.notifier.notify(
                "Timer finished",
                label.as_deref().unwrap_or("Your timer is done."),
            );
        }
        if env.depth >= MAX_DEPTH {
            tracing::warn!("automation depth limit reached, dropping {:?}", env.event);
            return vec![];
        }
        let rules = match self.rules.list_enabled().await {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!("cannot load rules: {e}");
                return vec![];
            }
        };
        let mut ran = vec![];
        for rule in rules {
            if env.origin_rule.as_deref() == Some(rule.id.as_str())
                || !rule.trigger.matches(&env.event)
            {
                continue;
            }
            if self.fire(&rule, env.depth, &env.event).await {
                ran.push(rule.id);
            }
        }
        ran
    }

    fn allow(&self, rule_id: &str) -> bool {
        let now = (self.clock)();
        let mut map = self.recent.lock().unwrap();
        let q = map.entry(rule_id.to_string()).or_default();
        while q.front().is_some_and(|t| now - *t > Duration::minutes(1)) {
            q.pop_front();
        }
        if q.len() >= RATE_LIMIT {
            return false;
        }
        q.push_back(now);
        true
    }

    /// Run a rule's action. `event` is the triggering event (None for scheduled rules).
    pub async fn fire(&self, rule: &Rule, depth: u8, event: &DomainEvent) -> bool {
        if !self.allow(&rule.id) {
            tracing::warn!("rule '{}' rate limited", rule.name);
            return false;
        }
        let title = event_title(event);
        let date = (self.clock)().date_naive().to_string();
        let task_id = event_task_id(event).map(str::to_string);
        // Events published by the services this action calls inherit this cause.
        let result = CAUSE
            .scope(
                (depth, rule.id.clone()),
                self.execute(&rule.action, &title, &date, task_id.as_deref()),
            )
            .await;
        match result {
            Ok(()) => true,
            Err(e) => {
                tracing::warn!("rule '{}' failed: {e}", rule.name);
                false
            }
        }
    }

    /// Fire a scheduled (time-based) rule.
    pub async fn fire_scheduled(&self, rule: &Rule) -> bool {
        self.fire(
            rule,
            0,
            &DomainEvent::InboxAdded {
                item_id: String::new(),
                text: String::new(),
            },
        )
        .await
    }

    async fn execute(
        &self,
        action: &Action,
        title: &str,
        date: &str,
        task_id: Option<&str>,
    ) -> DomainResult<()> {
        const A: Actor = Actor::Automation;
        match action {
            Action::CreateTask {
                column_id,
                title: t,
                description,
            } => {
                self.tasks
                    .create_task(
                        CreateTaskInput {
                            column_id: Some(column_id.clone()),
                            parent_id: None,
                            title: render(t, title, date),
                            description: description.as_deref().map(|d| render(d, title, date)),
                            priority: None,
                            due_at: None,
                            position: None,
                        },
                        A,
                    )
                    .await?;
            }
            Action::CreateNote { title: t, body } => {
                self.notes
                    .create(&render(t, title, date), &render(body, title, date), A)
                    .await?;
            }
            Action::MoveTask { column_id } => {
                if let Some(id) = task_id {
                    // No force: a warning (open subtasks, blockers) stops the automation.
                    self.tasks.move_task(id, column_id, 0, false, A).await?;
                }
            }
            Action::StartTimer {
                timer_kind,
                minutes,
                label,
            } => {
                self.timers
                    .start(
                        StartTimerInput {
                            kind: timer_kind.clone(),
                            label: label.as_deref().map(|l| render(l, title, date)),
                            task_id: None,
                            duration_ms: Some(minutes * 60_000),
                            phase: None,
                        },
                        A,
                    )
                    .await?;
            }
            Action::Notify { title: t, body } => {
                self.notifier.notify(
                    &render(t, title, date),
                    &body
                        .as_deref()
                        .map(|b| render(b, title, date))
                        .unwrap_or_default(),
                );
            }
        }
        Ok(())
    }
}
