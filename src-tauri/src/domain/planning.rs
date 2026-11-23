//! Pure scheduling helpers behind the AI planning tools (no I/O, fully unit-tested).

use chrono::{DateTime, Duration, NaiveDate, NaiveTime, TimeZone, Utc};
use chrono_tz::Tz;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Slot {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
}

impl Slot {
    pub fn minutes(&self) -> i64 {
        (self.end - self.start).num_minutes()
    }
}

/// Free gaps of at least `min_minutes` inside `[from, to)`, limited to the daily working window
/// `[work_start, work_end)` in `tz`. `busy` may overlap and be unsorted. A day with a DST gap in
/// its window simply uses the instants that exist.
pub fn free_slots(
    busy: &[(DateTime<Utc>, DateTime<Utc>)],
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    min_minutes: i64,
    tz: Tz,
    work_start: NaiveTime,
    work_end: NaiveTime,
) -> Vec<Slot> {
    if to <= from || work_end <= work_start {
        return vec![];
    }
    let mut merged: Vec<(DateTime<Utc>, DateTime<Utc>)> =
        busy.iter().copied().filter(|(s, e)| e > s).collect();
    merged.sort();
    let mut union: Vec<(DateTime<Utc>, DateTime<Utc>)> = vec![];
    for (s, e) in merged {
        match union.last_mut() {
            Some(last) if s <= last.1 => last.1 = last.1.max(e),
            _ => union.push((s, e)),
        }
    }

    let mut out = vec![];
    let mut day: NaiveDate = from.with_timezone(&tz).date_naive();
    let last_day = to.with_timezone(&tz).date_naive();
    while day <= last_day {
        let at = |t: NaiveTime| {
            tz.from_local_datetime(&day.and_time(t))
                .earliest()
                .map(|d| d.with_timezone(&Utc))
        };
        if let (Some(ws), Some(we)) = (at(work_start), at(work_end)) {
            let (mut cursor, end) = (ws.max(from), we.min(to));
            for &(bs, be) in &union {
                if be <= cursor {
                    continue;
                }
                if bs >= end {
                    break;
                }
                if bs > cursor {
                    push(&mut out, cursor, bs.min(end), min_minutes);
                }
                cursor = cursor.max(be);
            }
            if cursor < end {
                push(&mut out, cursor, end, min_minutes);
            }
        }
        day += Duration::days(1);
    }
    out
}

fn push(out: &mut Vec<Slot>, start: DateTime<Utc>, end: DateTime<Utc>, min_minutes: i64) {
    if (end - start).num_minutes() >= min_minutes {
        out.push(Slot { start, end });
    }
}

#[derive(Debug, Clone)]
pub struct RankInput {
    pub id: String,
    pub priority: i64,
    pub due_at: Option<DateTime<Utc>>,
    /// In a column of kind "active" (already being worked on).
    pub in_progress: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Ranked {
    pub id: String,
    pub score: i64,
    pub reasons: Vec<String>,
}

/// Rank open, unblocked tasks by what most deserves attention now: overdue first, then due
/// soon, then priority, with a nudge for work already in progress. Ties keep input order.
pub fn rank_tasks(tasks: &[RankInput], now: DateTime<Utc>, tz: Tz) -> Vec<Ranked> {
    let today = now.with_timezone(&tz).date_naive();
    let mut ranked: Vec<Ranked> = tasks
        .iter()
        .map(|t| {
            let mut score = 0;
            let mut reasons = vec![];
            if let Some(due) = t.due_at {
                let days = (due.with_timezone(&tz).date_naive() - today).num_days();
                if due < now && days < 0 {
                    score += 100 + (-days).min(14);
                    reasons.push(format!("overdue by {} day(s)", -days));
                } else if days <= 0 {
                    score += 60;
                    reasons.push("due today".to_string());
                } else if days <= 3 {
                    score += 30;
                    reasons.push(format!("due in {days} day(s)"));
                } else if days <= 7 {
                    score += 10;
                    reasons.push(format!("due in {days} days"));
                }
            }
            if t.priority > 0 {
                score += t.priority * 15;
                reasons.push(format!("priority {}", t.priority));
            }
            if t.in_progress {
                score += 20;
                reasons.push("already in progress".to_string());
            }
            Ranked {
                id: t.id.clone(),
                score,
                reasons,
            }
        })
        .collect();
    ranked.sort_by_key(|r| std::cmp::Reverse(r.score));
    ranked
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utc(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }
    fn hm(h: u32, m: u32) -> NaiveTime {
        NaiveTime::from_hms_opt(h, m, 0).unwrap()
    }

    #[test]
    fn gaps_between_overlapping_busy_blocks() {
        let tz: Tz = "Europe/Rome".parse().unwrap(); // UTC+1 in January
        let busy = [
            (utc("2027-01-11T09:00:00Z"), utc("2027-01-11T10:00:00Z")), // 10-11 local
            (utc("2027-01-11T09:30:00Z"), utc("2027-01-11T11:00:00Z")), // overlaps
            (utc("2027-01-11T13:00:00Z"), utc("2027-01-11T14:00:00Z")), // 14-15 local
        ];
        let s = free_slots(
            &busy,
            utc("2027-01-11T00:00:00Z"),
            utc("2027-01-12T00:00:00Z"),
            30,
            tz,
            hm(9, 0),
            hm(17, 0),
        );
        let got: Vec<(String, String)> = s
            .iter()
            .map(|x| (x.start.to_rfc3339(), x.end.to_rfc3339()))
            .collect();
        assert_eq!(
            got,
            vec![
                (
                    "2027-01-11T08:00:00+00:00".into(),
                    "2027-01-11T09:00:00+00:00".into()
                ),
                (
                    "2027-01-11T11:00:00+00:00".into(),
                    "2027-01-11T13:00:00+00:00".into()
                ),
                (
                    "2027-01-11T14:00:00+00:00".into(),
                    "2027-01-11T16:00:00+00:00".into()
                ),
            ]
        );
    }

    #[test]
    fn respects_range_edges_minimum_and_multiple_days() {
        let tz: Tz = "UTC".parse().unwrap();
        // Range starts at noon: the morning is not offered.
        let s = free_slots(
            &[],
            utc("2027-01-11T12:00:00Z"),
            utc("2027-01-12T10:00:00Z"),
            60,
            tz,
            hm(9, 0),
            hm(17, 0),
        );
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].start, utc("2027-01-11T12:00:00Z"));
        assert_eq!(s[1].end, utc("2027-01-12T10:00:00Z"));
        // A 45-minute gap is dropped when 60 are needed.
        let busy = [(utc("2027-01-11T09:45:00Z"), utc("2027-01-11T17:00:00Z"))];
        assert!(
            free_slots(
                &busy,
                utc("2027-01-11T00:00:00Z"),
                utc("2027-01-12T00:00:00Z"),
                60,
                tz,
                hm(9, 0),
                hm(17, 0)
            )
            .is_empty()
        );
        assert!(
            free_slots(
                &[],
                utc("2027-01-12T00:00:00Z"),
                utc("2027-01-11T00:00:00Z"),
                5,
                tz,
                hm(9, 0),
                hm(17, 0)
            )
            .is_empty()
        );
    }

    #[test]
    fn dst_day_uses_local_wall_clock() {
        let tz: Tz = "Europe/Rome".parse().unwrap();
        // 2027-03-28 is the spring-forward day: 09:00 local is 07:00 UTC, not 08:00.
        let s = free_slots(
            &[],
            utc("2027-03-28T00:00:00Z"),
            utc("2027-03-29T00:00:00Z"),
            30,
            tz,
            hm(9, 0),
            hm(10, 0),
        );
        assert_eq!(s[0].start, utc("2027-03-28T07:00:00Z"));
    }

    #[test]
    fn ranking_prefers_overdue_then_soon_then_priority() {
        let tz: Tz = "UTC".parse().unwrap();
        let now = utc("2027-01-11T10:00:00Z");
        let t = |id: &str, p: i64, due: Option<&str>, ip: bool| RankInput {
            id: id.into(),
            priority: p,
            due_at: due.map(utc),
            in_progress: ip,
        };
        let r = rank_tasks(
            &[
                t("later", 3, Some("2027-02-01T10:00:00Z"), false),
                t("overdue", 0, Some("2027-01-09T10:00:00Z"), false),
                t("today", 1, Some("2027-01-11T17:00:00Z"), false),
                t("wip", 0, None, true),
                t("none", 0, None, false),
            ],
            now,
            tz,
        );
        let ids: Vec<_> = r.iter().map(|x| x.id.as_str()).collect();
        assert_eq!(ids, ["overdue", "today", "later", "wip", "none"]);
        assert!(r[0].reasons[0].contains("overdue by 2"));
        assert_eq!(r[4].score, 0);
    }
}
