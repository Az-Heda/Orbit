use std::sync::Arc;

use crate::domain::activity::Actor;
use crate::domain::settings::{Setting, validate_key, validate_value};
use crate::error::DomainResult;
use crate::infrastructure::database::SettingsRepository;

pub struct SettingsService {
    repo: Arc<dyn SettingsRepository>,
}

impl SettingsService {
    pub fn new(repo: Arc<dyn SettingsRepository>) -> Self {
        Self { repo }
    }

    pub async fn get(&self, key: &str) -> DomainResult<Option<Setting>> {
        validate_key(key)?;
        self.repo.get(key).await
    }

    pub async fn list(&self) -> DomainResult<Vec<Setting>> {
        self.repo.list().await
    }

    pub async fn set(&self, key: &str, value: &str, actor: Actor) -> DomainResult<Setting> {
        validate_key(key)?;
        validate_value(value)?;
        self.repo.set(key, value, actor).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::database::{
        ActivityRepository, SqliteActivityRepository, SqliteSettingsRepository, open_in_memory,
    };

    async fn setup() -> (SettingsService, SqliteActivityRepository) {
        let pool = open_in_memory().await.unwrap();
        (
            SettingsService::new(Arc::new(SqliteSettingsRepository::new(pool.clone()))),
            SqliteActivityRepository::new(pool),
        )
    }

    #[tokio::test]
    async fn set_then_get_roundtrip() {
        let (svc, _) = setup().await;
        svc.set("ai.model", "\"some-model\"", Actor::User)
            .await
            .unwrap();
        let got = svc.get("ai.model").await.unwrap().unwrap();
        assert_eq!(got.value, "\"some-model\"");
        assert_eq!(svc.list().await.unwrap().len(), 1);
        assert!(svc.get("missing").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn write_produces_activity_log_rows_with_before_after() {
        let (svc, log) = setup().await;
        svc.set("widget.opacity", "0.8", Actor::User).await.unwrap();
        svc.set("widget.opacity", "0.5", Actor::Automation)
            .await
            .unwrap();

        let rows = log.for_entity("setting", "widget.opacity").await.unwrap();
        assert_eq!(rows.len(), 2);
        // newest first
        assert_eq!(rows[0].action, "update");
        assert_eq!(rows[0].actor, "automation");
        assert_eq!(rows[0].before_json.as_deref(), Some("0.8"));
        assert_eq!(rows[0].after_json.as_deref(), Some("0.5"));
        assert_eq!(rows[1].action, "create");
        assert_eq!(rows[1].actor, "user");
        assert_eq!(rows[1].before_json, None);
    }

    #[tokio::test]
    async fn rejects_invalid_input_without_logging() {
        let (svc, log) = setup().await;
        assert!(matches!(
            svc.set("Bad Key", "1", Actor::User).await,
            Err(crate::error::DomainError::Validation(_))
        ));
        assert!(matches!(
            svc.set("ok.key", "{not json", Actor::User).await,
            Err(crate::error::DomainError::Validation(_))
        ));
        assert!(log.recent(10).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn db_rejects_unknown_actor_and_enforces_foreign_keys_pragma() {
        let pool = open_in_memory().await.unwrap();
        let bad = sqlx::query(
            "INSERT INTO activity_log (id, entity_type, entity_id, action, actor, created_at) VALUES ('1','t','1','a','robot','x')",
        )
        .execute(&pool)
        .await;
        assert!(bad.is_err());
        let fk: (i64,) = sqlx::query_as("PRAGMA foreign_keys")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(fk.0, 1);
    }
}
