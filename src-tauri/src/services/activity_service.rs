use std::sync::Arc;

use crate::domain::activity::ActivityEntry;
use crate::error::{DomainError, DomainResult};
use crate::infrastructure::database::ActivityRepository;

pub struct ActivityService {
    repo: Arc<dyn ActivityRepository>,
}

impl ActivityService {
    pub fn new(repo: Arc<dyn ActivityRepository>) -> Self {
        Self { repo }
    }

    pub async fn recent(&self, limit: i64) -> DomainResult<Vec<ActivityEntry>> {
        if !(1..=500).contains(&limit) {
            return Err(DomainError::Validation(
                "limit must be between 1 and 500".into(),
            ));
        }
        self.repo.recent(limit).await
    }

    pub async fn for_entity(
        &self,
        entity_type: &str,
        entity_id: &str,
    ) -> DomainResult<Vec<ActivityEntry>> {
        self.repo.for_entity(entity_type, entity_id).await
    }
}
