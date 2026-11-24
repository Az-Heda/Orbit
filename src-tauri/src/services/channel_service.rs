use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Duration as StdDuration;

use chrono::{DateTime, Duration, Utc};

use super::notifier::Notifier;
use super::timer_service::Clock;
use crate::domain::activity::Actor;
use crate::domain::automation::RenderCtx;
use crate::domain::channels::{
    Channel, ChannelConfig, ChannelInput, ChannelKind, ChannelRow, clip_message,
};
use crate::domain::validation::{invalid, parse_uuid};
use crate::error::{DomainError, DomainResult};
use crate::infrastructure::channels::{ChannelSecrets, ChannelSender, Target};
use crate::infrastructure::database::{ChannelRepository, ChannelWrite};

/// At most this many messages per channel per minute (automation + tests combined).
pub const DELIVERY_RATE_LIMIT: usize = 20;
const SEND_TIMEOUT: StdDuration = StdDuration::from_secs(15);

/// Named notification channels and message delivery.
///
/// Secrets go straight to the credential store and are only read back to send. Delivery from
/// automation is detached (`dispatch`): a slow or failing service never delays the rule engine,
/// failures are written to `activity_log`, and a per-channel rate limit stops a looping rule from
/// flooding a chat.
pub struct ChannelService {
    repo: Arc<dyn ChannelRepository>,
    secrets: Arc<dyn ChannelSecrets>,
    sender: Arc<dyn ChannelSender>,
    notifier: Arc<dyn Notifier>,
    clock: Clock,
    recent: Mutex<HashMap<String, VecDeque<DateTime<Utc>>>>,
    /// When the last `rate_limited` row was written per channel (one per minute is enough).
    limit_logged: Mutex<HashMap<String, DateTime<Utc>>>,
}

impl ChannelService {
    pub fn new(
        repo: Arc<dyn ChannelRepository>,
        secrets: Arc<dyn ChannelSecrets>,
        sender: Arc<dyn ChannelSender>,
        notifier: Arc<dyn Notifier>,
        clock: Clock,
    ) -> Self {
        Self {
            repo,
            secrets,
            sender,
            notifier,
            clock,
            recent: Mutex::new(HashMap::new()),
            limit_logged: Mutex::new(HashMap::new()),
        }
    }

    fn view(&self, row: ChannelRow) -> DomainResult<Channel> {
        let has_secret = matches!(self.secrets.get(&row.id), Ok(Some(_)));
        row.into_channel(has_secret)
    }

    pub async fn list(&self) -> DomainResult<Vec<Channel>> {
        self.repo
            .list()
            .await?
            .into_iter()
            .map(|r| self.view(r))
            .collect()
    }

    pub async fn get(&self, id: &str) -> DomainResult<Channel> {
        let id = parse_uuid(id, "channel id")?;
        let row = self.repo.get(&id).await?.ok_or(DomainError::NotFound {
            entity: "notification_channel",
            id,
        })?;
        self.view(row)
    }

    pub async fn find_by_name(&self, name: &str) -> DomainResult<Option<Channel>> {
        self.repo
            .find_by_name(name)
            .await?
            .map(|r| self.view(r))
            .transpose()
    }

    fn config_json(chat_id: &Option<String>) -> DomainResult<String> {
        serde_json::to_string(&ChannelConfig {
            chat_id: chat_id.clone(),
        })
        .map_err(|e| DomainError::Storage(e.to_string()))
    }

    pub async fn create(&self, input: ChannelInput, actor: Actor) -> DomainResult<Channel> {
        let c = input.clean()?;
        if c.kind.needs_secret() && c.secret.is_none() {
            return Err(invalid(match c.kind {
                ChannelKind::Telegram => "enter the Telegram bot token",
                _ => "enter the Discord webhook URL",
            }));
        }
        let cfg = Self::config_json(&c.chat_id)?;
        let row = self
            .repo
            .create(
                &ChannelWrite {
                    name: &c.name,
                    kind: c.kind.as_str(),
                    config: &cfg,
                    template: &c.template,
                },
                actor,
            )
            .await?;
        if let Some(secret) = &c.secret
            && let Err(e) = self.secrets.set(&row.id, secret)
        {
            // Do not keep a channel that cannot send.
            let _ = self.repo.delete(&row.id, actor).await;
            return Err(e);
        }
        self.view(row)
    }

    pub async fn update(
        &self,
        id: &str,
        input: ChannelInput,
        actor: Actor,
    ) -> DomainResult<Channel> {
        let id = parse_uuid(id, "channel id")?;
        let c = input.clean()?;
        let before = self.repo.get(&id).await?.ok_or(DomainError::NotFound {
            entity: "notification_channel",
            id: id.clone(),
        })?;
        let kind_changed = before.kind != c.kind.as_str();
        // A secret for another kind of channel is useless: require a fresh one.
        if c.kind.needs_secret()
            && c.secret.is_none()
            && (kind_changed || !matches!(self.secrets.get(&id), Ok(Some(_))))
        {
            return Err(invalid(match c.kind {
                ChannelKind::Telegram => "enter the Telegram bot token",
                _ => "enter the Discord webhook URL",
            }));
        }
        let cfg = Self::config_json(&c.chat_id)?;
        let row = self
            .repo
            .update(
                &id,
                &ChannelWrite {
                    name: &c.name,
                    kind: c.kind.as_str(),
                    config: &cfg,
                    template: &c.template,
                },
                actor,
            )
            .await?;
        match (&c.secret, c.kind.needs_secret()) {
            (Some(s), true) => self.secrets.set(&id, s)?,
            (_, false) => self.secrets.delete(&id)?,
            _ => {}
        }
        self.view(row)
    }

    pub async fn delete(&self, id: &str, actor: Actor) -> DomainResult<()> {
        let id = parse_uuid(id, "channel id")?;
        self.repo.delete(&id, actor).await?;
        self.secrets.delete(&id)
    }

    fn allow(&self, channel_id: &str) -> bool {
        let now = (self.clock)();
        let mut map = self.recent.lock().unwrap();
        let q = map.entry(channel_id.to_string()).or_default();
        while q.front().is_some_and(|t| now - *t > Duration::minutes(1)) {
            q.pop_front();
        }
        if q.len() >= DELIVERY_RATE_LIMIT {
            return false;
        }
        q.push_back(now);
        true
    }

    /// Deliver one rendered message. Windows channels show a toast; the others need the stored
    /// secret. Never includes the secret in an error.
    async fn send(&self, ch: &Channel, title: &str, message: &str) -> DomainResult<()> {
        let text = clip_message(message);
        match ch.kind {
            ChannelKind::Windows => {
                self.notifier.notify(title, &text);
                Ok(())
            }
            ChannelKind::Telegram | ChannelKind::Discord => {
                let secret = self.secrets.get(&ch.id)?.ok_or_else(|| {
                    invalid("no token or webhook is saved for this channel; edit it and enter one")
                })?;
                let target = match ch.kind {
                    ChannelKind::Telegram => Target::Telegram {
                        token: secret,
                        chat_id: ch.chat_id.clone().unwrap_or_default(),
                    },
                    _ => Target::Discord {
                        webhook_url: secret,
                    },
                };
                tokio::time::timeout(SEND_TIMEOUT, self.sender.send(&target, &text))
                    .await
                    .map_err(|_| DomainError::External("delivery timed out".into()))?
            }
        }
    }

    /// Send the channel's template filled with sample data and report the outcome to the user.
    pub async fn send_test(&self, id: &str) -> DomainResult<()> {
        let ch = self.get(id).await?;
        if !self.allow(&ch.id) {
            return Err(invalid("too many messages in the last minute; wait a bit"));
        }
        let ctx = RenderCtx {
            title: "Sample item".into(),
            date: (self.clock)().date_naive().to_string(),
            rule_name: "Test rule".into(),
            trigger: "task_completed".into(),
            task_title: "Sample task".into(),
            timer_label: "Sample timer".into(),
            inbox_text: "Sample inbox note".into(),
        };
        self.send(&ch, "Orbit test", &ctx.render(&ch.template))
            .await
    }

    /// Queue a message from an automation rule. Fails only for configuration problems (the channel
    /// is gone, its secret is missing); the delivery itself runs detached so the engine never waits
    /// on the network, and its failures are logged instead of returned.
    pub async fn dispatch(self: &Arc<Self>, channel_id: &str, ctx: RenderCtx) -> DomainResult<()> {
        let ch = self.get(channel_id).await?;
        if ch.kind.needs_secret() && !ch.has_secret {
            return Err(invalid(format!(
                "channel “{}” has no saved token or webhook",
                ch.name
            )));
        }
        let message = ctx.render(&ch.template);
        let me = self.clone();
        tokio::spawn(async move {
            me.deliver_logged(&ch, &ctx.rule_name, &message).await;
        });
        Ok(())
    }

    async fn deliver_logged(&self, ch: &Channel, rule_name: &str, message: &str) {
        if !self.allow(&ch.id) {
            tracing::warn!("channel '{}' rate limited", ch.name);
            let now = (self.clock)();
            let first = {
                let mut m = self.limit_logged.lock().unwrap();
                let due = m
                    .get(&ch.id)
                    .is_none_or(|t| now - *t > Duration::minutes(1));
                if due {
                    m.insert(ch.id.clone(), now);
                }
                due
            };
            if first {
                self.log(
                    &ch.id,
                    "rate_limited",
                    serde_json::json!({ "rule": rule_name, "limit_per_minute": DELIVERY_RATE_LIMIT }),
                )
                .await;
            }
            return;
        }
        let title = if rule_name.is_empty() {
            "Orbit"
        } else {
            rule_name
        };
        if let Err(e) = self.send(ch, title, message).await {
            tracing::warn!("delivery to channel '{}' failed: {e}", ch.name);
            self.log(
                &ch.id,
                "delivery_failed",
                serde_json::json!({ "rule": rule_name, "error": e.to_string() }),
            )
            .await;
        }
    }

    async fn log(&self, id: &str, action: &'static str, detail: serde_json::Value) {
        if let Err(e) = self
            .repo
            .log_event(id, action, Actor::Automation, detail)
            .await
        {
            tracing::warn!("cannot log channel event: {e}");
        }
    }
}
