//! Network access for the self-updater: GitHub's public "latest release" endpoint and release
//! downloads. Unauthenticated; nothing about the user or their data is sent.

use async_trait::async_trait;

use crate::error::{DomainError, DomainResult};

#[async_trait]
pub trait UpdateSource: Send + Sync {
    /// The JSON of `GET /repos/<repo>/releases/latest`.
    async fn latest_release(&self, repo: &str) -> DomainResult<String>;
    /// Download a release asset (https only), refusing anything larger than `max_bytes`.
    async fn download(&self, url: &str, max_bytes: usize) -> DomainResult<Vec<u8>>;
}

pub struct HttpUpdateSource {
    http: reqwest::Client,
}

impl HttpUpdateSource {
    pub fn new() -> Self {
        let http = reqwest::Client::builder()
            .user_agent("orbit-updater")
            .timeout(std::time::Duration::from_secs(300))
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

    async fn get(&self, url: &str, accept: &str, max: usize) -> DomainResult<Vec<u8>> {
        let fail = |what: String| DomainError::External(format!("update check failed ({what})"));
        if !url.starts_with("https://") {
            return Err(fail("only https is allowed".into()));
        }
        let mut resp = self
            .http
            .get(url)
            .header("accept", accept)
            .send()
            .await
            .map_err(|e| fail(e.without_url().to_string()))?;
        if !resp.status().is_success() {
            return Err(fail(format!("HTTP {}", resp.status().as_u16())));
        }
        let mut body = vec![];
        while let Some(chunk) = resp
            .chunk()
            .await
            .map_err(|e| fail(e.without_url().to_string()))?
        {
            if body.len() + chunk.len() > max {
                return Err(fail("download is larger than expected".into()));
            }
            body.extend_from_slice(&chunk);
        }
        Ok(body)
    }
}

impl Default for HttpUpdateSource {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl UpdateSource for HttpUpdateSource {
    async fn latest_release(&self, repo: &str) -> DomainResult<String> {
        let url = format!("https://api.github.com/repos/{repo}/releases/latest");
        let b = self
            .get(&url, "application/vnd.github+json", 1024 * 1024)
            .await?;
        String::from_utf8(b).map_err(|_| DomainError::External("unexpected release data".into()))
    }

    async fn download(&self, url: &str, max_bytes: usize) -> DomainResult<Vec<u8>> {
        self.get(url, "application/octet-stream", max_bytes).await
    }
}
