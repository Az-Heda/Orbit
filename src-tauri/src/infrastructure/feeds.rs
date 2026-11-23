//! Calendar-feed URL storage and download. A feed URL usually embeds a private token, so it is
//! stored like the API key (Windows Credential Manager) and never sent to the frontend or logged.

use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;

use crate::error::{DomainError, DomainResult};

const MAX_FEED_BYTES: usize = 10 * 1024 * 1024;

pub trait FeedSecrets: Send + Sync {
    fn get(&self, calendar_id: &str) -> DomainResult<Option<String>>;
    fn set(&self, calendar_id: &str, url: &str) -> DomainResult<()>;
    fn delete(&self, calendar_id: &str) -> DomainResult<()>;
}

/// In-memory store: tests, and non-Windows builds (which have no credential store, so feeds
/// there must be re-added after a restart).
#[derive(Default)]
pub struct MemoryFeedSecrets(Mutex<HashMap<String, String>>);

impl FeedSecrets for MemoryFeedSecrets {
    fn get(&self, id: &str) -> DomainResult<Option<String>> {
        Ok(self.0.lock().unwrap().get(id).cloned())
    }
    fn set(&self, id: &str, url: &str) -> DomainResult<()> {
        self.0.lock().unwrap().insert(id.into(), url.into());
        Ok(())
    }
    fn delete(&self, id: &str) -> DomainResult<()> {
        self.0.lock().unwrap().remove(id);
        Ok(())
    }
}

#[cfg(windows)]
pub struct OsFeedSecrets;

#[cfg(windows)]
impl OsFeedSecrets {
    fn entry(id: &str) -> DomainResult<keyring_core::Entry> {
        keyring_core::Entry::new("orbit", &format!("calendar-feed:{id}"))
            .map_err(|e| DomainError::External(format!("credential store error: {e}")))
    }
}

#[cfg(windows)]
impl FeedSecrets for OsFeedSecrets {
    fn get(&self, id: &str) -> DomainResult<Option<String>> {
        match Self::entry(id)?.get_password() {
            Ok(v) => Ok(Some(v)),
            Err(keyring_core::Error::NoEntry) => Ok(None),
            Err(e) => Err(DomainError::External(format!(
                "credential store error: {e}"
            ))),
        }
    }
    fn set(&self, id: &str, url: &str) -> DomainResult<()> {
        Self::entry(id)?
            .set_password(url)
            .map_err(|e| DomainError::External(format!("credential store error: {e}")))
    }
    fn delete(&self, id: &str) -> DomainResult<()> {
        match Self::entry(id)?.delete_credential() {
            Ok(()) | Err(keyring_core::Error::NoEntry) => Ok(()),
            Err(e) => Err(DomainError::External(format!(
                "credential store error: {e}"
            ))),
        }
    }
}

/// Must run after the API-key store has installed the default keyring store (same process).
pub fn default_feed_secrets() -> std::sync::Arc<dyn FeedSecrets> {
    #[cfg(windows)]
    {
        std::sync::Arc::new(OsFeedSecrets)
    }
    #[cfg(not(windows))]
    {
        std::sync::Arc::new(MemoryFeedSecrets::default())
    }
}

#[async_trait]
pub trait FeedFetcher: Send + Sync {
    async fn fetch(&self, url: &str) -> DomainResult<String>;
}

pub struct HttpFeedFetcher {
    http: reqwest::Client,
}

impl HttpFeedFetcher {
    pub fn new() -> Self {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::custom(|a| {
                if a.previous().len() >= 5 || a.url().scheme() != "https" {
                    a.stop()
                } else {
                    a.follow()
                }
            }))
            .build()
            .unwrap_or_default();
        Self { http }
    }
}

impl Default for HttpFeedFetcher {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl FeedFetcher for HttpFeedFetcher {
    async fn fetch(&self, url: &str) -> DomainResult<String> {
        // Error texts never include the URL: it may contain a private token.
        let fail = |what: &str| {
            DomainError::External(format!("could not download the calendar feed ({what})"))
        };
        let mut resp = self
            .http
            .get(url)
            .header("accept", "text/calendar, */*;q=0.1")
            .send()
            .await
            .map_err(|e| fail(&e.without_url().to_string()))?;
        if !resp.status().is_success() {
            return Err(fail(&format!("HTTP {}", resp.status().as_u16())));
        }
        let mut body: Vec<u8> = vec![];
        while let Some(chunk) = resp
            .chunk()
            .await
            .map_err(|e| fail(&e.without_url().to_string()))?
        {
            if body.len() + chunk.len() > MAX_FEED_BYTES {
                return Err(fail("feed is larger than 10 MB"));
            }
            body.extend_from_slice(&chunk);
        }
        String::from_utf8(body).map_err(|_| fail("not valid UTF-8 text"))
    }
}
