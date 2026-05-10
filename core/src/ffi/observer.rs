//! State observer + TUN delegate callback adapters.
//!
//! Both surfaces are *callback interfaces* in the UDL (Rust → host calls).
//! The Rust traits below are what the UniFFI scaffolding wires up; concrete
//! implementations come from Kotlin / Swift on mobile and from the Tauri
//! shell on desktop (the desktop wrapper just forwards to a `tauri::Emitter`
//! event so the Vue side keeps using its existing `connection.state` listener).

use std::sync::Arc;

use super::errors::TunnelError;
use super::types::{ConnectionState, TunConfig};

/// UDL `callback interface StateObserver`. Implementations MUST be cheap
/// — the broadcast fan-out task ignores their return value, blocking here
/// just back-pressures the channel and lets state frames get dropped.
pub trait StateObserver: Send + Sync + std::fmt::Debug {
    fn on_state(&self, state: ConnectionState);
}

/// UDL `callback interface TunDelegate`. Mobile shells own the actual TUN
/// fd through `VpnService.Builder.establish()` (Android) or
/// `NEPacketTunnelProvider.setTunnelNetworkSettings` (iOS); the kernel
/// manager only needs the resulting integer fd to hand to mihomo /
/// sing-box. `close_tun` is always best-effort — the OS reclaims the fd
/// when the host process unwinds the VpnService / NE entry point.
pub trait TunDelegate: Send + Sync + std::fmt::Debug {
    fn establish_tun(&self, config: TunConfig) -> std::result::Result<i32, TunnelError>;
    fn close_tun(&self);
}

/// Helper invoked by the FFI manager on every state-machine transition.
/// Iterates over the registered observers and calls `on_state` on each.
/// Holds an `RwLock<Vec<Arc<dyn StateObserver>>>` rather than a single
/// observer so the desktop shell + a future tray icon listener can
/// subscribe independently.
#[derive(Debug, Default)]
pub(crate) struct StateFanout {
    observers: parking_lot::RwLock<Vec<Arc<dyn StateObserver>>>,
}

impl StateFanout {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&self, obs: Arc<dyn StateObserver>) {
        self.observers.write().push(obs);
    }

    pub fn clear(&self) {
        self.observers.write().clear();
    }

    pub fn emit(&self, state: ConnectionState) {
        // Snapshot the observer list so callbacks may run without holding
        // the write lock — observers are free to themselves call
        // `subscribe_state` / `unsubscribe_state` if they ever want to.
        let snapshot = self.observers.read().clone();
        for obs in snapshot {
            obs.on_state(state.clone());
        }
    }
}
