use crate::{
    error::{CommandError, CommandResult},
    state::AppState,
};
use tauri::{AppHandle, State};
use xboard_core::profile::custom_rules::CustomRule;

#[tauri::command]
pub async fn fetch_custom_rules(
    state: State<'_, AppState>,
    app: AppHandle,
) -> CommandResult<Vec<CustomRule>> {
    super::guest::refresh_client_config(&state).await?;
    if !super::guest::client_config_snapshot().features.custom_rules {
        return Err(CommandError::new("feature_disabled", "自定义规则尚未开放"));
    }
    if state.snapshot_auth().is_none() {
        return Err(CommandError::new("unauthorized", "请先登录"));
    }
    Ok(state.ensure_kernel(&app)?.custom_rules().await?)
}

#[tauri::command]
pub async fn save_custom_rules(
    state: State<'_, AppState>,
    app: AppHandle,
    rules: Vec<CustomRule>,
) -> CommandResult<()> {
    super::guest::refresh_client_config(&state).await?;
    if !super::guest::client_config_snapshot().features.custom_rules {
        return Err(CommandError::new("feature_disabled", "自定义规则尚未开放"));
    }
    if state.snapshot_auth().is_none() {
        return Err(CommandError::new("unauthorized", "请先登录"));
    }
    state.ensure_kernel(&app)?.set_custom_rules(rules).await?;
    Ok(())
}
