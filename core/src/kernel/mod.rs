//! Kernel abstraction & implementations.
//!
//! UI code interacts only with [`KernelDriver`]; specific kernels (mihomo,
//! xray, ...) are drop-in implementations that satisfy the trait.

pub mod driver;
pub mod ipc;
pub mod launcher;
pub mod manager;
pub mod mihomo;
pub mod xray;

pub use driver::{
    ConnectionItem, KernelConfig, KernelDriver, KernelKind, LogLine, ProxyGroup, RuleItem,
    TrafficStats,
};
pub use launcher::{
    DirectLauncher, KernelFailure, KernelLauncher, KernelSpawnSpec, LaunchHandle, LauncherError,
};
pub use manager::{
    ConnectStage, ConnectionState, KernelHealthEvent, KernelManager, TunnelMode,
};
pub use mihomo::MihomoDriver;
pub use xray::XrayDriver;

#[cfg(target_os = "linux")]
pub use launcher::linux_caps;
#[cfg(target_os = "macos")]
pub use launcher::{HelperInstaller, HelperSocketLauncher};
#[cfg(target_os = "windows")]
pub use launcher::{SvcInstaller, SvcPipeLauncher};
