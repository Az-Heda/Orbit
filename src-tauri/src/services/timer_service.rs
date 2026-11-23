use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::Deserialize;

use crate::domain::activity::Actor;
use crate::domain::timers::{FocusSession, NewSession, NewTimer, Timer, TimerChange, TimerView};
use crate::domain::validation::{
    clean_optional, fmt_utc, invalid, normalize_utc, parse_utc, parse_uuid,
};
use crate::error::{DomainError, DomainResult};
use crate::infrastructure::database::TimerRepository;

pub type Clock = Arc<dyn Fn() -> DateTime<Utc> + Send + Sync>;

const MAX_DURATION_MS: i64 = 24 * 3_600_000;
/// Focus time shorter than this is not worth a history row.
const MIN_SESSION_MS: i64 = 1_000;

#[derive(Debug, Deserialize)]
pub struct StartTimerInput {
    /// "pomodoro" | "countdown" | "stopwatch"
    pub kind: String,
    pub label: Option<String>,
    pub task_id: Option<String>,
    /// Required for pomodoro/countdown; must be absent for stopwatch.
    pub duration_ms: Option<i64>,
    /// "focus" (default) | "break" - breaks are not recorded as focus sessions.
    pub phase: Option<String>,
}

pub struct TimerService {
    repo: Arc<dyn TimerRepository>,
    clock: Clock,
}

impl TimerService {
    pub fn new(repo: Arc<dyn TimerRepository>) -> Self {
        Self::with_clock(repo, Arc::new(Utc::now))
    }

    pub fn with_clock(repo: Arc<dyn TimerRepository>, clock: Clock) -> Self {
        Self { repo, clock }
    }

    fn now(&self) -> DateTime<Utc> {
        (self.clock)()
    }

    async fn load(&self, id: &str) -> DomainResult<Timer> {
        let id = parse_uuid(id, "timer id")?;
        self.repo.get(&id).await?.ok_or(DomainError::NotFound {
            entity: "timer",
            id,
        })
    }

    /// Close a bounded timer whose time ran out (e.g. while the app was closed or the PC asleep).
    /// The end time is the moment it was *due*, so history stays accurate after a long sleep.
    async fn finalize_if_due(&self, t: Timer, now: DateTime<Utc>) -> DomainResult<Timer> {
        match t.due_at() {
            Some(due) if due <= now => self.close(t, "finished", due).await,
            _ => Ok(t),
        }
    }

    async fn close(&self, t: Timer, state: &'static str, at: DateTime<Utc>) -> DomainResult<Timer> {
        let elapsed = t.elapsed_ms(at);
        let duration = t.duration_ms.map_or(elapsed, |d| elapsed.min(d));
        let session = (t.phase == "focus" && duration >= MIN_SESSION_MS).then(|| NewSession {
            task_id: t.task_id.clone(),
            started_at: t.started_at.clone(),
            ended_at: fmt_utc(at),
            duration_ms: duration,
        });
        let change = TimerChange {
            expected_state: if t.state == "running" {
                "running"
            } else {
                "paused"
            },
            state,
            segment_started_at: None,
            accumulated_ms: duration,
            ended_at: Some(fmt_utc(at)),
        };
        self.repo
            .transition(&t.id, &change, session.as_ref(), Actor::User)
            .await
    }

    pub async fn start(&self, input: StartTimerInput, actor: Actor) -> DomainResult<TimerView> {
        let now = self.now();
        self.active_at(now).await?; // finalise anything overdue first
        if !self
            .repo
            .list_by_states(&["running", "paused"])
            .await?
            .is_empty()
        {
            return Err(invalid("another timer is already active; stop it first"));
        }
        let kind = match input.kind.as_str() {
            "pomodoro" | "countdown" | "stopwatch" => input.kind.clone(),
            _ => return Err(invalid("kind must be pomodoro, countdown or stopwatch")),
        };
        let phase = match input.phase.as_deref().unwrap_or("focus") {
            p @ ("focus" | "break") => p.to_string(),
            _ => return Err(invalid("phase must be focus or break")),
        };
        if kind != "pomodoro" && phase == "break" {
            return Err(invalid("only pomodoro timers have a break phase"));
        }
        let duration_ms = match (kind.as_str(), input.duration_ms) {
            ("stopwatch", None) => None,
            ("stopwatch", Some(_)) => return Err(invalid("a stopwatch has no duration")),
            (_, Some(d)) if (1_000..=MAX_DURATION_MS).contains(&d) => Some(d),
            (_, Some(_)) => return Err(invalid("duration must be between 1 second and 24 hours")),
            (_, None) => return Err(invalid("duration is required")),
        };
        let task_id = match input.task_id.as_deref() {
            Some(t) => {
                let t = parse_uuid(t, "task id")?;
                if !self.repo.task_exists(&t).await? {
                    return Err(DomainError::NotFound {
                        entity: "task",
                        id: t,
                    });
                }
                Some(t)
            }
            None => None,
        };
        let label = input
            .label
            .as_deref()
            .map(|l| clean_optional(l, "label", 120))
            .transpose()?
            .flatten();
        let t = self
            .repo
            .create(
                &NewTimer {
                    kind,
                    label,
                    task_id,
                    duration_ms,
                    phase,
                    started_at: fmt_utc(now),
                },
                actor,
            )
            .await?;
        Ok(t.view(now))
    }

    pub async fn pause(&self, id: &str) -> DomainResult<TimerView> {
        let now = self.now();
        let t = self.finalize_if_due(self.load(id).await?, now).await?;
        if t.state != "running" {
            return Err(invalid("timer is not running"));
        }
        let change = TimerChange {
            expected_state: "running",
            state: "paused",
            segment_started_at: None,
            accumulated_ms: t.elapsed_ms(now),
            ended_at: None,
        };
        Ok(self
            .repo
            .transition(&t.id, &change, None, Actor::User)
            .await?
            .view(now))
    }

    pub async fn resume(&self, id: &str) -> DomainResult<TimerView> {
        let now = self.now();
        let t = self.load(id).await?;
        if t.state != "paused" {
            return Err(invalid("timer is not paused"));
        }
        let change = TimerChange {
            expected_state: "paused",
            state: "running",
            segment_started_at: Some(fmt_utc(now)),
            accumulated_ms: t.accumulated_ms,
            ended_at: None,
        };
        Ok(self
            .repo
            .transition(&t.id, &change, None, Actor::User)
            .await?
            .view(now))
    }

    /// Stop an active timer. Focus time already spent is kept as a session.
    /// A stopwatch (or a timer that already ran out) ends as "finished"; stopping early is "cancelled".
    pub async fn stop(&self, id: &str) -> DomainResult<TimerView> {
        let now = self.now();
        let t = self.finalize_if_due(self.load(id).await?, now).await?;
        if !t.is_active() {
            return Ok(t.view(now)); // already ended (e.g. ran out): idempotent
        }
        let state = if t.kind == "stopwatch" {
            "finished"
        } else {
            "cancelled"
        };
        Ok(self.close(t, state, now).await?.view(now))
    }

    async fn active_at(&self, now: DateTime<Utc>) -> DomainResult<Vec<Timer>> {
        let mut out = Vec::new();
        for t in self.repo.list_by_states(&["running", "paused"]).await? {
            let t = self.finalize_if_due(t, now).await?;
            if t.is_active() {
                out.push(t);
            }
        }
        Ok(out)
    }

    /// Active timers with derived elapsed/remaining. Also settles timers that ran out meanwhile.
    pub async fn active(&self) -> DomainResult<Vec<TimerView>> {
        let now = self.now();
        Ok(self
            .active_at(now)
            .await?
            .into_iter()
            .map(|t| t.view(now))
            .collect())
    }

    pub async fn history(&self, limit: i64) -> DomainResult<Vec<TimerView>> {
        if !(1..=200).contains(&limit) {
            return Err(invalid("limit must be between 1 and 200"));
        }
        let now = self.now();
        self.active_at(now).await?;
        Ok(self
            .repo
            .recent_finished(limit)
            .await?
            .into_iter()
            .map(|t| t.view(now))
            .collect())
    }

    pub async fn sessions(
        &self,
        from: &str,
        to: &str,
        task_id: Option<&str>,
    ) -> DomainResult<Vec<FocusSession>> {
        let (from, to) = (normalize_utc(from, "from")?, normalize_utc(to, "to")?);
        if parse_utc(&from)? >= parse_utc(&to)? {
            return Err(invalid("from must be before to"));
        }
        let task_id = task_id.map(|t| parse_uuid(t, "task id")).transpose()?;
        self.repo.sessions(&from, &to, task_id.as_deref()).await
    }

    pub async fn set_session_task(
        &self,
        session_id: &str,
        task_id: Option<&str>,
        actor: Actor,
    ) -> DomainResult<FocusSession> {
        let sid = parse_uuid(session_id, "session id")?;
        let tid = match task_id {
            Some(t) => {
                let t = parse_uuid(t, "task id")?;
                if !self.repo.task_exists(&t).await? {
                    return Err(DomainError::NotFound {
                        entity: "task",
                        id: t,
                    });
                }
                Some(t)
            }
            None => None,
        };
        self.repo
            .set_session_task(&sid, tid.as_deref(), actor)
            .await
    }
}
