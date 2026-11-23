use std::sync::Arc;

use super::{CalendarService, CreateEventInput, CreateTaskInput, TaskService};
use crate::domain::activity::Actor;
use crate::domain::calendar::Event;
use crate::domain::events::{DomainEvent, EventBus, MaybeBus, emit};
use crate::domain::inbox::InboxItem;
use crate::domain::kanban::Task;
use crate::domain::validation::{invalid, parse_uuid};
use crate::error::{DomainError, DomainResult};
use crate::infrastructure::database::InboxRepository;

const MAX_CAPTURE: usize = 5_000;

/// Quick capture and triage. The captured text is stored unchanged and never discarded; triage
/// creates a task/event through the normal services and links it back to the item.
pub struct InboxService {
    repo: Arc<dyn InboxRepository>,
    tasks: Arc<TaskService>,
    calendar: Arc<CalendarService>,
    bus: MaybeBus,
}

/// First non-empty line, trimmed to a sensible title length.
fn derive_title(raw: &str) -> String {
    let line = raw
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("Untitled");
    line.chars().take(200).collect()
}

impl InboxService {
    pub fn new(
        repo: Arc<dyn InboxRepository>,
        tasks: Arc<TaskService>,
        calendar: Arc<CalendarService>,
    ) -> Self {
        Self {
            repo,
            tasks,
            calendar,
            bus: None,
        }
    }

    pub fn with_bus(mut self, bus: Arc<EventBus>) -> Self {
        self.bus = Some(bus);
        self
    }

    pub async fn capture(&self, raw_text: &str, actor: Actor) -> DomainResult<InboxItem> {
        if raw_text.trim().is_empty() {
            return Err(invalid("nothing to capture"));
        }
        if raw_text.chars().count() > MAX_CAPTURE {
            return Err(invalid("capture is too long"));
        }
        let item = self.repo.capture(raw_text, actor).await?;
        emit(
            &self.bus,
            DomainEvent::InboxAdded {
                item_id: item.id.clone(),
                text: item.raw_text.clone(),
            },
        );
        Ok(item)
    }

    pub async fn list(&self, status: Option<&str>, limit: i64) -> DomainResult<Vec<InboxItem>> {
        if !(1..=500).contains(&limit) {
            return Err(invalid("limit must be between 1 and 500"));
        }
        if let Some(s) = status
            && !matches!(s, "new" | "triaged" | "dismissed")
        {
            return Err(invalid("status must be new, triaged or dismissed"));
        }
        self.repo.list(status, limit).await
    }

    pub async fn count_new(&self) -> DomainResult<i64> {
        self.repo.count_new().await
    }

    async fn require_new(&self, id: &str) -> DomainResult<InboxItem> {
        let id = parse_uuid(id, "inbox item id")?;
        let item = self.repo.get(&id).await?.ok_or(DomainError::NotFound {
            entity: "inbox_item",
            id,
        })?;
        if item.status != "new" {
            return Err(invalid("inbox item was already handled"));
        }
        Ok(item)
    }

    /// File an item as a task in `column_id`. `title` defaults to the first line of the capture;
    /// the full original text goes into the description when it adds anything.
    pub async fn triage_to_task(
        &self,
        id: &str,
        column_id: &str,
        title: Option<&str>,
        due_at: Option<&str>,
        actor: Actor,
    ) -> DomainResult<(InboxItem, Task)> {
        let item = self.require_new(id).await?;
        let title = title
            .map(str::to_string)
            .unwrap_or_else(|| derive_title(&item.raw_text));
        let description = (item.raw_text.trim() != title).then(|| item.raw_text.clone());
        let task = self
            .tasks
            .create_task(
                CreateTaskInput {
                    column_id: Some(column_id.to_string()),
                    parent_id: None,
                    title,
                    description,
                    priority: None,
                    due_at: due_at.map(str::to_string),
                    position: None,
                },
                actor,
            )
            .await?;
        let item = self
            .repo
            .mark_triaged(&item.id, "task", &task.id, actor)
            .await?;
        Ok((item, task))
    }

    pub async fn triage_to_event(
        &self,
        id: &str,
        title: Option<&str>,
        start_at: &str,
        end_at: &str,
        timezone: &str,
        actor: Actor,
    ) -> DomainResult<(InboxItem, Event)> {
        let item = self.require_new(id).await?;
        let title = title
            .map(str::to_string)
            .unwrap_or_else(|| derive_title(&item.raw_text));
        let description = (item.raw_text.trim() != title).then(|| item.raw_text.clone());
        let event = self
            .calendar
            .create_event(
                CreateEventInput {
                    title,
                    description,
                    location: None,
                    all_day: None,
                    start_at: start_at.to_string(),
                    end_at: end_at.to_string(),
                    timezone: timezone.to_string(),
                    recurrence_rule: None,
                },
                actor,
            )
            .await?;
        let item = self
            .repo
            .mark_triaged(&item.id, "event", &event.id, actor)
            .await?;
        Ok((item, event))
    }

    pub async fn dismiss(&self, id: &str, actor: Actor) -> DomainResult<InboxItem> {
        let item = self.require_new(id).await?;
        self.repo.dismiss(&item.id, actor).await
    }
}
