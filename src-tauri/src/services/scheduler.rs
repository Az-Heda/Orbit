use std::sync::Arc;
use std::time::Duration as StdDuration;

use chrono::{DateTime, Datelike, Duration, NaiveTime, TimeZone, Utc};
use chrono_tz::Tz;

use super::notifier::Notifier;
use super::timer_service::Clock;
use super::{AutomationEngine, AutomationService, CalendarService, SettingsService, TimerService};
use crate::domain::automation::Trigger;
use crate::domain::validation::{fmt_utc, parse_utc};
use crate::error::DomainResult;
use crate::infrastructure::database::FiredRepository;

const LAST_TICK: &str = "scheduler.last_tick";
/// Default reminder lead time; override with the `reminders.lead_minutes` setting.
pub const DEFAULT_LEAD_MINUTES: i64 = 10;
/// Reminders for things that started up to this long ago (e.g. while the PC slept) still show,
/// once, marked as missed. Older ones are dropped.
pub const MISSED_WINDOW_HOURS: i64 = 6;
/// A scheduled rule that was due while asleep still runs once if it was due within this window.
pub const CATCH_UP_HOURS: i64 = 24;
/// Items that started less than this long ago are not considered "missed".
const ON_TIME_SECS: i64 = 60;

#[derive(Debug, Default, PartialEq, Eq)]
pub struct TickReport {
    pub reminders: usize,
    pub missed: usize,
    pub rules: usize,
}

/// Periodic work that must keep running in the background: settling timers, firing reminders
/// and time-based rules. All state it needs lives in the database (`fired_keys`), so a restart
/// or a long sleep just means the next tick catches up exactly once - nothing double-fires.
pub struct Scheduler {
    calendar: Arc<CalendarService>,
    timers: Arc<TimerService>,
    automation: Arc<AutomationService>,
    engine: Arc<AutomationEngine>,
    settings: Arc<SettingsService>,
    notifier: Arc<dyn Notifier>,
    fired: Arc<dyn FiredRepository>,
    clock: Clock,
}

impl Scheduler {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        calendar: Arc<CalendarService>,
        timers: Arc<TimerService>,
        automation: Arc<AutomationService>,
        engine: Arc<AutomationEngine>,
        settings: Arc<SettingsService>,
        notifier: Arc<dyn Notifier>,
        fired: Arc<dyn FiredRepository>,
        clock: Clock,
    ) -> Self {
        Self {
            calendar,
            timers,
            automation,
            engine,
            settings,
            notifier,
            fired,
            clock,
        }
    }

    /// Tick forever. The interval only decides how promptly things fire; correctness comes
    /// from comparing against the clock and the persisted keys.
    pub async fn run(self: Arc<Self>, every: StdDuration) {
        let mut interval = tokio::time::interval(every);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            interval.tick().await;
            if let Err(e) = self.tick().await {
                tracing::warn!("scheduler tick failed: {e}");
            }
        }
    }

    async fn setting_i64(&self, key: &str, default: i64) -> i64 {
        match self.settings.get(key).await {
            Ok(Some(s)) => serde_json::from_str::<i64>(&s.value).unwrap_or(default),
            _ => default,
        }
    }

    async fn setting_bool(&self, key: &str, default: bool) -> bool {
        match self.settings.get(key).await {
            Ok(Some(s)) => serde_json::from_str::<bool>(&s.value).unwrap_or(default),
            _ => default,
        }
    }

    pub async fn tick(&self) -> DomainResult<TickReport> {
        let now = (self.clock)();
        let last = match self.fired.marker(LAST_TICK).await? {
            Some(s) => parse_utc(&s).unwrap_or(now),
            None => now,
        };
        let mut report = TickReport::default();

        // 1. Settle timers that ran out (emits TimerEnded -> built-in notification).
        self.timers.active().await?;

        // 2. Reminders.
        if self.setting_bool("reminders.enabled", true).await {
            let lead = self
                .setting_i64("reminders.lead_minutes", DEFAULT_LEAD_MINUTES)
                .await
                .clamp(0, 10_080);
            self.reminders(last, now, lead, &mut report).await?;
        }

        // 3. Time-based rules.
        self.scheduled_rules(last, now, &mut report).await?;

        self.fired.set_marker(LAST_TICK, &fmt_utc(now)).await?;
        self.fired.prune(&fmt_utc(now - Duration::days(30))).await?;
        Ok(report)
    }

    async fn reminders(
        &self,
        last: DateTime<Utc>,
        now: DateTime<Utc>,
        lead: i64,
        report: &mut TickReport,
    ) -> DomainResult<()> {
        let from = now - Duration::hours(MISSED_WINDOW_HOURS);
        let to = now + Duration::minutes(lead) + Duration::seconds(1);
        for item in self
            .calendar
            .list_range(&fmt_utc(from), &fmt_utc(to))
            .await?
        {
            if item.all_day || item.completed {
                continue;
            }
            let Ok(start) = parse_utc(&item.start_at) else {
                continue;
            };
            if start < from || start - Duration::minutes(lead) > now {
                continue;
            }
            let age = (now - start).num_seconds();
            let missed = age > ON_TIME_SECS;
            // "Missed" only means the reminder came due while nothing was ticking (sleep, app
            // closed). A past item that merely appeared now (created late, imported) is not news.
            if missed && start - Duration::minutes(lead) <= last {
                continue;
            }
            let key = format!("reminder:{}:{lead}", item.key);
            if !self.fired.try_fire(&key, &fmt_utc(now), missed).await? {
                continue;
            }
            let (title, body) = if missed {
                report.missed += 1;
                (
                    format!("Missed: {}", item.title),
                    format!("Started {} min ago", age / 60),
                )
            } else {
                let mins = (-age + 59).max(0) / 60;
                let body = if mins == 0 {
                    "Starting now".to_string()
                } else {
                    format!("Starts in {mins} min")
                };
                (item.title.clone(), body)
            };
            report.reminders += 1;
            self.notifier.notify(&title, &body);
        }
        Ok(())
    }

    async fn scheduled_rules(
        &self,
        last: DateTime<Utc>,
        now: DateTime<Utc>,
        report: &mut TickReport,
    ) -> DomainResult<()> {
        for rule in self.automation.list_enabled().await? {
            let Trigger::Schedule {
                time,
                days,
                timezone,
            } = &rule.trigger
            else {
                continue;
            };
            let (Ok(tz), Ok(at)) = (
                timezone.parse::<Tz>(),
                NaiveTime::parse_from_str(time, "%H:%M"),
            ) else {
                continue;
            };
            let (first, lastd) = (
                last.with_timezone(&tz).date_naive(),
                now.with_timezone(&tz).date_naive(),
            );
            // Most recent due instant in (last, now]; one run even after a multi-day sleep.
            let mut due: Option<(DateTime<Utc>, String)> = None;
            let mut d = first;
            while d <= lastd {
                let weekday = d.weekday().number_from_monday() as u8;
                let local = (days.is_empty() || days.contains(&weekday))
                    .then(|| tz.from_local_datetime(&d.and_time(at)).earliest())
                    .flatten();
                if let Some(local) = local {
                    let inst = local.with_timezone(&Utc);
                    if inst > last && inst <= now && due.as_ref().is_none_or(|(i, _)| inst > *i) {
                        due = Some((inst, d.to_string()));
                    }
                }
                d += Duration::days(1);
            }
            let Some((inst, date)) = due else { continue };
            if now - inst > Duration::hours(CATCH_UP_HOURS) {
                continue;
            }
            if self
                .fired
                .try_fire(&format!("rule:{}:{date}", rule.id), &fmt_utc(now), false)
                .await?
                && self.engine.fire_scheduled(&rule).await
            {
                report.rules += 1;
            }
        }
        Ok(())
    }
}
