use serde::Serialize;
use xboard_core::XboardError;

/// Stable, JSON-serialisable error returned to the JS side. We deliberately
/// flatten every kind into `{ kind, message, status }` so the UI layer can
/// switch on `kind` without knowing Rust error variants.
#[derive(Debug, Serialize)]
pub struct CommandError {
    pub kind: &'static str,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<u16>,
}

impl CommandError {
    pub fn new(kind: &'static str, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            status: None,
        }
    }

    pub fn with_status(mut self, status: u16) -> Self {
        self.status = Some(status);
        self
    }
}

impl From<XboardError> for CommandError {
    fn from(err: XboardError) -> Self {
        match err {
            XboardError::Network(e) => CommandError::new("network", e.to_string()),
            XboardError::Io(e) => CommandError::new("io", e.to_string()),
            XboardError::Json(e) => CommandError::new("json", e.to_string()),
            XboardError::Yaml(e) => CommandError::new("yaml", e.to_string()),
            XboardError::Url(e) => CommandError::new("url", e.to_string()),
            XboardError::ApiFailure {
                status_code,
                message,
            } => CommandError::new("api", message).with_status(status_code),
            XboardError::Unauthorized => {
                CommandError::new("unauthorized", "未登录或登录已失效").with_status(401)
            }
            XboardError::SubscriptionUnavailable { status } => CommandError::new(
                "subscription_unavailable",
                "订阅当前不可用——套餐可能已到期或流量耗尽，请续费后重试",
            )
            .with_status(status),
            XboardError::KernelNotRunning => CommandError::new("kernel_not_running", "内核未运行"),
            XboardError::KernelStartTimeout => {
                CommandError::new("kernel_start_timeout", "内核启动超时")
            }
            XboardError::KernelBinaryMissing { path } => CommandError::new(
                "kernel_binary_missing",
                format!("内核可执行文件不存在: {path}"),
            ),
            XboardError::Kernel(msg) => CommandError::new("kernel", msg),
            XboardError::InvalidSignature => {
                CommandError::new("invalid_signature", "更新清单签名校验失败")
            }
            XboardError::ChecksumMismatch { expected, actual } => CommandError::new(
                "checksum_mismatch",
                format!("SHA-256 校验失败 (expected {expected}, got {actual})"),
            ),
            XboardError::NotImplemented(s) => CommandError::new("not_implemented", s),
            XboardError::Config(s) => CommandError::new("config", s),
            XboardError::Other(e) => CommandError::new("other", e.to_string()),
        }
    }
}

impl From<anyhow::Error> for CommandError {
    fn from(err: anyhow::Error) -> Self {
        CommandError::new("other", err.to_string())
    }
}

pub type CommandResult<T> = std::result::Result<T, CommandError>;
