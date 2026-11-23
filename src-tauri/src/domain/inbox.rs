use serde::Serialize;

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct InboxItem {
    pub id: String,
    /// The captured text exactly as typed. Never modified.
    pub raw_text: String,
    /// "new" | "triaged" | "dismissed"
    pub status: String,
    pub linked_entity_type: Option<String>,
    pub linked_entity_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

pub const INBOX_COLS: &str =
    "id, raw_text, status, linked_entity_type, linked_entity_id, created_at, updated_at";
