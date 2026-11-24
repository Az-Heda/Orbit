use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

use chrono::Duration;
use tokio::sync::broadcast;

use super::notifier::Notifier;
use super::timer_service::Clock;
use super::{
    AutomationService, ChannelService, CreateTaskInput, NoteService, StartTimerInput, TaskService,
    TimerService,
};
use crate::domain::activity::Actor;
use crate::domain::automation::{Action, RenderCtx, Rule};
use crate::domain::events::{CAUSE, DomainEvent, Envelope};
use crate::error::DomainResult;

/// Rule-triggered changes more than this many hops from a user action are dropped.
pub const MAX_DEPTH: u8 = 3;
/// At most this many runs per rule per minute.
pub const RATE_LIMIT: usize = 10;

/// Runs enabled rules against domain events. Actions go through the same services as the UI,
/// with `Actor::Automation`, so validation and `activity_log` apply.
///
/// A rule is a chain: its steps run in order, and a failing step is logged (tracing and
/// `activity_log`) and stops the chain. A "send notification" step only fails for configuration
/// problems (channel deleted, no secret); the delivery itself is detached and logged separately.
///
/// Loop protection: (1) a rule ignores events its own actions caused, (2) events carry a hop
/// depth and are dropped at `MAX_DEPTH`, (3) a per-rule rate limit (a whole chain is one run).
pub struct AutomationEngine {
    rules: Arc<AutomationService>,
    tasks: Arc<TaskService>,
    notes: Arc<NoteService>,
    timers: Arc<TimerService>,
    channels: Arc<ChannelService>,
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

fn trigger_kind(ev: &DomainEvent) -> &'static str {
    match ev {
        DomainEvent::TaskCreated { .. } => "task_created",
        DomainEvent::TaskMoved { .. } => "task_moved",
        DomainEvent::TaskCompleted { .. } => "task_completed",
        DomainEvent::TimerEnded { .. } => "timer_ended",
        DomainEvent::InboxAdded { .. } => "inbox_added",
    }
}

/// Template data for one run. `scheduled` rules have no triggering item.
fn render_ctx(rule: &Rule, ev: &DomainEvent, date: &str, scheduled: bool) -> RenderCtx {
    let title = if scheduled {
        String::new()
    } else {
        event_title(ev)
    };
    let mut ctx = RenderCtx {
        title: title.clone(),
        date: date.to_string(),
        rule_name: rule.name.clone(),
        trigger: if scheduled {
            "schedule".into()
        } else {
            trigger_kind(ev).into()
        },
        ..Default::default()
    };
    if !scheduled {
        match ev {
            DomainEvent::TaskCreated { .. }
            | DomainEvent::TaskMoved { .. }
            | DomainEvent::TaskCompleted { .. } => ctx.task_title = title,
            DomainEvent::TimerEnded { .. } => ctx.timer_label = title,
            DomainEvent::InboxAdded { text, .. } => ctx.inbox_text = text.trim().to_string(),
        }
    }
    ctx
}

impl AutomationEngine {
    pub fn new(
        rules: Arc<AutomationService>,
        tasks: Arc<TaskService>,
        notes: Arc<NoteService>,
        timers: Arc<TimerService>,
        channels: Arc<ChannelService>,
        notifier: Arc<dyn Notifier>,
        clock: Clock,
    ) -> Self {
        Self {
            rules,
            tasks,
            notes,
            timers,
            channels,
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

    /// Run a rule's chain. `event` is the triggering event (`scheduled`: there is none).
    /// Returns true when every step succeeded.
    pub async fn fire(&self, rule: &Rule, depth: u8, event: &DomainEvent) -> bool {
        self.run_chain(rule, depth, event, false).await
    }

    async fn run_chain(
        &self,
        rule: &Rule,
        depth: u8,
        event: &DomainEvent,
        scheduled: bool,
    ) -> bool {
        if !self.allow(&rule.id) {
            tracing::warn!("rule '{}' rate limited", rule.name);
            return false;
        }
        let date = (self.clock)().date_naive().to_string();
        let ctx = render_ctx(rule, event, &date, scheduled);
        let task_id = if scheduled {
            None
        } else {
            event_task_id(event).map(str::to_string)
        };
        // Events published by the services these actions call inherit this cause.
        CAUSE
            .scope((depth, rule.id.clone()), async {
                for (i, action) in rule.actions.iter().enumerate() {
                    if let Err(e) = self.execute(action, &ctx, task_id.as_deref()).await {
                        tracing::warn!(
                            "rule '{}' failed at step {} ({}): {e}",
                            rule.name,
                            i + 1,
                            action.kind()
                        );
                        self.rules
                            .log_step_failure(&rule.id, i, action, &e.to_string())
                            .await;
                        return false;
                    }
                }
                true
            })
            .await
    }

    /// Fire a scheduled (time-based) rule.
    pub async fn fire_scheduled(&self, rule: &Rule) -> bool {
        self.run_chain(
            rule,
            0,
            &DomainEvent::InboxAdded {
                item_id: String::new(),
                text: String::new(),
            },
            true,
        )
        .await
    }

    async fn execute(
        &self,
        action: &Action,
        ctx: &RenderCtx,
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
                            title: ctx.render(t),
                            description: description.as_deref().map(|d| ctx.render(d)),
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
                    .create(&ctx.render(t), &ctx.render(body), A)
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
                            label: label.as_deref().map(|l| ctx.render(l)),
                            task_id: None,
                            duration_ms: Some(minutes * 60_000),
                            phase: None,
                        },
                        A,
                    )
                    .await?;
            }
            Action::SendNotification { channel_id } => {
                self.channels.dispatch(channel_id, ctx.clone()).await?;
            }
            Action::Notify { title: t, body } => {
                self.notifier.notify(
                    &ctx.render(t),
                    &body.as_deref().map(|b| ctx.render(b)).unwrap_or_default(),
                );
            }
        }
        Ok(())
    }
}
