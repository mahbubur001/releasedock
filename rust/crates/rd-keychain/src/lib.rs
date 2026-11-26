//! Credentials in the operating system's store. Ported from `Keychain/KeychainService.swift`.
//!
//! On macOS items are generic passwords with the same service and account as the Swift
//! app, so existing SVN passwords are found (macOS asks once before another app reads
//! them). Windows uses the Credential Manager and Linux the Secret Service (GNOME
//! Keyring, KWallet), which must be running.

use std::collections::HashMap;
use std::sync::Mutex;

#[derive(Debug, thiserror::Error)]
pub enum KeychainError {
    #[error("Keychain error: {0}")]
    Store(String),
    #[error("Keychain item has invalid data")]
    InvalidData,
    #[error("No credential store is available. On Linux, start GNOME Keyring or KWallet.")]
    Unavailable,
}

/// A credential identity. `service` groups credentials by kind; `account` is the
/// user-visible identifier (an SVN username or a token label).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct KeychainItem {
    pub service: String,
    pub account: String,
}

impl KeychainItem {
    pub fn new(service: impl Into<String>, account: impl Into<String>) -> Self {
        Self {
            service: service.into(),
            account: account.into(),
        }
    }

    /// SVN password for a repository host (e.g. `plugins.svn.wordpress.org`) and username.
    pub fn svn(host: &str, username: &str) -> Self {
        Self::new(format!("com.releasedock.svn.{host}"), username)
    }

    /// GitHub token with Contents: read and write, used to create releases of products.
    pub fn github_release_token() -> Self {
        Self::new("com.releasedock.github", "release-token")
    }
}

pub trait Keychain: Send + Sync {
    fn password(&self, item: &KeychainItem) -> Result<Option<String>, KeychainError>;
    fn set_password(&self, password: &str, item: &KeychainItem) -> Result<(), KeychainError>;
    fn delete_password(&self, item: &KeychainItem) -> Result<(), KeychainError>;
}

/// The platform credential store.
#[derive(Clone, Copy, Debug, Default)]
pub struct NativeKeychain;

fn store_error(error: keyring::Error) -> KeychainError {
    match error {
        keyring::Error::BadEncoding(_) => KeychainError::InvalidData,
        keyring::Error::NoStorageAccess(_) | keyring::Error::PlatformFailure(_) => {
            KeychainError::Store(error.to_string())
        }
        other => KeychainError::Store(other.to_string()),
    }
}

impl Keychain for NativeKeychain {
    fn password(&self, item: &KeychainItem) -> Result<Option<String>, KeychainError> {
        let entry = keyring::Entry::new(&item.service, &item.account).map_err(store_error)?;
        match entry.get_password() {
            Ok(password) => Ok(Some(password)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(store_error(error)),
        }
    }

    fn set_password(&self, password: &str, item: &KeychainItem) -> Result<(), KeychainError> {
        let entry = keyring::Entry::new(&item.service, &item.account).map_err(store_error)?;
        entry.set_password(password).map_err(store_error)
    }

    fn delete_password(&self, item: &KeychainItem) -> Result<(), KeychainError> {
        let entry = keyring::Entry::new(&item.service, &item.account).map_err(store_error)?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(store_error(error)),
        }
    }
}

/// In-memory credentials for tests and previews.
#[derive(Debug, Default)]
pub struct InMemoryKeychain {
    items: Mutex<HashMap<KeychainItem, String>>,
}

impl InMemoryKeychain {
    pub fn new() -> Self {
        Self::default()
    }

    fn items(&self) -> std::sync::MutexGuard<'_, HashMap<KeychainItem, String>> {
        self.items.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl Keychain for InMemoryKeychain {
    fn password(&self, item: &KeychainItem) -> Result<Option<String>, KeychainError> {
        Ok(self.items().get(item).cloned())
    }

    fn set_password(&self, password: &str, item: &KeychainItem) -> Result<(), KeychainError> {
        self.items().insert(item.clone(), password.to_owned());
        Ok(())
    }

    fn delete_password(&self, item: &KeychainItem) -> Result<(), KeychainError> {
        self.items().remove(item);
        Ok(())
    }
}
