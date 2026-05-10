//! UI-facing kernel orchestrator.
//!
//! UI shells (Tauri / Compose) talk to *only* the manager — never to the
//! [`KernelDriver`] or [`KernelLauncher`] directly. The manager owns:
//!
//! * a single driver instance (today: [`super::mihomo::MihomoDriver`]),
//! * a launcher that probes platform privilege and runs the kernel
//!   subprocess ([`KernelLauncher`]),
//! * resolved paths to the mihomo binary + working directory,
//! * a profile fetcher for subscription text,
//! * an optional system-proxy setter for the fallback path,
//! * the connection state machine + a broadcast channel for live UI updates.
//!
//! Connect flow (TUN-first):
//!
//! ```text
//! fetch → [downgrade?] → write yaml → launcher.spawn → driver.start →
//! (optional system-proxy set) → Connected
//! ```
//!
//! The state machine prefers TUN; if `launcher.ensure_privileged()` reports
//! `NeedsConsent` / `ServiceMissing` / `NotPermitted` / `Unsupported`, the
//! manager transparently downgrades to `TunnelMode::SystemProxy` and re-runs
//! the kernel without the TUN block, then sets the OS proxy.
//!
//! Disconnect reverses the order: clear OS proxy → driver.stop (detach) →
//! launcher.stop (kill kernel).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use futures::stream::{BoxStream, StreamExt};
use parking_lot::RwLock;
use ring::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use tokio_stream::wrappers::BroadcastStream;

use super::driver::{KernelConfig, KernelDriver, LogLine, ProxyGroup, TrafficStats};
use super::launcher::{KernelLauncher, KernelSpawnSpec, LaunchHandle, LauncherError};
use crate::error::{Result, XboardError};
use crate::profile::{patch_mihomo, ProfileFetcher, TunnelMode as ProfileTunnelMode};
use crate::tunnel::{ProxyEndpoint, SystemProxySetter};

const DEFAULT_CONTROLLER_ADDR: &str = "127.0.0.1:9090";
const DEFAULT_MIXED_PORT: u16 = 7890;
const STATE_CHANNEL_CAPACITY: usize = 32;

/// Which transport the kernel currently exposes on the host. Mirrored to
/// [`crate::profile::TunnelMode`] when patching the YAML.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TunnelMode {
    #[default]
    Tun,
    SystemProxy,
}

impl From<TunnelMode> for ProfileTunnelMode {
    fn from(value: TunnelMode) -> Self {
        match value {
            TunnelMode::Tun => ProfileTunnelMode::Tun,
            TunnelMode::SystemProxy => ProfileTunnelMode::SystemProxy,
        }
    }
}

/// Steps inside `Connecting`. Strings are stable enough for the UI to
/// switch on directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectStage {
    Fetching,
    Writing,
    Elevating,
    Spawning,
    ApplyingRoute,
    FallbackProxy,
}

impl ConnectStage {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Fetching => "fetching",
            Self::Writing => "writing",
            Self::Elevating => "elevating",
            Self::Spawning => "spawning",
            Self::ApplyingRoute => "applying_route",
            Self::FallbackProxy => "fallback_proxy",
        }
    }
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ConnectionState {
    #[default]
    Disconnected,
    Connecting {
        stage: ConnectStage,
        mode: TunnelMode,
    },
    Connected {
        since: DateTime<Utc>,
        mode: TunnelMode,
        mixed_port: u16,
    },
    Error {
        message: String,
        mode: TunnelMode,
    },
}

pub struct KernelManager {
    driver: Arc<dyn KernelDriver>,
    launcher: Arc<dyn KernelLauncher>,
    proxy_setter: Option<Arc<dyn SystemProxySetter>>,
    fetcher: ProfileFetcher,
    binary_path: PathBuf,
    work_dir: PathBuf,
    state: RwLock<ConnectionState>,
    listeners: broadcast::Sender<ConnectionState>,
    requested_mode: RwLock<TunnelMode>,
    /// Live launch handle while connected. Taken on disconnect and handed
    /// back to `launcher.stop`.
    handle: RwLock<Option<LaunchHandle>>,
    controller_addr: String,
    mixed_port: u16,
}

impl std::fmt::Debug for KernelManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Trait-object members don't need Debug; print only the concrete
        // identifiers the manager owns.
        f.debug_struct("KernelManager")
            .field("kind", &self.driver.kind())
            .field("launcher", &self.launcher.name())
            .field("binary_path", &self.binary_path)
            .field("work_dir", &self.work_dir)
            .field("controller_addr", &self.controller_addr)
            .field("mixed_port", &self.mixed_port)
            .field("state", &*self.state.read())
            .field("requested_mode", &*self.requested_mode.read())
            .finish()
    }
}

impl KernelManager {
    pub fn new(
        driver: Arc<dyn KernelDriver>,
        launcher: Arc<dyn KernelLauncher>,
        proxy_setter: Option<Arc<dyn SystemProxySetter>>,
        fetcher: ProfileFetcher,
        binary_path: PathBuf,
        work_dir: PathBuf,
    ) -> Self {
        let (listeners, _) = broadcast::channel(STATE_CHANNEL_CAPACITY);
        Self {
            driver,
            launcher,
            proxy_setter,
            fetcher,
            binary_path,
            work_dir,
            state: RwLock::new(ConnectionState::Disconnected),
            listeners,
            requested_mode: RwLock::new(TunnelMode::default()),
            handle: RwLock::new(None),
            controller_addr: DEFAULT_CONTROLLER_ADDR.to_string(),
            mixed_port: DEFAULT_MIXED_PORT,
        }
    }

    pub fn state(&self) -> ConnectionState {
        self.state.read().clone()
    }

    /// Snapshot stream of state changes. Each subscriber gets a fresh
    /// receiver; lagging subscribers are silently dropped frames.
    pub fn subscribe_state(&self) -> BoxStream<'static, ConnectionState> {
        let rx = self.listeners.subscribe();
        BroadcastStream::new(rx)
            .filter_map(|res| async move { res.ok() })
            .boxed()
    }

    pub fn requested_mode(&self) -> TunnelMode {
        *self.requested_mode.read()
    }

    /// Switch user-preferred mode. The change takes effect on the *next*
    /// `connect()` call — we don't auto-reconnect here to keep the mental
    /// model simple. The caller (UI command layer) may decide to
    /// disconnect+connect itself if the manager is currently `Connected`.
    pub fn set_requested_mode(&self, mode: TunnelMode) {
        *self.requested_mode.write() = mode;
    }

    pub async fn proxies(&self) -> Result<Vec<ProxyGroup>> {
        self.driver.proxies().await
    }

    pub async fn select_proxy(&self, group: &str, name: &str) -> Result<()> {
        self.driver.select_proxy(group, name).await
    }

    pub async fn latency_test(&self, name: &str) -> Result<u32> {
        // Default test target + timeout — tunable later via per-user settings.
        self.driver
            .latency_test(name, "https://www.gstatic.com/generate_204", 5_000)
            .await
    }

    pub async fn current_traffic(&self) -> Result<TrafficStats> {
        self.driver.traffic().await
    }

    pub fn live_logs(&self) -> BoxStream<'static, LogLine> {
        self.driver.log_stream()
    }

    /// Fetch the subscription, generate a session secret, write the patched
    /// YAML, probe privilege, spawn the kernel, attach the driver, and
    /// settle into `Connected` (or fall back to SystemProxy). On any
    /// uncaught failure the state ends up in `Error` and partial work is
    /// rolled back.
    pub async fn connect(&self, subscribe_url: &str) -> Result<()> {
        let requested = self.requested_mode();

        // Phase A: fetch subscription text.
        self.publish(ConnectionState::Connecting {
            stage: ConnectStage::Fetching,
            mode: requested,
        });
        let snapshot = match self
            .fetcher
            .fetch(subscribe_url, self.driver.kind(), None)
            .await
        {
            Ok(s) => s,
            Err(e) => return self.fail(requested, format!("fetch subscribe: {e}")),
        };
        let yaml = match tokio::fs::read_to_string(&snapshot.bytes_path).await {
            Ok(t) => t,
            Err(e) => return self.fail(requested, format!("read subscribe cache: {e}")),
        };

        // Phase B: probe privilege; choose final mode.
        self.publish(ConnectionState::Connecting {
            stage: ConnectStage::Elevating,
            mode: requested,
        });
        let (final_mode, downgraded) = match requested {
            TunnelMode::SystemProxy => (TunnelMode::SystemProxy, false),
            TunnelMode::Tun => match self.launcher.ensure_privileged().await {
                Ok(()) => (TunnelMode::Tun, false),
                Err(LauncherError::NeedsConsent(_))
                | Err(LauncherError::ServiceMissing(_))
                | Err(LauncherError::NotPermitted(_))
                | Err(LauncherError::Unsupported) => (TunnelMode::SystemProxy, true),
                Err(other) => return self.fail(requested, format!("elevate: {other}")),
            },
        };

        if downgraded {
            self.publish(ConnectionState::Connecting {
                stage: ConnectStage::FallbackProxy,
                mode: final_mode,
            });
        }

        // Phase C: patch + write YAML to cfg_path.
        self.publish(ConnectionState::Connecting {
            stage: ConnectStage::Writing,
            mode: final_mode,
        });
        let session_secret = match generate_secret() {
            Ok(s) => s,
            Err(e) => return self.fail(final_mode, format!("generate secret: {e}")),
        };
        let patched = match patch_mihomo(
            &yaml,
            &self.controller_addr,
            &session_secret,
            self.mixed_port,
            final_mode.into(),
        ) {
            Ok(y) => y,
            Err(e) => return self.fail(final_mode, format!("patch yaml: {e}")),
        };
        if let Err(e) = tokio::fs::create_dir_all(&self.work_dir).await {
            return self.fail(final_mode, format!("mkdir work_dir: {e}"));
        }
        let cfg_path = self.work_dir.join("config.yaml");
        let log_path = self.work_dir.join("mihomo.log");
        if let Err(e) = tokio::fs::write(&cfg_path, &patched).await {
            return self.fail(final_mode, format!("write config: {e}"));
        }

        // Phase D: spawn kernel via the launcher.
        self.publish(ConnectionState::Connecting {
            stage: ConnectStage::Spawning,
            mode: final_mode,
        });
        let spec = KernelSpawnSpec {
            exec_path: self.binary_path.clone(),
            work_dir: self.work_dir.clone(),
            cfg_path,
            log_path,
            controller_addr: self.controller_addr.clone(),
            controller_secret: session_secret.clone(),
        };
        let launch_handle = match self.launcher.spawn(spec).await {
            Ok(h) => h,
            Err(e) => return self.fail(final_mode, format!("kernel spawn: {e}")),
        };
        *self.handle.write() = Some(launch_handle);

        // Phase E: attach the driver to the live controller.
        let cfg = KernelConfig::Mihomo {
            controller_addr: self.controller_addr.clone(),
            controller_secret: session_secret,
        };
        if let Err(e) = self.driver.start(&cfg).await {
            // Roll back the spawn — driver failed to attach.
            let pending = self.handle.write().take();
            if let Some(h) = pending {
                let _ = self.launcher.stop(h).await;
            }
            return self.fail(final_mode, format!("driver attach: {e}"));
        }

        // Phase F: settle. For TUN, give mihomo a beat to install routes
        // before we declare success.
        if matches!(final_mode, TunnelMode::Tun) {
            self.publish(ConnectionState::Connecting {
                stage: ConnectStage::ApplyingRoute,
                mode: final_mode,
            });
            tokio::time::sleep(Duration::from_millis(300)).await;
        }

        // For SystemProxy, also flip the OS proxy. If that fails, undo and
        // surface an error rather than declaring success.
        if matches!(final_mode, TunnelMode::SystemProxy) {
            if let Some(setter) = self.proxy_setter.as_ref() {
                let endpoint = ProxyEndpoint {
                    host: "127.0.0.1".into(),
                    port: self.mixed_port,
                    bypass: default_bypass(),
                };
                if let Err(e) = setter.set(&endpoint) {
                    let _ = self.driver.stop().await;
                    let pending = self.handle.write().take();
                    if let Some(h) = pending {
                        let _ = self.launcher.stop(h).await;
                    }
                    return self.fail(final_mode, format!("set system proxy: {e}"));
                }
            } else {
                let _ = self.driver.stop().await;
                let pending = self.handle.write().take();
                if let Some(h) = pending {
                    let _ = self.launcher.stop(h).await;
                }
                return self.fail(
                    final_mode,
                    "system-proxy fallback selected but no proxy setter installed".into(),
                );
            }
        }

        self.publish(ConnectionState::Connected {
            since: Utc::now(),
            mode: final_mode,
            mixed_port: self.mixed_port,
        });
        Ok(())
    }

    /// Reverse `connect`: clear OS proxy if we set it, detach the driver,
    /// then kill the kernel via the launcher.
    pub async fn disconnect(&self) -> Result<()> {
        let current = self.state.read().clone();
        let mode = match current {
            ConnectionState::Connected { mode, .. } => mode,
            ConnectionState::Connecting { mode, .. } => mode,
            ConnectionState::Error { mode, .. } => mode,
            ConnectionState::Disconnected => {
                // Already disconnected; still ensure no kernel is left
                // hanging from a partial connect (defensive).
                let _ = self.driver.stop().await;
                let pending = self.handle.write().take();
                if let Some(h) = pending {
                    let _ = self.launcher.stop(h).await;
                }
                self.publish(ConnectionState::Disconnected);
                return Ok(());
            }
        };

        // Clear OS proxy *before* stopping the kernel, otherwise the user's
        // browser keeps pointing at a dead 127.0.0.1:7890.
        if matches!(mode, TunnelMode::SystemProxy) {
            if let Some(setter) = self.proxy_setter.as_ref() {
                if let Err(e) = setter.clear() {
                    tracing::warn!("system-proxy clear: {e}");
                }
            }
        }

        // Detach driver first (cheap), then stop kernel.
        let _ = self.driver.stop().await;
        let pending = self.handle.write().take();
        let stop_err = if let Some(h) = pending {
            self.launcher.stop(h).await.err()
        } else {
            None
        };

        if let Some(e) = stop_err {
            return self.fail_result(mode, format!("kernel stop: {e}"));
        }
        self.publish(ConnectionState::Disconnected);
        Ok(())
    }

    fn publish(&self, next: ConnectionState) {
        *self.state.write() = next.clone();
        let _ = self.listeners.send(next);
    }

    /// Best-effort cleanup helper for connect-time failures: detach the
    /// driver, kill the kernel if we managed to spawn one, then publish
    /// the error state.
    fn fail(&self, mode: TunnelMode, message: String) -> Result<()> {
        let driver = self.driver.clone();
        let launcher = self.launcher.clone();
        let handle = self.handle.write().take();
        tokio::spawn(async move {
            let _ = driver.stop().await;
            if let Some(h) = handle {
                let _ = launcher.stop(h).await;
            }
        });
        let msg = message.clone();
        self.publish(ConnectionState::Error { message, mode });
        Err(XboardError::Kernel(msg))
    }

    /// Like `fail` but for paths where the caller already cleaned up —
    /// only publishes the error state.
    fn fail_result(&self, mode: TunnelMode, message: String) -> Result<()> {
        let msg = message.clone();
        self.publish(ConnectionState::Error { message, mode });
        Err(XboardError::Kernel(msg))
    }
}

fn generate_secret() -> std::result::Result<String, ring::error::Unspecified> {
    let rng = SystemRandom::new();
    let mut bytes = [0u8; 32];
    rng.fill(&mut bytes)?;
    Ok(hex::encode(bytes))
}

fn default_bypass() -> Vec<String> {
    [
        "localhost",
        "127.*",
        "10.*",
        "172.16.*",
        "172.17.*",
        "172.18.*",
        "172.19.*",
        "172.20.*",
        "172.21.*",
        "172.22.*",
        "172.23.*",
        "172.24.*",
        "172.25.*",
        "172.26.*",
        "172.27.*",
        "172.28.*",
        "172.29.*",
        "172.30.*",
        "172.31.*",
        "192.168.*",
        "<local>",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}
