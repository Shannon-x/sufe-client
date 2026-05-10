//! User-facing management of the privileged side that owns mihomo:
//!  * macOS — the `xboard-helper` LaunchDaemon (Unix socket).
//!  * Windows — the `xboard-svc` Windows service (named pipe).
//!  * Linux — N/A (cap_net_admin is granted by the deb/rpm postinst); we
//!    return `supported: false` so the UI hides the "辅助服务" panel.
//!
//! The first-connect path (`KernelLauncher::ensure_privileged`) installs
//! the helper / service transparently. These commands surface the same
//! flows in the UI so the user can:
//! - check whether the daemon is currently installed and reachable,
//! - reinstall after a corrupted update / accidental rm,
//! - cleanly uninstall when removing the app.

use serde::Serialize;
use tauri::{AppHandle, State};

use crate::error::{CommandError, CommandResult};
use crate::state::AppState;

/// What the UI needs to render the "辅助服务" panel:
/// - `supported` — false on Linux, true on macOS/Windows
/// - `installed` — helper binary + plist both present on disk (mac), or
///   `xboard-svc` registered with the SCM (win)
/// - `reachable` — ping over the Unix socket / named pipe succeeded just now
/// - paths — for diagnostics; mostly so users can copy-paste in support.
///   On Windows `plist_path` carries the SCM service name instead.
#[derive(Serialize, Clone, Debug)]
pub struct HelperStatus {
    pub supported: bool,
    pub installed: bool,
    pub reachable: bool,
    pub helper_path: Option<String>,
    pub plist_path: Option<String>,
}

#[tauri::command]
pub async fn helper_status(_state: State<'_, AppState>) -> CommandResult<HelperStatus> {
    #[cfg(target_os = "macos")]
    {
        use std::path::Path;
        let helper_path = crate::helper_install::INSTALLED_HELPER_PATH;
        let plist_path = crate::helper_install::PLIST_PATH;
        let installed = tokio::fs::try_exists(Path::new(helper_path))
            .await
            .unwrap_or(false)
            && tokio::fs::try_exists(Path::new(plist_path))
                .await
                .unwrap_or(false);

        // Reachable = a fresh Unix-socket Ping succeeds. Independent from
        // `installed`: launchctl might have unloaded a present plist, or
        // the helper might have been killed. Cap the probe at 1 s so a
        // hung daemon doesn't freeze the settings panel.
        let reachable = if installed {
            ping_helper().await
        } else {
            false
        };

        Ok(HelperStatus {
            supported: true,
            installed,
            reachable,
            helper_path: Some(helper_path.to_string()),
            plist_path: Some(plist_path.to_string()),
        })
    }
    #[cfg(target_os = "windows")]
    {
        // SCM lookup is sync; spawn_blocking keeps it off the tauri thread.
        let installed =
            tokio::task::spawn_blocking(crate::svc_install::is_registered)
                .await
                .unwrap_or(false);
        let reachable = if installed {
            crate::svc_install::ping_svc().await
        } else {
            false
        };
        Ok(HelperStatus {
            supported: true,
            installed,
            reachable,
            helper_path: Some(format!(r"\\.\pipe\xboard-client-svc")),
            plist_path: Some(crate::svc_install::SVC_NAME.to_string()),
        })
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        Ok(HelperStatus {
            supported: false,
            installed: false,
            reachable: false,
            helper_path: None,
            plist_path: None,
        })
    }
}

#[tauri::command]
pub async fn helper_install(app: AppHandle) -> CommandResult<()> {
    #[cfg(target_os = "macos")]
    {
        let installer = crate::helper_install::build_installer(&app);
        installer
            .install()
            .await
            .map_err(|e| CommandError::new("helper_install", e.to_string()))?;
        Ok(())
    }
    #[cfg(target_os = "windows")]
    {
        let installer = crate::svc_install::build_installer(&app);
        installer
            .install()
            .await
            .map_err(|e| CommandError::new("svc_install", e.to_string()))?;
        Ok(())
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = app;
        Err(CommandError::new(
            "helper_unsupported",
            "辅助服务管理目前仅支持 macOS / Windows",
        ))
    }
}

#[tauri::command]
pub async fn helper_uninstall(app: AppHandle) -> CommandResult<()> {
    #[cfg(target_os = "macos")]
    {
        let installer = crate::helper_install::build_installer(&app);
        installer
            .uninstall()
            .await
            .map_err(|e| CommandError::new("helper_uninstall", e.to_string()))?;
        Ok(())
    }
    #[cfg(target_os = "windows")]
    {
        let installer = crate::svc_install::build_installer(&app);
        installer
            .uninstall()
            .await
            .map_err(|e| CommandError::new("svc_uninstall", e.to_string()))?;
        Ok(())
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = app;
        Err(CommandError::new(
            "helper_unsupported",
            "辅助服务管理目前仅支持 macOS / Windows",
        ))
    }
}

#[cfg(target_os = "macos")]
async fn ping_helper() -> bool {
    use std::time::Duration;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::UnixStream;
    use xboard_core::kernel::ipc::{Frame, FrameBody, Request, HELPER_SOCKET_PATH};

    let connect = tokio::time::timeout(
        Duration::from_millis(500),
        UnixStream::connect(HELPER_SOCKET_PATH),
    )
    .await;
    let stream = match connect {
        Ok(Ok(s)) => s,
        _ => return false,
    };
    let (read_half, mut write_half) = stream.into_split();
    let frame = Frame {
        id: 1,
        body: FrameBody::Request(Request::Ping),
    };
    let mut line = match serde_json::to_string(&frame) {
        Ok(s) => s,
        Err(_) => return false,
    };
    line.push('\n');
    if write_half.write_all(line.as_bytes()).await.is_err() {
        return false;
    }
    let _ = write_half.shutdown().await;
    let mut reader = BufReader::new(read_half);
    let mut buf = String::new();
    let read = tokio::time::timeout(Duration::from_millis(500), reader.read_line(&mut buf)).await;
    matches!(read, Ok(Ok(n)) if n > 0)
}
