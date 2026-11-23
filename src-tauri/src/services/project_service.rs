use std::sync::Arc;

use serde::Deserialize;

use crate::domain::activity::Actor;
use crate::domain::kanban::{Board, Column, ColumnKind, Project};
use crate::domain::validation::{clean_optional, clean_text, parse_uuid, validate_color};
use crate::error::DomainResult;
use crate::infrastructure::database::ProjectRepository;

#[derive(Debug, Deserialize)]
pub struct UpdateProjectInput {
    pub name: Option<String>,
    /// Empty string clears.
    pub description: Option<String>,
    /// Empty string clears.
    pub color: Option<String>,
}

pub struct ProjectService {
    repo: Arc<dyn ProjectRepository>,
}

impl ProjectService {
    pub fn new(repo: Arc<dyn ProjectRepository>) -> Self {
        Self { repo }
    }

    pub async fn create_project(
        &self,
        name: &str,
        description: Option<&str>,
        color: Option<&str>,
        actor: Actor,
    ) -> DomainResult<Project> {
        let name = clean_text(name, "name", 120)?;
        let description = description
            .map(|d| clean_optional(d, "description", 2000))
            .transpose()?
            .flatten();
        let color = color
            .filter(|c| !c.is_empty())
            .map(validate_color)
            .transpose()?;
        self.repo
            .create_project(&name, description.as_deref(), color.as_deref(), actor)
            .await
    }

    pub async fn list_projects(&self) -> DomainResult<Vec<Project>> {
        self.repo.list_projects().await
    }

    pub async fn update_project(
        &self,
        id: &str,
        input: UpdateProjectInput,
        actor: Actor,
    ) -> DomainResult<Project> {
        let id = parse_uuid(id, "project id")?;
        let name = input
            .name
            .as_deref()
            .map(|n| clean_text(n, "name", 120))
            .transpose()?;
        let description = input
            .description
            .as_deref()
            .map(|d| clean_optional(d, "description", 2000))
            .transpose()?;
        let color = input
            .color
            .as_deref()
            .map(|c| {
                if c.is_empty() {
                    Ok(None)
                } else {
                    validate_color(c).map(Some)
                }
            })
            .transpose()?;
        self.repo
            .update_project(
                &id,
                name.as_deref(),
                description.as_ref().map(|d| d.as_deref()),
                color.as_ref().map(|c| c.as_deref()),
                actor,
            )
            .await
    }

    pub async fn delete_project(&self, id: &str, actor: Actor) -> DomainResult<()> {
        self.repo
            .delete_project(&parse_uuid(id, "project id")?, actor)
            .await
    }

    pub async fn create_board(
        &self,
        project_id: &str,
        name: &str,
        actor: Actor,
    ) -> DomainResult<Board> {
        self.repo
            .create_board(
                &parse_uuid(project_id, "project id")?,
                &clean_text(name, "name", 120)?,
                actor,
            )
            .await
    }

    pub async fn list_boards(&self, project_id: &str) -> DomainResult<Vec<Board>> {
        self.repo
            .list_boards(&parse_uuid(project_id, "project id")?)
            .await
    }

    pub async fn rename_board(&self, id: &str, name: &str, actor: Actor) -> DomainResult<Board> {
        self.repo
            .update_board(
                &parse_uuid(id, "board id")?,
                &clean_text(name, "name", 120)?,
                actor,
            )
            .await
    }

    pub async fn delete_board(&self, id: &str, actor: Actor) -> DomainResult<()> {
        self.repo
            .delete_board(&parse_uuid(id, "board id")?, actor)
            .await
    }

    pub async fn create_column(
        &self,
        board_id: &str,
        name: &str,
        kind: &str,
        actor: Actor,
    ) -> DomainResult<Column> {
        self.repo
            .create_column(
                &parse_uuid(board_id, "board id")?,
                &clean_text(name, "name", 60)?,
                ColumnKind::parse(kind)?,
                actor,
            )
            .await
    }

    pub async fn update_column(
        &self,
        id: &str,
        name: Option<&str>,
        kind: Option<&str>,
        actor: Actor,
    ) -> DomainResult<Column> {
        let name = name.map(|n| clean_text(n, "name", 60)).transpose()?;
        let kind = kind.map(ColumnKind::parse).transpose()?;
        self.repo
            .update_column(&parse_uuid(id, "column id")?, name.as_deref(), kind, actor)
            .await
    }

    pub async fn move_column(&self, id: &str, position: i64, actor: Actor) -> DomainResult<Column> {
        if position < 0 {
            return Err(crate::domain::validation::invalid("position must be >= 0"));
        }
        self.repo
            .move_column(&parse_uuid(id, "column id")?, position, actor)
            .await
    }

    pub async fn delete_column(&self, id: &str, actor: Actor) -> DomainResult<()> {
        self.repo
            .delete_column(&parse_uuid(id, "column id")?, actor)
            .await
    }
}
