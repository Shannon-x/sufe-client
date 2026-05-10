//! IPC layer between the UI process and the kernel subprocess.
//!
//! Today this is just the mihomo external-controller HTTP client embedded in
//! [`crate::kernel::MihomoDriver`]. The trait will grow when we bridge the
//! Android `VpnService` tun fd or add IPC-based log streaming.
