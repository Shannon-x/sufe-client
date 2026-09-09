//! Native Chatwoot wrapper; the trusted REST boundary is shared with mobile.
use crate::{
    error::{CommandError, CommandResult},
    state::AppState,
};
use tauri::State;

#[tauri::command]
pub async fn support_request(
    state: State<'_, AppState>,
    action: String,
    conversation_id: Option<u64>,
    content: Option<String>,
) -> CommandResult<serde_json::Value> {
    super::guest::refresh_client_config(&state).await?;
    let auth = state
        .snapshot_auth()
        .ok_or_else(|| CommandError::new("unauthorized", "请先登录").with_status(401))?;
    let config = super::guest::client_config_snapshot();
    let backend = state
        .snapshot_client()
        .ok_or_else(|| CommandError::new("not_initialized", "后端尚未就绪"))?
        .backend_base_url()
        .to_string();
    Ok(xboard_core::api::support::request(
        &config,
        &backend,
        &auth.email,
        state.snapshot_secure(),
        &action,
        conversation_id,
        content,
    )
    .await?)
}
