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
use serde_yaml::{Mapping, Value};
use tokio::sync::broadcast;
use tokio_stream::wrappers::BroadcastStream;

use super::driver::{
    ConnectionItem, KernelConfig, KernelDriver, LogLine, ProxyGroup, RuleItem, TrafficStats,
};
use super::launcher::{KernelFailure, KernelLauncher, KernelSpawnSpec, LaunchHandle, LauncherError};
use crate::error::{Result, XboardError};
use crate::profile::{patch_mihomo_with_tun_fd, ProfileFetcher, TunnelMode as ProfileTunnelMode};
use crate::tunnel::{ProxyEndpoint, SystemProxySetter};

const DEFAULT_CONTROLLER_ADDR: &str = "127.0.0.1:9090";
const DEFAULT_MIXED_PORT: u16 = 7890;
const STATE_CHANNEL_CAPACITY: usize = 32;
const FAILURE_CHANNEL_CAPACITY: usize = 16;

/// Heartbeat cadence — every 5 s we hit `/version` to confirm the kernel
/// is still alive. The traffic poller has its own (much higher) cadence
/// driven by the UI; this is the floor for "we'd notice within 10 s".
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);
/// Per-request timeout for the heartbeat probe. Long enough to absorb a
/// slow first-byte from a busy mihomo, short enough that two-in-a-row
/// failures still notify the UI within ~15 s of the kernel hanging.
const HEARTBEAT_TIMEOUT: Duration = Duration::from_secs(2);
/// Consecutive heartbeat failures before we broadcast `Unresponsive`.
/// One miss could be a transient hiccup; two is a pattern.
const HEARTBEAT_FAIL_THRESHOLD: u32 = 2;
/// Consecutive heartbeat failures before we declare the kernel *dead* (not
/// just hiccuping) and force a teardown. On macOS/Windows the privileged
/// launchers expose no crash stream, so the heartbeat is the *only* way we'd
/// ever notice a crashed kernel — without this escalation a SystemProxy-mode
/// crash would leave the OS proxy pointing at a dead port forever. ~6 misses
/// at the 5 s cadence ≈ 30 s of grace for a genuinely-slow kernel to recover.
const HEARTBEAT_TERMINAL_THRESHOLD: u32 = 6;
/// How often the system-proxy guard re-checks that the OS proxy still points
/// at our mixed port. Mirrors clash-verge's 30 s cadence — long enough not to
/// fight the user, short enough that "connected but not proxied" (another app
/// clobbered the proxy) self-heals quickly.
const SYSPROXY_GUARD_INTERVAL: Duration = Duration::from_secs(30);
/// Consecutive traffic-poll errors before we escalate. The traffic poller
/// runs much faster than the heartbeat (UI-driven), so we need a slightly
/// higher floor before declaring the kernel unresponsive.
const TRAFFIC_FAIL_THRESHOLD: u32 = 3;

/// Retry policy for the connect-path stages that touch the network or
/// privileged subprocess (fetch / spawn / apply-route). Total wall time
/// under the worst case is 250 ms + 1 s + 4 s = 5.25 s of *backoff* plus
/// the per-attempt cost, which keeps the UI's "Connecting…" indicator
/// responsive while smoothing out flaky DNS / transient EPERM races.
const RETRY_BACKOFFS_MS: &[u64] = &[250, 1_000, 4_000];

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

/// Health signal the supervisor / launcher emits to UI shells. The wire
/// shape is stable so Tauri can forward it untouched as
/// `connection://kernel-failure` / `connection://kernel-healthy`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum KernelHealthEvent {
    /// The kernel process exited without us asking. `log_tail` is the
    /// last few KiB of mihomo.log, useful for the "View logs" button.
    Exited {
        exit_code: Option<i32>,
        log_tail: Option<String>,
    },
    /// The kernel is still alive (or at least the OS hasn't reaped it) but
    /// has stopped answering `/version` or `/traffic`. Distinct from
    /// `Exited` because the UI advice differs ("try reconnecting" vs
    /// "kernel crashed, view logs").
    Unresponsive { reason: String },
    /// Recovery edge: emitted exactly once after `Unresponsive` when the
    /// next probe / traffic call succeeds, so the UI can clear a banner.
    Healthy,
}

pub struct KernelManager {
    driver: Arc<dyn KernelDriver>,
    launcher: Arc<dyn KernelLauncher>,
    proxy_setter: Option<Arc<dyn SystemProxySetter>>,
    fetcher: ProfileFetcher,
    binary_path: PathBuf,
    work_dir: PathBuf,
    // `Arc` so the background supervisor can update connection state and reap
    // the launch handle when the kernel dies out from under us (see
    // `teardown_after_failure`), not just the foreground connect/disconnect.
    state: Arc<RwLock<ConnectionState>>,
    listeners: broadcast::Sender<ConnectionState>,
    /// Supervisor channel — distinct from `listeners` because crash /
    /// unresponsive signals don't drive the state machine (they're an
    /// out-of-band notification). The Tauri shell subscribes to this and
    /// republishes as `connection://kernel-failure` etc.
    health: broadcast::Sender<KernelHealthEvent>,
    requested_mode: RwLock<TunnelMode>,
    primary_group: RwLock<Option<String>>,
    /// Live launch handle while connected. Taken on disconnect and handed
    /// back to `launcher.stop`. `Arc` so the supervisor can reap it on crash.
    handle: Arc<RwLock<Option<LaunchHandle>>>,
    /// Background task that periodically hits `/version` and watches the
    /// launcher's failure stream. Aborted on `disconnect`.
    supervisor_task: parking_lot::Mutex<Option<tokio::task::JoinHandle<()>>>,
    /// Atomic counter for traffic-poll consecutive errors. Read & written
    /// by [`KernelManager::current_traffic`] so the supervisor can promote
    /// the count to an `Unresponsive` event without owning the poll loop
    /// itself (the UI drives the cadence).
    traffic_err_streak: std::sync::Arc<std::sync::atomic::AtomicU32>,
    /// Sticky flag: `true` between an emitted `Unresponsive` and the next
    /// `Healthy`. Used to dedupe redundant emits and to know whether a
    /// success edge should fire `Healthy`. Same lifetime as the manager.
    in_unresponsive: std::sync::Arc<std::sync::atomic::AtomicBool>,
    // Controller addr + mixed port are picked fresh (free ports) on every
    // `connect` so we never collide with another Clash app or a previous
    // instance mid-teardown on the fixed 9090/7890. Hence `RwLock`.
    controller_addr: RwLock<String>,
    controller_secret: RwLock<Option<String>>,
    mixed_port: RwLock<u16>,
    /// Mobile only: the TUN file descriptor the host VpnService / NE handed
    /// us. `None` on desktop, where mihomo opens its own device. Set via
    /// [`KernelManager::set_tun_fd`] before `connect`.
    tun_fd: RwLock<Option<i32>>,
    /// Background task that re-asserts the OS proxy if another app overwrites
    /// it (SystemProxy mode only). Aborted on disconnect.
    sysproxy_guard: parking_lot::Mutex<Option<tokio::task::JoinHandle<()>>>,
    /// Master switch for the guard above — exposed so a settings toggle can
    /// disable it without tearing down the connection.
    proxy_guard_enabled: Arc<std::sync::atomic::AtomicBool>,
    /// Last subscribe URL passed to `connect`, so `reconnect` can re-establish
    /// after a kernel crash without the caller threading it back through.
    last_subscribe_url: RwLock<Option<String>>,
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
            .field("controller_addr", &*self.controller_addr.read())
            .field("mixed_port", &*self.mixed_port.read())
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
        let (health, _) = broadcast::channel(FAILURE_CHANNEL_CAPACITY);
        Self {
            driver,
            launcher,
            proxy_setter,
            fetcher,
            binary_path,
            work_dir,
            state: Arc::new(RwLock::new(ConnectionState::Disconnected)),
            listeners,
            health,
            requested_mode: RwLock::new(TunnelMode::default()),
            primary_group: RwLock::new(None),
            handle: Arc::new(RwLock::new(None)),
            supervisor_task: parking_lot::Mutex::new(None),
            traffic_err_streak: std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0)),
            in_unresponsive: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            controller_addr: RwLock::new(DEFAULT_CONTROLLER_ADDR.to_string()),
            controller_secret: RwLock::new(None),
            mixed_port: RwLock::new(DEFAULT_MIXED_PORT),
            tun_fd: RwLock::new(None),
            sysproxy_guard: parking_lot::Mutex::new(None),
            proxy_guard_enabled: Arc::new(std::sync::atomic::AtomicBool::new(true)),
            last_subscribe_url: RwLock::new(None),
        }
    }

    /// Re-establish the connection using the last URL passed to `connect`.
    /// Used by the host's auto-reconnect after a kernel crash (the supervisor
    /// emits an `Exited` health event; the UI calls this with backoff). Errors
    /// if we've never connected.
    pub async fn reconnect(&self) -> Result<()> {
        let url = self
            .last_subscribe_url
            .read()
            .clone()
            .ok_or_else(|| XboardError::Kernel("no previous connection to restore".into()))?;
        self.connect(&url).await
    }

    /// Enable/disable the system-proxy guard at runtime (settings toggle).
    /// Takes effect on the next 30 s tick; doesn't tear down the connection.
    pub fn set_proxy_guard_enabled(&self, enabled: bool) {
        self.proxy_guard_enabled
            .store(enabled, std::sync::atomic::Ordering::SeqCst);
    }

    pub fn proxy_guard_enabled(&self) -> bool {
        self.proxy_guard_enabled
            .load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Mobile hosts call this with the fd returned from their VpnService /
    /// NEPacketTunnelProvider *before* `connect`, so the patched mihomo config
    /// adopts the existing TUN instead of trying to create one (which the app
    /// sandbox forbids). Passing `None` reverts to native-device behaviour.
    pub fn set_tun_fd(&self, fd: Option<i32>) {
        *self.tun_fd.write() = fd;
    }

    /// Snapshot stream of kernel health events (crash / unresponsive /
    /// healthy). UI shells subscribe once at startup; the channel is
    /// long-lived and outlives individual connect / disconnect cycles so
    /// late subscribers still get future events. Slow consumers are
    /// silently dropped (BroadcastStream filters lag errors).
    pub fn subscribe_health(&self) -> BoxStream<'static, KernelHealthEvent> {
        let rx = self.health.subscribe();
        BroadcastStream::new(rx)
            .filter_map(|res| async move { res.ok() })
            .boxed()
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
        let mut groups = self.driver.proxies().await?;
        let primary = self.primary_group.read().clone();
        groups.sort_by(|a, b| {
            let ar = group_rank(a, primary.as_deref());
            let br = group_rank(b, primary.as_deref());
            ar.cmp(&br).then_with(|| a.name.cmp(&b.name))
        });
        Ok(groups)
    }

    pub async fn select_proxy(&self, group: &str, name: &str) -> Result<()> {
        self.driver.select_proxy(group, name).await?;
        // Flush live connections so existing flows immediately re-route through
        // the newly-selected node instead of lingering on the old one (a
        // mihomo selector switch only affects *new* connections). clash-verge
        // does the same. Best-effort — a flush failure doesn't undo the switch.
        let _ = self.driver.close_all_connections().await;
        Ok(())
    }

    /// Snapshot of active connections (mihomo `/connections`).
    pub async fn connections(&self) -> Result<Vec<ConnectionItem>> {
        self.driver.connections().await
    }

    /// Force-close one connection by id.
    pub async fn close_connection(&self, id: &str) -> Result<()> {
        self.driver.close_connection(id).await
    }

    /// Force-close every active connection.
    pub async fn close_all_connections(&self) -> Result<()> {
        self.driver.close_all_connections().await
    }

    /// The kernel's active routing rules.
    pub async fn rules(&self) -> Result<Vec<RuleItem>> {
        self.driver.rules().await
    }

    pub fn mixed_port(&self) -> u16 {
        *self.mixed_port.read()
    }

    pub async fn latency_test(&self, name: &str) -> Result<u32> {
        // Default test target + timeout — tunable later via per-user settings.
        self.driver
            .latency_test(name, "https://www.gstatic.com/generate_204", 5_000)
            .await
    }

    pub async fn current_traffic(&self) -> Result<TrafficStats> {
        match self.driver.traffic().await {
            Ok(stats) => {
                // Reset the consecutive-error counter on any success.
                self.traffic_err_streak
                    .store(0, std::sync::atomic::Ordering::SeqCst);
                // Edge: if we previously declared the kernel unresponsive,
                // a working traffic read is a recovery signal too.
                if self
                    .in_unresponsive
                    .swap(false, std::sync::atomic::Ordering::SeqCst)
                {
                    let _ = self.health.send(KernelHealthEvent::Healthy);
                }
                Ok(stats)
            }
            Err(e) => {
                // Only escalate while we believe we're connected — once
                // the user has disconnected (Disconnected/Error state),
                // poll errors are noise from in-flight requests racing the
                // teardown.
                if matches!(*self.state.read(), ConnectionState::Connected { .. }) {
                    let n = self
                        .traffic_err_streak
                        .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
                        + 1;
                    if n == TRAFFIC_FAIL_THRESHOLD && !self
                        .in_unresponsive
                        .swap(true, std::sync::atomic::Ordering::SeqCst)
                    {
                        let _ = self.health.send(KernelHealthEvent::Unresponsive {
                            reason: format!(
                                "{TRAFFIC_FAIL_THRESHOLD} consecutive /traffic failures: {e}"
                            ),
                        });
                    }
                }
                Err(e)
            }
        }
    }

    pub fn live_logs(&self) -> BoxStream<'static, LogLine> {
        self.driver.log_stream()
    }

    /// Fetch the subscription, generate a session secret, write the patched
    /// YAML, probe privilege, spawn the kernel, attach the driver, and
    /// settle into `Connected` (or fall back to SystemProxy). On any
    /// uncaught failure the state ends up in `Error` and partial work is
    /// rolled back.
    ///
    /// Network-touching stages (`Fetching`, `Spawning`, `ApplyingRoute`)
    /// are wrapped in exponential-backoff retries (250 ms / 1 s / 4 s).
    /// Other failures (config errors, missing binary) fail fast — retrying
    /// won't help and the user needs the message.
    pub async fn connect(&self, subscribe_url: &str) -> Result<()> {
        let requested = self.requested_mode();
        // Remember the URL so `reconnect()` (and the host's auto-reconnect on
        // a kernel crash) can re-establish without the caller re-supplying it.
        *self.last_subscribe_url.write() = Some(subscribe_url.to_string());

        // Pick fresh free ports each connect so we never collide with another
        // Clash app (or our own previous instance mid-teardown) on the fixed
        // 9090/7890. Falls back to whatever's currently set if the OS can't
        // hand us two distinct free ports (extremely unlikely).
        if let Some((controller_port, mixed)) = pick_two_free_ports() {
            *self.controller_addr.write() = format!("127.0.0.1:{controller_port}");
            *self.mixed_port.write() = mixed;
        }
        let controller_addr = self.controller_addr.read().clone();
        let mixed_port = *self.mixed_port.read();

        // Phase A: fetch subscription text — retry on transient network /
        // backend hiccups, fail fast on auth / 4xx (XboardError::Unauthorized).
        self.publish(ConnectionState::Connecting {
            stage: ConnectStage::Fetching,
            mode: requested,
        });
        let kind = self.driver.kind();
        let snapshot = match retry_with_backoff(
            "fetch subscribe",
            RETRY_BACKOFFS_MS,
            // Don't waste 5 s of backoff on a definitive backend answer:
            // an expired subscription (4xx) or a revoked bearer (401/403)
            // won't get better by retrying — fail fast so the user sees the
            // real reason immediately.
            |e: &XboardError| {
                !matches!(
                    e,
                    XboardError::SubscriptionUnavailable { .. } | XboardError::Unauthorized
                )
            },
            |attempt| {
                let fetcher = &self.fetcher;
                async move {
                    if attempt > 0 {
                        tracing::info!(attempt, "retrying subscribe fetch");
                    }
                    fetcher.fetch(subscribe_url, kind, None).await
                }
            },
        )
        .await
        {
            Ok(s) => s,
            Err(XboardError::SubscriptionUnavailable { status }) => {
                return self.fail(
                    requested,
                    format!(
                        "订阅当前不可用（HTTP {status}）——套餐可能已到期或流量耗尽，请续费后重试"
                    ),
                )
            }
            Err(e) => return self.fail(requested, format!("fetch subscribe: {e}")),
        };
        let yaml = match tokio::fs::read_to_string(&snapshot.bytes_path).await {
            Ok(t) => t,
            Err(e) => return self.fail(requested, format!("read subscribe cache: {e}")),
        };
        // Belt-and-suspenders: even a 2xx subscription can come back with an
        // empty `proxies:` list (e.g. a backend that serves a skeleton config
        // to out-of-traffic users). Refuse to spin up the kernel over a
        // node-less config — that produced the old "connected but no egress"
        // failure mode where TUN swallowed all traffic into a DIRECT/REJECT.
        if !subscription_has_nodes(&yaml) {
            return self.fail(
                requested,
                "订阅中没有可用节点——套餐可能已到期或流量耗尽，请续费后重试".to_string(),
            );
        }
        *self.primary_group.write() = pick_primary_proxy_group(&yaml);

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
        // On mobile the host (Android VpnService / iOS NE) establishes the
        // TUN fd out-of-band and stashes it via `set_tun_fd` before connect;
        // desktop leaves it `None` and mihomo opens its own device.
        let tun_fd = *self.tun_fd.read();
        let patched = match patch_mihomo_with_tun_fd(
            &yaml,
            &controller_addr,
            &session_secret,
            mixed_port,
            final_mode.into(),
            tun_fd,
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

        // Phase D: spawn kernel via the launcher. Retried because the
        // privileged side can race (helper just got installed and is
        // still binding its socket, or wintun adapter is mid-removal from
        // a previous run).
        self.publish(ConnectionState::Connecting {
            stage: ConnectStage::Spawning,
            mode: final_mode,
        });
        let spec_template = KernelSpawnSpec {
            exec_path: self.binary_path.clone(),
            work_dir: self.work_dir.clone(),
            cfg_path,
            log_path,
            controller_addr: controller_addr.clone(),
            controller_secret: session_secret.clone(),
        };
        let launch_handle = match retry_with_backoff(
            "kernel spawn",
            RETRY_BACKOFFS_MS,
            // Spawn races (helper still binding its socket, wintun adapter
            // mid-removal) are all worth retrying.
            |_e: &XboardError| true,
            |attempt| {
                let launcher = self.launcher.clone();
                let spec = spec_template.clone();
                async move {
                    if attempt > 0 {
                        tracing::info!(attempt, "retrying kernel spawn");
                    }
                    launcher
                        .spawn(spec)
                        .await
                        .map_err(|e| XboardError::Kernel(e.to_string()))
                }
            },
        )
        .await
        {
            Ok(h) => h,
            Err(e) => return self.fail(final_mode, format!("kernel spawn: {e}")),
        };
        *self.handle.write() = Some(launch_handle);

        // Phase E: attach the driver to the live controller.
        let cfg = KernelConfig::Mihomo {
            controller_addr: controller_addr.clone(),
            controller_secret: session_secret.clone(),
        };
        if let Err(e) = self.driver.start(&cfg).await {
            // Roll back the spawn — driver failed to attach.
            let pending = self.handle.write().take();
            if let Some(h) = pending {
                let _ = self.launcher.stop(h).await;
            }
            return self.fail(final_mode, format!("driver attach: {e}"));
        }
        // Remember the secret so the supervisor's /version heartbeat can
        // authenticate. Cleared in disconnect().
        *self.controller_secret.write() = Some(session_secret);

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
                    port: mixed_port,
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
                // Guard the OS proxy against being clobbered by other apps.
                self.start_sysproxy_guard(endpoint);
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
            mixed_port,
        });
        self.start_supervisor(final_mode);
        Ok(())
    }

    /// Spawn the system-proxy guard: every 30 s, if we're still Connected in
    /// SystemProxy mode and another app has overwritten the OS proxy, put ours
    /// back. The task reads the shared connection state and exits on its own
    /// the moment we leave Connected (disconnect / crash / error), so it can
    /// never re-assert a proxy pointing at a dead port after a teardown.
    fn start_sysproxy_guard(&self, endpoint: ProxyEndpoint) {
        if let Some(h) = self.sysproxy_guard.lock().take() {
            h.abort();
        }
        let setter = match self.proxy_setter.clone() {
            Some(s) => s,
            None => return,
        };
        let state = self.state.clone();
        let enabled = self.proxy_guard_enabled.clone();
        let handle = tokio::spawn(async move {
            let mut ticker = tokio::time::interval(SYSPROXY_GUARD_INTERVAL);
            ticker.tick().await; // skip the immediate first tick
            loop {
                ticker.tick().await;
                match *state.read() {
                    ConnectionState::Connected {
                        mode: TunnelMode::SystemProxy,
                        ..
                    } => {
                        if enabled.load(std::sync::atomic::Ordering::SeqCst)
                            && !setter.matches(&endpoint)
                        {
                            tracing::info!("system proxy drifted; re-asserting ours");
                            if let Err(e) = setter.set(&endpoint) {
                                tracing::warn!("sysproxy guard re-assert failed: {e}");
                            }
                        }
                    }
                    // Any non-(Connected+SystemProxy) state means we're done
                    // guarding — stop the task.
                    _ => return,
                }
            }
        });
        *self.sysproxy_guard.lock() = Some(handle);
    }

    fn stop_sysproxy_guard(&self) {
        if let Some(h) = self.sysproxy_guard.lock().take() {
            h.abort();
        }
    }

    /// Spawn the background supervisor: a periodic `/version` heartbeat
    /// and (if the launcher provides one) a subscription to the
    /// crash-event stream. Idempotent — calling while one's already
    /// running aborts and replaces it.
    fn start_supervisor(&self, mode: TunnelMode) {
        // Cancel any previous supervisor (e.g. user disconnect+reconnect).
        if let Some(h) = self.supervisor_task.lock().take() {
            h.abort();
        }
        // Reset the streak / sticky-unresponsive state for the new lifetime.
        self.traffic_err_streak
            .store(0, std::sync::atomic::Ordering::SeqCst);
        self.in_unresponsive
            .store(false, std::sync::atomic::Ordering::SeqCst);

        let addr = self.controller_addr.read().clone();
        let secret = self
            .controller_secret
            .read()
            .clone()
            .unwrap_or_default();
        let health_tx = self.health.clone();
        let unresp = self.in_unresponsive.clone();
        let mut launcher_failures = self.launcher.failure_stream();
        // Resources the supervisor needs to clean up after a crash, since the
        // spawned task can't borrow `self`.
        let state = self.state.clone();
        let listeners = self.listeners.clone();
        let proxy_setter = self.proxy_setter.clone();
        let handle = self.handle.clone();
        let launcher = self.launcher.clone();

        let handle = tokio::spawn(async move {
            let client = match reqwest::Client::builder()
                .timeout(HEARTBEAT_TIMEOUT)
                .build()
            {
                Ok(c) => c,
                Err(e) => {
                    tracing::error!(error=%e, "supervisor: build heartbeat client");
                    return;
                }
            };
            let url = format!("http://{addr}/version");
            let auth = format!("Bearer {secret}");
            let mut ticker = tokio::time::interval(HEARTBEAT_INTERVAL);
            // The first tick fires immediately; skip it so we don't race
            // the just-attached driver's own probe.
            ticker.tick().await;
            let mut fail_streak: u32 = 0;

            loop {
                tokio::select! {
                    _ = ticker.tick() => {
                        let ok = client
                            .get(&url)
                            .header("Authorization", &auth)
                            .send()
                            .await
                            .map(|r| r.status().is_success())
                            .unwrap_or(false);
                        if ok {
                            fail_streak = 0;
                            if unresp.swap(false, std::sync::atomic::Ordering::SeqCst) {
                                let _ = health_tx.send(KernelHealthEvent::Healthy);
                            }
                        } else {
                            fail_streak += 1;
                            if fail_streak == HEARTBEAT_FAIL_THRESHOLD
                                && !unresp.swap(true, std::sync::atomic::Ordering::SeqCst)
                            {
                                let _ = health_tx.send(KernelHealthEvent::Unresponsive {
                                    reason: format!(
                                        "{HEARTBEAT_FAIL_THRESHOLD} consecutive /version probes failed"
                                    ),
                                });
                            }
                            // Past the terminal threshold the kernel is dead,
                            // not slow. This is the only crash signal on
                            // macOS/Windows (no launcher failure stream), so
                            // we MUST clean up here or a SystemProxy-mode
                            // crash strands the OS proxy on a dead port.
                            if fail_streak >= HEARTBEAT_TERMINAL_THRESHOLD {
                                let _ = health_tx.send(KernelHealthEvent::Exited {
                                    exit_code: None,
                                    log_tail: None,
                                });
                                teardown_after_failure(
                                    &state,
                                    &listeners,
                                    &proxy_setter,
                                    &handle,
                                    &launcher,
                                    mode,
                                    "内核长时间无响应，已自动断开并清理系统代理".to_string(),
                                )
                                .await;
                                return;
                            }
                        }
                    }
                    // Launcher-side crash event (DirectLauncher on Linux; the
                    // macOS/Windows privileged launchers return None and this
                    // arm is just never-ready, leaving the heartbeat above to
                    // do all the detection on those platforms).
                    failure = recv_optional(&mut launcher_failures) => {
                        if let Some(failure) = failure {
                            let (event, reason) = match failure {
                                KernelFailure::Exited { exit_code, log_tail } => (
                                    KernelHealthEvent::Exited {
                                        exit_code,
                                        log_tail,
                                    },
                                    "内核进程已退出，已自动断开并清理系统代理".to_string(),
                                ),
                                KernelFailure::Unresponsive { reason } => (
                                    KernelHealthEvent::Unresponsive {
                                        reason: reason.clone(),
                                    },
                                    reason,
                                ),
                            };
                            let _ = health_tx.send(event);
                            // The kernel is gone — clear OS proxy / reap the
                            // handle / mark Error so we don't leave the
                            // machine pointing at a dead port, then stop.
                            teardown_after_failure(
                                &state,
                                &listeners,
                                &proxy_setter,
                                &handle,
                                &launcher,
                                mode,
                                reason,
                            )
                            .await;
                            return;
                        }
                    }
                }
            }
        });
        *self.supervisor_task.lock() = Some(handle);
    }

    /// Reverse `connect`: clear OS proxy if we set it, detach the driver,
    /// then kill the kernel via the launcher.
    pub async fn disconnect(&self) -> Result<()> {
        // Stop the supervisor *before* killing the kernel so a graceful
        // shutdown doesn't get reported as a crash via the heartbeat path.
        // The launcher's exit-watcher is suppressed via the launcher's
        // own `expecting_stop` flag (set inside `launcher.stop`).
        if let Some(h) = self.supervisor_task.lock().take() {
            h.abort();
        }
        self.stop_sysproxy_guard();
        *self.controller_secret.write() = None;
        self.in_unresponsive
            .store(false, std::sync::atomic::Ordering::SeqCst);
        self.traffic_err_streak
            .store(0, std::sync::atomic::Ordering::SeqCst);

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

        // Detach driver first (cheap), then stop kernel. Bound each network/
        // process step with its own timeout so a wedged kernel can never make
        // disconnect (and, through it, app exit) hang. The OS reaps the child
        // via kill_on_drop even if `launcher.stop` times out.
        let _ = tokio::time::timeout(Duration::from_millis(1500), self.driver.stop()).await;
        let pending = self.handle.write().take();
        let stop_err = if let Some(h) = pending {
            match tokio::time::timeout(Duration::from_secs(3), self.launcher.stop(h)).await {
                Ok(res) => res.err(),
                Err(_) => Some(LauncherError::Other("kernel stop timed out".into())),
            }
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

/// Treat a missing receiver as a never-ready future. Lets the supervisor
/// `tokio::select!` arm degrade cleanly on launchers that don't expose a
/// failure stream (svc / helper today).
async fn recv_optional(
    rx: &mut Option<broadcast::Receiver<KernelFailure>>,
) -> Option<KernelFailure> {
    match rx {
        Some(receiver) => match receiver.recv().await {
            Ok(v) => Some(v),
            // Lagged or closed: stop listening rather than spinning.
            Err(_) => {
                *rx = None;
                std::future::pending::<Option<KernelFailure>>().await
            }
        },
        None => std::future::pending::<Option<KernelFailure>>().await,
    }
}

/// Run `op` up to `1 + backoffs.len()` times, sleeping `backoffs[n]` ms
/// between attempts. Returns the first `Ok(_)` or the last `Err(_)`.
/// Used by the connect path to absorb transient DNS / svc-race / EPERM
/// hiccups before surfacing a hard error.
///
/// `attempt` starts at 0 (first try) so callers can log accordingly.
///
/// `is_retryable` lets the caller short-circuit definitive failures (e.g.
/// an expired-subscription 4xx or a revoked bearer) so we don't burn the
/// full backoff budget on an error that won't change.
async fn retry_with_backoff<T, E, F, Fut, R>(
    label: &'static str,
    backoffs: &[u64],
    is_retryable: R,
    mut op: F,
) -> std::result::Result<T, E>
where
    F: FnMut(usize) -> Fut,
    Fut: std::future::Future<Output = std::result::Result<T, E>>,
    E: std::fmt::Display,
    R: Fn(&E) -> bool,
{
    let mut last_err: Option<E> = None;
    for attempt in 0..=backoffs.len() {
        match op(attempt).await {
            Ok(v) => return Ok(v),
            Err(e) => {
                if attempt < backoffs.len() && is_retryable(&e) {
                    tracing::warn!(label, attempt, error=%e, "transient failure, retrying");
                    tokio::time::sleep(Duration::from_millis(backoffs[attempt])).await;
                    last_err = Some(e);
                } else {
                    // Either out of attempts or a non-retryable error —
                    // surface it immediately.
                    return Err(e);
                }
            }
        }
    }
    Err(last_err.expect("at least one attempt is always run"))
}

/// Clean up after the kernel dies out from under us (crash / terminal
/// unresponsiveness) — the path the foreground `disconnect()` can't cover
/// because nobody called it. Clears the OS proxy when we were in SystemProxy
/// mode (otherwise the user's traffic keeps pointing at a now-dead
/// `127.0.0.1` port and the whole machine looks offline), reaps the launch
/// handle, and publishes `Error` so the UI and `state()` stop claiming
/// "connected". Runs inside the supervisor task, which is why it takes the
/// manager's shared resources by clone rather than `&self`.
async fn teardown_after_failure(
    state: &Arc<RwLock<ConnectionState>>,
    listeners: &broadcast::Sender<ConnectionState>,
    proxy_setter: &Option<Arc<dyn SystemProxySetter>>,
    handle: &Arc<RwLock<Option<LaunchHandle>>>,
    launcher: &Arc<dyn KernelLauncher>,
    mode: TunnelMode,
    message: String,
) {
    if matches!(mode, TunnelMode::SystemProxy) {
        if let Some(setter) = proxy_setter.as_ref() {
            if let Err(e) = setter.clear() {
                tracing::warn!("system-proxy clear after kernel failure: {e}");
            }
        }
    }
    let pending = handle.write().take();
    if let Some(h) = pending {
        let _ = launcher.stop(h).await;
    }
    let next = ConnectionState::Error { message, mode };
    *state.write() = next.clone();
    let _ = listeners.send(next);
}

/// True if the (already-parsed-as-YAML) mihomo subscription carries at least
/// one entry under `proxies:`. A node-less config is never worth connecting
/// to — see the guard in [`KernelManager::connect`].
fn subscription_has_nodes(yaml: &str) -> bool {
    serde_yaml::from_str::<Value>(yaml)
        .ok()
        .as_ref()
        .and_then(Value::as_mapping)
        .and_then(|m| m.get(Value::String("proxies".into())))
        .and_then(Value::as_sequence)
        .is_some_and(|seq| !seq.is_empty())
}

/// Bind `127.0.0.1:0` and read back the port the OS assigned, then release it.
/// There's a tiny TOCTOU window before mihomo re-binds, but it's vastly safer
/// than a hard-coded port that collides with every other Clash app.
fn pick_free_port() -> Option<u16> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").ok()?;
    let port = listener.local_addr().ok()?.port();
    drop(listener);
    Some(port)
}

/// Two *distinct* free ports — for the external controller and the mixed
/// inbound. Returns `None` if the OS won't hand us two different ports.
fn pick_two_free_ports() -> Option<(u16, u16)> {
    let a = pick_free_port()?;
    for _ in 0..5 {
        let b = pick_free_port()?;
        if b != a {
            return Some((a, b));
        }
    }
    None
}

fn generate_secret() -> std::result::Result<String, ring::error::Unspecified> {
    let rng = SystemRandom::new();
    let mut bytes = [0u8; 32];
    rng.fill(&mut bytes)?;
    Ok(hex::encode(bytes))
}

fn group_rank(group: &ProxyGroup, primary: Option<&str>) -> u8 {
    if primary.is_some_and(|name| name == group.name) {
        return 0;
    }
    if matches!(
        group.name.as_str(),
        "PROXY" | "Proxy" | "GLOBAL" | "节点选择" | "手动切换"
    ) {
        return 1;
    }
    if group.kind == "Selector" {
        return 2;
    }
    if matches!(group.kind.as_str(), "URLTest" | "Fallback" | "LoadBalance") {
        return 3;
    }
    4
}

fn pick_primary_proxy_group(yaml: &str) -> Option<String> {
    let doc: Value = serde_yaml::from_str(yaml).ok()?;
    let groups = doc
        .as_mapping()?
        .get(Value::String("proxy-groups".into()))?
        .as_sequence()?;

    if let Some(name) = first_group_of_type(groups, "select") {
        return Some(name);
    }
    for ty in ["url-test", "fallback", "load-balance"] {
        if let Some(name) = first_group_of_type(groups, ty) {
            return Some(name);
        }
    }
    groups.iter().find_map(group_name)
}

fn first_group_of_type(groups: &[Value], wanted: &str) -> Option<String> {
    groups.iter().find_map(|g| {
        let map = g.as_mapping()?;
        let ty = yaml_get_str(map, "type").unwrap_or("");
        if ty.eq_ignore_ascii_case(wanted) {
            group_name(g)
        } else {
            None
        }
    })
}

fn group_name(group: &Value) -> Option<String> {
    yaml_get_str(group.as_mapping()?, "name").map(str::to_string)
}

fn yaml_get_str<'a>(map: &'a Mapping, key: &str) -> Option<&'a str> {
    map.get(Value::String(key.into()))?.as_str()
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;

    /// `retry_with_backoff` returns `Ok` on the first try and never sleeps.
    #[tokio::test]
    async fn retry_returns_first_ok_immediately() {
        let calls = Arc::new(AtomicU32::new(0));
        let calls_clone = calls.clone();
        let result: std::result::Result<u32, String> =
            retry_with_backoff("test", &[10, 20, 30], |_: &String| true, |_attempt| {
                let calls = calls_clone.clone();
                async move {
                    calls.fetch_add(1, Ordering::SeqCst);
                    Ok::<u32, String>(42)
                }
            })
            .await;
        assert_eq!(result.unwrap(), 42);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    /// Three failing tries → 4 total attempts (1 + 3 backoffs) and the
    /// final error is what surfaces.
    #[tokio::test(start_paused = true)]
    async fn retry_exhausts_then_surfaces_last_error() {
        let calls = Arc::new(AtomicU32::new(0));
        let calls_clone = calls.clone();
        let result: std::result::Result<u32, String> =
            retry_with_backoff("test", &[10, 20, 30], |_: &String| true, |attempt| {
                let calls = calls_clone.clone();
                async move {
                    calls.fetch_add(1, Ordering::SeqCst);
                    Err::<u32, String>(format!("boom-{attempt}"))
                }
            })
            .await;
        let err = result.unwrap_err();
        // 1 initial + 3 backoffs = 4 attempts; the last error label is 3.
        assert_eq!(calls.load(Ordering::SeqCst), 4);
        assert_eq!(err, "boom-3");
    }

    /// A flake on attempts 0..2 followed by a success on attempt 2 must
    /// surface `Ok` without ever reaching the third backoff.
    #[tokio::test(start_paused = true)]
    async fn retry_recovers_mid_backoff() {
        let calls = Arc::new(AtomicU32::new(0));
        let calls_clone = calls.clone();
        let result: std::result::Result<u32, String> =
            retry_with_backoff("test", &[10, 20, 30], |_: &String| true, |attempt| {
                let calls = calls_clone.clone();
                async move {
                    calls.fetch_add(1, Ordering::SeqCst);
                    if attempt < 2 {
                        Err("flake".into())
                    } else {
                        Ok(7)
                    }
                }
            })
            .await;
        assert_eq!(result.unwrap(), 7);
        assert_eq!(calls.load(Ordering::SeqCst), 3);
    }

    /// A non-retryable error must surface on the first attempt without
    /// consuming the backoff budget — this is what makes an expired-
    /// subscription 4xx fail fast instead of stalling the UI for ~5 s.
    #[tokio::test]
    async fn retry_short_circuits_non_retryable() {
        let calls = Arc::new(AtomicU32::new(0));
        let calls_clone = calls.clone();
        let result: std::result::Result<u32, String> =
            retry_with_backoff("test", &[10, 20, 30], |_: &String| false, |_attempt| {
                let calls = calls_clone.clone();
                async move {
                    calls.fetch_add(1, Ordering::SeqCst);
                    Err::<u32, String>("fatal".into())
                }
            })
            .await;
        assert_eq!(result.unwrap_err(), "fatal");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn subscription_has_nodes_detects_empty_and_present() {
        assert!(subscription_has_nodes(
            "proxies:\n  - name: a\n    type: ss\n    server: x\n    port: 1"
        ));
        assert!(!subscription_has_nodes("proxies: []"));
        assert!(!subscription_has_nodes("rules:\n  - MATCH,DIRECT"));
        assert!(!subscription_has_nodes(""));
    }

    /// `KernelHealthEvent` round-trips through serde under the wire tag
    /// shape the Tauri shell forwards verbatim. Guards against
    /// accidentally renaming a variant or its serialised name.
    #[test]
    fn health_event_wire_format_is_stable() {
        let exited = KernelHealthEvent::Exited {
            exit_code: Some(-1),
            log_tail: Some("oops\n".into()),
        };
        let json = serde_json::to_value(&exited).unwrap();
        assert_eq!(json["kind"], "exited");
        assert_eq!(json["exit_code"], -1);
        assert_eq!(json["log_tail"], "oops\n");

        let unresp = KernelHealthEvent::Unresponsive {
            reason: "/version timed out".into(),
        };
        let json = serde_json::to_value(&unresp).unwrap();
        assert_eq!(json["kind"], "unresponsive");
        assert_eq!(json["reason"], "/version timed out");

        let healthy = KernelHealthEvent::Healthy;
        let json = serde_json::to_value(&healthy).unwrap();
        assert_eq!(json["kind"], "healthy");
    }

    /// `KernelFailure` (launcher-side) must serialise to the same shape so
    /// the manager can pass it through to the UI without rewriting.
    #[test]
    fn kernel_failure_wire_format_matches_health_event() {
        let failure = KernelFailure::Exited {
            exit_code: Some(137),
            log_tail: None,
        };
        let json = serde_json::to_value(&failure).unwrap();
        assert_eq!(json["kind"], "exited");
        assert_eq!(json["exit_code"], 137);
    }

    /// Stand-alone reimplementation of the supervisor's heartbeat loop
    /// for unit-testing the consecutive-fail → `Unresponsive` →
    /// `Healthy` transitions without spinning a full `KernelManager`. The
    /// production loop in [`KernelManager::start_supervisor`] follows the
    /// same shape; if you change one, change both.
    async fn run_heartbeat_until<F>(
        addr: String,
        threshold: u32,
        mut probe_succeeds: F,
        ticks: u32,
        tx: broadcast::Sender<KernelHealthEvent>,
    ) where
        F: FnMut(u32) -> bool,
    {
        let unresp = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let mut fail_streak: u32 = 0;
        for n in 0..ticks {
            let _ = addr; // keeps the param shape identical to the real loop
            let ok = probe_succeeds(n);
            if ok {
                fail_streak = 0;
                if unresp.swap(false, std::sync::atomic::Ordering::SeqCst) {
                    let _ = tx.send(KernelHealthEvent::Healthy);
                }
            } else {
                fail_streak += 1;
                if fail_streak == threshold
                    && !unresp.swap(true, std::sync::atomic::Ordering::SeqCst)
                {
                    let _ = tx.send(KernelHealthEvent::Unresponsive {
                        reason: format!("{threshold} consecutive probe failures"),
                    });
                }
            }
        }
    }

    /// Two consecutive failures must fire exactly one `Unresponsive`
    /// event, and the next success must fire exactly one `Healthy`.
    #[tokio::test]
    async fn supervisor_emits_unresponsive_then_healthy() {
        let (tx, mut rx) = broadcast::channel::<KernelHealthEvent>(8);
        // Schedule of probe results across 5 ticks: fail, fail, success, success, success.
        let schedule = [false, false, true, true, true];
        let probe = move |n: u32| schedule[n as usize];
        run_heartbeat_until("ignored".into(), 2, probe, 5, tx.clone()).await;
        drop(tx);

        let mut events = Vec::new();
        while let Ok(e) = rx.try_recv() {
            events.push(e);
        }
        assert_eq!(events.len(), 2, "got {events:?}");
        assert!(matches!(events[0], KernelHealthEvent::Unresponsive { .. }));
        assert!(matches!(events[1], KernelHealthEvent::Healthy));
    }

    /// One failure followed by recovery never crosses the threshold and
    /// must not emit anything.
    #[tokio::test]
    async fn supervisor_single_failure_is_silent() {
        let (tx, mut rx) = broadcast::channel::<KernelHealthEvent>(8);
        let schedule = [false, true, true];
        let probe = move |n: u32| schedule[n as usize];
        run_heartbeat_until("ignored".into(), 2, probe, 3, tx.clone()).await;
        drop(tx);
        assert!(rx.try_recv().is_err());
    }

    /// Multiple consecutive failures after the threshold must still only
    /// fire `Unresponsive` once.
    #[tokio::test]
    async fn supervisor_dedupe_unresponsive() {
        let (tx, mut rx) = broadcast::channel::<KernelHealthEvent>(8);
        let schedule = [false, false, false, false, false];
        let probe = move |n: u32| schedule[n as usize];
        run_heartbeat_until("ignored".into(), 2, probe, 5, tx.clone()).await;
        drop(tx);
        let mut events = Vec::new();
        while let Ok(e) = rx.try_recv() {
            events.push(e);
        }
        assert_eq!(events.len(), 1, "expected exactly one Unresponsive: {events:?}");
        assert!(matches!(events[0], KernelHealthEvent::Unresponsive { .. }));
    }
}
