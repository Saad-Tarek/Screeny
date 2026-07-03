//! Secret storage behind a trait so the delivery pipeline is testable
//! without touching the OS keychain.

use std::collections::HashMap;
use std::sync::Mutex;

use crate::error::{CoreError, Result};

pub const SMTP_PASSWORD: &str = "smtp_password";
pub const LLM_API_KEY: &str = "llm_api_key";

const KEYRING_SERVICE: &str = "screeny";

pub trait SecretStore: Send + Sync {
    fn get(&self, key: &str) -> Result<Option<String>>;
    fn set(&self, key: &str, value: &str) -> Result<()>;
    fn delete(&self, key: &str) -> Result<()>;

    fn is_set(&self, key: &str) -> bool {
        matches!(self.get(key), Ok(Some(_)))
    }
}

/// OS-native storage: Windows Credential Manager, macOS Keychain, or the
/// Linux Secret Service (falls back with a clear error where absent).
pub struct KeyringStore;

impl KeyringStore {
    fn entry(key: &str) -> Result<keyring::Entry> {
        keyring::Entry::new(KEYRING_SERVICE, key).map_err(|e| CoreError::Secret(e.to_string()))
    }
}

impl SecretStore for KeyringStore {
    fn get(&self, key: &str) -> Result<Option<String>> {
        match Self::entry(key)?.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(CoreError::Secret(e.to_string())),
        }
    }

    fn set(&self, key: &str, value: &str) -> Result<()> {
        Self::entry(key)?
            .set_password(value)
            .map_err(|e| CoreError::Secret(e.to_string()))
    }

    fn delete(&self, key: &str) -> Result<()> {
        match Self::entry(key)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(CoreError::Secret(e.to_string())),
        }
    }
}

/// In-memory store for tests.
#[derive(Default)]
pub struct MemoryStore {
    values: Mutex<HashMap<String, String>>,
}

impl SecretStore for MemoryStore {
    fn get(&self, key: &str) -> Result<Option<String>> {
        Ok(self.values.lock().expect("poisoned").get(key).cloned())
    }

    fn set(&self, key: &str, value: &str) -> Result<()> {
        self.values
            .lock()
            .expect("poisoned")
            .insert(key.to_string(), value.to_string());
        Ok(())
    }

    fn delete(&self, key: &str) -> Result<()> {
        self.values.lock().expect("poisoned").remove(key);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_store_round_trip() {
        let store = MemoryStore::default();
        assert!(!store.is_set(SMTP_PASSWORD));
        store.set(SMTP_PASSWORD, "hunter2").unwrap();
        assert_eq!(
            store.get(SMTP_PASSWORD).unwrap().as_deref(),
            Some("hunter2")
        );
        assert!(store.is_set(SMTP_PASSWORD));
        store.delete(SMTP_PASSWORD).unwrap();
        assert!(!store.is_set(SMTP_PASSWORD));
    }
}
