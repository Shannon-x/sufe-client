//! `xboard-svc` — a tiny LocalSystem service that the Tauri UI talks to over
//! a named pipe on Windows. Mirrors the macOS `xboard-helper` design:
//!
//! * The UI process runs unprivileged. It holds the user keychain, fetches
//!   subscriptions, patches the YAML, and asks the privileged side to spawn
//!   mihomo for it.
//! * `xboard-svc.exe` runs as a Windows service under `LocalSystem` so
//!   mihomo can create a wintun adapter and rewrite the system route table.
//!
//! Wire protocol: [`xboard_core::kernel::ipc`] (newline-delimited JSON), the
//! same `Frame` / `Request` / `Response` enums used by the macOS helper.
//!
//! Access control:
//!  * The pipe is created with a DACL that grants connect rights to the
//!    "Authenticated Users" group plus LocalSystem + BUILTIN\Administrators.
//!    Pipes don't accept network connections by default, so the only
//!    callers can be local user processes or the service itself.
//!  * On every accept we resolve the connecting client's user SID via
//!    `GetNamedPipeClientProcessId` → `OpenProcess` →
//!    `OpenProcessToken` → `GetTokenInformation(TokenUser)`, and reject
//!    anything that isn't the SID stored at install time. This pins the
//!    service to the single user who installed it; another user logged in
//!    on the same machine cannot drive our mihomo.
//!
//! Path policy: identical to the helper's — the requested `exec_path` must
//! canonicalize to a binary under our install directory and have a known
//! mihomo basename. We never `CreateProcess` an arbitrary path the UI sent.
//!
//! Build note: this crate is gated to Windows in `Cargo.toml`. On non-Windows
//! hosts the binary still compiles (so `cargo check` at the workspace level
//! works on macOS / Linux) but `main()` is a no-op stub.

#[cfg(not(target_os = "windows"))]
fn main() {
    eprintln!(
        "xboard-svc only runs on Windows; running on {} is a no-op.",
        std::env::consts::OS
    );
}

#[cfg(target_os = "windows")]
fn main() -> anyhow::Result<()> {
    imp::main()
}

#[cfg(target_os = "windows")]
mod imp {
    use std::ffi::{OsStr, OsString};
    use std::os::windows::ffi::OsStrExt;
    use std::path::{Path, PathBuf};
    use std::process::Stdio;
    use std::sync::Arc;
    use std::time::Duration;

    use parking_lot::Mutex;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};
    use tokio::process::{Child, Command};
    use tokio::sync::Notify;

    use windows_service::service::{
        ServiceAccess, ServiceControl, ServiceControlAccept, ServiceErrorControl, ServiceExitCode,
        ServiceInfo, ServiceStartType, ServiceState, ServiceStatus, ServiceType,
    };
    use windows_service::service_control_handler::{self, ServiceControlHandlerResult};
    use windows_service::service_dispatcher;
    use windows_service::service_manager::{ServiceManager, ServiceManagerAccess};

    use windows_sys::Win32::Foundation::{
        CloseHandle, GetLastError, LocalFree, BOOL, FALSE, HANDLE, HLOCAL, TRUE,
    };
    use windows_sys::Win32::Security::Authorization::{
        ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
        SDDL_REVISION_1,
    };
    use windows_sys::Win32::Security::{
        EqualSid, GetTokenInformation, TokenUser, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES,
        TOKEN_QUERY, TOKEN_USER,
    };
    use windows_sys::Win32::System::Pipes::GetNamedPipeClientProcessId;
    use windows_sys::Win32::System::Threading::{
        GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    use xboard_core::kernel::ipc::{Frame, FrameBody, Request, Response, SVC_PIPE_PATH};

    const SVC_NAME: &str = "xboard-svc";
    const SVC_DISPLAY: &str = "Xboard VPN Helper Service";
    const SVC_DESCRIPTION: &str = "Owns the mihomo (Clash Meta) subprocess for the Xboard \
        desktop client. Runs as LocalSystem so mihomo can create wintun \
        adapters and modify the system route table.";
    const SVC_VERSION: &str = env!("CARGO_PKG_VERSION");

    /// SDDL granting full access to LocalSystem + Builtin\Administrators +
    /// Authenticated Users. The per-connection SID check downstream is what
    /// pins the service to the installing user; the DACL is just a coarse
    /// "no anonymous, no guest" filter.
    const PIPE_SDDL: &str = "D:(A;;GA;;;SY)(A;;GA;;;BA)(A;;GA;;;AU)";

    /// Service-startup arg name that carries the SID we accept connections
    /// from. The installer plants it at `xboard-svc.exe install` time using
    /// the user's pre-elevation token, and CreateService bakes it into the
    /// service's ImagePath so the service receives it as argv[0..n].
    const ALLOWED_SID_ARG: &str = "--allowed-sid";

    pub fn main() -> anyhow::Result<()> {
        init_tracing();
        let mut args = std::env::args().skip(1);
        match args.next().as_deref() {
            Some("install") => install_self(),
            Some("uninstall") => uninstall_self(),
            // Default: SCM is launching us in service mode.
            _ => service_dispatcher::start(SVC_NAME, ffi_service_main)
                .map_err(|e| anyhow::anyhow!("service_dispatcher::start: {e}")),
        }
    }

    fn init_tracing() {
        use tracing_subscriber::{fmt, EnvFilter};
        let filter = EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| EnvFilter::new("info,xboard_svc=debug"));
        // SCM captures stdout/stderr into the EventLog, so a plain stderr
        // formatter is enough — we don't need a separate file logger.
        let _ = fmt()
            .with_env_filter(filter)
            .with_target(false)
            .with_writer(std::io::stderr)
            .try_init();
    }

    // ----- install / uninstall ------------------------------------------------

    /// Register `xboard-svc.exe` with the SCM as a LocalSystem service. The
    /// caller must already be elevated; we don't try to self-elevate here.
    /// The Tauri side handles UAC by running this command via `runas`.
    fn install_self() -> anyhow::Result<()> {
        let exe = std::env::current_exe()?;
        let user_sid = current_user_sid()?;
        let manager =
            ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CREATE_SERVICE)
                .map_err(|e| anyhow::anyhow!("open SCM: {e}"))?;

        let info = ServiceInfo {
            name: OsString::from(SVC_NAME),
            display_name: OsString::from(SVC_DISPLAY),
            service_type: ServiceType::OWN_PROCESS,
            // AutoStart so users don't have to re-run install / re-elevate
            // after a reboot.
            start_type: ServiceStartType::AutoStart,
            error_control: ServiceErrorControl::Normal,
            executable_path: exe.clone(),
            launch_arguments: vec![OsString::from(ALLOWED_SID_ARG), OsString::from(&user_sid)],
            dependencies: vec![],
            account_name: None, // None == LocalSystem
            account_password: None,
        };

        // Idempotent: if the service is already there, `OpenService` it and
        // re-set the description / arg list so a re-install with a different
        // user SID actually rotates the SID.
        match manager.create_service(&info, ServiceAccess::CHANGE_CONFIG) {
            Ok(svc) => {
                let _ = svc.set_description(SVC_DESCRIPTION);
                tracing::info!("xboard-svc registered (sid={user_sid})");
            }
            Err(windows_service::Error::Winapi(io_err)) if io_err.raw_os_error() == Some(1073) => {
                // ERROR_SERVICE_EXISTS — open and reconfigure in place.
                let svc = manager
                    .open_service(SVC_NAME, ServiceAccess::CHANGE_CONFIG)
                    .map_err(|e| anyhow::anyhow!("open existing service: {e}"))?;
                svc.change_config(&info)
                    .map_err(|e| anyhow::anyhow!("update existing service: {e}"))?;
                let _ = svc.set_description(SVC_DESCRIPTION);
                tracing::info!("xboard-svc reconfigured (sid={user_sid})");
            }
            Err(e) => return Err(anyhow::anyhow!("create service: {e}")),
        }
        Ok(())
    }

    fn uninstall_self() -> anyhow::Result<()> {
        let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)
            .map_err(|e| anyhow::anyhow!("open SCM: {e}"))?;
        let svc = manager
            .open_service(SVC_NAME, ServiceAccess::STOP | ServiceAccess::DELETE)
            .map_err(|e| anyhow::anyhow!("open service: {e}"))?;
        // Best-effort stop; `delete` works even if it's already stopped.
        let _ = svc.stop();
        svc.delete()
            .map_err(|e| anyhow::anyhow!("delete service: {e}"))?;
        tracing::info!("xboard-svc deleted");
        Ok(())
    }

    // ----- service entry-point ------------------------------------------------

    windows_service::define_windows_service!(ffi_service_main, my_service_main);

    fn my_service_main(args: Vec<OsString>) {
        if let Err(e) = run_service(args) {
            tracing::error!(error = %format_anyhow(&e), "xboard-svc terminated with error");
        }
    }

    fn format_anyhow(e: &anyhow::Error) -> String {
        format!("{e:#}")
    }

    fn run_service(args: Vec<OsString>) -> anyhow::Result<()> {
        let allowed_sid = parse_allowed_sid(&args).ok_or_else(|| {
            anyhow::anyhow!(
                "{ALLOWED_SID_ARG} <SID> missing from service args — re-run xboard-svc.exe install"
            )
        })?;
        tracing::info!(version = SVC_VERSION, sid = %allowed_sid, "xboard-svc starting");

        let stop_signal = Arc::new(Notify::new());
        let stop_for_handler = stop_signal.clone();
        let event_handler = move |c: ServiceControl| -> ServiceControlHandlerResult {
            match c {
                ServiceControl::Stop | ServiceControl::Shutdown => {
                    stop_for_handler.notify_waiters();
                    ServiceControlHandlerResult::NoError
                }
                ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
                _ => ServiceControlHandlerResult::NotImplemented,
            }
        };
        let status_handle = service_control_handler::register(SVC_NAME, event_handler)
            .map_err(|e| anyhow::anyhow!("register control handler: {e}"))?;

        status_handle
            .set_service_status(ServiceStatus {
                service_type: ServiceType::OWN_PROCESS,
                current_state: ServiceState::Running,
                controls_accepted: ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN,
                exit_code: ServiceExitCode::Win32(0),
                checkpoint: 0,
                wait_hint: Duration::default(),
                process_id: None,
            })
            .map_err(|e| anyhow::anyhow!("set service status running: {e}"))?;

        // Tokio's service_dispatcher entry-point is sync, so we own the
        // runtime here. current_thread is fine — concurrency on the pipe is
        // low (the UI keeps connections short-lived).
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let state = Arc::new(SvcState::default());
        let kill_state = state.clone();
        let res = rt.block_on(async move {
            tokio::select! {
                r = serve_loop(allowed_sid, state.clone()) => r,
                _ = stop_signal.notified() => {
                    tracing::info!("stop requested; killing kernel");
                    state.kill_kernel().await;
                    Ok(())
                }
            }
        });
        // Ensure the kernel is dead even if serve_loop returned an error.
        rt.block_on(kill_state.kill_kernel());

        let _ = status_handle.set_service_status(ServiceStatus {
            service_type: ServiceType::OWN_PROCESS,
            current_state: ServiceState::Stopped,
            controls_accepted: ServiceControlAccept::empty(),
            exit_code: ServiceExitCode::Win32(0),
            checkpoint: 0,
            wait_hint: Duration::default(),
            process_id: None,
        });
        res
    }

    fn parse_allowed_sid(args: &[OsString]) -> Option<String> {
        // The first arg seen by my_service_main is the service name (windows
        // convention); everything after it is what we put in
        // `launch_arguments` at install time.
        let mut iter = args.iter();
        while let Some(a) = iter.next() {
            if a == ALLOWED_SID_ARG {
                if let Some(v) = iter.next() {
                    if let Some(s) = v.to_str() {
                        return Some(s.to_string());
                    }
                }
            }
        }
        // Backward-compat: also accept env var, useful for local testing
        // (`set XBOARD_SVC_ALLOWED_SID=...`).
        std::env::var("XBOARD_SVC_ALLOWED_SID").ok()
    }

    // ----- accept loop --------------------------------------------------------

    #[derive(Debug, Default)]
    struct SvcState {
        child: Mutex<Option<Child>>,
    }

    impl SvcState {
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

    async fn serve_loop(allowed_sid: String, state: Arc<SvcState>) -> anyhow::Result<()> {
        // `ServerOptions::create_with_security_attributes_raw` lets us hand
        // CreateNamedPipe a DACL we own. The first instance must use
        // `first_pipe_instance(true)` — subsequent ones reuse the same name
        // without that flag.
        let mut first = true;
        loop {
            let mut sa = build_security_attributes()?;
            let pipe = unsafe {
                ServerOptions::new()
                    .first_pipe_instance(first)
                    .reject_remote_clients(true)
                    .create_with_security_attributes_raw(
                        SVC_PIPE_PATH,
                        &mut sa.attrs as *mut _ as *mut _,
                    )?
            };
            first = false;

            // Wait for a client. ConnectNamedPipe under the hood; tokio
            // surfaces it as `connect`.
            pipe.connect().await?;

            let allowed = allowed_sid.clone();
            let st = state.clone();
            tokio::spawn(async move {
                if let Err(e) = handle_client(pipe, allowed, st).await {
                    tracing::warn!(error = %e, "client handler ended");
                }
            });
        }
    }

    /// Wrap a SECURITY_ATTRIBUTES with its backing security descriptor so
    /// callers don't have to manually `LocalFree` the descriptor on drop.
    struct OwnedSa {
        attrs: SECURITY_ATTRIBUTES,
        sd: PSECURITY_DESCRIPTOR,
    }

    impl Drop for OwnedSa {
        fn drop(&mut self) {
            if !self.sd.is_null() {
                unsafe {
                    LocalFree(self.sd as HLOCAL);
                }
            }
        }
    }

    fn build_security_attributes() -> anyhow::Result<OwnedSa> {
        let sddl_w: Vec<u16> = OsStr::new(PIPE_SDDL)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let mut sd: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
        let ok = unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl_w.as_ptr(),
                SDDL_REVISION_1 as u32,
                &mut sd,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            anyhow::bail!(
                "ConvertStringSecurityDescriptor failed: {}",
                last_error_string()
            );
        }
        let attrs = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: sd,
            bInheritHandle: 0,
        };
        Ok(OwnedSa { attrs, sd })
    }

    async fn handle_client(
        pipe: NamedPipeServer,
        allowed_sid: String,
        state: Arc<SvcState>,
    ) -> anyhow::Result<()> {
        // Resolve the client SID synchronously before we split the pipe for
        // I/O. Win32 SID lookups don't need to be async — they're fast in-
        // process token reads — so we don't pay a round-trip for the gate.
        use std::os::windows::io::AsRawHandle;
        let raw_handle: HANDLE = pipe.as_raw_handle() as HANDLE;
        let client_sid_result = caller_sid_from_pipe_handle(raw_handle);

        let (read, mut write) = tokio::io::split(pipe);

        let client_sid = match client_sid_result {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(error = %e, "failed to resolve client SID — closing");
                return Ok(());
            }
        };
        if client_sid != allowed_sid {
            tracing::warn!(
                got = %client_sid,
                want = %allowed_sid,
                "rejecting connection from unexpected SID"
            );
            // Send a single Error frame so the client sees a useful message
            // instead of a blank disconnect, then close.
            let f = Frame::response(
                0,
                Response::Error {
                    message: "client SID not authorized for this service".into(),
                },
            );
            if let Ok(mut s) = serde_json::to_string(&f) {
                s.push('\n');
                let _ = write.write_all(s.as_bytes()).await;
            }
            return Ok(());
        }

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

    async fn dispatch(state: &SvcState, req: Request) -> Response {
        match req {
            Request::Ping => Response::Pong {
                helper_version: SVC_VERSION.to_string(),
            },
            Request::Status => {
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
        state: &SvcState,
        exec_path: &Path,
        work_dir: &Path,
        cfg_path: &Path,
        log_path: &Path,
    ) -> anyhow::Result<u32> {
        let canonical = validate_exec_path(exec_path)?;
        // The per-connection SID check already restricts callers to the
        // install user, but still pin the data paths so a compromised UI (or
        // the install user themselves) can't have LocalSystem create_dir_all /
        // append to arbitrary locations (e.g. C:\Windows\System32\...) — a
        // service-to-SYSTEM escalation. Config / work-dir / log must live in
        // the user's own AppData bundle dir.
        validate_data_path(work_dir)?;
        validate_data_path(cfg_path)?;
        validate_data_path(log_path)?;
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
        // mihomo's wintun loader probes the executable's directory and the
        // process cwd. As a LocalSystem service we'd otherwise inherit
        // C:\Windows\System32 — pin cwd next to mihomo.exe so wintun.dll
        // (shipped as a bundle.resources sibling) is reachable either way.
        if let Some(parent) = canonical.parent() {
            cmd.current_dir(parent);
        }

        let child = cmd.spawn()?;
        let pid = child.id().unwrap_or(0);
        *state.child.lock() = Some(child);
        tracing::info!(pid, "mihomo spawned");
        Ok(pid)
    }

    // ----- path policy --------------------------------------------------------

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

    /// Allowed install roots on Windows.
    ///   * `C:\Program Files\Xboard\...` — production install (NSIS / MSI bundle).
    ///   * `<LocalAppData>\Xboard\...` — per-user install.
    ///   * `<repo>\target\...\binaries\` — `tauri dev` runs.
    ///
    /// We canonicalize first, so symlinks pointing outside these prefixes
    /// fail. Path-prefix checks are case-insensitive on Windows; we lowercase
    /// both sides before comparing.
    fn is_under_allowed_root(p: &Path) -> bool {
        let s = p.to_string_lossy().to_lowercase();
        // ProgramFiles can be drive-relative. Use the env var if present;
        // fall back to the common default so dev hosts still match.
        let pf = std::env::var("ProgramFiles")
            .unwrap_or_else(|_| "C:\\Program Files".into())
            .to_lowercase();
        let pfx_pf = format!("{pf}\\xboard\\");
        if s.starts_with(&pfx_pf) {
            return true;
        }
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            let pfx = format!("{}\\xboard\\", local.to_lowercase());
            if s.starts_with(&pfx) {
                return true;
            }
        }
        // dev: ...\target\debug\binaries\... or ...\target\release\binaries\...
        if s.contains("\\target\\debug\\binaries\\") || s.contains("\\target\\release\\binaries\\")
        {
            return true;
        }
        false
    }

    fn is_allowed_basename(p: &Path) -> bool {
        const ALLOWED: &[&str] = &["mihomo.exe", "mihomo-x86_64-pc-windows-msvc.exe"];
        p.file_name()
            .and_then(|n| n.to_str())
            .map(|n| ALLOWED.iter().any(|a| a.eq_ignore_ascii_case(n)))
            .unwrap_or(false)
    }

    /// Lexical allow-list for the kernel's data paths (work-dir / config /
    /// log). Tauri's `app_data_dir()` on Windows is
    /// `%APPDATA%\com.xboard.client.desktop` (Roaming); we also accept the
    /// Local variant. Anchored to `<drive>:\users\<user>\appdata\...` so a
    /// crafted path can't smuggle the segment in mid-string.
    fn is_under_allowed_data_root(p: &Path) -> bool {
        let mut s = p.to_string_lossy().to_lowercase().replace('/', "\\");
        if let Some(stripped) = s.strip_prefix(r"\\?\") {
            s = stripped.to_string();
        }
        let after_users = match s.split_once(r"\users\") {
            Some((drive, rest)) if drive.len() == 2 && drive.ends_with(':') => rest,
            _ => return false,
        };
        let tail = match after_users.split_once('\\') {
            Some((_user, tail)) => tail,
            None => return false,
        };
        tail.starts_with(r"appdata\roaming\com.xboard.client.desktop\")
            || tail.starts_with(r"appdata\local\com.xboard.client.desktop\")
    }

    /// Pin work-dir / config / log to the user's own AppData bundle dir:
    /// absolute, no `.`/`..`, lexically under the allowed root, and (for
    /// already-existing paths / ancestors) symlink-resolved still under it.
    fn validate_data_path(p: &Path) -> anyhow::Result<()> {
        use std::path::Component;
        if !p.is_absolute() {
            anyhow::bail!("data path must be absolute, got {}", p.display());
        }
        for comp in p.components() {
            if matches!(comp, Component::ParentDir | Component::CurDir) {
                anyhow::bail!("data path must not contain '.' or '..': {}", p.display());
            }
        }
        if !is_under_allowed_data_root(p) {
            anyhow::bail!(
                "data path {} is not under the per-user AppData bundle dir",
                p.display()
            );
        }
        // Resolve symlinks/junctions on the nearest existing ancestor and
        // re-check, so a reparse point can't redirect us outside the root.
        let mut existing = p;
        let canon = loop {
            if existing.exists() {
                break existing
                    .canonicalize()
                    .map_err(|e| anyhow::anyhow!("canonicalize {}: {e}", existing.display()))?;
            }
            match existing.parent() {
                Some(parent) => existing = parent,
                None => anyhow::bail!("no existing ancestor for {}", p.display()),
            }
        };
        if !is_under_allowed_data_root(&canon) {
            anyhow::bail!(
                "data path {} resolves outside the allowed root (reparse point?): {}",
                p.display(),
                canon.display()
            );
        }
        Ok(())
    }

    // ----- SID helpers --------------------------------------------------------

    /// Resolve the connecting client's user SID from an open pipe HANDLE.
    fn caller_sid_from_pipe_handle(raw: HANDLE) -> anyhow::Result<String> {
        // GetNamedPipeClientProcessId fills a u32 with the PID.
        let mut pid: u32 = 0;
        let ok = unsafe { GetNamedPipeClientProcessId(raw, &mut pid) };
        if ok == 0 {
            anyhow::bail!("GetNamedPipeClientProcessId: {}", last_error_string());
        }

        let proc_h = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, FALSE, pid) };
        if proc_h.is_null() {
            anyhow::bail!("OpenProcess({pid}): {}", last_error_string());
        }
        let result = sid_for_process(proc_h);
        unsafe { CloseHandle(proc_h) };
        result
    }

    fn current_user_sid() -> anyhow::Result<String> {
        let proc_h = unsafe { GetCurrentProcess() };
        sid_for_process(proc_h)
    }

    fn sid_for_process(proc_h: HANDLE) -> anyhow::Result<String> {
        let mut tok: HANDLE = std::ptr::null_mut();
        let ok = unsafe { OpenProcessToken(proc_h, TOKEN_QUERY, &mut tok) };
        if ok == 0 {
            anyhow::bail!("OpenProcessToken: {}", last_error_string());
        }

        // First call discovers the buffer size needed for TOKEN_USER.
        let mut needed: u32 = 0;
        unsafe {
            GetTokenInformation(tok, TokenUser, std::ptr::null_mut(), 0, &mut needed);
        }
        if needed == 0 {
            unsafe { CloseHandle(tok) };
            anyhow::bail!("GetTokenInformation(TokenUser) size query failed");
        }
        let mut buf = vec![0u8; needed as usize];
        let ok = unsafe {
            GetTokenInformation(
                tok,
                TokenUser,
                buf.as_mut_ptr() as *mut _,
                needed,
                &mut needed,
            )
        };
        unsafe { CloseHandle(tok) };
        if ok == 0 {
            anyhow::bail!("GetTokenInformation(TokenUser): {}", last_error_string());
        }
        let token_user = unsafe { &*(buf.as_ptr() as *const TOKEN_USER) };
        let sid_ptr = token_user.User.Sid;
        if sid_ptr.is_null() {
            anyhow::bail!("TOKEN_USER.Sid is null");
        }
        let mut sid_str_w: *mut u16 = std::ptr::null_mut();
        let ok = unsafe { ConvertSidToStringSidW(sid_ptr, &mut sid_str_w) };
        if ok == 0 || sid_str_w.is_null() {
            anyhow::bail!("ConvertSidToStringSidW: {}", last_error_string());
        }
        // Walk the wide string until NUL.
        let mut len = 0usize;
        unsafe {
            while *sid_str_w.add(len) != 0 {
                len += 1;
            }
        }
        let slice = unsafe { std::slice::from_raw_parts(sid_str_w, len) };
        let s = String::from_utf16_lossy(slice);
        unsafe {
            LocalFree(sid_str_w as HLOCAL);
        }
        Ok(s)
    }

    fn last_error_string() -> String {
        let code = unsafe { GetLastError() };
        format!("Win32 error {code}")
    }

    // ----- tests --------------------------------------------------------------

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn basename_allowlist() {
            assert!(is_allowed_basename(Path::new("C:\\x\\mihomo.exe")));
            assert!(is_allowed_basename(Path::new(
                "C:\\x\\mihomo-x86_64-pc-windows-msvc.exe"
            )));
            assert!(!is_allowed_basename(Path::new("C:\\x\\cmd.exe")));
            assert!(!is_allowed_basename(Path::new("C:\\x\\mihomo")));
        }

        #[test]
        fn root_allowlist_dev_paths() {
            // Even on a host where ProgramFiles is unset, the dev fallback
            // covers `cargo test` runs out of the workspace.
            let p = PathBuf::from(
                "C:\\src\\xboard-client\\target\\debug\\binaries\\mihomo-x86_64-pc-windows-msvc.exe",
            );
            assert!(is_under_allowed_root(&p));
        }
    }
}
