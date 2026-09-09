use crate::security::{InstalledPaths, PathGuards};
use anyhow::{bail, Context, Result};
use std::{
    net::{Ipv4Addr, TcpListener},
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    path::PathBuf,
    process::Stdio,
    time::Duration,
};
use tokio::process::{Child, Command};
use windows_sys::Win32::{
    Foundation::HANDLE,
    NetworkManagement::IpHelper::{
        FreeMibTable, GetExtendedTcpTable, GetIfTable2, MIB_IF_TABLE2, MIB_TCPTABLE_OWNER_PID,
        TCP_TABLE_OWNER_PID_LISTENER,
    },
    NetworkManagement::Ndis::IfOperStatusUp,
    Networking::WinSock::AF_INET,
    System::JobObjects::*,
};
use xboard_core::{
    kernel::control_proxy::{start_gateway, ControlGateway},
    profile::privileged::{prepare_privileged_config, PrivilegedConfig},
};

pub struct Prepared {
    config: PrivilegedConfig,
    private_addr: String,
    private_secret: String,
    private_port: u16,
}
impl Prepared {
    pub fn new(yaml: &str) -> Result<Self> {
        let reservation = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
        let private_port = reservation.local_addr()?.port();
        let private_addr = format!("127.0.0.1:{private_port}");
        let private_secret = format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        );
        let config = prepare_privileged_config(yaml, &private_addr, &private_secret)?;
        // Once released, the OS listener's owning PID is checked before any
        // private bearer is transmitted, closing the port-reuse secret leak.
        drop(reservation);
        Ok(Self {
            config,
            private_addr,
            private_secret,
            private_port,
        })
    }
}

pub struct Kernel {
    child: Child,
    gateway: Option<ControlGateway>,
    _job: OwnedHandle,
    directory: PathBuf,
    _directory_guard: PathGuards,
}
impl Kernel {
    pub fn pid(&self) -> Option<u32> {
        self.child.id()
    }
    pub fn has_exited(&mut self) -> bool {
        self.child
            .try_wait()
            .map(|status| status.is_some())
            .unwrap_or(true)
            || !tun_adapter_up().unwrap_or(false)
    }

    pub async fn start(paths: &InstalledPaths, prepared: Prepared) -> Result<Self> {
        if tun_adapter_up()? {
            bail!("Sufe TUN 网卡已被其他连接占用，请先断开该连接");
        }
        let (directory, guard) = paths.session_directory()?;
        let config_path = directory.join("config.yaml");
        let log_path = directory.join("mihomo.log");
        let mut config = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&config_path)?;
        use std::io::Write;
        config.write_all(prepared.config.yaml.as_bytes())?;
        config.sync_all()?;
        drop(config);
        let log = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&log_path)?;
        let stderr = log.try_clone()?;
        let job = new_kill_job()?;
        let mut command = Command::new(&paths.kernel);
        command
            .args(["-d"])
            .arg(&paths.kernel_home)
            .arg("-f")
            .arg(&config_path)
            .current_dir(&paths.dll_dir)
            .stdin(Stdio::null())
            .stdout(Stdio::from(log))
            .stderr(Stdio::from(stderr))
            .env_clear()
            .env("SystemRoot", system_root()?)
            .env("TEMP", &directory)
            .env("TMP", &directory)
            .creation_flags(0x08000000)
            .kill_on_drop(true);
        let mut child = command.spawn().context("spawn protected mihomo")?;
        let process = child.raw_handle().context("mihomo has no process handle")?;
        if unsafe { AssignProcessToJobObject(job.as_raw_handle() as HANDLE, process as HANDLE) }
            == 0
        {
            let error = std::io::Error::last_os_error();
            let _ = child.kill().await;
            let _ = child.wait().await;
            return Err(error).context("attach mihomo to service lifetime job");
        }
        let pid = child.id().context("mihomo has no PID")?;
        // PnP driver initialization can outlive the REST socket by many seconds.
        // Keep both phases within the client's 60 second IPC request timeout.
        let startup_deadline = tokio::time::Instant::now() + Duration::from_secs(55);
        let ready = wait_for_owned_listener(&mut child, prepared.private_port, pid).await;
        if let Err(error) = ready {
            let _ = child.kill().await;
            let _ = child.wait().await;
            let _ = std::fs::remove_file(&config_path);
            return Err(error);
        }
        // A live REST controller does not prove TUN started: mihomo continues
        // running after adapter/address failures. Confirm the real OS adapter
        // before publishing the gateway or returning Started to the UI.
        if let Err(error) = wait_for_tun_adapter(&mut child, &log_path, startup_deadline).await {
            let _ = child.kill().await;
            let _ = child.wait().await;
            let _ = std::fs::remove_file(&config_path);
            return Err(error);
        }
        let gateway = match start_gateway(
            &prepared.config.public_addr,
            &prepared.config.public_secret,
            &prepared.private_addr,
            &prepared.private_secret,
        )
        .await
        {
            Ok(gateway) => gateway,
            Err(error) => {
                let _ = child.kill().await;
                let _ = child.wait().await;
                let _ = std::fs::remove_file(&config_path);
                bail!("cannot start restricted controller gateway: {error}");
            }
        };
        Ok(Self {
            child,
            gateway: Some(gateway),
            _job: job,
            directory,
            _directory_guard: guard,
        })
    }

    pub async fn stop(mut self) {
        if let Some(gateway) = self.gateway.take() {
            gateway.stop().await;
        }
        let _ = self.child.kill().await;
        let _ = self.child.wait().await;
        // Remove the per-session credential once the process is gone. Only a
        // fixed filename inside our still-locked private directory is touched.
        let _ = std::fs::remove_file(self.directory.join("config.yaml"));
        tracing::info!(pid = self.child.id(), session = %self.directory.file_name().unwrap_or_default().to_string_lossy(), "service kernel stopped");
    }
}

async fn wait_for_tun_adapter(
    child: &mut Child,
    log_path: &std::path::Path,
    deadline: tokio::time::Instant,
) -> Result<()> {
    while tokio::time::Instant::now() < deadline {
        if child.try_wait()?.is_some() {
            bail!("mihomo exited before its TUN adapter was ready");
        }
        // Only scan a bounded, service-owned log. Never send raw log/config
        // content to the UI, which could expose credentials from a provider.
        use std::io::Read;
        let mut prefix = String::new();
        if let Ok(log) = std::fs::File::open(log_path) {
            let _ = log.take(64 * 1024).read_to_string(&mut prefix);
        }
        if prefix.contains("Start TUN listening error:") {
            bail!("TUN 网卡启动失败，请检查是否与其他 VPN 的地址或驱动冲突；详细日志仅管理员可读");
        }
        if tun_adapter_up()? {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    bail!("TUN 网卡未就绪，请检查驱动或其他 VPN 的地址冲突")
}

fn tun_adapter_up() -> Result<bool> {
    let mut table = std::ptr::null_mut();
    let result = unsafe { GetIfTable2(&mut table) };
    if result != 0 {
        return Err(std::io::Error::from_raw_os_error(result as i32).into());
    }
    if table.is_null() {
        bail!("Windows returned no interface table");
    }
    struct Table(*mut MIB_IF_TABLE2);
    impl Drop for Table {
        fn drop(&mut self) {
            unsafe { FreeMibTable(self.0 as _) }
        }
    }
    let table = Table(table);
    let rows = unsafe {
        std::slice::from_raw_parts((*table.0).Table.as_ptr(), (*table.0).NumEntries as usize)
    };
    Ok(rows.iter().any(|row| {
        let end = row
            .Alias
            .iter()
            .position(|value| *value == 0)
            .unwrap_or(row.Alias.len());
        row.OperStatus == IfOperStatusUp && String::from_utf16_lossy(&row.Alias[..end]) == "Sufe"
    }))
}

fn new_kill_job() -> Result<OwnedHandle> {
    let raw = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
    if raw.is_null() {
        return Err(std::io::Error::last_os_error().into());
    }
    let job = unsafe { OwnedHandle::from_raw_handle(raw as _) };
    let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    if unsafe {
        SetInformationJobObject(
            raw,
            JobObjectExtendedLimitInformation,
            &limits as *const _ as _,
            std::mem::size_of_val(&limits) as u32,
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(job)
}

fn system_root() -> Result<PathBuf> {
    // Query the OS, never an inherited user-controlled environment variable.
    let mut buffer = vec![0u16; 32768];
    let length = unsafe {
        windows_sys::Win32::System::SystemInformation::GetWindowsDirectoryW(
            buffer.as_mut_ptr(),
            buffer.len() as u32,
        )
    };
    if length == 0 || length as usize >= buffer.len() {
        bail!("cannot resolve Windows system directory");
    }
    use std::os::windows::ffi::OsStringExt;
    Ok(std::ffi::OsString::from_wide(&buffer[..length as usize]).into())
}

async fn wait_for_owned_listener(child: &mut Child, port: u16, pid: u32) -> Result<()> {
    for _ in 0..450 {
        if let Some(status) = child.try_wait()? {
            bail!("mihomo exited before its protected controller was ready: {status}");
        }
        match listener_pid(port)? {
            Some(owner) if owner == pid => return Ok(()),
            Some(_) => bail!("private controller port belongs to another process"),
            None => tokio::time::sleep(Duration::from_millis(100)).await,
        }
    }
    bail!("mihomo controller startup timed out; inspect the administrator-owned service log")
}

fn listener_pid(port: u16) -> Result<Option<u32>> {
    let mut size = 0;
    unsafe {
        GetExtendedTcpTable(
            std::ptr::null_mut(),
            &mut size,
            0,
            AF_INET as u32,
            TCP_TABLE_OWNER_PID_LISTENER,
            0,
        );
    }
    for _ in 0..4 {
        let mut buffer = vec![0u64; (size as usize).div_ceil(8).max(1)];
        let capacity = buffer.len() * 8;
        size = capacity as u32;
        let status = unsafe {
            GetExtendedTcpTable(
                buffer.as_mut_ptr() as _,
                &mut size,
                0,
                AF_INET as u32,
                TCP_TABLE_OWNER_PID_LISTENER,
                0,
            )
        };
        if status == 122 {
            continue;
        }
        if status != 0 {
            return Err(std::io::Error::from_raw_os_error(status as i32).into());
        }
        let table = unsafe { &*(buffer.as_ptr() as *const MIB_TCPTABLE_OWNER_PID) };
        let count = table.dwNumEntries as usize;
        if 4 + count * std::mem::size_of_val(&table.table[0]) > capacity {
            bail!("invalid TCP owner table");
        }
        let rows = unsafe { std::slice::from_raw_parts(table.table.as_ptr(), count) };
        return Ok(rows
            .iter()
            .find(|row| {
                u16::from_be(row.dwLocalPort as u16) == port
                    && row.dwLocalAddr == u32::from_ne_bytes([127, 0, 0, 1])
            })
            .map(|row| row.dwOwningPid));
    }
    bail!("TCP owner table kept changing")
}

#[cfg(test)]
mod tests {
    #[test]
    fn loopback_listener_owner_is_checked_without_sending_a_secret() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        assert_eq!(
            super::listener_pid(listener.local_addr().unwrap().port()).unwrap(),
            Some(std::process::id())
        );
    }
}
