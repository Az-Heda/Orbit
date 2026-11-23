//! Shared input validation used by services (and therefore by UI commands and AI tools alike).

use chrono::{DateTime, SecondsFormat, Utc};

use crate::error::{DomainError, DomainResult};

pub fn invalid(msg: impl Into<String>) -> DomainError {
    DomainError::Validation(msg.into())
}

/// Parse and normalise a UUID string.
pub fn parse_uuid(s: &str, label: &str) -> DomainResult<String> {
    uuid::Uuid::parse_str(s.trim())
        .map(|u| u.to_string())
        .map_err(|_| invalid(format!("{label} must be a valid UUID")))
}

/// Trim and bound a required text field.
pub fn clean_text(s: &str, label: &str, max: usize) -> DomainResult<String> {
    let t = s.trim();
    if t.is_empty() {
        return Err(invalid(format!("{label} must not be empty")));
    }
    if t.chars().count() > max {
        return Err(invalid(format!("{label} must be at most {max} characters")));
    }
    Ok(t.to_string())
}

/// Optional text: empty/whitespace becomes `None`.
pub fn clean_optional(s: &str, label: &str, max: usize) -> DomainResult<Option<String>> {
    let t = s.trim();
    if t.is_empty() {
        return Ok(None);
    }
    clean_text(t, label, max).map(Some)
}

/// Parse an RFC 3339 timestamp and normalise it to UTC with millisecond precision.
pub fn normalize_utc(s: &str, label: &str) -> DomainResult<String> {
    let dt = DateTime::parse_from_rfc3339(s.trim())
        .map_err(|_| invalid(format!("{label} must be an RFC 3339 timestamp")))?;
    Ok(dt
        .with_timezone(&Utc)
        .to_rfc3339_opts(SecondsFormat::Millis, true))
}

pub fn parse_utc(s: &str) -> DomainResult<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .map(|d| d.with_timezone(&Utc))
        .map_err(|_| invalid("invalid timestamp"))
}

pub fn fmt_utc(dt: DateTime<Utc>) -> String {
    dt.to_rfc3339_opts(SecondsFormat::Millis, true)
}

pub fn validate_color(s: &str) -> DomainResult<String> {
    let ok = s.len() == 7 && s.starts_with('#') && s[1..].chars().all(|c| c.is_ascii_hexdigit());
    if ok {
        Ok(s.to_ascii_lowercase())
    } else {
        Err(invalid("color must look like #rrggbb"))
    }
}
