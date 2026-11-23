use serde::Serialize;

/// A subscribed, read-only calendar. The feed URL is deliberately not part of this type.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct ExternalCalendar {
    pub id: String,
    pub name: String,
    pub created_at: String,
    pub last_synced_at: Option<String>,
    pub last_error: Option<String>,
    pub event_count: i64,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct SyncReport {
    pub created: usize,
    pub updated: usize,
    pub removed: usize,
    /// Feed events whose UID already belongs to one of the user's own events.
    pub skipped: usize,
}

/// Accept only https feed URLs (`webcal://` is the same thing). Returns the normalised URL.
pub fn normalize_feed_url(raw: &str) -> Result<String, String> {
    let u = raw.trim();
    let u = match u.strip_prefix("webcal://") {
        Some(rest) => format!("https://{rest}"),
        None => u.to_string(),
    };
    if u.len() > 2048 || u.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err("that does not look like a calendar feed URL".into());
    }
    let rest = u
        .strip_prefix("https://")
        .ok_or_else(|| "only https:// (or webcal://) feed URLs are accepted".to_string())?;
    if rest.split('/').next().is_none_or(str::is_empty) {
        return Err("the feed URL has no host".into());
    }
    Ok(u)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_are_normalised_and_restricted() {
        assert_eq!(
            normalize_feed_url(" webcal://x.org/a.ics ").unwrap(),
            "https://x.org/a.ics"
        );
        assert!(normalize_feed_url("http://x.org/a.ics").is_err());
        assert!(normalize_feed_url("file:///etc/passwd").is_err());
        assert!(normalize_feed_url("https:///nohost").is_err());
        assert!(normalize_feed_url("https://x.org/a b").is_err());
    }
}
