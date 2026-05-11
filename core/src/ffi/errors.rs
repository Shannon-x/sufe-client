//! UniFFI error types.
//!
//! `XboardError` carries `reqwest::Error`, `anyhow::Error` etc. — none of
//! which can cross a UniFFI boundary. We flatten every variant into
//! `FfiError`, preserving the original `Display` output as a plain `String`
//! so the message survives the round-trip into Kotlin / Swift.
//!
//! Mapping is one-way (`XboardError → FfiError`); the FFI surface always
//! produces `FfiError` directly without going back through `XboardError`.

use thiserror::Error;

use crate::error::XboardError;

#[derive(Debug, Error)]
pub enum FfiError {
    #[error("network error: {0}")]
    Network(String),

    #[error("io error: {0}")]
    Io(String),

    #[error("json error: {0}")]
    Json(String),

    #[error("yaml error: {0}")]
    Yaml(String),

    #[error("URL parse error: {0}")]
    Url(String),

    #[error("api failure ({status_code}): {message}")]
    ApiFailure { status_code: u16, message: String },

    #[error("unauthorized — re-login required")]
    Unauthorized,

    #[error("kernel not running")]
    KernelNotRunning,

    #[error("kernel start timed out")]
    KernelStartTimeout,

    #[error("kernel binary missing or not executable: {path}")]
    KernelBinaryMissing { path: String },

    #[error("kernel error: {0}")]
    Kernel(String),

    #[error("manifest signature invalid")]
    InvalidSignature,

    #[error("checksum mismatch (expected {expected}, got {actual})")]
    ChecksumMismatch { expected: String, actual: String },

    #[error("not implemented: {0}")]
    NotImplemented(String),

    #[error("config error: {0}")]
    Config(String),

    #[error("{0}")]
    Other(String),
}

impl From<XboardError> for FfiError {
    fn from(err: XboardError) -> Self {
        match err {
            XboardError::Network(e) => FfiError::Network(e.to_string()),
            XboardError::Io(e) => FfiError::Io(e.to_string()),
            XboardError::Json(e) => FfiError::Json(e.to_string()),
            XboardError::Yaml(e) => FfiError::Yaml(e.to_string()),
            XboardError::Url(e) => FfiError::Url(e.to_string()),
            XboardError::ApiFailure {
                status_code,
                message,
            } => FfiError::ApiFailure {
                status_code,
                message,
            },
            XboardError::Unauthorized => FfiError::Unauthorized,
            XboardError::KernelNotRunning => FfiError::KernelNotRunning,
            XboardError::KernelStartTimeout => FfiError::KernelStartTimeout,
            XboardError::KernelBinaryMissing { path } => FfiError::KernelBinaryMissing { path },
            XboardError::Kernel(s) => FfiError::Kernel(s),
            XboardError::InvalidSignature => FfiError::InvalidSignature,
            XboardError::ChecksumMismatch { expected, actual } => {
                FfiError::ChecksumMismatch { expected, actual }
            }
            XboardError::NotImplemented(s) => FfiError::NotImplemented(s.to_string()),
            XboardError::Config(s) => FfiError::Config(s),
            XboardError::Other(e) => FfiError::Other(e.to_string()),
        }
    }
}

/// Errors a host-side `SecureStore` callback may raise. Translated back into
/// `XboardError::Config(...)` when surfaced inside core code via
/// [`super::secure::CallbackSecureStore`] — the rest of the crate keeps a
/// single error type.
///
/// Declared as a struct-style variant (`Backend { reason: String }`) rather
/// than a tuple variant because UniFFI's `[Error] interface` rich-error mode
/// matches by named field and would reject `Backend(String)`. The field is
/// called `reason` (not `message`) because UniFFI-generated Kotlin classes
/// extend `Throwable`; a `val message: String` would shadow `Throwable.message`
/// and Kotlin rejects the override with a compile error.
#[derive(Debug, Error)]
pub enum StorageError {
    #[error("secure store backend error: {reason}")]
    Backend { reason: String },
}

impl From<StorageError> for XboardError {
    fn from(err: StorageError) -> Self {
        match err {
            StorageError::Backend { reason } => {
                XboardError::Config(format!("secure store: {reason}"))
            }
        }
    }
}

/// Errors a host-side `TunDelegate` callback may raise during `establish_tun`.
/// `Denied` means the user dismissed the OS prompt (Android `prepare()` /
/// iOS NEVPNManager auth) — the kernel manager downgrades to SystemProxy on
/// platforms where that's available, otherwise propagates as a hard error.
///
/// Same `reason`-not-`message` rationale as `StorageError` above (Throwable
/// shadowing).
#[derive(Debug, Error)]
pub enum TunnelError {
    #[error("tun delegate denied: {reason}")]
    Denied { reason: String },
    #[error("tun delegate backend error: {reason}")]
    Backend { reason: String },
}

impl From<TunnelError> for XboardError {
    fn from(err: TunnelError) -> Self {
        XboardError::Kernel(format!("tun delegate: {err}"))
    }
}
