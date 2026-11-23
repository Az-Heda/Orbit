//! iCalendar (.ics) conversion. Export writes events with RRULE/EXDATE and RECURRENCE-ID overrides;
//! import parses VEVENTs into validated domain input. Pure functions, no I/O.

use chrono::{DateTime, Duration, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Utc};
use icalendar::{
    Calendar, CalendarComponent, CalendarDateTime, Component, DatePerhapsTime, Event as IEvent,
    EventLike, Property,
};

use super::calendar::{Event, EventException, ImportSummary, NewEvent, NewException};
use super::recurrence::{clean_rule, parse_tz, validate};
use super::validation::{fmt_utc, parse_utc};

pub struct ParsedEvent {
    pub event: NewEvent,
    pub exceptions: Vec<NewException>,
}

fn ics_utc(dt: DateTime<Utc>) -> String {
    dt.format("%Y%m%dT%H%M%SZ").to_string()
}

fn local_naive(dt: DateTime<Utc>, tz: chrono_tz::Tz) -> NaiveDateTime {
    dt.with_timezone(&tz).naive_local()
}

fn set_times(
    e: &mut IEvent,
    ev: &Event,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    tz: chrono_tz::Tz,
) {
    if ev.all_day {
        e.starts(local_naive(start, tz).date());
        e.ends(local_naive(end, tz).date());
    } else if ev.recurrence_rule.is_some() {
        let tzid = ev.timezone.clone();
        e.starts(DatePerhapsTime::DateTime(CalendarDateTime::WithTimezone {
            date_time: local_naive(start, tz),
            tzid: tzid.clone(),
        }));
        e.ends(DatePerhapsTime::DateTime(CalendarDateTime::WithTimezone {
            date_time: local_naive(end, tz),
            tzid,
        }));
    } else {
        e.starts(start);
        e.ends(end);
    }
}

pub fn export(items: &[(Event, Vec<EventException>)]) -> String {
    let mut cal = Calendar::new();
    for (ev, exceptions) in items {
        let (Ok(start), Ok(end), Ok(tz)) = (
            parse_utc(&ev.start_at),
            parse_utc(&ev.end_at),
            parse_tz(&ev.timezone),
        ) else {
            continue;
        };
        let stamp = parse_utc(&ev.updated_at).unwrap_or_else(|_| Utc::now());
        let mut e = IEvent::new();
        e.uid(&ev.uid).summary(&ev.title).timestamp(stamp);
        if let Some(d) = &ev.description {
            e.description(d);
        }
        if let Some(l) = &ev.location {
            e.location(l);
        }
        set_times(&mut e, ev, start, end, tz);
        if let Some(rule) = &ev.recurrence_rule {
            e.add_property("RRULE", rule);
            for ex in exceptions.iter().filter(|x| x.kind == "skip") {
                if let Ok(orig) = parse_utc(&ex.original_start) {
                    e.add_multi_property("EXDATE", &ics_utc(orig));
                }
            }
        }
        cal.push(e.done());
        for ex in exceptions.iter().filter(|x| x.kind == "override") {
            let Ok(orig) = parse_utc(&ex.original_start) else {
                continue;
            };
            let o_start = ex
                .start_at
                .as_deref()
                .and_then(|s| parse_utc(s).ok())
                .unwrap_or(orig);
            let o_end = ex
                .end_at
                .as_deref()
                .and_then(|s| parse_utc(s).ok())
                .unwrap_or(o_start + (end - start));
            let mut o = IEvent::new();
            o.uid(&ev.uid)
                .summary(ex.title.as_deref().unwrap_or(&ev.title))
                .timestamp(stamp);
            o.add_property("RECURRENCE-ID", ics_utc(orig));
            if let Some(d) = ex.description.as_ref().or(ev.description.as_ref()) {
                o.description(d);
            }
            if let Some(l) = ex.location.as_ref().or(ev.location.as_ref()) {
                o.location(l);
            }
            if ev.all_day {
                o.starts(local_naive(o_start, tz).date());
                o.ends(local_naive(o_end, tz).date());
            } else {
                o.starts(o_start);
                o.ends(o_end);
            }
            cal.push(o.done());
        }
    }
    cal.to_string()
}

/// Resolve an iCalendar date/time to a UTC instant. Returns `(instant, is_date_only, warning)`.
fn resolve(
    dt: &DatePerhapsTime,
    default_tz: chrono_tz::Tz,
    warnings: &mut Vec<String>,
) -> Option<(DateTime<Utc>, bool)> {
    let local = |n: NaiveDateTime, tz: chrono_tz::Tz| {
        tz.from_local_datetime(&n)
            .earliest()
            .or_else(|| tz.from_local_datetime(&(n + Duration::hours(1))).earliest())
            .map(|d| d.with_timezone(&Utc))
    };
    match dt {
        DatePerhapsTime::Date(d) => {
            local(d.and_time(NaiveTime::MIN), default_tz).map(|u| (u, true))
        }
        DatePerhapsTime::DateTime(CalendarDateTime::Utc(u)) => Some((*u, false)),
        DatePerhapsTime::DateTime(CalendarDateTime::Floating(n)) => {
            local(*n, default_tz).map(|u| (u, false))
        }
        DatePerhapsTime::DateTime(CalendarDateTime::WithTimezone { date_time, tzid }) => {
            let tz = parse_tz(tzid).unwrap_or_else(|_| {
                warnings.push(format!("unknown timezone '{tzid}', using {default_tz}"));
                default_tz
            });
            local(*date_time, tz).map(|u| (u, false))
        }
    }
}

fn parse_ics_value(
    value: &str,
    tzid: Option<&str>,
    default_tz: chrono_tz::Tz,
) -> Option<DateTime<Utc>> {
    let value = value.trim();
    if let Ok(n) = NaiveDateTime::parse_from_str(value.trim_end_matches('Z'), "%Y%m%dT%H%M%S") {
        if value.ends_with('Z') {
            return Some(Utc.from_utc_datetime(&n));
        }
        let tz = tzid.and_then(|t| parse_tz(t).ok()).unwrap_or(default_tz);
        return tz
            .from_local_datetime(&n)
            .earliest()
            .map(|d| d.with_timezone(&Utc));
    }
    let d = NaiveDate::parse_from_str(value, "%Y%m%d").ok()?;
    default_tz
        .from_local_datetime(&d.and_time(NaiveTime::MIN))
        .earliest()
        .map(|d| d.with_timezone(&Utc))
}

fn exdates(e: &IEvent, default_tz: chrono_tz::Tz) -> Vec<DateTime<Utc>> {
    let mut out = vec![];
    let mut props: Vec<&Property> = e
        .multi_properties()
        .get("EXDATE")
        .map(|v| v.iter().collect())
        .unwrap_or_default();
    if let Some(p) = e.properties().get("EXDATE") {
        props.push(p);
    }
    for p in props {
        let tzid = p.params().get("TZID").map(|x| x.value().to_string());
        for v in p.value().split(',') {
            if let Some(d) = parse_ics_value(v, tzid.as_deref(), default_tz) {
                out.push(d);
            }
        }
    }
    out
}

fn text(s: Option<&str>) -> Option<String> {
    s.map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_string)
}

/// Parse an .ics document. Events lacking a title or valid start are skipped with a warning.
pub fn parse(
    input: &str,
    default_tz_name: &str,
) -> Result<(Vec<ParsedEvent>, ImportSummary), String> {
    let default_tz = parse_tz(default_tz_name).map_err(|e| e.to_string())?;
    let cal: Calendar = input
        .parse()
        .map_err(|e| format!("not a valid iCalendar file: {e}"))?;
    let mut summary = ImportSummary::default();
    let mut parsed: Vec<ParsedEvent> = vec![];
    let mut overrides: Vec<(String, NewException)> = vec![];

    for comp in cal.components.iter() {
        let CalendarComponent::Event(e) = comp else {
            continue;
        };
        let title = text(e.get_summary()).unwrap_or_else(|| "(no title)".into());
        let Some(start_raw) = e.get_start() else {
            summary
                .warnings
                .push(format!("skipped '{title}': missing start"));
            continue;
        };
        let Some((start, all_day)) = resolve(&start_raw, default_tz, &mut summary.warnings) else {
            summary
                .warnings
                .push(format!("skipped '{title}': invalid start"));
            continue;
        };
        let end = e
            .get_end()
            .and_then(|d| resolve(&d, default_tz, &mut summary.warnings))
            .map(|(u, _)| u)
            .filter(|end| *end >= start)
            .unwrap_or(if all_day {
                start + Duration::days(1)
            } else {
                start
            });
        let uid =
            text(e.get_uid()).unwrap_or_else(|| format!("{}@orbit-import", uuid::Uuid::new_v4()));
        let tz_name = match &start_raw {
            DatePerhapsTime::DateTime(CalendarDateTime::WithTimezone { tzid, .. })
                if parse_tz(tzid).is_ok() =>
            {
                tzid.clone()
            }
            _ => default_tz_name.to_string(),
        };

        if let Some(rid) = e.get_recurrence_id() {
            if let Some((orig, _)) = resolve(&rid, default_tz, &mut summary.warnings) {
                overrides.push((
                    uid,
                    NewException {
                        original_start: fmt_utc(orig),
                        skip: false,
                        title: Some(title),
                        description: text(e.get_description()),
                        location: text(e.get_location()),
                        start_at: Some(fmt_utc(start)),
                        end_at: Some(fmt_utc(end)),
                    },
                ));
            }
            continue;
        }

        let mut rule = None;
        if let Some(p) = e.properties().get("RRULE") {
            match clean_rule(p.value()).and_then(|r| validate(&r, &tz_name, start).map(|_| r)) {
                Ok(r) => rule = Some(r),
                Err(err) => summary
                    .warnings
                    .push(format!("'{title}': recurrence ignored ({err})")),
            }
        }
        let exceptions = if rule.is_some() {
            exdates(e, default_tz)
                .into_iter()
                .map(|d| NewException {
                    original_start: fmt_utc(d),
                    skip: true,
                    ..Default::default()
                })
                .collect()
        } else {
            vec![]
        };
        parsed.push(ParsedEvent {
            event: NewEvent {
                uid,
                title,
                description: text(e.get_description()),
                location: text(e.get_location()),
                all_day,
                start_at: fmt_utc(start),
                end_at: fmt_utc(end),
                timezone: tz_name,
                recurrence_rule: rule,
            },
            exceptions,
        });
    }
    for (uid, ex) in overrides {
        match parsed
            .iter_mut()
            .find(|p| p.event.uid == uid && p.event.recurrence_rule.is_some())
        {
            Some(p) => p.exceptions.push(ex),
            None => summary
                .warnings
                .push(format!("override for unknown series {uid} ignored")),
        }
    }
    Ok((parsed, summary))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(uid: &str, rule: Option<&str>, all_day: bool) -> Event {
        Event {
            id: "id".into(),
            uid: uid.into(),
            title: "Standup".into(),
            description: Some("daily".into()),
            location: None,
            all_day,
            start_at: "2026-10-19T07:00:00.000Z".into(),
            end_at: "2026-10-19T07:30:00.000Z".into(),
            timezone: "Europe/Rome".into(),
            recurrence_rule: rule.map(str::to_string),
            created_at: "2026-01-01T00:00:00.000Z".into(),
            updated_at: "2026-01-01T00:00:00.000Z".into(),
        }
    }

    #[test]
    fn roundtrip_recurring_with_skip_and_override() {
        let skip = EventException {
            id: "1".into(),
            event_id: "id".into(),
            original_start: "2026-10-26T08:00:00.000Z".into(),
            kind: "skip".into(),
            title: None,
            description: None,
            location: None,
            start_at: None,
            end_at: None,
            created_at: String::new(),
        };
        let over = EventException {
            id: "2".into(),
            kind: "override".into(),
            original_start: "2026-11-02T08:00:00.000Z".into(),
            title: Some("Moved".into()),
            start_at: Some("2026-11-02T10:00:00.000Z".into()),
            end_at: Some("2026-11-02T10:30:00.000Z".into()),
            ..skip.clone()
        };
        let text = export(&[(
            ev("a@orbit", Some("FREQ=WEEKLY;BYDAY=MO"), false),
            vec![skip, over],
        )]);
        assert!(text.contains("RRULE:FREQ=WEEKLY;BYDAY=MO"));
        let (parsed, summary) = parse(&text, "UTC").unwrap();
        assert!(summary.warnings.is_empty(), "{:?}", summary.warnings);
        assert_eq!(parsed.len(), 1);
        let p = &parsed[0];
        assert_eq!(p.event.uid, "a@orbit");
        assert_eq!(p.event.timezone, "Europe/Rome");
        assert_eq!(p.event.start_at, "2026-10-19T07:00:00.000Z");
        assert_eq!(
            p.event.recurrence_rule.as_deref(),
            Some("FREQ=WEEKLY;BYDAY=MO")
        );
        assert_eq!(p.exceptions.len(), 2);
        assert!(
            p.exceptions
                .iter()
                .any(|e| e.skip && e.original_start == "2026-10-26T08:00:00.000Z")
        );
        assert!(
            p.exceptions
                .iter()
                .any(|e| !e.skip && e.title.as_deref() == Some("Moved"))
        );
    }

    #[test]
    fn roundtrip_all_day_and_plain_events() {
        let mut a = ev("b@orbit", None, true);
        a.start_at = "2026-12-24T23:00:00.000Z".into(); // 25 Dec 00:00 Rome
        a.end_at = "2026-12-25T23:00:00.000Z".into();
        let b = ev("c@orbit", None, false);
        let (parsed, _) = parse(&export(&[(a, vec![]), (b, vec![])]), "Europe/Rome").unwrap();
        assert_eq!(parsed.len(), 2);
        assert!(parsed[0].event.all_day);
        assert_eq!(parsed[0].event.start_at, "2026-12-24T23:00:00.000Z");
        assert_eq!(parsed[0].event.end_at, "2026-12-25T23:00:00.000Z");
        assert!(!parsed[1].event.all_day);
        assert_eq!(parsed[1].event.start_at, "2026-10-19T07:00:00.000Z");
    }

    #[test]
    fn parses_foreign_ics_with_floating_and_tzid_times() {
        let ics = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//x//y//EN\r\nBEGIN:VEVENT\r\nUID:ext-1\r\nSUMMARY:Lunch\r\nDTSTART;TZID=America/New_York:20260301T120000\r\nDTEND;TZID=America/New_York:20260301T130000\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nUID:ext-2\r\nSUMMARY:Float\r\nDTSTART:20260301T090000\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
        let (parsed, _) = parse(ics, "Europe/Rome").unwrap();
        assert_eq!(parsed[0].event.start_at, "2026-03-01T17:00:00.000Z");
        assert_eq!(parsed[0].event.timezone, "America/New_York");
        assert_eq!(parsed[1].event.start_at, "2026-03-01T08:00:00.000Z");
        assert!(parse("garbage", "UTC").is_err() || parse("garbage", "UTC").unwrap().0.is_empty());
    }
}
