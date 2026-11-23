use serde::Serialize;

use super::validation::invalid;
use crate::error::DomainResult;

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Note {
    pub id: String,
    pub title: String,
    pub body: String,
    /// "note" or "journal"
    pub kind: String,
    pub journal_date: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

pub const NOTE_COLS: &str = "id, title, body, kind, journal_date, created_at, updated_at";

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct NoteLink {
    pub entity_type: String,
    pub entity_id: String,
    /// Resolved title of the linked entity (None if it no longer exists).
    pub title: Option<String>,
}

pub fn validate_entity_type(t: &str) -> DomainResult<&'static str> {
    match t {
        "task" => Ok("task"),
        "event" => Ok("event"),
        "project" => Ok("project"),
        _ => Err(invalid("entity_type must be task, event or project")),
    }
}

/// yyyy-mm-dd
pub fn validate_date(d: &str) -> DomainResult<String> {
    chrono::NaiveDate::parse_from_str(d.trim(), "%Y-%m-%d")
        .map(|d| d.format("%Y-%m-%d").to_string())
        .map_err(|_| invalid("date must look like yyyy-mm-dd"))
}
