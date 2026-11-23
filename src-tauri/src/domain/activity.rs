use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Actor {
    User,
    Ai,
    Automation,
}

impl Actor {
    pub fn as_str(self) -> &'static str {
        match self {
            Actor::User => "user",
            Actor::Ai => "ai",
            Actor::Automation => "automation",
        }
    }
}

/// A mutation record, written in the same transaction as the mutation itself.
#[derive(Debug, Clone)]
pub struct NewActivity {
    pub entity_type: &'static str,
    pub entity_id: String,
    pub action: &'static str,
    pub actor: Actor,
    pub before_json: Option<String>,
    pub after_json: Option<String>,
    pub batch_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct ActivityEntry {
    pub id: String,
    pub entity_type: String,
    pub entity_id: String,
    pub action: String,
    pub actor: String,
    pub before_json: Option<String>,
    pub after_json: Option<String>,
    pub batch_id: Option<String>,
    pub created_at: String,
}
