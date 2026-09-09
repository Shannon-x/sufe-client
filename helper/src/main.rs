//! Service-owned macOS mihomo launcher. IPC v2 accepts YAML bytes, never paths.

#[cfg(any(target_os = "macos", test))]
mod acl_policy;
#[cfg(unix)]
mod secure_fs;
#[cfg(any(target_os = "macos", test))]
mod tun_readiness;
#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("xboard-helper requires macOS");
}
#[cfg(target_os = "macos")]
fn main() -> anyhow::Result<()> {
    macos::main()
}

#[cfg(target_os = "macos")]
mod macos {
    use crate::{secure_fs, tun_readiness};
    use std::{
        path::{Path, PathBuf},
        process::Stdio,
        sync::Arc,
        time::Duration,
    };
    use tokio::{
        io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
        net::{UnixListener, UnixStream},
        process::{Child, Command},
        sync::{Mutex, Semaphore},
    };
    use xboard_core::{
        kernel::{
            control_proxy::{start_gateway, ControlGateway},
            ipc::{Frame, FrameBody, Request, Response, HELPER_SOCKET_PATH},
        },
        profile::privileged::prepare_privileged_config,
    };

    const INSTALL_DIR: &str = "/Library/Application Support/com.xboard.client";
    const KERNEL_PATH: &str = "/Library/Application Support/com.xboard.client/mihomo";
    const OWNER_PATH: &str = "/Library/Application Support/com.xboard.client/owner.uid";
    const STATE_DIR: &str = "/Library/Application Support/com.xboard.client/state";

    const MAX_FRAME: u64 = 12 * 1024 * 1024;

    struct Running {
        owner: u32,
        child: Child,
        gateway: ControlGateway,
        directory: PathBuf,
    }
    struct Service {
        owner: u32,
        running: Mutex<Option<Running>>,
    }

    #[tokio::main(flavor = "current_thread")]
    pub async fn main() -> anyhow::Result<()> {
        use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
        use tracing_subscriber::EnvFilter;
        tracing_subscriber::fmt()
            .with_env_filter(EnvFilter::new("info"))
            .with_target(false)
            .init();
        // No environment-variable bypass in the shipped executable.
        if unsafe { libc::geteuid() } != 0 {
            anyhow::bail!("helper must run as root");
        }
        unsafe {
            libc::umask(0o077);
        }
        secure_fs::validate_root_chain(Path::new(INSTALL_DIR))?;
        secure_fs::validate_root_file(Path::new(KERNEL_PATH), true)?;
        let owner = secure_fs::read_owner(Path::new(OWNER_PATH))?;
        secure_fs::private_directory(Path::new(STATE_DIR))?;
        let socket = Path::new(HELPER_SOCKET_PATH);
        let parent = socket
            .parent()
            .ok_or_else(|| anyhow::anyhow!("invalid socket path"))?;
        if parent != Path::new(INSTALL_DIR).join("ipc") {
            anyhow::bail!("unsafe helper socket constant");
        }
        secure_fs::public_directory(parent)?;
        if let Ok(meta) = std::fs::symlink_metadata(socket) {
            if !meta.file_type().is_socket() || meta.uid() != 0 {
                anyhow::bail!("unexpected object at helper socket");
            }
            if UnixStream::connect(socket).await.is_ok() {
                anyhow::bail!("another helper is already listening");
            }
            std::fs::remove_file(socket)?;
        }
        let listener = UnixListener::bind(socket)?;
        let cpath = std::ffi::CString::new(socket.as_os_str().as_encoded_bytes())?;
        if unsafe { libc::chown(cpath.as_ptr(), 0, 20) } != 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        std::fs::set_permissions(socket, std::fs::Permissions::from_mode(0o660))?;
        let state = Arc::new(Service {
            owner,
            running: Mutex::new(None),
        });
        let limit = Arc::new(Semaphore::new(8));
        let mut clients = tokio::task::JoinSet::new();
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
        loop {
            tokio::select! {
                _ = term.recv() => break,
                _ = tokio::signal::ctrl_c() => break,
                Some(_) = clients.join_next(), if !clients.is_empty() => {},
                accepted = listener.accept() => {
                    let (stream, _) = accepted?;
                    let Ok(permit) = limit.clone().try_acquire_owned() else { continue };
                    let state = state.clone();
                    clients.spawn(async move {
                        let _permit = permit;
                        if let Err(error) = handle(stream, state).await { tracing::warn!(%error, "helper request rejected"); }
                    });
                }
            }
        }
        clients.abort_all();
        while clients.join_next().await.is_some() {}
        terminate(&mut *state.running.lock().await).await;
        drop(listener);
        std::fs::remove_file(socket)?;
        Ok(())
    }

    async fn handle(stream: UnixStream, state: Arc<Service>) -> anyhow::Result<()> {
        // Effective UID from the kernel, before parsing caller data.
        let peer = stream.peer_cred()?.uid();
        if peer != state.owner {
            anyhow::bail!("connecting UID is not installation owner");
        }
        let (reader, mut writer) = stream.into_split();
        let mut reader = BufReader::new(reader).take(MAX_FRAME + 1);
        let mut line = Vec::new();
        let size =
            tokio::time::timeout(Duration::from_secs(10), reader.read_until(b'\n', &mut line))
                .await??;
        if size == 0 || size as u64 > MAX_FRAME || line.last() != Some(&b'\n') {
            anyhow::bail!("invalid or oversized IPC frame");
        }
        let frame: Frame = serde_json::from_slice(&line)?;
        let FrameBody::Request(request) = frame.body else {
            anyhow::bail!("expected request");
        };
        let response = dispatch(&state, request, peer).await;
        let mut encoded = serde_json::to_vec(&Frame::response(frame.id, response))?;
        encoded.push(b'\n');
        tokio::time::timeout(Duration::from_secs(5), writer.write_all(&encoded)).await??;
        Ok(())
    }

    async fn dispatch(state: &Service, request: Request, peer: u32) -> Response {
        if peer != state.owner {
            return error("unauthorised installation owner");
        }
        match request {
            Request::Ping => Response::Pong {
                helper_version: xboard_core::kernel::ipc::service_version(),
            },
            Request::StartKernel { .. } => {
                error("legacy path-based IPC is disabled; reinstall/update Sufe")
            }
            Request::StartKernelV2 { config_yaml } => {
                match start(state, peer, &config_yaml).await {
                    Ok(pid) => Response::Started { pid },
                    Err(failure) => {
                        tracing::warn!("privileged kernel start rejected");
                        error(&failure.to_string())
                    }
                }
            }
            Request::Status => {
                let mut guard = state.running.lock().await;
                if guard
                    .as_mut()
                    .is_some_and(|running| running.child.try_wait().ok().flatten().is_some())
                {
                    terminate(&mut guard).await;
                }
                Response::Status {
                    running: guard.is_some(),
                    pid: guard.as_ref().and_then(|running| running.child.id()),
                }
            }
            Request::StopKernel => {
                let mut guard = state.running.lock().await;
                if guard.as_ref().is_some_and(|running| running.owner != peer) {
                    return error("kernel session belongs to another user");
                }
                terminate(&mut guard).await;
                Response::Stopped
            }
        }
    }

    fn error(message: &str) -> Response {
        Response::Error {
            message: message.into(),
        }
    }

    async fn start(state: &Service, peer: u32, yaml: &str) -> anyhow::Result<u32> {
        if !xboard_core::kernel::launcher::PRIVILEGED_LAUNCH_ENABLED {
            anyhow::bail!("privileged launcher disabled by build policy");
        }
        secure_fs::validate_root_file(Path::new(KERNEL_PATH), true)?;
        let reservation = std::net::TcpListener::bind("127.0.0.1:0")?;
        let private_addr = reservation.local_addr()?.to_string();
        let private_secret =
            uuid::Uuid::new_v4().simple().to_string() + &uuid::Uuid::new_v4().simple().to_string();
        let prepared =
            prepare_privileged_config(yaml, &private_addr, &private_secret).map_err(|_| {
                anyhow::anyhow!("subscription contains unsupported privileged configuration")
            })?;
        // Hold ownership through stop/stage/spawn/activate, not only assignment.
        let mut guard = state.running.lock().await;
        if guard.as_ref().is_some_and(|running| running.owner != peer) {
            anyhow::bail!("kernel belongs to another user");
        }
        terminate(&mut guard).await;
        // A controller can be healthy even when mihomo failed to create TUN.
        // Never mistake a device belonging to another process for our own.
        let vacant_deadline = tokio::time::Instant::now() + Duration::from_secs(3);
        while read_interface().await?.is_some() {
            if tokio::time::Instant::now() >= vacant_deadline {
                anyhow::bail!(
                    "utun1989 is already in use; stop the conflicting VPN before connecting"
                );
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        let directory = Path::new(STATE_DIR).join(uuid::Uuid::new_v4().simple().to_string());
        secure_fs::private_directory(&directory)?;
        let staged = secure_fs::stage_config(&directory, prepared.yaml.as_bytes())?;
        let log = secure_fs::create_private_file(&directory.join("mihomo.log"))?;
        let mut command = Command::new(KERNEL_PATH);
        command
            .env_clear()
            .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
            .env("HOME", &directory)
            .env("SAFE_PATHS", &directory)
            .env("LC_ALL", "C")
            .current_dir(&directory)
            .arg("-d")
            .arg(&directory)
            .arg("-f")
            .arg(&staged)
            .stdin(Stdio::null())
            .stdout(Stdio::from(log.try_clone()?))
            .stderr(Stdio::from(log))
            .kill_on_drop(true);
        drop(reservation);
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(error) => {
                let _ = std::fs::remove_dir_all(&directory);
                return Err(error.into());
            }
        };
        let pid = child
            .id()
            .ok_or_else(|| anyhow::anyhow!("kernel has no process ID"))?;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
        let readiness = async {
            verify_listener(&mut child, &private_addr).await?;
            loop {
                if let Some(status) = child.try_wait()? {
                    anyhow::bail!("mihomo exited before TUN readiness: {status}");
                }
                if read_interface()
                    .await?
                    .as_deref()
                    .is_some_and(tun_readiness::ready)
                {
                    if child.try_wait()?.is_none() {
                        return Ok::<(), anyhow::Error>(());
                    }
                    anyhow::bail!("mihomo exited while TUN became ready");
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        };
        let ready = match tokio::time::timeout_at(deadline, readiness).await {
            Ok(result) => result,
            Err(_) => Err(anyhow::anyhow!(
                "TUN utun1989 did not become UP with IPv4 within 15 seconds"
            )),
        };
        if let Err(failure) = ready {
            let _ = child.kill().await;
            let _ = child.wait().await;
            let detail = tun_readiness::diagnostic(
                &directory.join("mihomo.log"),
                &failure.to_string(),
                &[&private_secret, &prepared.public_secret, &private_addr],
            );
            let _ = std::fs::remove_dir_all(&directory);
            return Err(anyhow::anyhow!(detail));
        }
        let gateway = match start_gateway(
            &prepared.public_addr,
            &prepared.public_secret,
            &private_addr,
            &private_secret,
        )
        .await
        {
            Ok(gateway) => gateway,
            Err(failure) => {
                let _ = child.kill().await;
                let _ = child.wait().await;
                let detail = tun_readiness::diagnostic(
                    &directory.join("mihomo.log"),
                    &failure,
                    &[&private_secret, &prepared.public_secret, &private_addr],
                );
                let _ = std::fs::remove_dir_all(&directory);
                return Err(anyhow::anyhow!(detail));
            }
        };
        *guard = Some(Running {
            owner: peer,
            child,
            gateway,
            directory,
        });
        Ok(pid)
    }

    async fn read_interface() -> anyhow::Result<Option<String>> {
        let output = tokio::time::timeout(
            Duration::from_secs(2),
            Command::new("/sbin/ifconfig")
                .env_clear()
                .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
                .env("LC_ALL", "C")
                .arg("utun1989")
                .kill_on_drop(true)
                .output(),
        )
        .await??;
        if output.status.success() {
            Ok(Some(String::from_utf8_lossy(&output.stdout).into_owned()))
        } else {
            let error = String::from_utf8_lossy(&output.stderr);
            if error.contains("does not exist") || error.contains("no such interface") {
                Ok(None)
            } else {
                anyhow::bail!("cannot inspect TUN utun1989: {}", error.trim());
            }
        }
    }

    async fn verify_listener(child: &mut Child, private_addr: &str) -> anyhow::Result<()> {
        let pid = child
            .id()
            .ok_or_else(|| anyhow::anyhow!("missing child process"))?;
        let addr: std::net::SocketAddr = private_addr.parse()?;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(8);
        while tokio::time::Instant::now() < deadline {
            if child.try_wait()?.is_some() {
                anyhow::bail!("mihomo exited during startup; consult administrator logs");
            }
            // Never send the private bearer to an unverified listener.
            let output = tokio::time::timeout(
                Duration::from_secs(2),
                Command::new("/usr/sbin/lsof")
                    .env_clear()
                    .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
                    .env("LC_ALL", "C")
                    .args([
                        "-nP",
                        "-a",
                        "-p",
                        &pid.to_string(),
                        "-iTCP",
                        "-sTCP:LISTEN",
                        "-Fpn",
                    ])
                    .output(),
            )
            .await??;
            let listing = String::from_utf8_lossy(&output.stdout);
            if output.status.success()
                && listing.lines().any(|line| line == format!("p{pid}"))
                && listing
                    .lines()
                    .any(|line| line == format!("n{}:{}", addr.ip(), addr.port()))
            {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        anyhow::bail!("private controller ownership could not be verified")
    }

    async fn terminate(running: &mut Option<Running>) {
        if let Some(mut running) = running.take() {
            running.gateway.stop().await;
            let _ = running.child.kill().await;
            let _ = running.child.wait().await;
            // The path was generated in the root-only state directory.
            if let Err(error) = std::fs::remove_dir_all(&running.directory) {
                tracing::warn!(%error, "remove retired session state");
            }
        }
    }
}
