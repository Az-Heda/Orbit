//! Recurrence rules: RFC 5545 RRULE strings stored on the series, expanded on read in the
//! series' own IANA timezone (so "every Monday 09:00" stays 09:00 local across DST changes).

use std::str::FromStr;

use chrono::{DateTime, Utc};
use rrule::{RRule, RRuleSet, Tz as RTz, Unvalidated};

use super::validation::invalid;
use crate::error::DomainResult;

const MAX_RULE_LEN: usize = 500;
/// Hard cap on iterations while expanding one series (guards pathological rules).
const MAX_ITERATIONS: usize = 100_000;

pub fn parse_tz(name: &str) -> DomainResult<chrono_tz::Tz> {
    name.trim().parse::<chrono_tz::Tz>().map_err(|_| {
        invalid(format!(
            "unknown timezone '{name}' (expected an IANA name like Europe/Rome)"
        ))
    })
}

/// Validate and normalise a bare RRULE value (no DTSTART/EXDATE lines).
pub fn clean_rule(rule: &str) -> DomainResult<String> {
    let r = rule.trim();
    let r = r.strip_prefix("RRULE:").unwrap_or(r).trim();
    if r.is_empty() || r.len() > MAX_RULE_LEN || r.contains('\n') || r.contains('\r') {
        return Err(invalid("invalid recurrence rule"));
    }
    RRule::<Unvalidated>::from_str(r)
        .map_err(|e| invalid(format!("invalid recurrence rule: {e}")))?;
    Ok(r.to_string())
}

fn build(rule: &str, tz: chrono_tz::Tz, dtstart: DateTime<Utc>) -> DomainResult<RRuleSet> {
    let start = dtstart.with_timezone(&RTz::Tz(tz));
    RRule::<Unvalidated>::from_str(rule)
        .and_then(|r| r.build(start))
        .map_err(|e| invalid(format!("invalid recurrence rule: {e}")))
        .map(RRuleSet::limit)
}

/// Check that a rule is usable for a series starting at `dtstart` in `tz_name`.
pub fn validate(rule: &str, tz_name: &str, dtstart: DateTime<Utc>) -> DomainResult<()> {
    build(rule, parse_tz(tz_name)?, dtstart).map(|_| ())
}

/// Start instants (UTC) of occurrences in `[from, to)`, computed in the series' local timezone.
pub fn occurrences(
    rule: &str,
    tz_name: &str,
    dtstart: DateTime<Utc>,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
) -> DomainResult<Vec<DateTime<Utc>>> {
    let set = build(rule, parse_tz(tz_name)?, dtstart)?;
    let mut out = Vec::new();
    for (i, dt) in set.into_iter().enumerate() {
        let utc = dt.with_timezone(&Utc);
        if utc >= to || i >= MAX_ITERATIONS {
            break;
        }
        if utc >= from {
            out.push(utc);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn utc(y: i32, m: u32, d: u32, h: u32, mi: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, m, d, h, mi, 0).unwrap()
    }

    #[test]
    fn weekly_rule_stays_at_local_time_across_dst_end() {
        // Mon 2026-10-19 09:00 Europe/Rome (CEST, UTC+2) = 07:00Z. DST ends Sun 2026-10-25.
        let start = utc(2026, 10, 19, 7, 0);
        let occ = occurrences(
            "FREQ=WEEKLY;BYDAY=MO",
            "Europe/Rome",
            start,
            utc(2026, 10, 1, 0, 0),
            utc(2026, 11, 10, 0, 0),
        )
        .unwrap();
        assert_eq!(
            occ,
            vec![
                utc(2026, 10, 19, 7, 0),
                utc(2026, 10, 26, 8, 0),
                utc(2026, 11, 2, 8, 0),
                utc(2026, 11, 9, 8, 0)
            ]
        );
    }

    #[test]
    fn daily_rule_across_dst_start() {
        // 2026-03-28 09:00 Rome (CET, +1) = 08:00Z; DST starts 2026-03-29.
        let start = utc(2026, 3, 28, 8, 0);
        let occ = occurrences(
            "FREQ=DAILY;COUNT=3",
            "Europe/Rome",
            start,
            utc(2026, 3, 1, 0, 0),
            utc(2026, 4, 30, 0, 0),
        )
        .unwrap();
        assert_eq!(
            occ,
            vec![
                utc(2026, 3, 28, 8, 0),
                utc(2026, 3, 29, 7, 0),
                utc(2026, 3, 30, 7, 0)
            ]
        );
    }

    #[test]
    fn range_is_half_open_and_skips_earlier_occurrences() {
        let start = utc(2026, 1, 1, 10, 0);
        let occ = occurrences(
            "FREQ=DAILY",
            "UTC",
            start,
            utc(2026, 1, 5, 10, 0),
            utc(2026, 1, 7, 10, 0),
        )
        .unwrap();
        assert_eq!(occ, vec![utc(2026, 1, 5, 10, 0), utc(2026, 1, 6, 10, 0)]);
    }

    #[test]
    fn rejects_bad_rules_and_timezones() {
        assert!(clean_rule("FREQ=NOPE").is_err());
        assert!(clean_rule("").is_err());
        assert!(clean_rule("FREQ=DAILY\nDTSTART:20260101T000000Z").is_err());
        assert_eq!(clean_rule("RRULE:FREQ=DAILY").unwrap(), "FREQ=DAILY");
        assert!(parse_tz("Mars/Olympus").is_err());
        assert!(validate("FREQ=DAILY", "Nope/Zone", utc(2026, 1, 1, 0, 0)).is_err());
    }

    #[test]
    fn impossible_rule_terminates() {
        // Feb 30 never exists: must return (empty or error), never hang.
        let r = occurrences(
            "FREQ=YEARLY;BYMONTH=2;BYMONTHDAY=30",
            "UTC",
            utc(2026, 1, 1, 0, 0),
            utc(2026, 1, 1, 0, 0),
            utc(2040, 1, 1, 0, 0),
        );
        assert!(r.map(|v| v.is_empty()).unwrap_or(true));
    }
}
