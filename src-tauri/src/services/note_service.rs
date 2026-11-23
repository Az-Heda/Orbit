use std::sync::Arc;

use serde::Serialize;

use crate::domain::activity::Actor;
use crate::domain::notes::{Note, NoteLink, validate_date, validate_entity_type};
use crate::domain::validation::{clean_text, invalid, parse_uuid};
use crate::error::{DomainError, DomainResult};
use crate::infrastructure::database::NoteRepository;

const MAX_BODY: usize = 500_000;

#[derive(Debug, Serialize)]
pub struct NoteDetail {
    #[serde(flatten)]
    pub note: Note,
    pub links: Vec<NoteLink>,
}

pub struct NoteService {
    repo: Arc<dyn NoteRepository>,
}

impl NoteService {
    pub fn new(repo: Arc<dyn NoteRepository>) -> Self {
        Self { repo }
    }

    fn check_body(body: &str) -> DomainResult<()> {
        if body.len() > MAX_BODY {
            return Err(invalid("note is too large"));
        }
        Ok(())
    }

    pub async fn create(&self, title: &str, body: &str, actor: Actor) -> DomainResult<Note> {
        Self::check_body(body)?;
        self.repo
            .create(&clean_text(title, "title", 300)?, body, actor)
            .await
    }

    pub async fn get(&self, id: &str) -> DomainResult<NoteDetail> {
        let id = parse_uuid(id, "note id")?;
        let note = self.repo.get(&id).await?.ok_or(DomainError::NotFound {
            entity: "note",
            id: id.clone(),
        })?;
        Ok(NoteDetail {
            links: self.repo.links(&id).await?,
            note,
        })
    }

    pub async fn update(
        &self,
        id: &str,
        title: Option<&str>,
        body: Option<&str>,
        actor: Actor,
    ) -> DomainResult<Note> {
        let id = parse_uuid(id, "note id")?;
        if let Some(b) = body {
            Self::check_body(b)?;
        }
        let title = title.map(|t| clean_text(t, "title", 300)).transpose()?;
        self.repo.update(&id, title.as_deref(), body, actor).await
    }

    pub async fn delete(&self, id: &str, actor: Actor) -> DomainResult<()> {
        self.repo.delete(&parse_uuid(id, "note id")?, actor).await
    }

    /// `kind`: "note" | "journal" | None. `entity`: (type, id) to list notes linked to it.
    pub async fn list(
        &self,
        kind: Option<&str>,
        entity: Option<(&str, &str)>,
        limit: i64,
    ) -> DomainResult<Vec<Note>> {
        if !(1..=200).contains(&limit) {
            return Err(invalid("limit must be between 1 and 200"));
        }
        if let Some(k) = kind
            && !matches!(k, "note" | "journal")
        {
            return Err(invalid("kind must be note or journal"));
        }
        let entity = match entity {
            Some((t, i)) => Some((validate_entity_type(t)?, parse_uuid(i, "entity id")?)),
            None => None,
        };
        self.repo
            .list(kind, entity.as_ref().map(|(t, i)| (*t, i.as_str())), limit)
            .await
    }

    pub async fn journal(&self, date: &str, actor: Actor) -> DomainResult<Note> {
        self.repo.journal(&validate_date(date)?, actor).await
    }

    pub async fn add_link(
        &self,
        note_id: &str,
        entity_type: &str,
        entity_id: &str,
        actor: Actor,
    ) -> DomainResult<()> {
        self.repo
            .add_link(
                &parse_uuid(note_id, "note id")?,
                validate_entity_type(entity_type)?,
                &parse_uuid(entity_id, "entity id")?,
                actor,
            )
            .await
    }

    pub async fn remove_link(
        &self,
        note_id: &str,
        entity_type: &str,
        entity_id: &str,
        actor: Actor,
    ) -> DomainResult<()> {
        self.repo
            .remove_link(
                &parse_uuid(note_id, "note id")?,
                validate_entity_type(entity_type)?,
                &parse_uuid(entity_id, "entity id")?,
                actor,
            )
            .await
    }
}
