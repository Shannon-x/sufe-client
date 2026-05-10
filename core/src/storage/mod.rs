//! Local persistence (SQLite + secure key storage).

pub mod db;
pub mod secure;

pub use secure::SecureStore;

#[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
pub use secure::KeyringStore;
