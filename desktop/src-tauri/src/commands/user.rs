use tauri::State;
use xboard_core::api::{SubscribeInfo, UserInfo};

use crate::error::{CommandError, CommandResult};
use crate::state::AppState;

#[tauri::command]
pub async fn current_user(state: State<'_, AppState>) -> CommandResult<UserInfo> {
    let client = state
        .snapshot_client()
        .ok_or_else(|| CommandError::new("not_initialized", "请先选择后端服务地址"))?;
    if state.snapshot_auth().is_none() {
        return Err(CommandError::new("unauthorized", "未登录").with_status(401));
    }
    let info = client.user_info().await?;
    Ok(info)
}

#[tauri::command]
pub async fn current_subscribe(state: State<'_, AppState>) -> CommandResult<SubscribeInfo> {
    let client = state
        .snapshot_client()
        .ok_or_else(|| CommandError::new("not_initialized", "请先选择后端服务地址"))?;
    if state.snapshot_auth().is_none() {
        return Err(CommandError::new("unauthorized", "未登录").with_status(401));
    }
    let info = client.user_subscribe().await?;
    Ok(info)
}
