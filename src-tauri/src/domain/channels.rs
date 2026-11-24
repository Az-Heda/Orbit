//! Notification channels: named destinations (Windows toast, Telegram, Discord) with a message
//! template. Secrets (bot token, webhook URL) are not part of these types: they are written to the
//! credential store and never read back into anything the frontend or the AI can see.

use serde::{Deserialize, Serialize};

use super::validation::{clean_text, invalid};
use crate::error::DomainResult;

pub const MAX_TEMPLATE: usize = 2000;
/// Telegram allows 4096 characters and Discord 2000; stay under the smaller one.
pub const MAX_MESSAGE: usize = 1900;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChannelKind {
    Windows,
    Telegram,
    Discord,
}

impl ChannelKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ChannelKind::Windows => "windows",
            ChannelKind::Telegram => "telegram",
            ChannelKind::Discord => "discord",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "windows" => Some(ChannelKind::Windows),
            "telegram" => Some(ChannelKind::Telegram),
            "discord" => Some(ChannelKind::Discord),
            _ => None,
        }
    }

    pub fn needs_secret(self) -> bool {
        !matches!(self, ChannelKind::Windows)
    }
}

/// What the frontend and the AI may see of a channel. No secret, only whether one is stored.
#[derive(Debug, Clone, Serialize)]
pub struct Channel {
    pub id: String,
    pub name: String,
    pub kind: ChannelKind,
    /// Telegram only (not a secret).
    pub chat_id: Option<String>,
    pub template: String,
    pub has_secret: bool,
    pub created_at: String,
    pub updated_at: String,
}

/// Raw database row.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ChannelRow {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub config: String,
    pub template: String,
    pub created_at: String,
    pub updated_at: String,
}

pub const CHANNEL_COLS: &str = "id, name, kind, config, template, created_at, updated_at";

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct ChannelConfig {
    #[serde(default)]
    pub chat_id: Option<String>,
}

impl ChannelRow {
    pub fn into_channel(self, has_secret: bool) -> DomainResult<Channel> {
        let corrupt =
            || crate::error::DomainError::Storage(format!("corrupt channel: {}", self.id));
        let kind = ChannelKind::parse(&self.kind).ok_or_else(corrupt)?;
        let cfg: ChannelConfig = serde_json::from_str(&self.config).map_err(|_| corrupt())?;
        Ok(Channel {
            id: self.id,
            name: self.name,
            kind,
            chat_id: cfg.chat_id,
            template: self.template,
            has_secret,
            created_at: self.created_at,
            updated_at: self.updated_at,
        })
    }
}

/// Create/update payload. `secret` is the bot token (Telegram) or webhook URL (Discord); on update
/// `None` keeps the stored one.
#[derive(Deserialize)]
pub struct ChannelInput {
    pub name: String,
    pub kind: ChannelKind,
    pub template: String,
    #[serde(default)]
    pub chat_id: Option<String>,
    #[serde(default)]
    pub secret: Option<String>,
}

// Hand-written so a stray `{:?}` can never print the token.
impl std::fmt::Debug for ChannelInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChannelInput")
            .field("name", &self.name)
            .field("kind", &self.kind)
            .field("secret", &self.secret.as_ref().map(|_| "<redacted>"))
            .finish_non_exhaustive()
    }
}

/// Validated, normalised channel fields.
pub struct CleanChannel {
    pub name: String,
    pub kind: ChannelKind,
    pub template: String,
    pub chat_id: Option<String>,
    pub secret: Option<String>,
}

impl ChannelInput {
    pub fn clean(&self) -> DomainResult<CleanChannel> {
        let name = clean_text(&self.name, "channel name", 80)?;
        let template = self.template.trim().to_string();
        if template.is_empty() {
            return Err(invalid("the message template cannot be empty"));
        }
        if template.chars().count() > MAX_TEMPLATE {
            return Err(invalid(format!(
                "the message template is longer than {MAX_TEMPLATE} characters"
            )));
        }
        let secret = self
            .secret
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        let mut chat_id = None;
        match self.kind {
            ChannelKind::Windows => {}
            ChannelKind::Telegram => {
                let id = self.chat_id.as_deref().unwrap_or("").trim();
                validate_chat_id(id)?;
                chat_id = Some(id.to_string());
                if let Some(t) = &secret {
                    validate_bot_token(t)?;
                }
            }
            ChannelKind::Discord => {
                if let Some(u) = &secret {
                    validate_webhook_url(u)?;
                }
            }
        }
        Ok(CleanChannel {
            name,
            kind: self.kind,
            template,
            chat_id,
            secret,
        })
    }
}

fn validate_chat_id(id: &str) -> DomainResult<()> {
    let numeric = id
        .strip_prefix('-')
        .unwrap_or(id)
        .chars()
        .all(|c| c.is_ascii_digit());
    let handle = id.strip_prefix('@').is_some_and(|h| {
        (4..=64).contains(&h.len()) && h.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    });
    if id.is_empty() || !((numeric && id != "-") || handle) {
        return Err(invalid(
            "the Telegram chat id must be a number (e.g. 123456789) or @channelname",
        ));
    }
    Ok(())
}

/// `<digits>:<token>`; the value is inserted into a URL path, so keep it to URL-safe characters.
fn validate_bot_token(t: &str) -> DomainResult<()> {
    let ok = t.split_once(':').is_some_and(|(id, tok)| {
        !id.is_empty()
            && id.chars().all(|c| c.is_ascii_digit())
            && !tok.is_empty()
            && tok
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    });
    if !ok {
        // Never echo the value back.
        return Err(invalid(
            "the Telegram bot token looks wrong (expected 123456:ABC-def from @BotFather)",
        ));
    }
    Ok(())
}

/// Only Discord's own webhook endpoints over https, so a channel can never be pointed at an
/// arbitrary (e.g. local network) address.
fn validate_webhook_url(u: &str) -> DomainResult<()> {
    let bad = || {
        invalid(
            "the Discord webhook URL must look like https://discord.com/api/webhooks/<id>/<token>",
        )
    };
    let rest = u.strip_prefix("https://").ok_or_else(bad)?;
    let (host, path) = rest.split_once('/').ok_or_else(bad)?;
    let host_ok = matches!(
        host,
        "discord.com" | "discordapp.com" | "canary.discord.com" | "ptb.discord.com"
    );
    let parts: Vec<&str> = path.split('/').collect();
    let path_ok = parts.len() == 4
        && parts[0] == "api"
        && parts[1] == "webhooks"
        && !parts[2].is_empty()
        && parts[2].chars().all(|c| c.is_ascii_digit())
        && !parts[3].is_empty()
        && parts[3]
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    if host_ok && path_ok {
        Ok(())
    } else {
        Err(bad())
    }
}

/// Cut to `MAX_MESSAGE` characters (on a char boundary).
pub fn clip_message(s: &str) -> String {
    if s.chars().count() <= MAX_MESSAGE {
        return s.to_string();
    }
    let mut out: String = s.chars().take(MAX_MESSAGE - 1).collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(kind: ChannelKind) -> ChannelInput {
        ChannelInput {
            name: " Phone ".into(),
            kind,
            template: "{{task.title}} done".into(),
            chat_id: None,
            secret: None,
        }
    }

    #[test]
    fn telegram_needs_chat_id_and_a_sane_token() {
        let mut i = input(ChannelKind::Telegram);
        assert!(i.clean().is_err(), "chat id required");
        i.chat_id = Some("-1001234".into());
        assert_eq!(i.clean().unwrap().name, "Phone");
        i.chat_id = Some("@my_chan".into());
        assert!(i.clean().is_ok());
        i.chat_id = Some("12 34".into());
        assert!(i.clean().is_err());
        i.chat_id = Some("1".into());
        i.secret = Some("123:abc/../evil".into());
        let e = i.clean().err().unwrap().to_string();
        assert!(!e.contains("evil"), "error must not echo the token");
        i.secret = Some("123456:AA-bb_cc".into());
        assert!(i.clean().is_ok());
    }

    #[test]
    fn discord_webhook_is_pinned_to_discord() {
        let mut i = input(ChannelKind::Discord);
        for ok in [
            "https://discord.com/api/webhooks/123/abc-DEF_1",
            "https://ptb.discord.com/api/webhooks/1/x",
        ] {
            i.secret = Some(ok.into());
            assert!(i.clean().is_ok(), "{ok}");
        }
        for bad in [
            "http://discord.com/api/webhooks/123/abc",
            "https://evil.example/api/webhooks/123/abc",
            "https://discord.com.evil.example/api/webhooks/123/abc",
            "https://discord.com/api/webhooks/123/abc?x=1",
            "https://discord.com/api/webhooks/abc/abc",
            "https://127.0.0.1/api/webhooks/123/abc",
            "https://discord.com@evil.example/api/webhooks/123/abc",
        ] {
            i.secret = Some(bad.into());
            assert!(i.clean().is_err(), "{bad}");
        }
    }

    #[test]
    fn template_and_debug_hygiene() {
        let mut i = input(ChannelKind::Windows);
        i.template = "   ".into();
        assert!(i.clean().is_err());
        i.template = "x".repeat(MAX_TEMPLATE + 1);
        assert!(i.clean().is_err());
        i.kind = ChannelKind::Discord;
        i.secret = Some("https://discord.com/api/webhooks/1/SECRETTOKEN".into());
        assert!(!format!("{i:?}").contains("SECRETTOKEN"));
        assert_eq!(clip_message(&"é".repeat(5000)).chars().count(), MAX_MESSAGE);
    }
}
