//! Adapter that bridges UniFFI's `callback interface SecureStore` to the
//! crate-local `crate::storage::SecureStore` trait — so the rest of the
//! codebase keeps using the trait without ever caring whether the backing
//! implementation is an OS keychain (desktop), `EncryptedSharedPreferences`
//! (Android) or `Security.framework` (iOS).
//!
//! Errors raised by the host implementation come back as
//! `super::errors::StorageError` and are translated into `XboardError::Config`
//! via the `From` impl so they slot straight into `Result<T, XboardError>`.

use std::sync::Arc;

use super::errors::StorageError;
use crate::error::Result;
use crate::storage::SecureStore as CoreSecureStore;

/// Trait UniFFI generates for the `callback interface SecureStore` in the
/// UDL. Defined locally so the scaffolding macro finds it at the crate root
/// (re-exported via `crate::SecureStore` once `lib.rs` is wired up).
pub trait SecureStore: Send + Sync + std::fmt::Debug {
    fn put(&self, key: String, value: String) -> std::result::Result<(), StorageError>;
    fn get(&self, key: String) -> std::result::Result<Option<String>, StorageError>;
    fn delete(&self, key: String) -> std::result::Result<(), StorageError>;
}

/// Wraps an `Arc<dyn SecureStore>` (the UniFFI callback object) and exposes
/// the crate-local `CoreSecureStore` trait so legacy Rust code keeps working
/// unchanged. All errors flow through `XboardError::Config`.
#[derive(Debug)]
pub(crate) struct CallbackSecureStore {
    inner: Arc<dyn SecureStore>,
}

impl CallbackSecureStore {
    pub fn new(inner: Arc<dyn SecureStore>) -> Self {
        Self { inner }
    }
}

impl CoreSecureStore for CallbackSecureStore {
    fn put(&self, key: &str, value: &str) -> Result<()> {
        self.inner
            .put(key.to_string(), value.to_string())
            .map_err(Into::into)
    }

    fn get(&self, key: &str) -> Result<Option<String>> {
        self.inner.get(key.to_string()).map_err(Into::into)
    }

    fn delete(&self, key: &str) -> Result<()> {
        self.inner.delete(key.to_string()).map_err(Into::into)
    }
}
