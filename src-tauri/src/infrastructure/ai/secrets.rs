//! API key storage. The key lives only in the OS credential store (Windows Credential Manager)
//! and is read by the backend at request time. It is never logged and never sent to the frontend.

use std::sync::Mutex;

use serde::Serialize;

use crate::domain::validation::invalid;
use crate::error::{DomainError, DomainResult};

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct KeyStatus {
    pub configured: bool,
    /// "credential_manager" | "environment" | "none"
    pub source: &'static str,
    /// Whether this platform can store a key from the UI.
    pub can_store: bool,
}

pub trait SecretStore: Send + Sync {
    fn get(&self) -> DomainResult<Option<String>>;
    fn set(&self, key: &str) -> DomainResult<()>;
    fn clear(&self) -> DomainResult<()>;
    fn status(&self) -> DomainResult<KeyStatus>;
}

pub fn validate_key(key: &str) -> DomainResult<String> {
    let k = key.trim();
    if k.is_empty() || k.len() > 512 || k.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(invalid("that does not look like an API key"));
    }
    Ok(k.to_string())
}

fn env_key() -> Option<String> {
    std::env::var("ANTHROPIC_API_KEY")
        .ok()
        .map(|k| k.trim().to_string())
        .filter(|k| !k.is_empty())
}

#[cfg(windows)]
pub struct OsSecretStore;

#[cfg(windows)]
impl OsSecretStore {
    pub fn new() -> DomainResult<Self> {
        let store = windows_native_keyring_store::Store::new()
            .map_err(|e| DomainError::External(format!("credential store unavailable: {e}")))?;
        keyring_core::set_default_store(store);
        Ok(Self)
    }
    fn entry() -> DomainResult<keyring_core::Entry> {
        keyring_core::Entry::new("orbit", "anthropic-api-key")
            .map_err(|e| DomainError::External(format!("credential store error: {e}")))
    }
}

#[cfg(windows)]
impl SecretStore for OsSecretStore {
    fn get(&self) -> DomainResult<Option<String>> {
        match Self::entry()?.get_password() {
            Ok(k) => Ok(Some(k)),
            Err(keyring_core::Error::NoEntry) => Ok(env_key()),
            Err(e) => Err(DomainError::External(format!(
                "credential store error: {e}"
            ))),
        }
    }
    fn set(&self, key: &str) -> DomainResult<()> {
        let key = validate_key(key)?;
        Self::entry()?
            .set_password(&key)
            .map_err(|e| DomainError::External(format!("credential store error: {e}")))
    }
    fn clear(&self) -> DomainResult<()> {
        match Self::entry()?.delete_credential() {
            Ok(()) | Err(keyring_core::Error::NoEntry) => Ok(()),
            Err(e) => Err(DomainError::External(format!(
                "credential store error: {e}"
            ))),
        }
    }
    fn status(&self) -> DomainResult<KeyStatus> {
        let stored = match Self::entry()?.get_password() {
            Ok(_) => true,
            Err(keyring_core::Error::NoEntry) => false,
            Err(e) => {
                return Err(DomainError::External(format!(
                    "credential store error: {e}"
                )));
            }
        };
        Ok(KeyStatus {
            configured: stored || env_key().is_some(),
            source: if stored {
                "credential_manager"
            } else if env_key().is_some() {
                "environment"
            } else {
                "none"
            },
            can_store: true,
        })
    }
}

/// Non-Windows builds (development, Linux): no secure store, only `ANTHROPIC_API_KEY`.
#[cfg(not(windows))]
pub struct EnvSecretStore;

#[cfg(not(windows))]
impl SecretStore for EnvSecretStore {
    fn get(&self) -> DomainResult<Option<String>> {
        Ok(env_key())
    }
    fn set(&self, _key: &str) -> DomainResult<()> {
        Err(DomainError::External(
            "no secure key storage on this platform; set the ANTHROPIC_API_KEY environment variable".into(),
        ))
    }
    fn clear(&self) -> DomainResult<()> {
        Ok(())
    }
    fn status(&self) -> DomainResult<KeyStatus> {
        let configured = env_key().is_some();
        Ok(KeyStatus {
            configured,
            source: if configured { "environment" } else { "none" },
            can_store: false,
        })
    }
}

/// The store for this platform.
pub fn default_store() -> std::sync::Arc<dyn SecretStore> {
    #[cfg(windows)]
    {
        match OsSecretStore::new() {
            Ok(s) => std::sync::Arc::new(s),
            Err(e) => {
                tracing::warn!("{e}");
                std::sync::Arc::new(MemorySecretStore::default())
            }
        }
    }
    #[cfg(not(windows))]
    {
        std::sync::Arc::new(EnvSecretStore)
    }
}

/// In-memory store (tests, and the fallback if the OS store cannot be opened).
#[derive(Default)]
pub struct MemorySecretStore(Mutex<Option<String>>);

impl SecretStore for MemorySecretStore {
    fn get(&self) -> DomainResult<Option<String>> {
        Ok(self.0.lock().unwrap().clone())
    }
    fn set(&self, key: &str) -> DomainResult<()> {
        *self.0.lock().unwrap() = Some(validate_key(key)?);
        Ok(())
    }
    fn clear(&self) -> DomainResult<()> {
        *self.0.lock().unwrap() = None;
        Ok(())
    }
    fn status(&self) -> DomainResult<KeyStatus> {
        let configured = self.0.lock().unwrap().is_some();
        Ok(KeyStatus {
            configured,
            source: if configured {
                "credential_manager"
            } else {
                "none"
            },
            can_store: true,
        })
    }
}
