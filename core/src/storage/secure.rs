//! Trait abstracting OS keychains.
//!
//! Implementations:
//! - Desktop (Win/Mac/Linux): [`KeyringStore`], a thin wrapper around the
//!   `keyring` crate using the per-OS native backend (Keychain /
//!   Credential Vault / Secret Service via `org.freedesktop.secrets`).
//! - Android: JNI bridge to `AndroidKeyStore` (lands with Android UI).
//!
//! `keyring`'s "no entry" sentinel is *not* an error to us — callers should
//! treat a missing key as `Ok(None)`. We translate that case here so the
//! `SecureStore` trait surface stays simple.

use crate::error::{Result, XboardError};

pub trait SecureStore: Send + Sync + std::fmt::Debug {
    fn put(&self, key: &str, value: &str) -> Result<()>;
    fn get(&self, key: &str) -> Result<Option<String>>;
    fn delete(&self, key: &str) -> Result<()>;
}

/// Desktop OS-keychain-backed [`SecureStore`].
///
/// `service` is the global namespace (Keychain "service" / Credential Vault
/// target / Secret Service collection attribute). Callers should supply the
/// full reverse-DNS bundle id (e.g. `com.xboard.client`) so multiple
/// keys-per-account live next to each other and a single `delete` doesn't
/// blow up unrelated app credentials.
#[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
#[derive(Debug, Clone)]
pub struct KeyringStore {
    service: String,
}

#[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
impl KeyringStore {
    pub fn new(service: impl Into<String>) -> Self {
        Self {
            service: service.into(),
        }
    }
}

#[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
impl SecureStore for KeyringStore {
    fn put(&self, key: &str, value: &str) -> Result<()> {
        keyring::Entry::new(&self.service, key)
            .map_err(map_keyring_err)?
            .set_password(value)
            .map_err(map_keyring_err)
    }

    fn get(&self, key: &str) -> Result<Option<String>> {
        let entry = keyring::Entry::new(&self.service, key).map_err(map_keyring_err)?;
        match entry.get_password() {
            Ok(p) => Ok(Some(p)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(map_keyring_err(e)),
        }
    }

    fn delete(&self, key: &str) -> Result<()> {
        let entry = keyring::Entry::new(&self.service, key).map_err(map_keyring_err)?;
        match entry.delete_credential() {
            Ok(()) => Ok(()),
            // "Already gone" is the desired post-condition of `delete`.
            Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(map_keyring_err(e)),
        }
    }
}

#[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
fn map_keyring_err(e: keyring::Error) -> XboardError {
    XboardError::Config(format!("keyring: {e}"))
}
