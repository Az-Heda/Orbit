use serde::Serialize;

pub const KINDS: [&str; 5] = ["task", "event", "note", "project", "inbox"];

/// Highlight markers wrapped around matched terms in snippets.
pub const MARK_OPEN: &str = "⟦";
pub const MARK_CLOSE: &str = "⟧";

#[derive(Debug, Clone, Serialize)]
pub struct SearchHit {
    /// "task" | "event" | "note" | "project" | "inbox"
    pub kind: &'static str,
    pub id: String,
    pub title: String,
    /// Matching excerpt with ⟦highlighted⟧ terms.
    pub snippet: String,
    /// Higher is better.
    pub score: f64,
    pub project_id: Option<String>,
    pub board_id: Option<String>,
    /// Event start (UTC) for events.
    pub date: Option<String>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct HitRow {
    pub id: String,
    pub title: String,
    pub snippet: String,
    pub rank: f64,
    pub project_id: Option<String>,
    pub board_id: Option<String>,
    pub date: Option<String>,
}

/// Turn free user input into a safe FTS5 MATCH expression: each word becomes a quoted
/// prefix term (`"foo"*`), joined with implicit AND. Punctuation and operators are dropped.
/// Returns `None` when nothing searchable remains.
pub fn match_expression(input: &str) -> Option<String> {
    let terms: Vec<String> = input
        .split_whitespace()
        .map(|w| {
            w.chars()
                .filter(|c| c.is_alphanumeric())
                .take(64)
                .collect::<String>()
        })
        .filter(|w| !w.is_empty())
        .take(8)
        .map(|w| format!("\"{w}\"*"))
        .collect();
    if terms.is_empty() {
        None
    } else {
        Some(terms.join(" "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitises_hostile_input() {
        assert_eq!(
            match_expression("foo bar").as_deref(),
            Some("\"foo\"* \"bar\"*")
        );
        assert_eq!(match_expression("foo-").as_deref(), Some("\"foo\"*"));
        assert_eq!(
            match_expression("\"unbalanced").as_deref(),
            Some("\"unbalanced\"*")
        );
        assert_eq!(
            match_expression("a OR NEAR(b c) -d *").as_deref(),
            Some("\"a\"* \"OR\"* \"NEARb\"* \"c\"* \"d\"*")
        );
        assert_eq!(match_expression("  \"*-() ").as_deref(), None);
        assert_eq!(match_expression(""), None);
        assert_eq!(match_expression("città").as_deref(), Some("\"città\"*"));
    }
}
