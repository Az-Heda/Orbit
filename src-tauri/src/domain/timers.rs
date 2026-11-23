//! Timers store timestamps; elapsed time is *derived* from the clock, never counted by a ticker.
//! That makes them correct across app restarts and system sleep/resume.

use chrono::{DateTime, Utc};
use serde::Serialize;

use super::validation::parse_utc;

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Timer {
    pub id: String,
    /// "pomodoro" | "countdown" | "stopwatch"
    pub kind: String,
    pub label: Option<String>,
    pub task_id: Option<String>,
    pub duration_ms: Option<i64>,
    /// "focus" | "break" (pomodoro)
    pub phase: String,
    /// "running" | "paused" | "finished" | "cancelled"
    pub state: String,
    pub started_at: String,
    pub segment_started_at: Option<String>,
    pub accumulated_ms: i64,
    pub ended_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

pub const TIMER_COLS: &str = "id, kind, label, task_id, duration_ms, phase, state, started_at, segment_started_at, accumulated_ms, ended_at, created_at, updated_at";

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct FocusSession {
    pub id: String,
    pub timer_id: Option<String>,
    pub task_id: Option<String>,
    pub started_at: String,
    pub ended_at: String,
    pub duration_ms: i64,
    pub created_at: String,
}

/// A timer plus values derived from `now` (the frontend renders these; it never counts time itself).
#[derive(Debug, Clone, Serialize)]
pub struct TimerView {
    #[serde(flatten)]
    pub timer: Timer,
    pub elapsed_ms: i64,
    pub remaining_ms: Option<i64>,
    /// Server clock used for the derivation (UTC), so the UI can correct for skew.
    pub now: String,
}

impl Timer {
    pub fn is_active(&self) -> bool {
        matches!(self.state.as_str(), "running" | "paused")
    }

    /// Total running time as of `now`. Uses the clock, so time spent asleep still counts.
    pub fn elapsed_ms(&self, now: DateTime<Utc>) -> i64 {
        let running = match (&self.state[..], &self.segment_started_at) {
            ("running", Some(s)) => parse_utc(s)
                .map(|s| (now - s).num_milliseconds().max(0))
                .unwrap_or(0),
            _ => 0,
        };
        self.accumulated_ms + running
    }

    pub fn remaining_ms(&self, now: DateTime<Utc>) -> Option<i64> {
        self.duration_ms.map(|d| (d - self.elapsed_ms(now)).max(0))
    }

    /// The instant a running, bounded timer reaches its duration (None if it never will).
    pub fn due_at(&self) -> Option<DateTime<Utc>> {
        let (d, s) = (self.duration_ms?, self.segment_started_at.as_deref()?);
        if self.state != "running" {
            return None;
        }
        parse_utc(s)
            .ok()
            .map(|s| s + chrono::Duration::milliseconds((d - self.accumulated_ms).max(0)))
    }

    pub fn view(self, now: DateTime<Utc>) -> TimerView {
        TimerView {
            elapsed_ms: self.elapsed_ms(now),
            remaining_ms: self.remaining_ms(now),
            now: super::validation::fmt_utc(now),
            timer: self,
        }
    }
}

#[derive(Debug, Clone)]
pub struct NewTimer {
    pub kind: String,
    pub label: Option<String>,
    pub task_id: Option<String>,
    pub duration_ms: Option<i64>,
    pub phase: String,
    pub started_at: String,
}

/// State transition applied atomically (guarded by the expected current state).
#[derive(Debug, Clone)]
pub struct TimerChange {
    pub expected_state: &'static str,
    pub state: &'static str,
    pub segment_started_at: Option<String>,
    pub accumulated_ms: i64,
    pub ended_at: Option<String>,
}

#[derive(Debug, Clone)]
pub struct NewSession {
    pub task_id: Option<String>,
    pub started_at: String,
    pub ended_at: String,
    pub duration_ms: i64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn t(state: &str, seg: Option<&str>, acc: i64, dur: Option<i64>) -> Timer {
        Timer {
            id: "x".into(),
            kind: "countdown".into(),
            label: None,
            task_id: None,
            duration_ms: dur,
            phase: "focus".into(),
            state: state.into(),
            started_at: "2026-01-01T10:00:00.000Z".into(),
            segment_started_at: seg.map(str::to_string),
            accumulated_ms: acc,
            ended_at: None,
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    #[test]
    fn elapsed_is_derived_from_clock_including_sleep() {
        let timer = t(
            "running",
            Some("2026-01-01T10:00:00.000Z"),
            5_000,
            Some(3_600_000),
        );
        let before_sleep = Utc.with_ymd_and_hms(2026, 1, 1, 10, 10, 0).unwrap();
        let after_8h_sleep = Utc.with_ymd_and_hms(2026, 1, 1, 18, 10, 0).unwrap();
        assert_eq!(timer.elapsed_ms(before_sleep), 605_000);
        assert_eq!(
            timer.elapsed_ms(after_8h_sleep),
            5_000 + 8 * 3_600_000 + 600_000
        );
        assert_eq!(timer.remaining_ms(after_8h_sleep), Some(0));
        assert_eq!(
            timer.due_at(),
            Some(Utc.with_ymd_and_hms(2026, 1, 1, 10, 59, 55).unwrap())
        );
    }

    #[test]
    fn paused_timers_do_not_advance_and_clock_skew_is_clamped() {
        let paused = t("paused", None, 42_000, Some(60_000));
        let later = Utc.with_ymd_and_hms(2030, 1, 1, 0, 0, 0).unwrap();
        assert_eq!(paused.elapsed_ms(later), 42_000);
        assert_eq!(paused.remaining_ms(later), Some(18_000));
        assert_eq!(paused.due_at(), None);
        let running = t("running", Some("2026-01-01T10:00:00.000Z"), 1_000, None);
        let earlier = Utc.with_ymd_and_hms(2026, 1, 1, 9, 0, 0).unwrap(); // clock moved backwards
        assert_eq!(running.elapsed_ms(earlier), 1_000);
        assert_eq!(running.remaining_ms(earlier), None);
    }
}
