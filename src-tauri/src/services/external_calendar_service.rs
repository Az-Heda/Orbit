use std::sync::Arc;

use crate::domain::calendar::ImportSummary;
use crate::domain::external::{ExternalCalendar, SyncReport, normalize_feed_url};
use crate::domain::ics;
use crate::domain::validation::{clean_text, invalid};
use crate::error::{DomainError, DomainResult};
use crate::infrastructure::database::{ExternalCalendarRepository, ImportEvent};
use crate::infrastructure::feeds::{FeedFetcher, FeedSecrets};

/// Opt-in, read-only calendar subscriptions (ICS feeds, e.g. Google/Outlook "secret address").
/// Events are tagged with their calendar and can never be edited. The feed URL is a secret.
pub struct ExternalCalendarService {
    repo: Arc<dyn ExternalCalendarRepository>,
    secrets: Arc<dyn FeedSecrets>,
    fetcher: Arc<dyn FeedFetcher>,
}

impl ExternalCalendarService {
    pub fn new(
        repo: Arc<dyn ExternalCalendarRepository>,
        secrets: Arc<dyn FeedSecrets>,
        fetcher: Arc<dyn FeedFetcher>,
    ) -> Self {
        Self {
            repo,
            secrets,
            fetcher,
        }
    }

    pub async fn list(&self) -> DomainResult<Vec<ExternalCalendar>> {
        self.repo.list().await
    }

    /// Subscribe and do a first sync. A failing first sync keeps the subscription (with its error
    /// shown) so a temporary network problem does not lose the URL.
    pub async fn add(&self, name: &str, url: &str) -> DomainResult<ExternalCalendar> {
        let name = clean_text(name, "name", 80)?;
        let url = normalize_feed_url(url).map_err(DomainError::Validation)?;
        let cal = self.repo.create(&name).await?;
        if let Err(e) = self.secrets.set(&cal.id, &url) {
            let _ = self.repo.delete(&cal.id).await;
            return Err(e);
        }
        let _ = self.sync(&cal.id).await;
        self.repo.get(&cal.id).await?.ok_or(DomainError::NotFound {
            entity: "external_calendar",
            id: cal.id,
        })
    }

    pub async fn remove(&self, id: &str) -> DomainResult<()> {
        let id = crate::domain::validation::parse_uuid(id, "calendar id")?;
        self.repo.get(&id).await?.ok_or(DomainError::NotFound {
            entity: "external_calendar",
            id: id.clone(),
        })?;
        self.repo.delete(&id).await?;
        self.secrets.delete(&id)
    }

    pub async fn sync(&self, id: &str) -> DomainResult<SyncReport> {
        let id = crate::domain::validation::parse_uuid(id, "calendar id")?;
        self.repo.get(&id).await?.ok_or(DomainError::NotFound {
            entity: "external_calendar",
            id: id.clone(),
        })?;
        match self.sync_inner(&id).await {
            Ok(r) => Ok(r),
            Err(e) => {
                let _ = self.repo.set_status(&id, Some(&e.to_string())).await;
                Err(e)
            }
        }
    }

    async fn sync_inner(&self, id: &str) -> DomainResult<SyncReport> {
        let url = self.secrets.get(id)?.ok_or_else(|| {
            invalid(
                "the feed URL is not stored on this device; remove the calendar and add it again",
            )
        })?;
        let text = self.fetcher.fetch(&url).await?;
        let (parsed, _summary): (_, ImportSummary) =
            ics::parse(&text, "UTC").map_err(DomainError::Validation)?;
        let items: Vec<ImportEvent> = parsed
            .into_iter()
            .map(|p| ImportEvent {
                event: p.event,
                exceptions: p.exceptions,
            })
            .collect();
        self.repo.replace_events(id, &items).await
    }

    /// Sync every subscription; one failing feed does not stop the others.
    pub async fn sync_all(&self) -> usize {
        let mut ok = 0;
        for c in self.list().await.unwrap_or_default() {
            if self.sync(&c.id).await.is_ok() {
                ok += 1;
            }
        }
        ok
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use async_trait::async_trait;

    use super::*;
    use crate::domain::activity::Actor;
    use crate::infrastructure::database::{
        SqliteCalendarRepository, SqliteExternalCalendarRepository, open_in_memory,
    };
    use crate::infrastructure::feeds::MemoryFeedSecrets;
    use crate::services::{CalendarService, UpdateEventInput};

    struct Feed(Mutex<Result<String, String>>);
    #[async_trait]
    impl FeedFetcher for Feed {
        async fn fetch(&self, _url: &str) -> DomainResult<String> {
            self.0
                .lock()
                .unwrap()
                .clone()
                .map_err(DomainError::External)
        }
    }

    fn ics(events: &[(&str, &str)]) -> String {
        let mut s = String::from("BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//x//EN\r\n");
        for (uid, title) in events {
            s.push_str(&format!(
                "BEGIN:VEVENT\r\nUID:{uid}\r\nDTSTAMP:20270101T000000Z\r\nDTSTART:20270111T090000Z\r\nDTEND:20270111T100000Z\r\nSUMMARY:{title}\r\nEND:VEVENT\r\n"
            ));
        }
        s.push_str("END:VCALENDAR\r\n");
        s
    }

    async fn setup(
        feed: &str,
    ) -> (
        ExternalCalendarService,
        Arc<Feed>,
        CalendarService,
        Arc<MemoryFeedSecrets>,
    ) {
        let pool = open_in_memory().await.unwrap();
        let fetcher = Arc::new(Feed(Mutex::new(Ok(feed.to_string()))));
        let secrets = Arc::new(MemoryFeedSecrets::default());
        let svc = ExternalCalendarService::new(
            Arc::new(SqliteExternalCalendarRepository::new(pool.clone())),
            secrets.clone(),
            fetcher.clone(),
        );
        (
            svc,
            fetcher,
            CalendarService::new(Arc::new(SqliteCalendarRepository::new(pool))),
            secrets,
        )
    }

    async fn titles(c: &CalendarService) -> Vec<String> {
        let mut v: Vec<String> = c
            .list_range("2027-01-11T00:00:00Z", "2027-01-12T00:00:00Z")
            .await
            .unwrap()
            .into_iter()
            .map(|i| i.title)
            .collect();
        v.sort();
        v
    }

    #[tokio::test]
    async fn sync_adds_updates_and_removes_and_events_are_read_only() {
        let (svc, feed, cal, secrets) = setup(&ics(&[("a@x", "Alpha"), ("b@x", "Beta")])).await;
        let c = svc
            .add("Work", "webcal://example.org/private-token/basic.ics")
            .await
            .unwrap();
        assert_eq!(c.event_count, 2);
        assert_eq!(titles(&cal).await, ["Alpha", "Beta"]);
        assert_eq!(
            secrets.get(&c.id).unwrap().unwrap(),
            "https://example.org/private-token/basic.ics"
        );

        // The feed changed: Beta renamed, Alpha gone, Gamma new.
        *feed.0.lock().unwrap() = Ok(ics(&[("b@x", "Beta 2"), ("c@x", "Gamma")]));
        let r = svc.sync(&c.id).await.unwrap();
        assert_eq!((r.created, r.updated, r.removed), (1, 1, 1));
        assert_eq!(titles(&cal).await, ["Beta 2", "Gamma"]);

        // Nobody (UI or AI share this service) can edit or delete an external event.
        let id = cal
            .list_range("2027-01-11T00:00:00Z", "2027-01-12T00:00:00Z")
            .await
            .unwrap()[0]
            .source_id
            .clone();
        let edit = UpdateEventInput {
            title: Some("Mine".into()),
            ..Default::default()
        };
        assert!(cal.update_event(&id, edit, Actor::Ai).await.is_err());
        assert!(cal.delete_event(&id, Actor::User).await.is_err());

        svc.remove(&c.id).await.unwrap();
        assert!(titles(&cal).await.is_empty());
        assert!(secrets.get(&c.id).unwrap().is_none());
        assert!(svc.list().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn own_events_are_never_overwritten_by_a_feed_uid_collision() {
        let (svc, _f, cal, _s) = setup(&ics(&[("mine@x", "Feed version")])).await;
        cal.import_ics(&ics(&[("mine@x", "My own")]), "UTC", Actor::User)
            .await
            .unwrap();
        let c = svc.add("Other", "https://example.org/a.ics").await.unwrap();
        assert_eq!(c.event_count, 0);
        assert_eq!(titles(&cal).await, ["My own"]);
    }

    #[tokio::test]
    async fn failures_are_recorded_without_losing_the_subscription_or_leaking_the_url() {
        let (svc, feed, _cal, _s) = setup("").await;
        *feed.0.lock().unwrap() = Err("could not download the calendar feed (HTTP 404)".into());
        let c = svc
            .add("Down", "https://example.org/secret-token.ics")
            .await
            .unwrap();
        let err = c.last_error.unwrap();
        assert!(err.contains("404") && !err.contains("secret-token"));
        assert!(
            svc.add("x", "http://example.org/a.ics").await.is_err(),
            "http is refused"
        );
        assert!(svc.add("   ", "https://example.org/a.ics").await.is_err());
        *feed.0.lock().unwrap() = Ok("not a calendar".into());
        assert!(svc.sync(&c.id).await.is_err());
    }
}
