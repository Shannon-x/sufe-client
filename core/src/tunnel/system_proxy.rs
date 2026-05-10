//! Desktop fallback: drive the OS-level proxy settings via the `sysproxy`
//! crate (winreg + WinINET on Windows, system-configuration on macOS,
//! gsettings/iptools on Linux).
//!
//! Used by `KernelManager` only when TUN privilege escalation fails
//! (e.g. xboard-svc not installed, user denied SMAppService prompt, no
//! `cap_net_admin`). In TUN mode mihomo captures all egress and we leave
//! the system proxy alone.

#![cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]

use std::io;

use super::proxy_settings::{ProxyEndpoint, SystemProxySetter};

/// Default `SystemProxySetter` implementation backed by `sysproxy`.
#[derive(Debug, Default)]
pub struct DefaultSystemProxy;

impl DefaultSystemProxy {
    pub fn new() -> Self {
        Self
    }
}

impl SystemProxySetter for DefaultSystemProxy {
    fn set(&self, ep: &ProxyEndpoint) -> io::Result<()> {
        let cfg = sysproxy::Sysproxy {
            enable: true,
            host: ep.host.clone(),
            port: ep.port,
            bypass: ep.bypass.join(","),
        };
        cfg.set_system_proxy()
            .map_err(|e| io::Error::other(format!("sysproxy set: {e}")))
    }

    fn clear(&self) -> io::Result<()> {
        // Read the current setting so we preserve host/port/bypass that other
        // tools may have written, then flip `enable=false`. Falls back to a
        // sane no-op record if reading fails.
        let mut cfg = sysproxy::Sysproxy::get_system_proxy().unwrap_or(sysproxy::Sysproxy {
            enable: false,
            host: String::new(),
            port: 0,
            bypass: String::new(),
        });
        cfg.enable = false;
        cfg.set_system_proxy()
            .map_err(|e| io::Error::other(format!("sysproxy clear: {e}")))
    }
}
