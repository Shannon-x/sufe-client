//! Kernel diagnostics: pre-flight presence checks and `mihomo.log` tail.
//!
//! These commands are read-only and side-effect-free — they never spawn
//! mihomo or mutate `KernelManager`. The Home page calls `kernel_health`
//! once on mount so it can show a "missing component" banner before the
//! user discovers the problem on Connect; `tail_kernel_log` powers the
//! "View logs" panel that surfaces in the connection-error state.

use std::path::PathBuf;

use serde::Serialize;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_shell::ShellExt;
use tokio::io::{AsyncReadExt, AsyncSeekExt};

use crate::error::{CommandError, CommandResult};
use crate::state::AppState;

/// Default tail size for `tail_kernel_log`. 64 KiB keeps the IPC payload
/// small while still being enough to see a few minutes of mihomo output.
const DEFAULT_LOG_TAIL_BYTES: u64 = 64 * 1024;

#[derive(Serialize, Clone)]
pub struct KernelHealth {
    /// True when the bundled `mihomo` sidecar exists on disk. False here
    /// is a critical install error (corrupted bundle / antivirus removed).
    pub mihomo_present: bool,
    pub mihomo_path: String,
    /// Whether the platform-specific privileged path is ready for TUN.
    ///   * macOS: `Some(true)` once the LaunchDaemon helper is installed.
    ///   * Linux: `Some(true)` once the deb/rpm postinst has granted
    ///     `cap_net_admin` to mihomo via setcap. `Some(false)` on AppImage
    ///     installs (no postinst hook) — the UI uses this to nudge the
    ///     user toward deb/rpm.
    ///   * Windows: `Some(true)` once `xboard-svc` is registered with the
    ///     SCM. `Some(false)` means first-connect will trigger the UAC
    ///     install flow.
    pub helper_present: Option<bool>,
    pub helper_path: Option<String>,
    /// Where mihomo will write `config.yaml` + `mihomo.log`. Useful for
    /// the "View logs" / "Open data dir" affordances.
    pub work_dir: String,
}

#[tauri::command]
pub async fn kernel_health(
    _state: State<'_, AppState>,
    app: AppHandle,
) -> CommandResult<KernelHealth> {
    let mihomo_path = resolve_mihomo_path(&app)?;
    let mihomo_present = tokio::fs::try_exists(&mihomo_path).await.unwrap_or(false);

    let work_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| CommandError::new("app_data_dir", e.to_string()))?
        .join("kernel");

    #[cfg(target_os = "macos")]
    let (helper_present, helper_path) = {
        let path = PathBuf::from(crate::helper_install::INSTALLED_HELPER_PATH);
        let present = tokio::fs::try_exists(&path).await.unwrap_or(false);
        (Some(present), Some(path.display().to_string()))
    };
    // Linux: probe the file capability on the bundled mihomo. Set by the
    // deb/rpm postinst (build_extras/postinst.sh); absent on AppImage and
    // on dev-mode runs from a target/ directory. We surface the path here
    // too so the UI's "no TUN" hint can show the user exactly which
    // binary is missing the capability.
    #[cfg(target_os = "linux")]
    let (helper_present, helper_path) = {
        let present = xboard_core::kernel::linux_caps::has_file_capability(&mihomo_path);
        (Some(present), Some(mihomo_path.display().to_string()))
    };
    // Windows: SCM lookup. `is_registered` is sync, so push it off the
    // tauri thread; failure to query is treated as "not present" so the UI
    // surfaces the install nudge — the worst case is one redundant UAC
    // prompt, which is harmless because `xboard-svc.exe install` is
    // idempotent.
    #[cfg(target_os = "windows")]
    let (helper_present, helper_path) = {
        let present = tokio::task::spawn_blocking(crate::svc_install::is_registered)
            .await
            .unwrap_or(false);
        (
            Some(present),
            Some(format!(r"\\.\pipe\xboard-client-svc")),
        )
    };
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    let (helper_present, helper_path) = (None::<bool>, None::<String>);

    Ok(KernelHealth {
        mihomo_present,
        mihomo_path: mihomo_path.display().to_string(),
        helper_present,
        helper_path,
        work_dir: work_dir.display().to_string(),
    })
}

#[tauri::command]
pub async fn tail_kernel_log(
    app: AppHandle,
    max_bytes: Option<u64>,
) -> CommandResult<String> {
    let log_path = app
        .path()
        .app_data_dir()
        .map_err(|e| CommandError::new("app_data_dir", e.to_string()))?
        .join("kernel")
        .join("mihomo.log");

    if !tokio::fs::try_exists(&log_path).await.unwrap_or(false) {
        return Ok(String::new());
    }

    let max = max_bytes.unwrap_or(DEFAULT_LOG_TAIL_BYTES).max(1);
    let metadata = tokio::fs::metadata(&log_path)
        .await
        .map_err(|e| CommandError::new("io", format!("stat mihomo.log: {e}")))?;
    let len = metadata.len();
    let start = len.saturating_sub(max);

    let mut file = tokio::fs::File::open(&log_path)
        .await
        .map_err(|e| CommandError::new("io", format!("open mihomo.log: {e}")))?;
    file.seek(std::io::SeekFrom::Start(start))
        .await
        .map_err(|e| CommandError::new("io", format!("seek mihomo.log: {e}")))?;
    let mut buf = Vec::with_capacity((len - start).min(max) as usize);
    file.read_to_end(&mut buf)
        .await
        .map_err(|e| CommandError::new("io", format!("read mihomo.log: {e}")))?;

    let text = String::from_utf8_lossy(&buf).into_owned();
    // We sliced mid-file — the first line is almost certainly truncated;
    // drop it so the UI shows clean lines.
    if start > 0 {
        if let Some(idx) = text.find('\n') {
            return Ok(text[idx + 1..].to_string());
        }
    }
    Ok(text)
}

/// What `kernel_version` returns to the UI. The version string is parsed
/// best-effort from `mihomo -v`; if anything goes wrong we surface the raw
/// stdout so a support copy-paste still has signal.
#[derive(Serialize, Clone)]
pub struct KernelVersion {
    /// `"v1.18.7"` on success, `null` if we couldn't parse the output.
    pub version: Option<String>,
    /// Raw `mihomo -v` stdout (a few lines at most). Useful for showing
    /// build metadata in the "内核信息" modal even when parsing fails.
    pub raw: String,
    /// Path the version was sourced from — same as `kernel_health.mihomo_path`.
    pub mihomo_path: String,
}

/// Run `<mihomo> -v` and parse the version string. The kernel ships
/// alongside the app via Tauri's `externalBin`, so today this always
/// reflects the bundled release; an out-of-app updater (deferred) would
/// rewrite the same binary in-place and this command would pick up the
/// new version automatically.
#[tauri::command]
pub async fn kernel_version(app: AppHandle) -> CommandResult<KernelVersion> {
    use tokio::process::Command;
    let mihomo_path = resolve_mihomo_path(&app)?;
    if !tokio::fs::try_exists(&mihomo_path).await.unwrap_or(false) {
        return Err(CommandError::new(
            "mihomo_missing",
            format!("kernel binary missing: {}", mihomo_path.display()),
        ));
    }

    let output = Command::new(&mihomo_path)
        .arg("-v")
        .output()
        .await
        .map_err(|e| CommandError::new("mihomo_exec", format!("mihomo -v: {e}")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        return Err(CommandError::new(
            "mihomo_failed",
            format!("mihomo -v exited {}: {}", output.status, stderr),
        ));
    }

    let raw = String::from_utf8_lossy(&output.stdout).into_owned();
    let version = parse_mihomo_version(&raw);
    Ok(KernelVersion {
        version,
        raw,
        mihomo_path: mihomo_path.display().to_string(),
    })
}

/// `mihomo -v` first line is e.g.
///   `Mihomo Meta v1.18.7 darwin arm64 with go1.22.5 …`
/// — pick the first whitespace token starting with `v`. The parser is
/// deliberately forgiving: any future format change just means version
/// shows up as `null` and the raw text stays visible.
fn parse_mihomo_version(raw: &str) -> Option<String> {
    raw.lines()
        .next()?
        .split_whitespace()
        .find(|tok| tok.starts_with('v') && tok.len() > 1)
        .map(|s| s.to_string())
}

/// Mirror of `state::resolve_mihomo_path`. Duplicated rather than exported
/// so `state.rs` keeps its launcher plumbing private.
fn resolve_mihomo_path(app: &AppHandle) -> Result<PathBuf, CommandError> {
    let cmd = app
        .shell()
        .sidecar("mihomo")
        .map_err(|e| CommandError::new("sidecar", format!("locate mihomo sidecar: {e}")))?;
    let std_cmd: std::process::Command = cmd.into();
    Ok(PathBuf::from(std_cmd.get_program()))
}
