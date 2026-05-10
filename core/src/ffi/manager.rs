//! `interface ConnectionManager` backing implementation.
//!
//! Thin wrapper around [`crate::kernel::KernelManager`] that adapts its
//! Rust-native types into the UDL ones, and forwards every state change to
//! the [`super::observer::StateFanout`] so registered host observers see
//! Compose `StateFlow` / SwiftUI `@Observable` updates.
//!
//! Mobile note (M4 follow-up): the constructor accepts an optional
//! `TunDelegate` for VpnService / NEPacketTunnelProvider integration, but
//! the connect path doesn't consult it yet — the launcher is currently
//! always `DirectLauncher`, which on Android falls through to the kernel
//! manager's SystemProxy fallback. M4 will introduce a mobile-specific
//! launcher that drives the host TunDelegate to produce a real fd before
//! spawning mihomo.

use std::path::PathBuf;
use std::sync::Arc;

use futures::stream::StreamExt;
use parking_lot::Mutex;
use tokio::task::JoinHandle;

use super::client::Client;
use super::errors::FfiError;
use super::observer::{StateFanout, StateObserver, TunDelegate};
use super::types::{
    ConnectionState as FfiConnectionState, ProxyGroup, TrafficStats, TunnelMode,
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
    /// Held for future M4 mobile integration. The desktop / current-M3 connect
    /// path doesn't consult it; the launcher is always `DirectLauncher` here.
    _tun_delegate: Option<Arc<dyn TunDelegate>>,
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
        let launcher = Arc::new(
            DirectLauncher::new().with_binary_hint(binary_path.clone()),
        );
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
        let mut stream = inner.subscribe_state();
        let fanout_clone = fanout.clone();
        let task = tokio::spawn(async move {
            while let Some(state) = stream.next().await {
                fanout_clone.emit(FfiConnectionState::from(state));
            }
        });

        let tun_delegate: Option<Arc<dyn TunDelegate>> = tun_delegate.map(Arc::from);

        Ok(Self {
            client,
            inner,
            fanout,
            fanout_task: Mutex::new(Some(task)),
            _tun_delegate: tun_delegate,
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
        // Fetch the active subscribe URL. The bearer is set on the shared
        // HTTP client at login / hydrate, so this works without any extra
        // wiring as long as the caller is authenticated.
        let info = self.client.http_client().user_subscribe().await?;
        self.inner.connect(&info.subscribe_url).await?;
        Ok(())
    }

    pub async fn disconnect(&self) -> Result<(), FfiError> {
        self.inner.disconnect().await?;
        Ok(())
    }

    pub fn set_tunnel_mode(&self, mode: TunnelMode) -> Result<(), FfiError> {
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

impl Drop for ConnectionManager {
    fn drop(&mut self) {
        if let Some(task) = self.fanout_task.lock().take() {
            task.abort();
        }
    }
}
