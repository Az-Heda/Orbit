use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::validation::invalid;
use crate::error::DomainResult;

pub const DEFAULT_MODEL: &str = "claude-opus-5-5";
pub const MAX_MEMORY_CHARS: usize = 300;
/// Total characters of memory injected into the system prompt.
pub const MEMORY_PROMPT_CAP: usize = 2_000;

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Conversation {
    pub id: String,
    pub title: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct StoredMessage {
    pub id: String,
    pub seq: i64,
    pub role: String,
    /// Content blocks exactly as exchanged with the API.
    pub content: Value,
    pub created_at: String,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct MessageRow {
    pub id: String,
    pub seq: i64,
    pub role: String,
    pub content_json: String,
    pub created_at: String,
}

impl MessageRow {
    pub fn into_message(self) -> StoredMessage {
        StoredMessage {
            id: self.id,
            seq: self.seq,
            role: self.role,
            content: serde_json::from_str(&self.content_json).unwrap_or(Value::Null),
            created_at: self.created_at,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Proposal {
    pub id: String,
    pub conversation_id: String,
    pub tool_name: String,
    pub input: Value,
    pub preview: Value,
    /// pending | applied | rejected | failed | undone
    pub status: String,
    pub result: Option<Value>,
    pub created_at: String,
    pub resolved_at: Option<String>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ProposalRow {
    pub id: String,
    pub conversation_id: String,
    pub tool_name: String,
    pub input_json: String,
    pub preview_json: String,
    pub status: String,
    pub result_json: Option<String>,
    pub created_at: String,
    pub resolved_at: Option<String>,
}

impl ProposalRow {
    pub fn into_proposal(self) -> Proposal {
        let parse = |s: &str| serde_json::from_str(s).unwrap_or(Value::Null);
        Proposal {
            input: parse(&self.input_json),
            preview: parse(&self.preview_json),
            result: self.result_json.as_deref().map(parse),
            id: self.id,
            conversation_id: self.conversation_id,
            tool_name: self.tool_name,
            status: self.status,
            created_at: self.created_at,
            resolved_at: self.resolved_at,
        }
    }
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Memory {
    pub id: String,
    pub text: String,
    pub created_at: String,
    pub updated_at: String,
}

/// What the user lets the assistant read. Everything defaults to the least that is useful:
/// tasks and calendar on, notes and inbox off.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SharingSettings {
    pub tasks: bool,
    pub calendar: bool,
    pub notes: bool,
    pub inbox: bool,
    pub memories: bool,
}

impl Default for SharingSettings {
    fn default() -> Self {
        Self {
            tasks: true,
            calendar: true,
            notes: false,
            inbox: false,
            memories: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AiConfig {
    pub model: String,
    /// low | medium | high | xhigh | max
    pub effort: String,
}

impl Default for AiConfig {
    fn default() -> Self {
        Self {
            model: DEFAULT_MODEL.into(),
            effort: "medium".into(),
        }
    }
}

pub fn validate_model(m: &str) -> DomainResult<String> {
    let m = m.trim();
    let ok = !m.is_empty()
        && m.len() <= 80
        && m.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_' | ':' | '@'));
    if ok {
        Ok(m.to_string())
    } else {
        Err(invalid("model id contains invalid characters"))
    }
}

pub fn validate_effort(e: &str) -> DomainResult<String> {
    match e {
        "low" | "medium" | "high" | "xhigh" | "max" => Ok(e.to_string()),
        _ => Err(invalid("effort must be low, medium, high, xhigh or max")),
    }
}

/// UI state passed with each message so "this" and "here" resolve. All ids are validated
/// server-side before they reach the prompt.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ChatContext {
    /// Route the user was looking at, e.g. `/projects/<id>`.
    pub view: Option<String>,
    pub timezone: Option<String>,
    pub project_id: Option<String>,
    pub board_id: Option<String>,
    pub task_id: Option<String>,
    pub event_id: Option<String>,
    pub note_id: Option<String>,
}

/// Memory text must be short, plain and free of anything that looks like a secret.
pub fn validate_memory_text(text: &str) -> DomainResult<String> {
    let t = super::validation::clean_text(text, "memory", MAX_MEMORY_CHARS)?;
    let lower = t.to_lowercase();
    let digits: String = t.chars().filter(|c| c.is_ascii_digit()).collect();
    let sensitive = [
        "password",
        "passwd",
        "secret",
        "api key",
        "api-key",
        "apikey",
        "token",
        "sk-ant",
        "private key",
    ]
    .iter()
    .any(|w| lower.contains(w))
        || t.split_whitespace().any(|w| {
            w.len() >= 24
                && w.chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
        })
        || digits.len() >= 13;
    if sensitive {
        return Err(invalid(
            "memories must not contain passwords, keys, tokens or other sensitive data",
        ));
    }
    Ok(t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_text_rules() {
        assert!(validate_memory_text("Prefers morning meetings").is_ok());
        assert!(validate_memory_text("my password is hunter2").is_err());
        assert!(validate_memory_text("key sk-ant-api03-abcdefghijklmnopqrstuvwxyz").is_err());
        assert!(validate_memory_text("card 4111 1111 1111 1111").is_err());
        assert!(validate_memory_text("   ").is_err());
        assert!(validate_memory_text(&"x ".repeat(200)).is_err());
    }

    #[test]
    fn model_and_effort_validation() {
        assert!(validate_model("claude-opus-5-5").is_ok());
        assert!(validate_model("bad model!").is_err());
        assert!(validate_effort("xhigh").is_ok());
        assert!(validate_effort("turbo").is_err());
    }
}
