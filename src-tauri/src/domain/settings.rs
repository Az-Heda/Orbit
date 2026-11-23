use serde::Serialize;

use crate::error::{DomainError, DomainResult};

pub const MAX_KEY_LEN: usize = 128;
pub const MAX_VALUE_LEN: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, sqlx::FromRow)]
pub struct Setting {
    pub key: String,
    /// JSON-encoded value.
    pub value: String,
    pub updated_at: String,
}

/// Keys are dotted lowercase identifiers, e.g. `ai.model` or `widget.opacity`.
pub fn validate_key(key: &str) -> DomainResult<()> {
    let ok = !key.is_empty()
        && key.len() <= MAX_KEY_LEN
        && key
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_' | '-'));
    if ok {
        Ok(())
    } else {
        Err(DomainError::Validation(format!(
            "invalid setting key (lowercase letters, digits, '.', '_', '-', max {MAX_KEY_LEN})"
        )))
    }
}

pub fn validate_value(value: &str) -> DomainResult<()> {
    if value.len() > MAX_VALUE_LEN {
        return Err(DomainError::Validation("setting value too large".into()));
    }
    serde_json::from_str::<serde_json::Value>(value)
        .map(|_| ())
        .map_err(|_| DomainError::Validation("setting value must be valid JSON".into()))
}
