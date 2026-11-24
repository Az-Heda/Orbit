use std::sync::Arc;

use serde::Deserialize;

use crate::domain::activity::Actor;
use crate::domain::automation::{Action, Rule, Trigger};
use crate::domain::validation::{clean_text, parse_uuid};
use crate::error::{DomainError, DomainResult};
use crate::infrastructure::database::{AutomationRepository, RuleWrite};

#[derive(Debug, Deserialize)]
pub struct RuleInput {
    pub name: String,
    pub enabled: Option<bool>,
    pub trigger: Trigger,
    /// The chain: 1 to 10 steps, run in order.
    pub actions: Vec<Action>,
}

/// CRUD for automation rules. A rule the AI drafts is only created after the user confirms the
/// proposal, so it is never created behind the user's back.
pub struct AutomationService {
    repo: Arc<dyn AutomationRepository>,
}

impl AutomationService {
    pub fn new(repo: Arc<dyn AutomationRepository>) -> Self {
        Self { repo }
    }

    /// Validate a rule without saving it.
    pub fn validate(input: &RuleInput) -> DomainResult<()> {
        clean_text(&input.name, "name", 120)?;
        input.trigger.validate()?;
        Action::validate_chain(&input.actions)
    }

    async fn save(&self, id: Option<&str>, input: RuleInput, actor: Actor) -> DomainResult<Rule> {
        let name = clean_text(&input.name, "name", 120)?;
        input.trigger.validate()?;
        Action::validate_chain(&input.actions)?;
        let t = serde_json::to_string(&input.trigger)
            .map_err(|e| DomainError::Storage(e.to_string()))?;
        let a = serde_json::to_string(&input.actions)
            .map_err(|e| DomainError::Storage(e.to_string()))?;
        let w = RuleWrite {
            name: &name,
            enabled: input.enabled.unwrap_or(true),
            trigger_kind: input.trigger.kind(),
            trigger_config: &t,
            actions: &a,
        };
        let row = match id {
            None => self.repo.create(&w, actor).await?,
            Some(id) => self.repo.update(id, &w, actor).await?,
        };
        row.into_rule()
    }

    pub async fn create(&self, input: RuleInput, actor: Actor) -> DomainResult<Rule> {
        self.save(None, input, actor).await
    }

    pub async fn update(&self, id: &str, input: RuleInput, actor: Actor) -> DomainResult<Rule> {
        let id = parse_uuid(id, "rule id")?;
        self.save(Some(&id), input, actor).await
    }

    pub async fn set_enabled(&self, id: &str, enabled: bool, actor: Actor) -> DomainResult<Rule> {
        let id = parse_uuid(id, "rule id")?;
        let cur = self
            .repo
            .get(&id)
            .await?
            .ok_or(DomainError::NotFound {
                entity: "automation_rule",
                id: id.clone(),
            })?
            .into_rule()?;
        self.save(
            Some(&id),
            RuleInput {
                name: cur.name,
                enabled: Some(enabled),
                trigger: cur.trigger,
                actions: cur.actions,
            },
            actor,
        )
        .await
    }

    pub async fn delete(&self, id: &str, actor: Actor) -> DomainResult<()> {
        self.repo.delete(&parse_uuid(id, "rule id")?, actor).await
    }

    pub async fn list(&self) -> DomainResult<Vec<Rule>> {
        self.repo
            .list(false)
            .await?
            .into_iter()
            .map(|r| r.into_rule())
            .collect()
    }

    /// A step failed: the chain stopped there. Best effort (a logging failure must not matter).
    pub async fn log_step_failure(&self, rule_id: &str, step: usize, action: &Action, error: &str) {
        if let Err(e) = self
            .repo
            .log_step_failure(rule_id, step, action.kind(), error)
            .await
        {
            tracing::warn!("cannot log failed step: {e}");
        }
    }

    pub async fn list_enabled(&self) -> DomainResult<Vec<Rule>> {
        self.repo
            .list(true)
            .await?
            .into_iter()
            .map(|r| r.into_rule())
            .collect()
    }
}
