//! Crate-wide error type.

use thiserror::Error;

pub type Result<T, E = XboardError> = std::result::Result<T, E>;

#[derive(Debug, Error)]
pub enum XboardError {
    #[error("network error: {0}")]
    Network(#[from] reqwest::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("yaml error: {0}")]
    Yaml(#[from] serde_yaml::Error),

    #[error("URL parse error: {0}")]
    Url(#[from] url::ParseError),

    /// Backend returned `{ status: "fail", message }`.
    #[error("api failure ({status_code}): {message}")]
    ApiFailure { status_code: u16, message: String },

    /// 401 / 403 from a `user`-middleware endpoint.
    #[error("unauthorized — re-login required")]
    Unauthorized,

    #[error("kernel not running")]
    KernelNotRunning,

    #[error("kernel start timed out")]
    KernelStartTimeout,

    #[error("kernel binary missing or not executable: {path}")]
    KernelBinaryMissing { path: String },

    /// Kernel control-plane returned an error (non-2xx, malformed body, etc).
    #[error("kernel error: {0}")]
    Kernel(String),

    /// Manifest signature failed ed25519 verification.
    #[error("manifest signature invalid")]
    InvalidSignature,

    #[error("checksum mismatch (expected {expected}, got {actual})")]
    ChecksumMismatch { expected: String, actual: String },

    #[error("not implemented: {0}")]
    NotImplemented(&'static str),

    #[error("config error: {0}")]
    Config(String),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}
