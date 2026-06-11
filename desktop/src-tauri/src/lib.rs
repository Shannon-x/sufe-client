//! Tauri shell entry. Keeps the desktop binary intentionally thin —
//! all business logic lives in the `xboard-core` crate. Commands here
//! are 1:1 wrappers that hand a `tauri::State<AppState>` to a
//! function from `commands/` and surface the result back to JS.

mod commands;
mod config;
mod error;
#[cfg(target_os = "macos")]
mod helper_install;
mod persistence;
mod state;
#[cfg(target_os = "windows")]
mod svc_install;

pub use error::CommandError;
pub use state::AppState;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Emitter, Manager, RunEvent, WindowEvent};
use xboard_core::api::HttpClient;
#[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
use xboard_core::storage::KeyringStore;
use xboard_core::storage::SecureStore;

use crate::commands::session::SECURE_SERVICE;
use crate::persistence::Persistence;

/// Entry point invoked by `main.rs` (and, on mobile, by the platform shim).
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    init_tracing();

    let app_state = AppState::default();

    // True only when the user actually wants to terminate the process — set
    // by the tray "Quit" item and by `RunEvent::ExitRequested` (Cmd+Q on
    // macOS, host shutdown on Windows). Used by `WindowEvent::CloseRequested`
    // to decide between "hide to tray" and "let the close go through".
    let quit_requested = Arc::new(AtomicBool::new(false));
    let qr_for_close = quit_requested.clone();
    let qr_for_run = quit_requested.clone();
    let qr_for_setup = quit_requested.clone();

    let mut builder = tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_os::init())
        .plugin(tauri_plugin_process::init());

    #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
    {
        builder = builder
            .plugin(tauri_plugin_deep_link::init())
            .plugin(tauri_plugin_updater::Builder::new().build());
    }

    let app = builder
        .manage(app_state)
        .setup(move |app| {
            // Backend URL is fixed at build time (see `config.rs`). Wire up
            // the HttpClient eagerly so the frontend can call any auth/user
            // command without first asking the user for a host.
            if let Some(state) = app.try_state::<AppState>() {
                match HttpClient::new(config::BACKEND_URL, config::DEFAULT_LOCALE) {
                    Ok(client) => *state.client.write() = Some(client),
                    Err(e) => tracing::error!(error = %e, "failed to init HttpClient"),
                }

                // Preferences (subscribe_token, last login email, last
                // checkLogin timestamp) — failure here just means hydrate
                // returns None and the user re-logs in.
                match Persistence::load(app.handle()) {
                    Ok(p) => {
                        let _ = state.persistence.set(Arc::new(p));
                    }
                    Err(e) => {
                        tracing::error!(error = ?e, "failed to load preferences store");
                    }
                }

                // OS keychain handle. The trait object lets a future
                // Linux-no-dbus / Android backend slot in here.
                #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
                {
                    let secure: Arc<dyn SecureStore> = Arc::new(KeyringStore::new(SECURE_SERVICE));
                    let _ = state.secure.set(secure);
                }
            }
            // Broadcast kernel state changes to the frontend. The receiver
            // is created lazily — we (cheaply) poll the AppState for an
            // initialized kernel; subscribing once it appears costs O(1).
            spawn_state_forwarder(app.handle().clone());
            // Sibling forwarder for kernel-health events
            // (`connection://kernel-failure` / `connection://kernel-healthy`).
            spawn_health_forwarder(app.handle().clone());
            // Stream kernel logs to the frontend Logs page as `connection://log`.
            spawn_log_forwarder(app.handle().clone());

            // System tray with quick connect / disconnect / mode controls.
            // Tray actions emit `tray://*` events that the frontend store
            // handles, so all the auth / kernel-readiness logic lives in one
            // place (the Pinia connection store) instead of being duplicated
            // here. The tooltip is kept live by `spawn_state_forwarder`.
            let connect_item = MenuItem::with_id(app, "tray-connect", "连接", true, None::<&str>)?;
            let disconnect_item =
                MenuItem::with_id(app, "tray-disconnect", "断开", true, None::<&str>)?;
            let mode_tun_item =
                MenuItem::with_id(app, "tray-mode-tun", "TUN 模式", true, None::<&str>)?;
            let mode_sys_item = MenuItem::with_id(
                app,
                "tray-mode-sysproxy",
                "系统代理模式",
                true,
                None::<&str>,
            )?;
            let logs_item = MenuItem::with_id(app, "tray-logs", "实时日志", true, None::<&str>)?;
            let show_item = MenuItem::with_id(app, "tray-show", "显示主窗口", true, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "tray-quit", "退出 Xboard", true, None::<&str>)?;
            let sep1 = PredefinedMenuItem::separator(app)?;
            let sep2 = PredefinedMenuItem::separator(app)?;
            let sep3 = PredefinedMenuItem::separator(app)?;
            let menu = Menu::with_items(
                app,
                &[
                    &connect_item,
                    &disconnect_item,
                    &sep1,
                    &mode_tun_item,
                    &mode_sys_item,
                    &sep2,
                    &logs_item,
                    &show_item,
                    &sep3,
                    &quit_item,
                ],
            )?;
            let qr_for_menu = qr_for_setup.clone();
            let _tray = TrayIconBuilder::with_id("xboard-main")
                .icon(
                    app.default_window_icon()
                        .cloned()
                        .expect("default window icon should be configured"),
                )
                .tooltip("Xboard — 未连接")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(move |app, event| match event.id().as_ref() {
                    "tray-connect" => {
                        let _ = app.emit("tray://connect", ());
                    }
                    "tray-disconnect" => {
                        let _ = app.emit("tray://disconnect", ());
                    }
                    "tray-mode-tun" => {
                        let _ = app.emit("tray://set-mode", "tun");
                    }
                    "tray-mode-sysproxy" => {
                        let _ = app.emit("tray://set-mode", "system_proxy");
                    }
                    "tray-logs" => {
                        show_main_window(app);
                        let _ = app.emit("tray://open-logs", ());
                    }
                    "tray-show" => show_main_window(app),
                    "tray-quit" => {
                        qr_for_menu.store(true, Ordering::SeqCst);
                        app.exit(0);
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    // Left-click toggles the main window — same idiom as the
                    // Tauri 2 docs example. Right-click is reserved for the
                    // platform's native menu (handled by Tauri).
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        show_main_window(tray.app_handle());
                    }
                })
                .build(app)?;
            Ok(())
        })
        .on_window_event(move |window, event| match event {
            // Intercept user-initiated close on the main window: hide to
            // tray instead of tearing the app down. The `quit_requested`
            // flag is the escape hatch — set by the tray Quit item or by
            // `RunEvent::ExitRequested` — and lets the close fall through
            // so the kernel cleanup branch below can run.
            WindowEvent::CloseRequested { api, .. }
                if window.label() == "main" && !qr_for_close.load(Ordering::SeqCst) =>
            {
                api.prevent_close();
                let _ = window.hide();
                // Frontend uses this to show a one-shot toast explaining
                // that the app is now in the tray (gated on its own
                // localStorage flag so users only see it once).
                let _ = window.app_handle().emit("xboard://hidden-to-tray", ());
            }
            // Cleanly tear down the kernel when the main window is being
            // destroyed. Otherwise mihomo lingers and TUN routes / system
            // proxy stay applied — exactly the failure mode users hate.
            WindowEvent::Destroyed if window.label() == "main" => {
                let state = match window.app_handle().try_state::<AppState>() {
                    Some(s) => s,
                    None => return,
                };
                let Some(km) = state.kernel.get().cloned() else {
                    return;
                };
                // Bound the teardown: `disconnect` clears the OS proxy *first*
                // and then stops the kernel, so even if a hung kernel makes the
                // stop step stall, the proxy is already restored. The 3 s cap
                // guarantees exit never blocks indefinitely (the OS reaps the
                // child via kill_on_drop regardless).
                tauri::async_runtime::block_on(async move {
                    let _ =
                        tokio::time::timeout(std::time::Duration::from_secs(3), km.disconnect())
                            .await;
                });
            }
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            commands::meta::app_version,
            commands::meta::core_version,
            commands::auth::login,
            commands::auth::register,
            commands::auth::send_email_verify,
            commands::auth::forget_password,
            commands::auth::logout,
            commands::session::hydrate_session,
            commands::session::check_login,
            commands::guest::fetch_site_config,
            commands::user::current_user,
            commands::user::current_subscribe,
            commands::connection::connect,
            commands::connection::disconnect,
            commands::connection::connection_state,
            commands::connection::set_tunnel_mode,
            commands::connection::proxies,
            commands::connection::select_proxy,
            commands::connection::latency_test,
            commands::connection::node_geo_test,
            commands::connection::resolve_node_geo_batch,
            commands::connection::current_traffic,
            commands::connection::connections,
            commands::connection::close_connection,
            commands::connection::close_all_connections,
            commands::connection::rules,
            commands::connection::reconnect,
            commands::connection::set_proxy_guard_enabled,
            commands::connection::proxy_guard_enabled,
            commands::nodes::preview_subscribe_nodes,
            commands::kernel::kernel_health,
            commands::kernel::kernel_version,
            commands::kernel::tail_kernel_log,
            commands::helper::helper_status,
            commands::helper::helper_install,
            commands::helper::helper_uninstall,
            commands::notice::fetch_notices,
            commands::billing::fetch_plans,
            commands::billing::fetch_orders,
            commands::billing::fetch_payment_methods,
            commands::billing::save_order,
            commands::billing::checkout_order,
            commands::billing::check_order,
            commands::billing::check_coupon,
            commands::billing::cancel_order,
            commands::ticket::fetch_tickets,
            commands::ticket::fetch_ticket,
            commands::ticket::reply_ticket,
            commands::ticket::close_ticket,
            commands::ticket::save_ticket,
        ])
        .build(tauri::generate_context!())
        .expect("error while running tauri application");

    // Switch from `.run(context)` to `.build(context)` + manual loop so we
    // can latch `quit_requested` on `ExitRequested`. Without this, Cmd+Q on
    // macOS / shutdown on Windows would be silently swallowed by our
    // close-to-tray handler.
    app.run(move |_app, event| {
        if let RunEvent::ExitRequested { code: None, .. } = event {
            qr_for_run.store(true, Ordering::SeqCst);
        }
    });
}

/// Bring the main window to the foreground. Used by both the tray menu
/// "Show" action and left-clicking the tray icon. Idempotent — calling on
/// an already-visible window just refocuses it.
fn show_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// Background task: once the user triggers their first connect, the
/// `KernelManager` is initialized; we then subscribe to its broadcast
/// channel and re-emit each state to the JS side as
/// `xboard://connection-state`.
fn spawn_state_forwarder(app: tauri::AppHandle) {
    use futures::StreamExt;
    use std::time::Duration;
    use tauri::Emitter;

    tauri::async_runtime::spawn(async move {
        // Poll until the kernel is built — no busy spin: 250 ms cadence
        // is well below the user-perceptible threshold for state updates.
        let manager = loop {
            tokio::time::sleep(Duration::from_millis(250)).await;
            if let Some(state) = app.try_state::<AppState>() {
                if let Some(km) = state.kernel.get().cloned() {
                    break km;
                }
            }
        };

        // Replay the *current* state once so listeners that hooked up
        // before the manager existed get a well-defined initial value, and
        // seed the tray tooltip.
        let initial = manager.state();
        let _ = app.emit("xboard://connection-state", initial.clone());
        update_tray_tooltip(&app, &initial);

        let mut stream = manager.subscribe_state();
        while let Some(next) = stream.next().await {
            let _ = app.emit("xboard://connection-state", next.clone());
            update_tray_tooltip(&app, &next);
        }
    });
}

/// Reflect the live connection state in the tray tooltip so the user can read
/// it without opening the window.
fn update_tray_tooltip(app: &tauri::AppHandle, state: &xboard_core::kernel::ConnectionState) {
    use xboard_core::kernel::{ConnectionState, TunnelMode};
    let label = match state {
        ConnectionState::Disconnected => "未连接".to_string(),
        ConnectionState::Connecting { .. } => "连接中…".to_string(),
        ConnectionState::Connected { mode, .. } => {
            let m = match mode {
                TunnelMode::Tun => "TUN",
                TunnelMode::SystemProxy => "系统代理",
            };
            format!("已连接 · {m}")
        }
        ConnectionState::Error { .. } => "连接错误".to_string(),
    };
    if let Some(tray) = app.tray_by_id("xboard-main") {
        let _ = tray.set_tooltip(Some(&format!("Xboard — {label}")));
    }
}

/// Background task: same shape as [`spawn_state_forwarder`] but pumping
/// the kernel-health channel into two distinct frontend events so the UI
/// can wire them independently. `Exited` / `Unresponsive` both land on
/// `connection://kernel-failure` (the UI dispatches on `kind`), and
/// `Healthy` lands on `connection://kernel-healthy` so a banner can clear.
fn spawn_health_forwarder(app: tauri::AppHandle) {
    use futures::StreamExt;
    use std::time::Duration;
    use tauri::Emitter;
    use xboard_core::kernel::KernelHealthEvent;

    tauri::async_runtime::spawn(async move {
        let manager = loop {
            tokio::time::sleep(Duration::from_millis(250)).await;
            if let Some(state) = app.try_state::<AppState>() {
                if let Some(km) = state.kernel.get().cloned() {
                    break km;
                }
            }
        };
        let mut stream = manager.subscribe_health();
        while let Some(evt) = stream.next().await {
            match evt {
                KernelHealthEvent::Healthy => {
                    let _ = app.emit("connection://kernel-healthy", serde_json::json!({}));
                }
                other @ (KernelHealthEvent::Exited { .. }
                | KernelHealthEvent::Unresponsive { .. }) => {
                    let _ = app.emit("connection://kernel-failure", other);
                }
            }
        }
    });
}

/// Background task: pump the kernel's live log stream to the frontend Logs
/// page as `connection://log`. The driver's broadcast channel exists for the
/// manager's whole lifetime, so a single early subscription captures logs
/// across every connect/disconnect cycle.
fn spawn_log_forwarder(app: tauri::AppHandle) {
    use futures::StreamExt;
    use std::time::Duration;
    use tauri::Emitter;

    tauri::async_runtime::spawn(async move {
        let manager = loop {
            tokio::time::sleep(Duration::from_millis(250)).await;
            if let Some(state) = app.try_state::<AppState>() {
                if let Some(km) = state.kernel.get().cloned() {
                    break km;
                }
            }
        };
        let mut stream = manager.live_logs();
        while let Some(line) = stream.next().await {
            let _ = app.emit("connection://log", line);
        }
    });
}

fn init_tracing() {
    use tracing_subscriber::{fmt, EnvFilter};
    let filter = EnvFilter::try_from_env("XBOARD_LOG")
        .unwrap_or_else(|_| EnvFilter::new("info,xboard_core=debug,xboard_desktop_lib=debug"));
    let _ = fmt().with_env_filter(filter).with_target(false).try_init();
}
