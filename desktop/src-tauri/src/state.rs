use std::path::PathBuf;
use std::sync::Arc;

use once_cell::sync::OnceCell;
use parking_lot::RwLock;
use serde::Serialize;
use tauri::{AppHandle, Manager};
use tauri_plugin_shell::ShellExt;
use xboard_core::api::HttpClient;
use xboard_core::kernel::TunnelMode;
use xboard_core::profile::ProfileFetcher;
use xboard_core::storage::SecureStore;
use xboard_core::tunnel::DefaultSystemProxy;
use xboard_core::{KernelLauncher, KernelManager, MihomoDriver};

use crate::error::CommandError;
use crate::persistence::Persistence;

/// Process-wide state. Held inside a `tauri::State<AppState>` and shared
/// across all commands. Both fields are guarded by sync `RwLock`s — locks
/// MUST be dropped before any `.await`.
#[derive(Default)]
pub struct AppState {
    pub client: RwLock<Option<HttpClient>>,
    pub auth: RwLock<Option<AuthSession>>,
    /// User-requested tunnel mode. This is stored before the lazy
    /// `KernelManager` exists so the first connect honors the UI selection.
    pub requested_mode: RwLock<TunnelMode>,
    /// Lazily initialized on the first `connect` call so we don't spin up
    /// the kernel manager (which spawns a broadcast channel) for users who
    /// never get past the login screen.
    pub kernel: OnceCell<Arc<KernelManager>>,
    /// JSON-on-disk preferences (last login email, subscribe token, last
    /// `checkLogin` timestamp). Filled in `lib.rs::setup`.
    pub persistence: OnceCell<Arc<Persistence>>,
    /// OS keychain handle for the Sanctum bearer. Filled in `lib.rs::setup`.
    /// Kept as a trait object so a future Linux-no-dbus / Android backend
    /// can plug in without touching command surfaces.
    pub secure: OnceCell<Arc<dyn SecureStore>>,
    /// User preference for the system-proxy guard, cached before the kernel
    /// exists. `None` = use the manager default (enabled); `Some(b)` = the
    /// user toggled it. Applied to the manager in `ensure_kernel`.
    pub proxy_guard_enabled: RwLock<Option<bool>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AuthSession {
    pub email: String,
    pub is_admin: bool,
    /// The Xboard subscribe token (the `?token=` query value, NOT the
    /// Sanctum bearer). Safe to send to the UI — the bearer is held only
    /// in `HttpClient::set_bearer` and never crosses the IPC boundary.
    pub subscribe_token: String,
}

impl AppState {
    pub fn snapshot_client(&self) -> Option<HttpClient> {
        self.client.read().clone()
    }

    pub fn snapshot_auth(&self) -> Option<AuthSession> {
        self.auth.read().clone()
    }

    pub fn snapshot_persistence(&self) -> Option<Arc<Persistence>> {
        self.persistence.get().cloned()
    }

    pub fn snapshot_secure(&self) -> Option<Arc<dyn SecureStore>> {
        self.secure.get().cloned()
    }

    /// Get-or-create the `KernelManager`. Resolves the bundled mihomo
    /// sidecar through `tauri-plugin-shell` and seats the per-platform
    /// launcher (DirectLauncher on Linux today; svc/helper stubs return
    /// fallback errors so the manager downgrades to system proxy on
    /// Windows/macOS until those crates ship).
    pub fn ensure_kernel(&self, app: &AppHandle) -> Result<Arc<KernelManager>, CommandError> {
        if let Some(km) = self.kernel.get() {
            return Ok(km.clone());
        }

        let binary_path = resolve_mihomo_path(app)?;
        let app_data = app
            .path()
            .app_data_dir()
            .map_err(|e| CommandError::new("app_data_dir", e.to_string()))?;
        let work_dir: PathBuf = app_data.join("kernel");
        let cache_dir: PathBuf = app_data.join("profiles");

        let driver = Arc::new(MihomoDriver::new());
        let launcher: Arc<dyn KernelLauncher> = pick_launcher(app, &binary_path);

        let http = self
            .snapshot_client()
            .ok_or_else(|| CommandError::new("not_initialized", "请先选择后端服务地址"))?;
        let fetcher = ProfileFetcher::new(http, cache_dir);

        let proxy_setter = Some(Arc::new(DefaultSystemProxy) as Arc<_>);

        let manager = Arc::new(KernelManager::new(
            driver,
            launcher,
            proxy_setter,
            fetcher,
            binary_path,
            work_dir,
        ));
        manager.set_requested_mode(*self.requested_mode.read());
        if let Some(g) = *self.proxy_guard_enabled.read() {
            manager.set_proxy_guard_enabled(g);
        }
        let _ = self.kernel.set(manager.clone());
        Ok(manager)
    }
}

/// Pick the right launcher for the host OS. macOS routes through
/// `xboard-helper` (LaunchDaemon → Unix socket) so mihomo can create a
/// utun device; Windows still falls back to the direct launcher until
/// `xboard-svc` ships; Linux relies on the deb/rpm `setcap` install and
/// uses `binary_path` to verify the install actually granted the cap
/// before announcing TUN as available.
fn pick_launcher(app: &AppHandle, binary_path: &std::path::Path) -> Arc<dyn KernelLauncher> {
    #[cfg(target_os = "macos")]
    {
        let _ = binary_path;
        use xboard_core::kernel::launcher::HelperSocketLauncher;
        let installer = crate::helper_install::build_installer(app);
        Arc::new(HelperSocketLauncher::new().with_installer(installer))
    }
    #[cfg(target_os = "linux")]
    {
        let _ = app;
        use xboard_core::DirectLauncher;
        Arc::new(DirectLauncher::new().with_binary_hint(binary_path.to_path_buf()))
    }
    #[cfg(target_os = "windows")]
    {
        let _ = binary_path;
        use xboard_core::kernel::launcher::SvcPipeLauncher;
        let installer = crate::svc_install::build_installer(app);
        Arc::new(SvcPipeLauncher::new().with_installer(installer))
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        let _ = app;
        let _ = binary_path;
        use xboard_core::DirectLauncher;
        Arc::new(DirectLauncher::new())
    }
}

/// Resolve the path to the `mihomo` sidecar bundled with the app. Tauri
/// drops it next to the main executable in production and under
/// `target/<profile>/binaries/mihomo-<triple>` in dev. We let the shell
/// plugin compute the right path, then extract it via `From<Command>`.
fn resolve_mihomo_path(app: &AppHandle) -> Result<PathBuf, CommandError> {
    let cmd = app
        .shell()
        .sidecar("mihomo")
        .map_err(|e| CommandError::new("sidecar", format!("locate mihomo sidecar: {e}")))?;
    let std_cmd: std::process::Command = cmd.into();
    Ok(PathBuf::from(std_cmd.get_program()))
}
