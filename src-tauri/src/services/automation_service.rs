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
    pub action: Action,
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

    fn write<'a>(
        name: &'a str,
        enabled: bool,
        trigger: &'a Trigger,
        action: &'a Action,
        t: &'a str,
        a: &'a str,
    ) -> RuleWrite<'a> {
        RuleWrite {
            name,
            enabled,
            trigger_kind: trigger.kind(),
            trigger_config: t,
            action_kind: action.kind(),
            action_config: a,
        }
    }

    /// Validate a rule without saving it.
    pub fn validate(input: &RuleInput) -> DomainResult<()> {
        clean_text(&input.name, "name", 120)?;
        input.trigger.validate()?;
        input.action.validate()
    }

    async fn save(&self, id: Option<&str>, input: RuleInput, actor: Actor) -> DomainResult<Rule> {
        let name = clean_text(&input.name, "name", 120)?;
        input.trigger.validate()?;
        input.action.validate()?;
        let t = serde_json::to_string(&input.trigger)
            .map_err(|e| DomainError::Storage(e.to_string()))?;
        let a = serde_json::to_string(&input.action)
            .map_err(|e| DomainError::Storage(e.to_string()))?;
        let w = Self::write(
            &name,
            input.enabled.unwrap_or(true),
            &input.trigger,
            &input.action,
            &t,
            &a,
        );
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
                action: cur.action,
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

    pub async fn list_enabled(&self) -> DomainResult<Vec<Rule>> {
        self.repo
            .list(true)
            .await?
            .into_iter()
            .map(|r| r.into_rule())
            .collect()
    }
}
