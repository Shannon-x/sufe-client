//! Windows-only: install / uninstall the `xboard-svc` Windows Service.
//!
//! Mirrors `helper_install` on macOS but the elevation channel is different:
//!  * macOS — one `osascript … with administrator privileges` call.
//!  * Windows — UAC via `ShellExecuteExW` with `lpVerb = "runas"`. The OS
//!    pops the consent dialog; the elevated child process is the bundled
//!    `xboard-svc.exe` re-invoked with `install` / `uninstall` subcommand.
//!
//! We don't try to register the service from inside this elevated UI
//! process — we hand the privilege off to `xboard-svc.exe`, which has the
//! windows-service crate plumbing already. That keeps the elevated surface
//! tiny and reuses the same install code path that an admin running
//! `xboard-svc.exe install` from a shell would take.

#![cfg(target_os = "windows")]

use std::ffi::OsString;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use tauri::{AppHandle, Manager};
use tauri_plugin_shell::ShellExt;
use windows_sys::Win32::Foundation::{GetLastError, ERROR_CANCELLED};
use windows_sys::Win32::UI::Shell::{
    ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::SW_HIDE;
use windows_sys::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject, INFINITE};
use windows_sys::Win32::Foundation::CloseHandle;

use xboard_core::kernel::launcher::{LauncherError, SvcInstaller};

/// Service name registered with the SCM. Mirrors the constant in `xboard-svc`.
pub(crate) const SVC_NAME: &str = "xboard-svc";

/// Build a ready-to-use `SvcInstaller` for the running Tauri app. The shell
/// keeps a handle to the app so we can resolve the `xboard-svc.exe` sidecar
/// at install time (it ships next to the main exe in production, and under
/// `target/<profile>/binaries/` during `tauri dev`).
pub fn build_installer(app: &AppHandle) -> Arc<dyn SvcInstaller> {
    Arc::new(RunasInstaller {
        bundled_svc: bundled_svc_path(app),
    })
}

#[derive(Debug)]
struct RunasInstaller {
    bundled_svc: Option<PathBuf>,
}

#[async_trait]
impl SvcInstaller for RunasInstaller {
    async fn install(&self) -> Result<(), LauncherError> {
        let exe = self.bundled_svc.clone().ok_or_else(|| {
            LauncherError::ServiceMissing(
                "bundled xboard-svc.exe missing from the app".into(),
            )
        })?;
        if !exe.exists() {
            return Err(LauncherError::ServiceMissing(format!(
                "xboard-svc.exe not found at {}",
                exe.display()
            )));
        }
        // Run the elevation off-thread; the Win32 dialog is modal and will
        // stall the tauri runtime if we await it inline.
        tokio::task::spawn_blocking(move || run_elevated(&exe, "install"))
            .await
            .map_err(|e| LauncherError::Other(format!("join: {e}")))??;

        // Give SCM a moment to finish creating + auto-starting the service
        // so the launcher's follow-up Ping doesn't race the install.
        tokio::time::sleep(std::time::Duration::from_millis(800)).await;
        Ok(())
    }

    async fn uninstall(&self) -> Result<(), LauncherError> {
        let exe = self.bundled_svc.clone().ok_or_else(|| {
            LauncherError::ServiceMissing(
                "bundled xboard-svc.exe missing from the app".into(),
            )
        })?;
        tokio::task::spawn_blocking(move || run_elevated(&exe, "uninstall"))
            .await
            .map_err(|e| LauncherError::Other(format!("join: {e}")))?
    }
}

/// Run `<exe> <verb>` elevated. Returns Ok on exit status 0; maps the UAC
/// "user clicked No" to `LauncherError::NeedsConsent` so the manager can
/// fall back to SystemProxy without screaming.
fn run_elevated(exe: &Path, verb: &str) -> Result<(), LauncherError> {
    let exe_w: Vec<u16> = exe
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let runas_w: Vec<u16> = OsString::from("runas")
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let args_w: Vec<u16> = OsString::from(verb)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    let mut sei: SHELLEXECUTEINFOW = unsafe { std::mem::zeroed() };
    sei.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
    sei.fMask = SEE_MASK_NOCLOSEPROCESS;
    sei.lpVerb = runas_w.as_ptr();
    sei.lpFile = exe_w.as_ptr();
    sei.lpParameters = args_w.as_ptr();
    sei.nShow = SW_HIDE as i32;

    let ok = unsafe { ShellExecuteExW(&mut sei) };
    if ok == 0 {
        let code = unsafe { GetLastError() };
        if code == ERROR_CANCELLED {
            return Err(LauncherError::NeedsConsent("用户拒绝了管理员授权".into()));
        }
        return Err(LauncherError::Other(format!(
            "ShellExecuteEx({verb}) failed: Win32 error {code}"
        )));
    }
    if sei.hProcess == 0 {
        // Some shell verbs don't return a process handle — treat as success
        // because `runas` should always give us one, but be defensive.
        return Ok(());
    }

    let process = sei.hProcess;
    unsafe { WaitForSingleObject(process, INFINITE) };
    let mut exit_code: u32 = 1;
    let got = unsafe { GetExitCodeProcess(process, &mut exit_code) };
    unsafe { CloseHandle(process) };
    if got == 0 {
        return Err(LauncherError::Other(
            "GetExitCodeProcess failed after runas".into(),
        ));
    }
    if exit_code != 0 {
        return Err(LauncherError::Other(format!(
            "xboard-svc.exe {verb} exited with code {exit_code}"
        )));
    }
    Ok(())
}

/// Locate the bundled `xboard-svc.exe`. Tauri 2 places `externalBin` entries
/// next to the main executable in production, and under
/// `target/<profile>/binaries/xboard-svc-<triple>.exe` during dev.
///
/// We don't go through `tauri-plugin-shell::sidecar()` here because we don't
/// want the shell scope to whitelist the service for direct invocation from
/// the UI — only this elevated flow should ever launch it.
fn bundled_svc_path(app: &AppHandle) -> Option<PathBuf> {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            let candidate = parent.join("xboard-svc.exe");
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }
    if let Ok(resource_root) = app.path().resource_dir() {
        let candidate = resource_root.join("xboard-svc.exe");
        if candidate.exists() {
            return Some(candidate);
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        let mut cur = exe.as_path();
        while let Some(parent) = cur.parent() {
            let candidate = parent.join("binaries");
            if candidate.is_dir() {
                if let Ok(rd) = std::fs::read_dir(&candidate) {
                    for entry in rd.flatten() {
                        let name = entry.file_name();
                        let s = name.to_string_lossy();
                        if s.starts_with("xboard-svc") && s.ends_with(".exe") {
                            return Some(entry.path());
                        }
                    }
                }
            }
            cur = parent;
        }
    }
    None
}

/// Best-effort ping: try opening the named pipe and reading a Pong. Used by
/// the user-facing "辅助服务" status panel to surface "运行中" vs
/// "未运行/未安装" without forcing the connection through the kernel manager.
pub async fn ping_svc() -> bool {
    use std::time::Duration;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::windows::named_pipe::ClientOptions;
    use xboard_core::kernel::ipc::{Frame, FrameBody, Request, SVC_PIPE_PATH};

    let connect = tokio::time::timeout(Duration::from_millis(500), async {
        ClientOptions::new().open(SVC_PIPE_PATH)
    })
    .await;
    let mut client = match connect {
        Ok(Ok(c)) => c,
        _ => return false,
    };
    let frame = Frame {
        id: 1,
        body: FrameBody::Request(Request::Ping),
    };
    let mut line = match serde_json::to_string(&frame) {
        Ok(s) => s,
        Err(_) => return false,
    };
    line.push('\n');
    if client.write_all(line.as_bytes()).await.is_err() {
        return false;
    }
    if client.flush().await.is_err() {
        return false;
    }
    let (read, _write) = tokio::io::split(client);
    let mut reader = BufReader::new(read);
    let mut buf = String::new();
    let read =
        tokio::time::timeout(Duration::from_millis(500), reader.read_line(&mut buf)).await;
    matches!(read, Ok(Ok(n)) if n > 0)
}

/// Probe whether the service is registered with the SCM. Used by `helper_status`
/// to populate the "installed" flag without going through the named pipe.
pub fn is_registered() -> bool {
    use windows_service::service::ServiceAccess;
    use windows_service::service_manager::{ServiceManager, ServiceManagerAccess};
    let Ok(manager) =
        ServiceManager::local_computer(None::<&str>, ServiceManagerAccess::CONNECT)
    else {
        return false;
    };
    manager
        .open_service(SVC_NAME, ServiceAccess::QUERY_STATUS)
        .is_ok()
}
