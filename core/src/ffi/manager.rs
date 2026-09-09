//! `interface ConnectionManager` backing implementation.
//!
//! Thin wrapper around [`crate::kernel::KernelManager`] that adapts its
//! Rust-native types into the UDL ones, and forwards every state change to
//! the [`super::observer::StateFanout`] so registered host observers see
//! Compose `StateFlow` / SwiftUI `@Observable` updates.
//!
//! Mobile note: when a `TunDelegate` is supplied (Android `VpnService` / iOS
//! NE), the connect path now drives it — it asks the host to `establish_tun`,
//! hands the resulting fd to the [`KernelManager`] via `set_tun_fd`, and the
//! mihomo config is patched to adopt that fd (`file-descriptor`, no
//! `auto-route`). Desktop passes no delegate and mihomo opens its own device.

use std::path::PathBuf;
use std::sync::Arc;

use futures::stream::StreamExt;
use parking_lot::Mutex;
use tokio::task::JoinHandle;

use super::client::Client;
use super::errors::FfiError;
use super::observer::{StateFanout, StateObserver, TunDelegate};
use super::types::{
    ConnectionState as FfiConnectionState, ProxyGroup, TrafficStats, TunConfig, TunnelMode,
};
use crate::kernel::launcher::DirectLauncher;
use crate::kernel::KernelManager;
use crate::kernel::MihomoDriver;
use crate::profile::ProfileFetcher;

#[derive(Debug)]
pub struct ConnectionManager {
    client: Arc<Client>,
    inner: Arc<KernelManager>,
    fanout: Arc<StateFanout>,
    fanout_task: Mutex<Option<JoinHandle<()>>>,
    /// Host TUN factory (Android VpnService / iOS NE). When present, `connect`
    /// asks it for a fd and feeds it to the kernel manager; `None` on desktop.
    tun_delegate: Option<Arc<dyn TunDelegate>>,
    operation: Arc<tokio::sync::Mutex<()>>,
    runtime: tokio::runtime::Handle,
}

impl ConnectionManager {
    /// UDL constructor. Returns bare `Self` — UniFFI wraps interface
    /// returns in `Arc<...>` automatically, returning `Arc<Self>` here
    /// would double-wrap. `tun_delegate` arrives as `Box<dyn TunDelegate>`
    /// (UniFFI's calling convention for `callback interface` args); we
    /// re-bind to `Arc` so future M4 launchers can clone the handle into
    /// the manager state machine task.
    pub fn new(
        client: Arc<Client>,
        kernel_binary_path: String,
        work_dir: String,
        cache_dir: String,
        tun_delegate: Option<Box<dyn TunDelegate>>,
    ) -> Result<Self, FfiError> {
        let binary_path = PathBuf::from(kernel_binary_path);
        let work_dir = PathBuf::from(work_dir);
        let cache_dir = PathBuf::from(cache_dir);

        let driver = Arc::new(MihomoDriver::new());
        let launcher = Arc::new(DirectLauncher::new().with_binary_hint(binary_path.clone()));
        let fetcher = ProfileFetcher::new(client.http_client(), cache_dir);

        let inner = Arc::new(KernelManager::new(
            driver,
            launcher,
            None, // proxy_setter — mobile doesn't have a system proxy concept
            fetcher,
            binary_path,
            work_dir,
        ));

        let fanout = Arc::new(StateFanout::new());

        // Spawn the broadcast → fanout forwarder once at construction. We can't
        // do this lazily inside `subscribe_state` because UniFFI calls that
        // method synchronously (no `.await`) — and we want the fanout to see
        // every state change, not only those that happen after a subscriber
        // joined.
        let tun_delegate: Option<Arc<dyn TunDelegate>> = tun_delegate.map(Arc::from);
        let delegate_for_state = tun_delegate.clone();
        let operation = Arc::new(tokio::sync::Mutex::new(()));
        let operation_for_state = operation.clone();
        let inner_for_state = inner.clone();
        let mut stream = inner.subscribe_state();
        let fanout_clone = fanout.clone();
        // UniFFI constructors run synchronously on the Kotlin/Swift caller,
        // outside a Tokio context. Spawning directly here used to panic on
        // the first Android connection. Keep a process-lifetime fallback.
        static MOBILE_RUNTIME: once_cell::sync::OnceCell<tokio::runtime::Runtime> =
            once_cell::sync::OnceCell::new();
        let runtime = match tokio::runtime::Handle::try_current() {
            Ok(handle) => handle,
            Err(_) => MOBILE_RUNTIME
                .get_or_try_init(|| {
                    tokio::runtime::Builder::new_multi_thread()
                        .worker_threads(2)
                        .enable_all()
                        .thread_name("sufe-mobile")
                        .build()
                })
                .map_err(|e| FfiError::Io(e.to_string()))?
                .handle()
                .clone(),
        };
        let task = runtime.spawn(async move {
            while let Some(state) = stream.next().await {
                if matches!(
                    state,
                    crate::kernel::manager::ConnectionState::Error { .. }
                        | crate::kernel::manager::ConnectionState::Disconnected
                ) {
                    let _operation = operation_for_state.lock().await;
                    // An older failure event must never close a newer VPN.
                    if matches!(
                        inner_for_state.state(),
                        crate::kernel::manager::ConnectionState::Error { .. }
                            | crate::kernel::manager::ConnectionState::Disconnected
                    ) {
                        if let Some(delegate) = &delegate_for_state {
                            delegate.close_tun();
                        }
                        inner_for_state.set_tun_fd(None);
                    }
                }
                fanout_clone.emit(FfiConnectionState::from(state));
            }
        });

        Ok(Self {
            client,
            inner,
            fanout,
            fanout_task: Mutex::new(Some(task)),
            tun_delegate,
            operation,
            runtime,
        })
    }

    pub fn subscribe_state(&self, observer: Box<dyn StateObserver>) {
        // Replay the current state once so a fresh observer doesn't sit on
        // `Disconnected` until the next transition.
        let observer: Arc<dyn StateObserver> = Arc::from(observer);
        let snap = FfiConnectionState::from(self.inner.state());
        observer.on_state(snap);
        self.fanout.push(observer);
    }

    pub fn unsubscribe_state(&self) {
        self.fanout.clear();
    }

    pub async fn connect(&self) -> Result<(), FfiError> {
        let _operation = self.operation.lock().await;
        if matches!(
            self.inner.state(),
            crate::kernel::manager::ConnectionState::Connected { .. }
        ) {
            return Ok(());
        }
        self.client.require_session()?;
        let generation = self.client.generation();
        let custom_rules = self.client.rules_for_connection().await?;
        self.inner.set_custom_rules(custom_rules).await?;
        // Fetch the active subscribe URL. The bearer is set on the shared
        // HTTP client at login / hydrate, so this works without any extra
        // wiring as long as the caller is authenticated.
        let info = self.client.http_client().user_subscribe().await?;
        self.client.require_session()?;
        if self.client.generation() != generation {
            return Err(FfiError::Unauthorized);
        }

        // Mobile: have the host stand up the VpnService / NE tunnel and give
        // us its fd, then tell the kernel manager to adopt it. Without this
        // mihomo would try to create its own /dev/net/tun device — denied in
        // the app sandbox, so the connection could never establish.
        if let Some(delegate) = self.tun_delegate.as_ref() {
            if matches!(self.requested_mode(), TunnelMode::Tun) {
                let fd = delegate.establish_tun(default_tun_config())?;
                if fd < 3 {
                    delegate.close_tun();
                    return Err(FfiError::Kernel(
                        "VPN service returned an invalid TUN descriptor".into(),
                    ));
                }
                self.inner.set_tun_fd(Some(fd));
            }
        }

        if let Err(e) = self.inner.connect(&info.subscribe_url).await {
            // Roll back the host tunnel if the kernel failed to come up, so we
            // don't strand an established VpnService with no kernel behind it.
            if let Some(delegate) = self.tun_delegate.as_ref() {
                delegate.close_tun();
                self.inner.set_tun_fd(None);
            }
            return Err(e.into());
        }
        if self.client.require_session().is_err() || self.client.generation() != generation {
            let _ = self.inner.disconnect().await;
            if let Some(delegate) = &self.tun_delegate {
                delegate.close_tun();
            }
            self.inner.set_tun_fd(None);
            return Err(FfiError::Unauthorized);
        }
        Ok(())
    }

    pub async fn disconnect(&self) -> Result<(), FfiError> {
        let _operation = self.operation.lock().await;
        let result = self.inner.disconnect().await;
        if let Some(delegate) = self.tun_delegate.as_ref() {
            delegate.close_tun();
            self.inner.set_tun_fd(None);
        }
        result.map_err(Into::into)
    }

    pub fn set_tunnel_mode(&self, mode: TunnelMode) -> Result<(), FfiError> {
        if self.tun_delegate.is_some() && matches!(mode, TunnelMode::SystemProxy) {
            return Err(FfiError::Config("移动端仅支持 VPN 隧道模式".into()));
        }
        self.inner.set_requested_mode(mode.into());
        Ok(())
    }

    pub fn requested_mode(&self) -> TunnelMode {
        self.inner.requested_mode().into()
    }

    pub fn current_state(&self) -> FfiConnectionState {
        self.inner.state().into()
    }

    pub async fn proxies(&self) -> Result<Vec<ProxyGroup>, FfiError> {
        let raw = self.inner.proxies().await?;
        Ok(raw.into_iter().map(ProxyGroup::from).collect())
    }

    pub async fn select_proxy(&self, group: String, node: String) -> Result<(), FfiError> {
        self.inner.select_proxy(&group, &node).await?;
        Ok(())
    }

    pub async fn latency_test(&self, node: String) -> Result<u32, FfiError> {
        Ok(self.inner.latency_test(&node).await?)
    }

    pub async fn current_traffic(&self) -> Result<TrafficStats, FfiError> {
        Ok(self.inner.current_traffic().await?.into())
    }
}

/// Default TUN parameters handed to the host VpnService / NE when we ask it
/// to stand up the interface. Mirrors the addressing the sing-box translator
/// uses (`172.19.0.1/30`) and a full-tunnel route; the host applies these via
/// `VpnService.Builder` / `NEPacketTunnelNetworkSettings`.
fn default_tun_config() -> TunConfig {
    TunConfig {
        session: "Sufe".to_string(),
        ipv4_addr: "172.19.0.1".to_string(),
        ipv4_prefix: 30,
        routes: vec!["0.0.0.0/0".to_string()],
        dns: vec!["1.1.1.1".to_string(), "223.5.5.5".to_string()],
        mtu: 1500,
    }
}

impl Drop for ConnectionManager {
    fn drop(&mut self) {
        if let Some(task) = self.fanout_task.lock().take() {
            task.abort();
        }
        let inner = self.inner.clone();
        let delegate = self.tun_delegate.clone();
        self.runtime.spawn(async move {
            let _ = inner.disconnect().await;
            if let Some(delegate) = delegate {
                delegate.close_tun();
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ffi::{SecureStore, StorageError};

    #[derive(Debug)]
    struct EmptyStore;
    impl SecureStore for EmptyStore {
        fn get(&self, _: String) -> std::result::Result<Option<String>, StorageError> {
            Ok(None)
        }
        fn put(&self, _: String, _: String) -> std::result::Result<(), StorageError> {
            Ok(())
        }
        fn delete(&self, _: String) -> std::result::Result<(), StorageError> {
            Ok(())
        }
    }

    #[test]
    fn synchronous_mobile_constructor_does_not_require_a_tokio_context() {
        assert!(tokio::runtime::Handle::try_current().is_err());
        let client = Arc::new(
            Client::new(
                "https://example.invalid".into(),
                "zh-CN".into(),
                Box::new(EmptyStore),
            )
            .unwrap(),
        );
        let path = std::env::temp_dir().join(format!("sufe-ffi-{}", uuid::Uuid::new_v4()));
        let manager = ConnectionManager::new(
            client,
            path.join("mihomo").to_string_lossy().into(),
            path.to_string_lossy().into(),
            path.join("cache").to_string_lossy().into(),
            None,
        )
        .unwrap();
        assert!(matches!(
            manager.current_state(),
            FfiConnectionState::Disconnected
        ));
        drop(manager);
    }
}
