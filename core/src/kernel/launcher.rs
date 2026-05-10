//! Privilege adapter and process owner for the kernel subprocess.
//!
//! TUN-first connectivity needs the kernel to bind a privileged tunnel
//! device (wintun on Windows, utun on macOS, /dev/net/tun on Linux).
//! Each platform reaches that privilege differently:
//!
//! - Windows: an out-of-process service (`xboard-svc`) running as
//!   LocalSystem; clients send commands over a named pipe.
//! - macOS: a LaunchDaemon (`xboard-helper`) installed via SMAppService;
//!   clients send commands over a Unix socket.
//! - Linux: deb/rpm install scripts grant `cap_net_admin` to the mihomo
//!   binary via `setcap`, so the launching process needs no extra rights.
//!
//! `KernelLauncher` is the trait the [`super::manager::KernelManager`] uses
//! to (1) **probe** whether the privileged path is currently usable and
//! (2) actually **spawn / stop** the kernel subprocess. The split lets the
//! manager fall back to `TunnelMode::SystemProxy` from `ensure_privileged()`
//! before paying the cost of writing a config and forking.
//!
//! After this refactor the launcher fully owns the mihomo `Child`:
//! [`super::mihomo::MihomoDriver`] is now an *attach-only* control client
//! that talks to mihomo's External Controller HTTP API but no longer
//! supervises a process. The Win / Mac stubs return `Unsupported` for
//! `spawn` until the `svc/` workspace crate ships; the manager's mode
//! probe path keeps users on SystemProxy until then.

use std::fmt::Debug;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use async_trait::async_trait;
use parking_lot::Mutex;
use reqwest::Client;
use tokio::process::{Child, Command};

/// Outcome of [`KernelLauncher::ensure_privileged`] / [`KernelLauncher::spawn`].
/// The manager turns `NeedsConsent` / `ServiceMissing` / `NotPermitted` /
/// `Unsupported` into a transparent fallback to `TunnelMode::SystemProxy`;
/// everything else surfaces as an error to the UI.
#[derive(Debug)]
pub enum LauncherError {
    /// The platform asks the user to approve a system-level prompt that we
    /// cannot pre-empt (UAC, SMAppService dialog). The manager downgrades
    /// to SystemProxy until the user approves out-of-band.
    NeedsConsent(String),
    /// The backing privileged process (Windows service / macOS helper) is
    /// not installed. Same fallback as `NeedsConsent`.
    ServiceMissing(String),
    /// The OS reports we lack the required capability or right. On Linux
    /// this is the `setcap` path missing on the mihomo binary.
    NotPermitted(String),
    /// This launcher is not implemented on the current platform / build.
    Unsupported,
    /// IPC framing error talking to the privileged side (Win/Mac only).
    Ipc(String),
    /// mihomo started but didn't respond on the External Controller within
    /// the wait window. The manager treats this as a hard error rather than
    /// silently retrying — usually means a config bug.
    StartTimeout,
    Io(std::io::Error),
    Other(String),
}

impl std::fmt::Display for LauncherError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LauncherError::NeedsConsent(s) => write!(f, "user consent required: {s}"),
            LauncherError::ServiceMissing(s) => write!(f, "privileged service not installed: {s}"),
            LauncherError::NotPermitted(s) => write!(f, "operation not permitted: {s}"),
            LauncherError::Unsupported => write!(f, "launcher not supported on this platform"),
            LauncherError::Ipc(s) => write!(f, "ipc: {s}"),
            LauncherError::StartTimeout => write!(f, "kernel did not respond within timeout"),
            LauncherError::Io(e) => write!(f, "io: {e}"),
            LauncherError::Other(s) => write!(f, "{s}"),
        }
    }
}

impl std::error::Error for LauncherError {}

impl From<std::io::Error> for LauncherError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

/// All inputs the privileged side needs to fork mihomo. Built by the
/// manager from its own `binary_path` / `work_dir` plus the patched YAML.
/// Kept platform-agnostic — Win svc / macOS helper deserialise this
/// straight off the wire.
#[derive(Debug, Clone)]
pub struct KernelSpawnSpec {
    /// Absolute path to the mihomo binary (sidecar in production, dev
    /// target dir in `tauri dev`).
    pub exec_path: PathBuf,
    /// Working directory passed to mihomo via `-d`. Will be created if
    /// missing. Stores `cache.db` and other runtime artefacts.
    pub work_dir: PathBuf,
    /// Path to the patched YAML config, passed to mihomo via `-f`.
    pub cfg_path: PathBuf,
    /// stdout/stderr destination. mihomo's structured logs go through the
    /// External Controller `/logs` endpoint; this captures any panics or
    /// startup-phase output that pre-dates the controller binding.
    pub log_path: PathBuf,
    /// Address of the External Controller, e.g. `127.0.0.1:9090`. The
    /// launcher uses it post-fork to wait for `/version` before declaring
    /// the kernel healthy.
    pub controller_addr: String,
    /// Bearer secret matching `secret:` in the YAML. Required to hit
    /// `/version` once the kernel is up.
    pub controller_secret: String,
}

/// Opaque handle returned by [`KernelLauncher::spawn`]. The manager stores
/// it and hands it back to [`KernelLauncher::stop`] on disconnect. The
/// variant only matters to the launcher implementation; everything else
/// treats it as a black box.
#[derive(Debug)]
pub enum LaunchHandle {
    /// Direct spawn (Linux, or Windows/macOS dev mode). The launcher holds
    /// the actual `Child`; the handle just carries identity for logs.
    Local { pid: u32 },
    /// Privileged spawn (Win svc / Mac helper). The kernel runs in a
    /// different process tree; the handle carries the IPC endpoint we
    /// use to send `StopKernel`.
    Remote {
        ipc_path: String,
        pid: Option<u32>,
    },
}

#[async_trait]
pub trait KernelLauncher: Send + Sync + Debug {
    /// Probe whether the privileged path is usable *right now* without
    /// actually spawning anything. Manager calls this before every connect
    /// attempt to decide TUN vs. SystemProxy.
    async fn ensure_privileged(&self) -> Result<(), LauncherError>;

    /// Spawn mihomo against the given spec. On success the launcher must
    /// have verified that the External Controller responds — the manager
    /// will hit it immediately after this returns.
    async fn spawn(&self, spec: KernelSpawnSpec) -> Result<LaunchHandle, LauncherError>;

    /// Stop the previously-spawned kernel. Idempotent: calling on a
    /// detached handle is a no-op.
    async fn stop(&self, handle: LaunchHandle) -> Result<(), LauncherError>;

    /// Human-readable name for logs (`direct`, `svc-pipe`, `helper-socket`).
    fn name(&self) -> &'static str;
}

/// Phase-1 launcher: spawn the kernel directly from this process. Works
/// on Linux when the deb/rpm postinst granted `cap_net_admin` to mihomo,
/// and during desktop development on macOS/Windows when the user runs
/// the app from an already-elevated terminal or accepts that the kernel
/// will fall back to SystemProxy mode (no TUN device needed).
#[derive(Debug)]
pub struct DirectLauncher {
    /// At most one mihomo per launcher instance; new spawns wait for the
    /// previous one to be `stop`-ed first.
    child: Mutex<Option<Child>>,
    /// Path to the kernel binary, used by the Linux capability probe in
    /// `ensure_privileged`. Optional because non-Linux hosts don't need it
    /// and tests construct the launcher without one. Set via
    /// [`DirectLauncher::with_binary_hint`].
    binary_hint: Option<PathBuf>,
}

impl Default for DirectLauncher {
    fn default() -> Self {
        Self::new()
    }
}

impl DirectLauncher {
    pub fn new() -> Self {
        Self {
            child: Mutex::new(None),
            binary_hint: None,
        }
    }

    /// Tell the launcher where the kernel binary lives so the Linux
    /// `ensure_privileged` step can read its file capabilities and report
    /// `NotPermitted("setcap missing")` instead of letting the spawn
    /// silently fail with EPERM after we already started writing config.
    /// No-op on non-Linux hosts.
    pub fn with_binary_hint(mut self, binary_path: PathBuf) -> Self {
        self.binary_hint = Some(binary_path);
        self
    }
}

#[async_trait]
impl KernelLauncher for DirectLauncher {
    async fn ensure_privileged(&self) -> Result<(), LauncherError> {
        // Linux deb/rpm postinst grants `cap_net_admin` to the mihomo binary
        // so a normal-user spawn opens `/dev/net/tun` fine. macOS / Windows
        // don't have an equivalent yet — until `xboard-helper` /
        // `xboard-svc` ship, advertise NotPermitted so the manager falls
        // back to SystemProxy (which doesn't need elevation).
        #[cfg(target_os = "linux")]
        {
            // If the caller didn't tell us where the binary lives we can't
            // probe; assume the postinst ran and let `spawn` surface any
            // late EPERM. This preserves the legacy behaviour for tests
            // and anything that constructs DirectLauncher without a hint.
            let Some(path) = self.binary_hint.as_ref() else {
                return Ok(());
            };
            if !path.exists() {
                // The "kernel binary missing" diagnostic belongs to the
                // KernelHealth probe; here we just decline to gate.
                return Ok(());
            }
            if linux_caps::has_file_capability(path) {
                Ok(())
            } else {
                Err(LauncherError::NotPermitted(format!(
                    "setcap missing on {} — install the deb/rpm package or run \
                     `sudo setcap cap_net_admin,cap_net_bind_service+ep {}` to enable TUN mode",
                    path.display(),
                    path.display(),
                )))
            }
        }
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        {
            Err(LauncherError::NotPermitted(
                "TUN 模式需要 xboard-helper / xboard-svc 特权进程，当前版本尚未提供".into(),
            ))
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
        {
            Ok(())
        }
    }

    async fn spawn(&self, spec: KernelSpawnSpec) -> Result<LaunchHandle, LauncherError> {
        if !spec.exec_path.exists() {
            return Err(LauncherError::Other(format!(
                "kernel binary missing: {}",
                spec.exec_path.display()
            )));
        }

        // Bail out early if we still hold a live child — caller forgot to
        // stop the previous run.
        if self.child.lock().is_some() {
            return Err(LauncherError::Other(
                "another kernel is already running under this launcher".into(),
            ));
        }

        tokio::fs::create_dir_all(&spec.work_dir).await?;
        if let Some(parent) = spec.log_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let log_file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&spec.log_path)?;
        let log_clone = log_file.try_clone()?;

        let mut cmd = Command::new(&spec.exec_path);
        cmd.arg("-d")
            .arg(&spec.work_dir)
            .arg("-f")
            .arg(&spec.cfg_path)
            .stdin(Stdio::null())
            .stdout(Stdio::from(log_file))
            .stderr(Stdio::from(log_clone))
            .kill_on_drop(true);

        let child = cmd.spawn()?;
        let pid = child.id().unwrap_or(0);
        *self.child.lock() = Some(child);

        match wait_for_controller(&spec.controller_addr, &spec.controller_secret).await {
            Ok(()) => Ok(LaunchHandle::Local { pid }),
            Err(e) => {
                let taken = self.child.lock().take();
                if let Some(mut c) = taken {
                    let _ = c.kill().await;
                    let _ = c.wait().await;
                }
                Err(e)
            }
        }
    }

    async fn stop(&self, _handle: LaunchHandle) -> Result<(), LauncherError> {
        let taken = self.child.lock().take();
        if let Some(mut c) = taken {
            let _ = c.kill().await;
            let _ = c.wait().await;
        }
        Ok(())
    }

    fn name(&self) -> &'static str {
        "direct"
    }
}

/// Poll the External Controller's `/version` endpoint until it responds OK
/// or the 5 s budget elapses. The `secret` is required because the patched
/// YAML always sets one (manager generates a fresh hex secret per session).
async fn wait_for_controller(addr: &str, secret: &str) -> Result<(), LauncherError> {
    let client = Client::builder()
        .timeout(Duration::from_millis(500))
        .build()
        .map_err(|e| LauncherError::Other(format!("reqwest client: {e}")))?;
    let url = format!("http://{addr}/version");
    let auth = format!("Bearer {secret}");

    for _ in 0..50 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        match client
            .get(&url)
            .header("Authorization", &auth)
            .send()
            .await
        {
            Ok(r) if r.status().is_success() => return Ok(()),
            // Anything else (connection refused, 401, 500) → keep polling.
            _ => continue,
        }
    }
    Err(LauncherError::StartTimeout)
}

/// Windows named-pipe launcher: connects to `xboard-svc` over the named
/// pipe at [`super::ipc::SVC_PIPE_PATH`], asks it to spawn mihomo as
/// LocalSystem so the kernel can create a wintun adapter. The service is
/// installed (once) on first connect via an elevated `xboard-svc.exe install`
/// invocation that the Tauri shell drives — see
/// `desktop/src-tauri/src/svc_install.rs`.
///
/// The shape mirrors `HelperSocketLauncher` on macOS deliberately: the IPC
/// frames are the same `Frame` / `Request` / `Response` enums, only the
/// transport differs (named pipe vs Unix socket). Wins / losses are
/// translated into the same `LauncherError` variants so the
/// `KernelManager`'s SystemProxy-fallback logic doesn't need to special-case
/// the platform.
#[cfg(target_os = "windows")]
#[derive(Debug)]
pub struct SvcPipeLauncher {
    pipe_path: String,
    next_id: parking_lot::Mutex<u64>,
    /// Optional callback invoked when the service is missing. Returns `Ok`
    /// once `xboard-svc` has been installed and is reachable.
    installer: Option<std::sync::Arc<dyn SvcInstaller>>,
}

#[cfg(target_os = "windows")]
impl Default for SvcPipeLauncher {
    fn default() -> Self {
        Self::new()
    }
}

/// Strategy plug for installing `xboard-svc` on first run. Tauri injects a
/// concrete impl that knows how to invoke `runas` (UAC) on the bundled
/// `xboard-svc.exe`. Same semantics as macOS `HelperInstaller`.
#[cfg(target_os = "windows")]
#[async_trait]
pub trait SvcInstaller: Send + Sync + Debug {
    async fn install(&self) -> Result<(), LauncherError>;
    async fn uninstall(&self) -> Result<(), LauncherError> {
        Err(LauncherError::Unsupported)
    }
}

#[cfg(target_os = "windows")]
impl SvcPipeLauncher {
    pub fn new() -> Self {
        Self {
            pipe_path: super::ipc::SVC_PIPE_PATH.to_string(),
            next_id: parking_lot::Mutex::new(1),
            installer: None,
        }
    }

    pub fn with_installer(mut self, installer: std::sync::Arc<dyn SvcInstaller>) -> Self {
        self.installer = Some(installer);
        self
    }

    fn next_request_id(&self) -> u64 {
        let mut g = self.next_id.lock();
        let id = *g;
        *g = id.wrapping_add(1);
        id
    }

    /// Open a fresh client connection, send one request, read one response.
    /// `ClientOptions::open` returns immediately on success; we retry briefly
    /// on `ERROR_PIPE_BUSY` to handle the race between the service finishing
    /// one client and being ready for the next.
    async fn call(&self, req: super::ipc::Request) -> Result<super::ipc::Response, LauncherError> {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
        use tokio::net::windows::named_pipe::ClientOptions;

        let mut client = loop {
            match ClientOptions::new().open(&self.pipe_path) {
                Ok(c) => break c,
                Err(e) => {
                    let raw = e.raw_os_error();
                    // ERROR_PIPE_BUSY (231): another client beat us; the docs
                    // recommend WaitNamedPipe but a short async sleep + retry
                    // is just as good for our scale.
                    if raw == Some(231) {
                        tokio::time::sleep(Duration::from_millis(50)).await;
                        continue;
                    }
                    // ERROR_FILE_NOT_FOUND (2): service isn't installed or
                    // hasn't created its first instance yet. Map to
                    // ServiceMissing so the manager + first-run installer
                    // path can take over.
                    if raw == Some(2) {
                        return Err(LauncherError::ServiceMissing(format!(
                            "xboard-svc pipe {} not reachable: {}",
                            self.pipe_path, e
                        )));
                    }
                    return match e.kind() {
                        std::io::ErrorKind::PermissionDenied => Err(LauncherError::NotPermitted(
                            format!("pipe {} access denied: {}", self.pipe_path, e),
                        )),
                        _ => Err(LauncherError::Io(e)),
                    };
                }
            }
        };

        let id = self.next_request_id();
        let frame = super::ipc::Frame::request(id, req);
        let mut line = serde_json::to_string(&frame)
            .map_err(|e| LauncherError::Ipc(format!("encode: {e}")))?;
        line.push('\n');
        client
            .write_all(line.as_bytes())
            .await
            .map_err(|e| LauncherError::Ipc(format!("write: {e}")))?;
        client
            .flush()
            .await
            .map_err(|e| LauncherError::Ipc(format!("flush: {e}")))?;

        // We can't shutdown_write on a duplex named pipe (no half-close), so
        // rely on a single-line response framing and a read timeout.
        let (read, _write) = tokio::io::split(client);
        let mut reader = BufReader::new(read);
        let mut buf = String::new();
        let read_n = tokio::time::timeout(Duration::from_secs(15), reader.read_line(&mut buf))
            .await
            .map_err(|_| LauncherError::Ipc("response timeout".into()))?
            .map_err(|e| LauncherError::Ipc(format!("read: {e}")))?;
        if read_n == 0 {
            return Err(LauncherError::Ipc("svc closed without response".into()));
        }
        let resp_frame: super::ipc::Frame = serde_json::from_str(buf.trim_end())
            .map_err(|e| LauncherError::Ipc(format!("decode: {e}")))?;
        if resp_frame.id != id && resp_frame.id != 0 {
            // id == 0 is reserved for the SID-rejection error frame the
            // service emits before reading a request, so we accept it.
            return Err(LauncherError::Ipc(format!(
                "id mismatch: sent {id}, got {}",
                resp_frame.id
            )));
        }
        resp_frame
            .into_response()
            .ok_or_else(|| LauncherError::Ipc("expected response, got request".into()))
    }
}

#[cfg(target_os = "windows")]
#[async_trait]
impl KernelLauncher for SvcPipeLauncher {
    async fn ensure_privileged(&self) -> Result<(), LauncherError> {
        match self.call(super::ipc::Request::Ping).await {
            Ok(super::ipc::Response::Pong { .. }) => Ok(()),
            Ok(other) => Err(LauncherError::Ipc(format!(
                "unexpected ping response: {other:?}"
            ))),
            Err(LauncherError::ServiceMissing(_)) => {
                if let Some(installer) = &self.installer {
                    installer.install().await?;
                    match self.call(super::ipc::Request::Ping).await {
                        Ok(super::ipc::Response::Pong { .. }) => Ok(()),
                        Ok(other) => Err(LauncherError::Ipc(format!(
                            "unexpected ping after install: {other:?}"
                        ))),
                        Err(e) => Err(e),
                    }
                } else {
                    Err(LauncherError::ServiceMissing(
                        "xboard-svc not installed".into(),
                    ))
                }
            }
            Err(e) => Err(e),
        }
    }

    async fn spawn(&self, spec: KernelSpawnSpec) -> Result<LaunchHandle, LauncherError> {
        let resp = self
            .call(super::ipc::Request::StartKernel {
                exec_path: spec.exec_path.clone(),
                work_dir: spec.work_dir.clone(),
                cfg_path: spec.cfg_path.clone(),
                log_path: spec.log_path.clone(),
            })
            .await?;
        match resp {
            super::ipc::Response::Started { pid } => {
                wait_for_controller(&spec.controller_addr, &spec.controller_secret).await?;
                Ok(LaunchHandle::Remote {
                    ipc_path: self.pipe_path.clone(),
                    pid: Some(pid),
                })
            }
            super::ipc::Response::Error { message } => Err(LauncherError::Other(format!(
                "svc rejected start: {message}"
            ))),
            other => Err(LauncherError::Ipc(format!(
                "unexpected start response: {other:?}"
            ))),
        }
    }

    async fn stop(&self, _handle: LaunchHandle) -> Result<(), LauncherError> {
        match self.call(super::ipc::Request::StopKernel).await {
            Ok(super::ipc::Response::Stopped) => Ok(()),
            Ok(super::ipc::Response::Error { message }) => Err(LauncherError::Other(format!(
                "svc rejected stop: {message}"
            ))),
            Ok(other) => Err(LauncherError::Ipc(format!(
                "unexpected stop response: {other:?}"
            ))),
            Err(LauncherError::ServiceMissing(_)) => Ok(()),
            Err(e) => Err(e),
        }
    }

    fn name(&self) -> &'static str {
        "svc-pipe"
    }
}

/// macOS LaunchDaemon launcher: connects to the `xboard-helper` Unix socket,
/// asks it to fork mihomo with root privileges so the kernel can create a
/// `utun*` device. The helper is installed once via an `osascript` admin
/// prompt on first connect — no SMAppService dependency, mirroring the
/// install pattern used by clash-verge-rev.
#[cfg(target_os = "macos")]
#[derive(Debug)]
pub struct HelperSocketLauncher {
    socket_path: std::path::PathBuf,
    next_id: parking_lot::Mutex<u64>,
    /// Optional callback invoked when the helper is missing. Returns the
    /// path to the helper binary + plist that should be installed. If
    /// `None`, `ensure_privileged` returns `ServiceMissing` and the manager
    /// falls back to SystemProxy.
    installer: Option<std::sync::Arc<dyn HelperInstaller>>,
}

/// Strategy plug for installing the helper. The Tauri shell injects a
/// concrete impl that knows where the bundled helper / plist live.
#[cfg(target_os = "macos")]
#[async_trait]
pub trait HelperInstaller: Send + Sync + Debug {
    /// Run the privileged install. Returns Ok(()) once the LaunchDaemon
    /// is loaded and accepting connections, NeedsConsent if the user
    /// dismissed the auth dialog, or another LauncherError on plumbing
    /// failures.
    async fn install(&self) -> Result<(), LauncherError>;

    /// Bootout the LaunchDaemon and remove its on-disk artefacts. Same
    /// `NeedsConsent` semantics as `install`. Used by the user-facing
    /// "卸载辅助服务" entry. Default impl returns `Unsupported` so
    /// non-macOS strategies (none today) don't have to provide one.
    async fn uninstall(&self) -> Result<(), LauncherError> {
        Err(LauncherError::Unsupported)
    }
}

#[cfg(target_os = "macos")]
impl Default for HelperSocketLauncher {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(target_os = "macos")]
impl HelperSocketLauncher {
    pub fn new() -> Self {
        Self {
            socket_path: std::path::PathBuf::from(super::ipc::HELPER_SOCKET_PATH),
            next_id: parking_lot::Mutex::new(1),
            installer: None,
        }
    }

    pub fn with_installer(mut self, installer: std::sync::Arc<dyn HelperInstaller>) -> Self {
        self.installer = Some(installer);
        self
    }

    fn next_request_id(&self) -> u64 {
        let mut g = self.next_id.lock();
        let id = *g;
        *g = id.wrapping_add(1);
        id
    }

    /// Open a fresh connection, send one request, read one response. Each
    /// call gets its own socket — connections are short-lived so we don't
    /// have to manage reconnect / heartbeat state.
    async fn call(&self, req: super::ipc::Request) -> Result<super::ipc::Response, LauncherError> {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
        use tokio::net::UnixStream;

        let stream = UnixStream::connect(&self.socket_path).await.map_err(|e| {
            match e.kind() {
                std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused => {
                    LauncherError::ServiceMissing(format!(
                        "helper socket {} not reachable: {}",
                        self.socket_path.display(),
                        e
                    ))
                }
                std::io::ErrorKind::PermissionDenied => LauncherError::NotPermitted(format!(
                    "helper socket {} access denied: {}",
                    self.socket_path.display(),
                    e
                )),
                _ => LauncherError::Io(e),
            }
        })?;
        let (read_half, mut write_half) = stream.into_split();
        let id = self.next_request_id();
        let frame = super::ipc::Frame::request(id, req);
        let mut line = serde_json::to_string(&frame)
            .map_err(|e| LauncherError::Ipc(format!("encode: {e}")))?;
        line.push('\n');
        write_half
            .write_all(line.as_bytes())
            .await
            .map_err(|e| LauncherError::Ipc(format!("write: {e}")))?;
        write_half
            .shutdown()
            .await
            .map_err(|e| LauncherError::Ipc(format!("shutdown: {e}")))?;

        let mut reader = BufReader::new(read_half);
        let mut buf = String::new();
        let read = tokio::time::timeout(Duration::from_secs(15), reader.read_line(&mut buf))
            .await
            .map_err(|_| LauncherError::Ipc("response timeout".into()))?
            .map_err(|e| LauncherError::Ipc(format!("read: {e}")))?;
        if read == 0 {
            return Err(LauncherError::Ipc("helper closed without response".into()));
        }
        let resp_frame: super::ipc::Frame = serde_json::from_str(buf.trim_end())
            .map_err(|e| LauncherError::Ipc(format!("decode: {e}")))?;
        if resp_frame.id != id {
            return Err(LauncherError::Ipc(format!(
                "id mismatch: sent {id}, got {}",
                resp_frame.id
            )));
        }
        resp_frame
            .into_response()
            .ok_or_else(|| LauncherError::Ipc("expected response, got request".into()))
    }
}

#[cfg(target_os = "macos")]
#[async_trait]
impl KernelLauncher for HelperSocketLauncher {
    async fn ensure_privileged(&self) -> Result<(), LauncherError> {
        match self.call(super::ipc::Request::Ping).await {
            Ok(super::ipc::Response::Pong { .. }) => Ok(()),
            Ok(other) => Err(LauncherError::Ipc(format!(
                "unexpected ping response: {other:?}"
            ))),
            Err(LauncherError::ServiceMissing(_)) => {
                // First-run path: try to install the helper if a strategy
                // was registered, then re-probe once. If still missing, the
                // manager downgrades to SystemProxy.
                if let Some(installer) = &self.installer {
                    installer.install().await?;
                    match self.call(super::ipc::Request::Ping).await {
                        Ok(super::ipc::Response::Pong { .. }) => Ok(()),
                        Ok(other) => Err(LauncherError::Ipc(format!(
                            "unexpected ping after install: {other:?}"
                        ))),
                        Err(e) => Err(e),
                    }
                } else {
                    Err(LauncherError::ServiceMissing(
                        "xboard-helper not installed".into(),
                    ))
                }
            }
            Err(e) => Err(e),
        }
    }

    async fn spawn(&self, spec: KernelSpawnSpec) -> Result<LaunchHandle, LauncherError> {
        let resp = self
            .call(super::ipc::Request::StartKernel {
                exec_path: spec.exec_path.clone(),
                work_dir: spec.work_dir.clone(),
                cfg_path: spec.cfg_path.clone(),
                log_path: spec.log_path.clone(),
            })
            .await?;
        match resp {
            super::ipc::Response::Started { pid } => {
                wait_for_controller(&spec.controller_addr, &spec.controller_secret).await?;
                Ok(LaunchHandle::Remote {
                    ipc_path: self.socket_path.display().to_string(),
                    pid: Some(pid),
                })
            }
            super::ipc::Response::Error { message } => {
                Err(LauncherError::Other(format!("helper rejected start: {message}")))
            }
            other => Err(LauncherError::Ipc(format!(
                "unexpected start response: {other:?}"
            ))),
        }
    }

    async fn stop(&self, _handle: LaunchHandle) -> Result<(), LauncherError> {
        match self.call(super::ipc::Request::StopKernel).await {
            Ok(super::ipc::Response::Stopped) => Ok(()),
            Ok(super::ipc::Response::Error { message }) => Err(LauncherError::Other(format!(
                "helper rejected stop: {message}"
            ))),
            Ok(other) => Err(LauncherError::Ipc(format!(
                "unexpected stop response: {other:?}"
            ))),
            // Helper socket gone or restarted between connect and stop —
            // treat as already-stopped rather than surfacing a hard error.
            Err(LauncherError::ServiceMissing(_)) => Ok(()),
            Err(e) => Err(e),
        }
    }

    fn name(&self) -> &'static str {
        "helper-socket"
    }
}

/// Linux file-capability probe. Used by [`DirectLauncher::ensure_privileged`]
/// to decide whether `cap_net_admin` is set on the bundled mihomo binary;
/// when it isn't we surface `NotPermitted("setcap missing")` so the manager
/// can fall back to system-proxy mode without first writing a config and
/// crashing on EPERM at spawn-time.
#[cfg(target_os = "linux")]
pub mod linux_caps {
    use std::ffi::CString;
    use std::path::Path;

    /// Returns true iff `path` has any value stored under the
    /// `security.capability` extended attribute. We deliberately don't
    /// decode the blob (which would require parsing `struct vfs_cap_data`
    /// and matching against the right cap_net_admin bit) — the presence
    /// of the xattr is a strong-enough signal that someone has already
    /// run `setcap` on this binary, which is the only state we currently
    /// produce via the deb/rpm postinst.
    ///
    /// Failure modes that map to `false`:
    ///   * file doesn't exist (ENOENT)
    ///   * filesystem doesn't support xattrs (ENOTSUP — e.g. AppImage tmpfs mount)
    ///   * EACCES on `security.*` (rare; would imply kernel hardening that
    ///     also blocks the spawn itself).
    pub fn has_file_capability(path: &Path) -> bool {
        let Ok(cpath) = CString::new(path.as_os_str().as_encoded_bytes()) else {
            return false;
        };
        let Ok(attr) = CString::new("security.capability") else {
            return false;
        };
        // SAFETY: getxattr is a stable Linux syscall; passing a null buffer
        // with size 0 is the documented way to query whether the xattr
        // exists. Returns the size of the value (>=0) on success or -1 on
        // any error.
        let rc = unsafe {
            libc_getxattr(
                cpath.as_ptr(),
                attr.as_ptr(),
                std::ptr::null_mut(),
                0,
            )
        };
        rc >= 0
    }

    extern "C" {
        #[link_name = "getxattr"]
        fn libc_getxattr(
            path: *const std::os::raw::c_char,
            name: *const std::os::raw::c_char,
            value: *mut std::ffi::c_void,
            size: usize,
        ) -> isize;
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn missing_file_has_no_capability() {
            // ENOENT path — the probe must report false rather than panic.
            assert!(!has_file_capability(Path::new("/nonexistent/xboard-test")));
        }
    }
}
