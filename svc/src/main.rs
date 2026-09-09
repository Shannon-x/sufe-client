//! Windows TUN broker with authenticated IPC and protected configuration.
#[cfg(windows)]
mod ipc_probe;
#[cfg(windows)]
mod probe;
#[cfg(windows)]
mod runtime;
#[cfg(windows)]
mod security;
#[cfg(windows)]
mod service;
#[cfg(windows)]
fn main() -> anyhow::Result<()> {
    service::main()
}
#[cfg(not(windows))]
fn main() {
    eprintln!("xboard-svc is a Windows-only service");
}
