use crate::{
    runtime::{Kernel, Prepared},
    security::{self, InstalledPaths, SecurityAttributes},
};
use anyhow::{bail, Context, Result};
use std::{
    ffi::OsString,
    os::windows::io::AsRawHandle,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::windows::named_pipe::{NamedPipeServer, ServerOptions},
    sync::{Mutex, Notify, Semaphore},
};
use windows_service::{
    service::*,
    service_control_handler::{self, ServiceControlHandlerResult},
    service_dispatcher,
    service_manager::{ServiceManager, ServiceManagerAccess},
};
use xboard_core::kernel::ipc::{Frame, FrameBody, Request, Response, SVC_PIPE_PATH};

const NAME: &str = "xboard-svc";

// JSON quoting can double the bounded 2 MiB YAML payload.
const MAX_FRAME: usize = 4 * 1024 * 1024 + 4096;

pub fn main() -> Result<()> {
    let _ = tracing_subscriber::fmt()
        .with_target(false)
        .with_writer(std::io::stderr)
        .try_init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("install") => {
            if args.len() != 3 || args[1] != "--allowed-sid" {
                bail!("usage: xboard-svc install --allowed-sid <pre-elevation user SID>");
            }
            security::validate_sid(&args[2])?;
            install(&args[2])
        }
        Some("uninstall") if args.len() == 1 => uninstall(),
        Some("check-install") if args.len() == 1 => {
            let paths = InstalledPaths::load()?;
            println!(
                "Protected service installation verified: {}",
                paths.service.display()
            );
            Ok(())
        }
        Some("probe-tun") if args.len() == 1 => crate::probe::run(&InstalledPaths::load()?),
        Some("probe-ipc") if args.len() == 1 => crate::ipc_probe::run(),
        Some("--allowed-sid") if args.len() == 2 => {
            service_dispatcher::start(NAME, ffi_service_main).map_err(Into::into)
        }
        _ => bail!("service arguments missing or invalid; use the Sufe installer"),
    }
}

fn install(allowed_sid: &str) -> Result<()> {
    let paths = InstalledPaths::load()?;
    let manager = ServiceManager::local_computer(
        None::<&str>,
        ServiceManagerAccess::CONNECT | ServiceManagerAccess::CREATE_SERVICE,
    )?;
    let info = ServiceInfo {
        name: NAME.into(),
        display_name: "Sufe TUN Service".into(),
        service_type: ServiceType::OWN_PROCESS,
        start_type: ServiceStartType::AutoStart,
        error_control: ServiceErrorControl::Normal,
        executable_path: paths.service.clone(),
        launch_arguments: vec!["--allowed-sid".into(), allowed_sid.into()],
        dependencies: vec![],
        account_name: None,
        account_password: None,
    };
    let access = ServiceAccess::CHANGE_CONFIG
        | ServiceAccess::QUERY_STATUS
        | ServiceAccess::START
        | ServiceAccess::STOP;
    let service = match manager.create_service(&info, access) {
        Ok(service) => service,
        Err(windows_service::Error::Winapi(error)) if error.raw_os_error() == Some(1073) => {
            let service = manager.open_service(NAME, access)?;
            if service.query_status()?.current_state != ServiceState::Stopped {
                service.stop()?;
                wait_state(&service, ServiceState::Stopped)?;
            }
            service.change_config(&info)?;
            service
        }
        Err(error) => return Err(error.into()),
    };
    service.set_description("Sufe private TUN broker. Only validated inline configuration and restricted controller operations are accepted.")?;
    // AutoStart only controls reboot behavior; start explicitly now.
    service.start(&[] as &[OsString])?;
    wait_state(&service, ServiceState::Running)?;
    Ok(())
}

fn wait_state(service: &windows_service::service::Service, expected: ServiceState) -> Result<()> {
    for _ in 0..200 {
        let status = service.query_status()?;
        if status.current_state == expected {
            return Ok(());
        }
        if expected == ServiceState::Running && status.current_state == ServiceState::Stopped {
            bail!("Sufe service failed to start ({:?})", status.exit_code);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    bail!("Sufe service did not reach {expected:?} before timeout")
}

fn uninstall() -> Result<()> {
    let manager = ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)?;
    let service = match manager.open_service(
        NAME,
        ServiceAccess::STOP | ServiceAccess::DELETE | ServiceAccess::QUERY_STATUS,
    ) {
        Ok(service) => service,
        Err(windows_service::Error::Winapi(error)) if error.raw_os_error() == Some(1060) => {
            return Ok(())
        }
        Err(error) => return Err(error.into()),
    };
    if service.query_status()?.current_state != ServiceState::Stopped {
        service.stop()?;
        wait_state(&service, ServiceState::Stopped)?;
    }
    service.delete()?;
    Ok(())
}

windows_service::define_windows_service!(ffi_service_main, service_main);
fn service_main(_args: Vec<OsString>) {
    if let Err(error) = run_service() {
        tracing::error!(error = %error, "Sufe service stopped");
    }
}

struct State {
    paths: InstalledPaths,
    kernel: Mutex<Option<Kernel>>,
    stopping: AtomicBool,
}
impl State {
    async fn stop(&self) {
        if let Some(kernel) = self.kernel.lock().await.take() {
            kernel.stop().await;
        }
    }
}

fn run_service() -> Result<()> {
    if security::current_user_sid()? != "S-1-5-18" {
        bail!("broker must be started by SCM as LocalSystem");
    }
    // ServiceMain receives StartService arguments, not ImagePath arguments.
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 2 || args[0] != "--allowed-sid" {
        bail!("invalid installed service arguments");
    }
    let allowed_sid = args[1].clone();
    security::validate_sid(&allowed_sid)?;
    let stop = Arc::new(Notify::new());
    let stop_handler = stop.clone();
    let status = service_control_handler::register(NAME, move |control| match control {
        ServiceControl::Stop | ServiceControl::Shutdown => {
            stop_handler.notify_one();
            ServiceControlHandlerResult::NoError
        }
        ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
        _ => ServiceControlHandlerResult::NotImplemented,
    })?;
    let make_status = |state| ServiceStatus {
        service_type: ServiceType::OWN_PROCESS,
        current_state: state,
        controls_accepted: if state == ServiceState::Running {
            ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN
        } else {
            ServiceControlAccept::empty()
        },
        exit_code: ServiceExitCode::Win32(0),
        checkpoint: 0,
        wait_hint: Duration::from_secs(15),
        process_id: None,
    };
    status.set_service_status(make_status(ServiceState::StartPending))?;
    let state = Arc::new(State {
        paths: InstalledPaths::load()?,
        kernel: Mutex::new(None),
        stopping: AtomicBool::new(false),
    });
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let result = runtime.block_on(async {
        let initial_pipe = make_pipe(&allowed_sid, true)?;
        status.set_service_status(make_status(ServiceState::Running))?;
        let accept = serve(initial_pipe, allowed_sid, state.clone()); tokio::pin!(accept);
        let mut health = tokio::time::interval(Duration::from_secs(1));
        let result = loop {
            tokio::select! {
                result = &mut accept => break result,
                _ = stop.notified() => break Ok(()),
                _ = health.tick() => {
                    let mut kernel = state.kernel.lock().await;
                    if kernel.as_mut().is_some_and(Kernel::has_exited) { if let Some(exited) = kernel.take() { exited.stop().await; } }
                }
            }
        };
        state.stopping.store(true, Ordering::Release); state.stop().await; result
    });
    status.set_service_status(make_status(ServiceState::Stopped))?;
    result
}

fn make_pipe(sid: &str, first: bool) -> Result<NamedPipeServer> {
    // FILE_GENERIC_READ | FILE_WRITE_DATA: no FILE_CREATE_PIPE_INSTANCE or
    // WRITE_DAC for ordinary callers. No remote/anonymous/group-wide access.
    let mut security = SecurityAttributes::from_sddl(&format!(
        "O:SYG:SYD:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;0x12008b;;;{sid})"
    ))?;
    Ok(unsafe {
        ServerOptions::new()
            .first_pipe_instance(first)
            .reject_remote_clients(true)
            .max_instances(20)
            .create_with_security_attributes_raw(
                SVC_PIPE_PATH,
                &mut security.attrs as *mut _ as _,
            )?
    })
}

async fn serve(mut pipe: NamedPipeServer, sid: String, state: Arc<State>) -> Result<()> {
    let permits = Arc::new(Semaphore::new(16));
    loop {
        let permit = permits.clone().acquire_owned().await?;
        pipe.connect().await?;
        // Keep a pending instance so the pipe name can never be taken over.
        let next = make_pipe(&sid, false)?;
        let connected = std::mem::replace(&mut pipe, next);
        let sid = sid.clone();
        let state = state.clone();
        tokio::spawn(async move {
            let _permit = permit;
            if let Err(error) = handle(connected, &sid, &state).await {
                tracing::warn!(error = %error, "rejected or failed pipe operation");
            }
        });
    }
}

async fn handle(mut pipe: NamedPipeServer, sid: &str, state: &State) -> Result<()> {
    let mut line = String::new();
    {
        let read = (&mut pipe).take((MAX_FRAME + 1) as u64);
        let mut reader = BufReader::new(read);
        tokio::time::timeout(Duration::from_secs(10), reader.read_line(&mut line))
            .await
            .context("pipe frame timeout")??;
    }
    if line.len() > MAX_FRAME || !line.ends_with('\n') {
        bail!("invalid or oversized IPC frame");
    }
    if security::pipe_caller_sid(pipe.as_raw_handle() as _)? != sid {
        bail!("pipe caller is not the installing user");
    }
    let frame: Frame = serde_json::from_str(&line).context("invalid IPC JSON")?;
    let FrameBody::Request(request) = frame.body else {
        bail!("expected IPC request");
    };
    let response = dispatch(state, request).await;
    let mut encoded = serde_json::to_vec(&Frame::response(frame.id, response))?;
    encoded.push(b'\n');
    tokio::time::timeout(Duration::from_secs(10), pipe.write_all(&encoded)).await??;
    Ok(())
}

async fn dispatch(state: &State, request: Request) -> Response {
    if state.stopping.load(Ordering::Acquire) {
        return Response::Error {
            message: "Sufe service is stopping".into(),
        };
    }
    match request {
        Request::Ping => Response::Pong {
            helper_version: xboard_core::kernel::ipc::service_version(),
        },
        Request::Status => {
            let mut kernel = state.kernel.lock().await;
            if kernel.as_mut().is_some_and(Kernel::has_exited) {
                if let Some(exited) = kernel.take() {
                    exited.stop().await;
                }
            }
            Response::Status {
                running: kernel.is_some(),
                pid: kernel.as_ref().and_then(Kernel::pid),
            }
        }
        Request::StartKernel { .. } => Response::Error {
            message: "Path-based privileged execution was removed; update the Sufe client".into(),
        },
        Request::StartKernelV2 { config_yaml } => {
            let prepared = match Prepared::new(&config_yaml) {
                Ok(value) => value,
                Err(error) => {
                    return Response::Error {
                        message: format!("Unsafe or unsupported TUN configuration: {error}"),
                    }
                }
            };
            let mut kernel = state.kernel.lock().await;
            if state.stopping.load(Ordering::Acquire) {
                return Response::Error {
                    message: "Sufe service is stopping".into(),
                };
            }
            if let Some(previous) = kernel.take() {
                previous.stop().await;
            }
            match Kernel::start(&state.paths, prepared).await {
                Ok(started) => {
                    let pid = started.pid().unwrap_or(0);
                    *kernel = Some(started);
                    Response::Started { pid }
                }
                Err(error) => {
                    tracing::error!(error = %error, "protected kernel startup failed");
                    Response::Error {
                        message: error.to_string(),
                    }
                }
            }
        }
        Request::StopKernel => {
            state.stop().await;
            Response::Stopped
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn pipe_permissions_exclude_instance_creation() {
        assert_eq!(0x12008bu32 & (4 | 0x40000 | 0x80000), 0);
    }
}
