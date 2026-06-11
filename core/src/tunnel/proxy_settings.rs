//! Cross-platform system proxy setter.
//!
//! Exposes the public surface today; per-OS implementations (`networksetup`
//! on macOS, `gsettings`/`kwriteconfig` on Linux, WinINET on Windows) land
//! with the desktop UI work.

#[derive(Debug, Clone)]
pub struct ProxyEndpoint {
    pub host: String,
    pub port: u16,
    /// e.g. `"localhost"`, `"127.0.0.1"`, `"::1"`, `"<local>"`.
    pub bypass: Vec<String>,
}

pub trait SystemProxySetter: Send + Sync + std::fmt::Debug {
    fn set(&self, endpoint: &ProxyEndpoint) -> std::io::Result<()>;
    fn clear(&self) -> std::io::Result<()>;

    /// Whether the live OS proxy is currently enabled AND pointing at
    /// `endpoint` (host+port). Used by the guard monitor to detect drift
    /// caused by other apps overwriting the system proxy. Default `true`
    /// disables guarding for setters that can't read back state.
    fn matches(&self, _endpoint: &ProxyEndpoint) -> bool {
        true
    }
}
