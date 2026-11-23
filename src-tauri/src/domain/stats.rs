use std::collections::BTreeMap;

use chrono::{DateTime, Duration, NaiveDate};
use chrono_tz::Tz;
use serde::Serialize;

/// A focus session joined with its project (if linked through a task).
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct FocusRow {
    pub started_at: String,
    pub duration_ms: i64,
    pub project_id: Option<String>,
    pub project_name: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DayStat {
    /// Local calendar date, yyyy-mm-dd
    pub date: String,
    pub focus_ms: i64,
    pub sessions: i64,
    pub completed: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProjectStat {
    pub project_id: Option<String>,
    pub name: String,
    pub focus_ms: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Stats {
    pub timezone: String,
    pub from: String,
    pub to: String,
    /// One entry per local day in range (zero-filled), oldest first. Drives charts and the heatmap.
    pub days: Vec<DayStat>,
    pub by_project: Vec<ProjectStat>,
    pub total_focus_ms: i64,
    pub total_completed: i64,
    /// Consecutive active days (a day with focus time or a completed task) ending today, or
    /// yesterday if today has no activity yet. Limited to the requested range.
    pub current_streak: i64,
    pub longest_streak: i64,
}

fn local_date(utc: &str, tz: Tz) -> Option<NaiveDate> {
    DateTime::parse_from_rfc3339(utc)
        .ok()
        .map(|d| d.with_timezone(&tz).date_naive())
}

/// Bucket rows into local days of `tz`. `today` is the current local date; the range is the last
/// `days` days including today. Pure so it can be tested without a database.
pub fn build_stats(
    tz: Tz,
    today: NaiveDate,
    days: i64,
    focus: &[FocusRow],
    completions: &[String],
) -> Stats {
    let first = today - Duration::days(days - 1);
    let mut map: BTreeMap<NaiveDate, DayStat> = (0..days)
        .map(|i| {
            let d = first + Duration::days(i);
            (
                d,
                DayStat {
                    date: d.to_string(),
                    focus_ms: 0,
                    sessions: 0,
                    completed: 0,
                },
            )
        })
        .collect();
    let mut projects: BTreeMap<Option<String>, ProjectStat> = BTreeMap::new();
    for r in focus {
        let Some(d) = local_date(&r.started_at, tz) else {
            continue;
        };
        let Some(day) = map.get_mut(&d) else { continue };
        day.focus_ms += r.duration_ms;
        day.sessions += 1;
        projects
            .entry(r.project_id.clone())
            .or_insert_with(|| ProjectStat {
                project_id: r.project_id.clone(),
                name: r
                    .project_name
                    .clone()
                    .unwrap_or_else(|| "No project".into()),
                focus_ms: 0,
            })
            .focus_ms += r.duration_ms;
    }
    for c in completions {
        if let Some(day) = local_date(c, tz).and_then(|d| map.get_mut(&d)) {
            day.completed += 1;
        }
    }
    let list: Vec<DayStat> = map.into_values().collect();
    let active = |d: &DayStat| d.focus_ms > 0 || d.completed > 0;

    let mut longest = 0;
    let mut run = 0;
    for d in &list {
        run = if active(d) { run + 1 } else { 0 };
        longest = longest.max(run);
    }
    // Current streak: walk back from today; an empty today doesn't break it yet.
    let mut it = list.iter().rev().peekable();
    if it.peek().is_some_and(|d| !active(d)) {
        it.next();
    }
    let current = it.take_while(|d| active(d)).count() as i64;

    let mut by_project: Vec<ProjectStat> = projects.into_values().collect();
    by_project.sort_by(|a, b| b.focus_ms.cmp(&a.focus_ms).then(a.name.cmp(&b.name)));
    Stats {
        timezone: tz.to_string(),
        from: first.to_string(),
        to: today.to_string(),
        total_focus_ms: list.iter().map(|d| d.focus_ms).sum(),
        total_completed: list.iter().map(|d| d.completed).sum(),
        days: list,
        by_project,
        current_streak: current,
        longest_streak: longest,
    }
}
