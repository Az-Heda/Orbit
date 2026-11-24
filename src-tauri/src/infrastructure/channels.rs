//! Notification-channel secrets and delivery. A Telegram bot token or a Discord webhook URL grants
//! the right to post as the user, so it is stored like the API key (Windows Credential Manager),
//! never in the database, and never logged, returned to the frontend or shown to the AI. Error
//! text built here never contains the token or the URL.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use async_trait::async_trait;
use serde_json::json;

use crate::error::{DomainError, DomainResult};

pub trait ChannelSecrets: Send + Sync {
    fn get(&self, channel_id: &str) -> DomainResult<Option<String>>;
    fn set(&self, channel_id: &str, secret: &str) -> DomainResult<()>;
    fn delete(&self, channel_id: &str) -> DomainResult<()>;
}

/// In-memory store: tests, and non-Windows builds (channels there need their secret re-entered
/// after a restart).
#[derive(Default)]
pub struct MemoryChannelSecrets(Mutex<HashMap<String, String>>);

impl ChannelSecrets for MemoryChannelSecrets {
    fn get(&self, id: &str) -> DomainResult<Option<String>> {
        Ok(self.0.lock().unwrap().get(id).cloned())
    }
    fn set(&self, id: &str, secret: &str) -> DomainResult<()> {
        self.0.lock().unwrap().insert(id.into(), secret.into());
        Ok(())
    }
    fn delete(&self, id: &str) -> DomainResult<()> {
        self.0.lock().unwrap().remove(id);
        Ok(())
    }
}

#[cfg(windows)]
pub struct OsChannelSecrets;

#[cfg(windows)]
impl OsChannelSecrets {
    fn entry(id: &str) -> DomainResult<keyring_core::Entry> {
        keyring_core::Entry::new("orbit", &format!("notification-channel:{id}"))
            .map_err(|e| DomainError::External(format!("credential store error: {e}")))
    }
}

#[cfg(windows)]
impl ChannelSecrets for OsChannelSecrets {
    fn get(&self, id: &str) -> DomainResult<Option<String>> {
        match Self::entry(id)?.get_password() {
            Ok(v) => Ok(Some(v)),
            Err(keyring_core::Error::NoEntry) => Ok(None),
            Err(e) => Err(DomainError::External(format!(
                "credential store error: {e}"
            ))),
        }
    }
    fn set(&self, id: &str, secret: &str) -> DomainResult<()> {
        Self::entry(id)?
            .set_password(secret)
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
pub fn default_channel_secrets() -> std::sync::Arc<dyn ChannelSecrets> {
    #[cfg(windows)]
    {
        std::sync::Arc::new(OsChannelSecrets)
    }
    #[cfg(not(windows))]
    {
        std::sync::Arc::new(MemoryChannelSecrets::default())
    }
}

/// Where a remote message goes, with its secret. Deliberately no `Debug`/`Serialize`.
pub enum Target {
    Telegram { token: String, chat_id: String },
    Discord { webhook_url: String },
}

#[async_trait]
pub trait ChannelSender: Send + Sync {
    async fn send(&self, target: &Target, text: &str) -> DomainResult<()>;
}

const TELEGRAM_API: &str = "https://api.telegram.org";

pub struct HttpChannelSender {
    http: reqwest::Client,
    telegram_base: String,
}

impl HttpChannelSender {
    pub fn new() -> Self {
        Self::with_telegram_base(TELEGRAM_API)
    }

    /// Tests point Telegram at a local server.
    pub fn with_telegram_base(base: &str) -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            // A webhook never needs a redirect, and following one could leak the payload.
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap_or_default();
        Self {
            http,
            telegram_base: base.trim_end_matches('/').to_string(),
        }
    }
}

impl Default for HttpChannelSender {
    fn default() -> Self {
        Self::new()
    }
}

/// A short, single-line excerpt of the service's reply, for the error text.
fn excerpt(body: &str) -> String {
    let one: String = body.split_whitespace().collect::<Vec<_>>().join(" ");
    one.chars().take(120).collect()
}

#[async_trait]
impl ChannelSender for HttpChannelSender {
    async fn send(&self, target: &Target, text: &str) -> DomainResult<()> {
        let (service, req) = match target {
            Target::Telegram { token, chat_id } => (
                "Telegram",
                self.http
                    .post(format!("{}/bot{token}/sendMessage", self.telegram_base))
                    .json(&json!({
                        "chat_id": chat_id,
                        "text": text,
                        "disable_web_page_preview": true,
                    })),
            ),
            Target::Discord { webhook_url } => (
                "Discord",
                self.http.post(webhook_url).json(&json!({
                    "content": text,
                    // Never let a template ping @everyone or a role.
                    "allowed_mentions": { "parse": [] },
                })),
            ),
        };
        // `without_url`: the URL carries the token.
        let resp = req
            .send()
            .await
            .map_err(|e| DomainError::External(format!("{service}: {}", e.without_url())))?;
        let status = resp.status();
        if status.is_success() {
            return Ok(());
        }
        let body = resp.text().await.unwrap_or_default();
        Err(DomainError::External(format!(
            "{service} rejected the message (HTTP {}): {}",
            status.as_u16(),
            excerpt(&body)
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    /// One-shot HTTP server: returns (base url, receiver of the raw request).
    async fn fake(status: &str, body: &str) -> (String, tokio::sync::oneshot::Receiver<String>) {
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = l.local_addr().unwrap();
        let (tx, rx) = tokio::sync::oneshot::channel();
        let reply = format!(
            "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        );
        tokio::spawn(async move {
            let (mut s, _) = l.accept().await.unwrap();
            let mut buf = vec![0u8; 8192];
            let mut got = String::new();
            // Read until the JSON body (ends with '}') has arrived.
            while !got.trim_end().ends_with('}') {
                let n = s.read(&mut buf).await.unwrap();
                if n == 0 {
                    break;
                }
                got.push_str(&String::from_utf8_lossy(&buf[..n]));
            }
            let _ = s.write_all(reply.as_bytes()).await;
            let _ = tx.send(got);
        });
        (format!("http://{addr}"), rx)
    }

    #[tokio::test]
    async fn telegram_request_shape() {
        let (base, rx) = fake("200 OK", r#"{"ok":true}"#).await;
        HttpChannelSender::with_telegram_base(&base)
            .send(
                &Target::Telegram {
                    token: "123:abc".into(),
                    chat_id: "42".into(),
                },
                "hello",
            )
            .await
            .unwrap();
        let req = rx.await.unwrap();
        assert!(req.starts_with("POST /bot123:abc/sendMessage "), "{req}");
        assert!(req.contains(r#""chat_id":"42""#) && req.contains(r#""text":"hello""#));
    }

    #[tokio::test]
    async fn failure_text_never_contains_the_token() {
        let (base, _rx) = fake(
            "401 Unauthorized",
            r#"{"ok":false,"description":"Unauthorized"}"#,
        )
        .await;
        let e = HttpChannelSender::with_telegram_base(&base)
            .send(
                &Target::Telegram {
                    token: "123:TOPSECRET".into(),
                    chat_id: "42".into(),
                },
                "x",
            )
            .await
            .unwrap_err()
            .to_string();
        assert!(e.contains("HTTP 401") && e.contains("Unauthorized"), "{e}");
        assert!(!e.contains("TOPSECRET"), "{e}");

        // Connection failure: the URL (with the token) must not leak either.
        let e = HttpChannelSender::with_telegram_base("http://127.0.0.1:1")
            .send(
                &Target::Telegram {
                    token: "123:TOPSECRET".into(),
                    chat_id: "42".into(),
                },
                "x",
            )
            .await
            .unwrap_err()
            .to_string();
        assert!(!e.contains("TOPSECRET"), "{e}");
    }

    #[test]
    fn memory_store_round_trip() {
        let s = MemoryChannelSecrets::default();
        assert_eq!(s.get("a").unwrap(), None);
        s.set("a", "x").unwrap();
        assert_eq!(s.get("a").unwrap().as_deref(), Some("x"));
        s.delete("a").unwrap();
        assert_eq!(s.get("a").unwrap(), None);
    }
}
