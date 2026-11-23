use serde::{Serialize, Serializer};

/// Errors raised by domain rules and services. No I/O types leak through here
/// except via the explicit `Storage` variant.
#[derive(Debug, thiserror::Error)]
pub enum DomainError {
    #[error("validation failed: {0}")]
    Validation(String),
    #[error("{entity} not found: {id}")]
    NotFound { entity: &'static str, id: String },
    #[error("storage error: {0}")]
    Storage(String),
}

impl From<sqlx::Error> for DomainError {
    fn from(e: sqlx::Error) -> Self {
        DomainError::Storage(e.to_string())
    }
}

pub type DomainResult<T> = Result<T, DomainError>;

/// Error type returned from Tauri commands. Serializes to a `{ kind, message }`
/// object so the frontend can branch on `kind`.
#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub struct AppError(#[from] pub DomainError);

impl Serialize for AppError {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let kind = match self.0 {
            DomainError::Validation(_) => "validation",
            DomainError::NotFound { .. } => "not_found",
            DomainError::Storage(_) => "storage",
        };
        let mut st = s.serialize_struct("AppError", 2)?;
        st.serialize_field("kind", kind)?;
        st.serialize_field("message", &self.0.to_string())?;
        st.end()
    }
}

pub type AppResult<T> = Result<T, AppError>;
