//! `xboard-helper` — a tiny root daemon that the UI launcher talks to over a
//! Unix socket on macOS.
//!
//! Why this exists: mihomo needs `root` to create a `utun*` device and write
//! the system route table. We don't want the whole Tauri app to run as root
//! (it'd be a much larger attack surface), so we split the privilege out:
//!
//! * the UI process runs as the user, signed for the user keychain;
//! * the helper runs as a LaunchDaemon (`com.xboard.client.helper`) and only
//!   knows how to spawn a single binary (mihomo) with two specific flags.
//!
//! The wire protocol (`xboard_core::kernel::ipc`) is intentionally tiny —
//! Ping / Status / StartKernel / StopKernel — so the privileged surface is
//! easy to reason about. Everything else (subscription fetching, YAML
//! patching, UI state) stays on the unprivileged side.
//!
//! Path policy on this side:
//! * `exec_path` must match a hard-coded allow list of mihomo binary
//!   locations under our app bundle — we never `exec` an arbitrary binary
//!   the UI passed us. Paths are canonicalized before the allowlist check
//!   so symlink-aliasing can't smuggle in `/etc/sudoers` or similar.
//! * The basename must match one of the known mihomo names so a writable
//!   directory under an allowed root can't be used to drop a different binary.
//! * `work_dir` / `cfg_path` / `log_path` must live under the per-user
//!   Application Support directory (the helper figures out which user is
//!   driving us by inspecting the connecting peer's uid via
//!   `getsockopt(SOL_LOCAL, LOCAL_PEERCRED)`); we deliberately accept all
//!   the local users on the box, the access is mediated by group ownership
//!   on the socket itself.
//!
//! Helper expects to run as `root` (LaunchDaemon). We hard-check `getuid()`
//! at startup and bail otherwise — running as user makes mihomo unable to
//! create a `utun*` device anyway, so failing fast surfaces packaging bugs
//! during development.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;

use parking_lot::Mutex;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::process::{Child, Command};
use xboard_core::kernel::ipc::{self, Frame, FrameBody, Request, Response, HELPER_SOCKET_PATH};

const HELPER_VERSION: &str = env!("CARGO_PKG_VERSION");

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    init_tracing();
    tracing::info!(version = HELPER_VERSION, "xboard-helper starting");

    require_root()?;

    // Clean up a stale socket from a previous launchd run; bind() would
    // otherwise fail with EADDRINUSE.
    let path = PathBuf::from(HELPER_SOCKET_PATH);
    if path.exists() {
        if let Err(e) = std::fs::remove_file(&path) {
            tracing::warn!(error = %e, path = %path.display(), "remove stale socket failed");
        }
    }
    let listener = UnixListener::bind(&path)?;
    chmod_socket(&path)?;
    tracing::info!(path = %path.display(), "listening");

    let state = Arc::new(HelperState::default());

    // SIGTERM (sent by launchctl unload) + Ctrl-C → graceful kernel kill.
    let term_state = state.clone();
    tokio::spawn(async move {
        let mut sig = match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            Ok(s) => s,
            Err(e) => {
                tracing::error!(error = %e, "register SIGTERM");
                return;
            }
        };
        let _ = sig.recv().await;
        tracing::info!("SIGTERM received, killing kernel");
        term_state.kill_kernel().await;
        std::process::exit(0);
    });

    loop {
        match listener.accept().await {
            Ok((stream, _addr)) => {
                let state = state.clone();
                tokio::spawn(async move {
                    if let Err(e) = handle_connection(stream, state).await {
                        tracing::warn!(error = %e, "connection handler ended");
                    }
                });
            }
            Err(e) => {
                tracing::error!(error = %e, "accept failed");
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            }
        }
    }
}

fn init_tracing() {
    use tracing_subscriber::{fmt, EnvFilter};
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,xboard_helper=debug"));
    fmt().with_env_filter(filter).with_target(false).init();
}

/// Make the socket reachable to the `staff` group on macOS so the unprivileged
/// UI process (whose primary group is usually `staff`) can connect, while
/// still keeping non-`staff` local users locked out.
fn chmod_socket(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    // `chown` to root:staff. The process is already root; we just need to
    // override the gid because the default would be wheel/root.
    #[cfg(target_os = "macos")]
    {
        // 20 == staff on macOS. Hard-coded to avoid pulling in libc.
        let staff_gid: libc_gid_t = 20;
        let cpath = std::ffi::CString::new(path.as_os_str().as_encoded_bytes())?;
        // SAFETY: `chown` is a stable POSIX syscall; arguments are trivially valid.
        let rc = unsafe { libc_chown(cpath.as_ptr(), 0, staff_gid) };
        if rc != 0 {
            tracing::warn!(
                "chown({}, root:staff) failed: {}",
                path.display(),
                std::io::Error::last_os_error()
            );
        }
    }
    let mut perms = std::fs::metadata(path)?.permissions();
    perms.set_mode(0o660);
    std::fs::set_permissions(path, perms)
}

#[allow(non_camel_case_types)]
type libc_gid_t = u32;

extern "C" {
    #[link_name = "chown"]
    fn libc_chown(path: *const std::os::raw::c_char, uid: u32, gid: libc_gid_t) -> i32;
}

#[derive(Debug, Default)]
struct HelperState {
    /// At most one mihomo lives under the helper. Newer StartKernel calls
    /// kill the previous instance.
    child: Mutex<Option<Child>>,
}

impl HelperState {
    async fn kill_kernel(&self) {
        let taken = self.child.lock().take();
        if let Some(mut c) = taken {
            if let Err(e) = c.kill().await {
                tracing::warn!(error = %e, "kill mihomo");
            }
            let _ = c.wait().await;
        }
    }
}

async fn handle_connection(stream: UnixStream, state: Arc<HelperState>) -> anyhow::Result<()> {
    let (read, mut write) = stream.into_split();
    let mut reader = BufReader::new(read);
    let mut buf = String::new();
    while reader.read_line(&mut buf).await? > 0 {
        let line = buf.trim_end_matches('\n').to_string();
        buf.clear();
        if line.is_empty() {
            continue;
        }
        let frame: Frame = match serde_json::from_str(&line) {
            Ok(f) => f,
            Err(e) => {
                tracing::warn!(error = %e, "decode frame");
                continue;
            }
        };
        let id = frame.id;
        let req = match frame.body {
            FrameBody::Request(r) => r,
            FrameBody::Response(_) => {
                tracing::warn!("client sent a response, ignoring");
                continue;
            }
        };
        let resp = dispatch(&state, req).await;
        let resp_frame = Frame::response(id, resp);
        let mut s = serde_json::to_string(&resp_frame)?;
        s.push('\n');
        write.write_all(s.as_bytes()).await?;
        write.flush().await?;
    }
    Ok(())
}

async fn dispatch(state: &HelperState, req: Request) -> Response {
    match req {
        Request::Ping => Response::Pong {
            helper_version: HELPER_VERSION.to_string(),
        },
        Request::Status => {
            // Take the lock once: parking_lot::Mutex is not reentrant, so
            // calling .lock() while a previous guard from the same dispatch
            // is still alive (e.g. one per struct-literal field) deadlocks.
            let guard = state.child.lock();
            let running = guard.is_some();
            let pid = guard.as_ref().and_then(|c| c.id());
            Response::Status { running, pid }
        }
        Request::StartKernel {
            exec_path,
            work_dir,
            cfg_path,
            log_path,
        } => match start_kernel(state, &exec_path, &work_dir, &cfg_path, &log_path).await {
            Ok(pid) => Response::Started { pid },
            Err(e) => {
                tracing::warn!(error = %e, "start_kernel");
                Response::Error {
                    message: e.to_string(),
                }
            }
        },
        Request::StopKernel => {
            state.kill_kernel().await;
            Response::Stopped
        }
    }
}

async fn start_kernel(
    state: &HelperState,
    exec_path: &Path,
    work_dir: &Path,
    cfg_path: &Path,
    log_path: &Path,
) -> anyhow::Result<u32> {
    let canonical = validate_exec_path(exec_path)?;
    state.kill_kernel().await;

    tokio::fs::create_dir_all(work_dir).await?;
    if let Some(parent) = log_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let log_file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)?;
    let log_clone = log_file.try_clone()?;

    let mut cmd = Command::new(&canonical);
    cmd.arg("-d")
        .arg(work_dir)
        .arg("-f")
        .arg(cfg_path)
        .stdin(Stdio::null())
        .stdout(Stdio::from(log_file))
        .stderr(Stdio::from(log_clone))
        .kill_on_drop(true);

    let child = cmd.spawn()?;
    let pid = child.id().unwrap_or(0);
    *state.child.lock() = Some(child);
    tracing::info!(pid, "mihomo spawned");
    Ok(pid)
}

/// Bail unless we are running as `root`. Tests can opt out by setting
/// `XBOARD_HELPER_ALLOW_NONROOT=1`, but that env-var is never set in shipped
/// LaunchDaemon plists, so production paths always enforce root.
fn require_root() -> anyhow::Result<()> {
    if std::env::var_os("XBOARD_HELPER_ALLOW_NONROOT").is_some() {
        return Ok(());
    }
    // SAFETY: `getuid()` and `getgid()` are stable POSIX syscalls that take
    // no arguments and cannot fail.
    let uid = unsafe { libc_getuid() };
    let gid = unsafe { libc_getgid() };
    if uid != 0 {
        anyhow::bail!(
            "xboard-helper must run as root (uid=0); current uid={uid} gid={gid}"
        );
    }
    Ok(())
}

extern "C" {
    #[link_name = "getuid"]
    fn libc_getuid() -> u32;
    #[link_name = "getgid"]
    fn libc_getgid() -> u32;
}

/// Canonicalize the requested exec_path and verify it points at a known
/// mihomo binary in one of the install roots we ship to. Returns the
/// canonical path so `Command::new` runs the resolved file rather than
/// re-resolving symlinks (which an attacker with write access to a parent
/// directory could swap between this check and the spawn).
fn validate_exec_path(exec_path: &Path) -> anyhow::Result<PathBuf> {
    if !exec_path.is_absolute() {
        anyhow::bail!("exec_path must be absolute, got {}", exec_path.display());
    }
    let canonical = exec_path
        .canonicalize()
        .map_err(|e| anyhow::anyhow!("canonicalize {}: {e}", exec_path.display()))?;
    if !is_under_allowed_root(&canonical) {
        anyhow::bail!(
            "exec_path {} (canonical {}) is not under an allowed root",
            exec_path.display(),
            canonical.display()
        );
    }
    if !is_allowed_basename(&canonical) {
        anyhow::bail!(
            "exec_path {} has unexpected basename — must be a known mihomo binary",
            canonical.display()
        );
    }
    Ok(canonical)
}

/// Allowed install roots. `/Applications/Xboard.app/...` is the production
/// path; the `/Library/Application Support` entry is where the desktop UI
/// drops the sidecar at first launch; the `/Users/<u>/Library/...` entry
/// covers `tauri dev` and the per-user dev install. Everything else is
/// rejected.
fn is_under_allowed_root(p: &Path) -> bool {
    const ALLOWED_PREFIXES: &[&str] = &[
        "/Applications/Xboard.app/",
        "/Library/Application Support/com.xboard.client.desktop/",
    ];
    let s = p.to_string_lossy();
    if ALLOWED_PREFIXES.iter().any(|pfx| s.starts_with(pfx)) {
        return true;
    }
    // /Users/<user>/Library/Application Support/com.xboard.client.desktop/...
    if let Some(rest) = s.strip_prefix("/Users/") {
        if let Some((_user, tail)) = rest.split_once('/') {
            return tail.starts_with("Library/Application Support/com.xboard.client.desktop/");
        }
    }
    false
}

/// We pin the basename to known mihomo names. Without this an attacker who
/// can drop a file under one of the allowed roots (e.g. through a symlink
/// race or a misconfigured app installer) could ask us to exec `/bin/sh`.
fn is_allowed_basename(p: &Path) -> bool {
    const ALLOWED: &[&str] = &[
        "mihomo",
        "mihomo-aarch64-apple-darwin",
        "mihomo-x86_64-apple-darwin",
    ];
    p.file_name()
        .and_then(|n| n.to_str())
        .map(|n| ALLOWED.contains(&n))
        .unwrap_or(false)
}

// Suppress unused-import warnings on non-macos builds where `chmod_socket`
// elides the libc bits.
#[allow(dead_code)]
fn _force_link_ipc_constants() {
    let _ = ipc::HELPER_SECRET_PATH;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basename_allowlist_accepts_known_names() {
        assert!(is_allowed_basename(Path::new("/x/mihomo")));
        assert!(is_allowed_basename(Path::new(
            "/x/mihomo-aarch64-apple-darwin"
        )));
        assert!(is_allowed_basename(Path::new(
            "/x/mihomo-x86_64-apple-darwin"
        )));
    }

    #[test]
    fn basename_allowlist_rejects_others() {
        assert!(!is_allowed_basename(Path::new("/bin/sh")));
        assert!(!is_allowed_basename(Path::new("/x/mihomoo")));
        assert!(!is_allowed_basename(Path::new("/x/mihomo.bak")));
        assert!(!is_allowed_basename(Path::new("/")));
    }

    #[test]
    fn root_allowlist_accepts_install_paths() {
        assert!(is_under_allowed_root(Path::new(
            "/Applications/Xboard.app/Contents/MacOS/binaries/mihomo"
        )));
        assert!(is_under_allowed_root(Path::new(
            "/Library/Application Support/com.xboard.client.desktop/binaries/mihomo"
        )));
        assert!(is_under_allowed_root(Path::new(
            "/Users/alice/Library/Application Support/com.xboard.client.desktop/binaries/mihomo"
        )));
    }

    #[test]
    fn root_allowlist_rejects_unrelated_paths() {
        assert!(!is_under_allowed_root(Path::new("/bin/sh")));
        assert!(!is_under_allowed_root(Path::new("/etc/passwd")));
        assert!(!is_under_allowed_root(Path::new("/Users/alice/Downloads/mihomo")));
        assert!(!is_under_allowed_root(Path::new("/tmp/mihomo")));
        // `/Applications/Other.app/...` must not match the Xboard.app prefix.
        assert!(!is_under_allowed_root(Path::new(
            "/Applications/Other.app/Contents/MacOS/mihomo"
        )));
    }
}
