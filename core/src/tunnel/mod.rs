//! Platform-specific tunnel adapters (system proxy / VPN / TUN).
//!
//! Phase 1 ships system-proxy on desktop and a thin `VpnService` bridge on
//! Android. TUN with privileged helpers is reserved for Phase 6.

pub mod proxy_settings;

#[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
pub mod system_proxy;

pub use proxy_settings::{ProxyEndpoint, SystemProxySetter};

#[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
pub use system_proxy::DefaultSystemProxy;
