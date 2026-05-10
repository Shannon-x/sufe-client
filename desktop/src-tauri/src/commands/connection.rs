//! Tauri commands for the "Connect" toggle and live kernel readouts.
//!
//! All heavy lifting lives in `xboard_core::KernelManager`; these wrappers
//! only translate the JS-side arguments and surface `CommandError`s.

use tauri::{AppHandle, State};
use xboard_core::kernel::{ConnectionState, ProxyGroup, TrafficStats, TunnelMode};

use crate::error::{CommandError, CommandResult};
use crate::state::AppState;

#[tauri::command]
pub async fn connect(
    state: State<'_, AppState>,
    app: AppHandle,
) -> CommandResult<ConnectionState> {
    let auth = state
        .snapshot_auth()
        .ok_or_else(|| CommandError::new("unauthorized", "未登录").with_status(401))?;

    let client = state
        .snapshot_client()
        .ok_or_else(|| CommandError::new("not_initialized", "请先选择后端服务地址"))?;

    // Pull the *current* subscribe URL — server might have rotated the token.
    let subscribe = client.user_subscribe().await?;
    if subscribe.token != auth.subscribe_token {
        // Refresh our cached session token; the bearer is still valid.
        state.auth.write().as_mut().unwrap().subscribe_token = subscribe.token.clone();
    }

    let manager = state.ensure_kernel(&app)?;
    manager.connect(&subscribe.subscribe_url).await?;
    Ok(manager.state())
}

#[tauri::command]
pub async fn disconnect(state: State<'_, AppState>) -> CommandResult<ConnectionState> {
    let manager = state
        .kernel
        .get()
        .cloned()
        .ok_or_else(|| CommandError::new("kernel_not_running", "内核未启动"))?;
    manager.disconnect().await?;
    Ok(manager.state())
}

#[tauri::command]
pub fn connection_state(state: State<'_, AppState>) -> CommandResult<ConnectionState> {
    Ok(state
        .kernel
        .get()
        .map(|m| m.state())
        .unwrap_or(ConnectionState::Disconnected))
}

#[tauri::command]
pub fn set_tunnel_mode(state: State<'_, AppState>, mode: TunnelMode) -> CommandResult<()> {
    if let Some(manager) = state.kernel.get() {
        manager.set_requested_mode(mode);
    } else {
        // Manager not yet built; stash the preference so first connect picks it up.
        // For now we only persist post-init (manager is built on first connect),
        // so the no-op here is fine: UI default is Tun.
        tracing::debug!("set_tunnel_mode before kernel init — ignoring (defaults stick)");
    }
    Ok(())
}

#[tauri::command]
pub async fn proxies(state: State<'_, AppState>) -> CommandResult<Vec<ProxyGroup>> {
    let manager = state
        .kernel
        .get()
        .cloned()
        .ok_or_else(|| CommandError::new("kernel_not_running", "内核未启动"))?;
    Ok(manager.proxies().await?)
}

#[tauri::command]
pub async fn select_proxy(
    state: State<'_, AppState>,
    group: String,
    name: String,
) -> CommandResult<()> {
    let manager = state
        .kernel
        .get()
        .cloned()
        .ok_or_else(|| CommandError::new("kernel_not_running", "内核未启动"))?;
    manager.select_proxy(&group, &name).await?;
    Ok(())
}

#[tauri::command]
pub async fn latency_test(state: State<'_, AppState>, name: String) -> CommandResult<u32> {
    let manager = state
        .kernel
        .get()
        .cloned()
        .ok_or_else(|| CommandError::new("kernel_not_running", "内核未启动"))?;
    Ok(manager.latency_test(&name).await?)
}

#[tauri::command]
pub async fn current_traffic(state: State<'_, AppState>) -> CommandResult<TrafficStats> {
    let manager = state
        .kernel
        .get()
        .cloned()
        .ok_or_else(|| CommandError::new("kernel_not_running", "内核未启动"))?;
    Ok(manager.current_traffic().await?)
}
