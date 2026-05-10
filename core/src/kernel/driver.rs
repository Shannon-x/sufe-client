//! The single trait every kernel implementation satisfies.

use async_trait::async_trait;
use futures::stream::BoxStream;
use serde::{Deserialize, Serialize};

use crate::error::Result;

/// Identifier for a kernel implementation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum KernelKind {
    Mihomo,
    Xray,
    /// iOS NetworkExtension uses sing-box (Libbox.xcframework). Xboard
    /// backend doesn't ship a sing-box subscription flag, so we fetch
    /// the mihomo YAML and translate it client-side via
    /// `profile::inject_singbox::patch_singbox`.
    SingBox,
}

impl KernelKind {
    /// The `?flag=` value to append when fetching the Xboard subscription URL.
    ///
    /// Values match `App\Support\ProtocolManager::matchProtocolClassName` in the
    /// backend (case-insensitive substring match against the protocol's `flags`
    /// array).
    pub fn flag(&self) -> &'static str {
        match self {
            // Hits ClashMeta protocol (flags include "meta")
            KernelKind::Mihomo => "clash.meta",
            // Hits General protocol (flags include "v2rayng")
            KernelKind::Xray => "v2rayng",
            // No native flag; we reuse the mihomo subscription and translate
            // it client-side. iOS PacketTunnelProvider gets the JSON.
            KernelKind::SingBox => "clash.meta",
        }
    }
}

/// Configuration handed to a driver to **attach** to an already-running
/// kernel. The manager / launcher own process spawn + YAML writes; the
/// driver only needs to know where to talk and how to authenticate.
#[derive(Debug, Clone)]
pub enum KernelConfig {
    Mihomo {
        controller_addr: String,   // e.g. "127.0.0.1:9090"
        controller_secret: String, // matches `secret:` in the patched YAML
    },
    Xray {
        json: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyGroup {
    pub name: String,
    /// e.g. "Selector" / "URLTest" / "Fallback" / "LoadBalance"
    #[serde(rename = "type")]
    pub kind: String,
    pub now: Option<String>,
    pub all: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TrafficStats {
    pub up: u64, // bytes/sec
    pub down: u64,
    pub up_total: u64,
    pub down_total: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogLine {
    pub level: String,
    pub message: String,
    pub at: chrono::DateTime<chrono::Utc>,
}

/// All implementations MUST be `Send + Sync` because they are wrapped in
/// `Arc<dyn KernelDriver>` and shared across UI threads.
#[async_trait]
pub trait KernelDriver: Send + Sync {
    /// Static identifier ("mihomo" / "xray").
    fn kind(&self) -> KernelKind;

    /// Currently running kernel binary version (e.g. mihomo "v1.18.7").
    async fn version(&self) -> Result<String>;

    /// Attach the driver to an already-running kernel. Stores the
    /// controller address + secret, runs a basic health probe and starts
    /// any background tasks (e.g. log streaming).
    /// MUST be idempotent: calling on an attached driver re-targets it.
    async fn start(&self, cfg: &KernelConfig) -> Result<()>;

    /// Detach the driver. Stops background tasks and clears stored state.
    /// Does NOT kill the kernel — process lifecycle belongs to the
    /// [`super::launcher::KernelLauncher`].
    async fn stop(&self) -> Result<()>;

    /// Re-target with a new config (controller addr / secret rotation).
    async fn reload(&self, cfg: &KernelConfig) -> Result<()>;

    /// Whether the driver currently believes a kernel is attached and
    /// reachable. Cheap: backed by an in-memory flag, not an HTTP probe.
    async fn is_running(&self) -> bool;

    /// List of proxy groups (with their member proxies inlined).
    async fn proxies(&self) -> Result<Vec<ProxyGroup>>;

    /// Switch the active proxy in a group.
    async fn select_proxy(&self, group: &str, name: &str) -> Result<()>;

    /// Run a latency test against a proxy. Returns ms (`u32::MAX` on timeout).
    async fn latency_test(&self, name: &str, url: &str, timeout_ms: u32) -> Result<u32>;

    /// Live traffic counters.
    async fn traffic(&self) -> Result<TrafficStats>;

    /// Subscribe to streaming logs. The caller MAY drop the stream early.
    fn log_stream(&self) -> BoxStream<'static, LogLine>;
}
