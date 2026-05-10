//! Operational notice surface — single read endpoint, mirrors `user.rs`'s
//! auth gate so a stale local session can't reach a fresh listing.

use tauri::State;
use xboard_core::api::Notice;

use crate::error::{CommandError, CommandResult};
use crate::state::AppState;

#[tauri::command]
pub async fn fetch_notices(state: State<'_, AppState>) -> CommandResult<Vec<Notice>> {
    let client = state
        .snapshot_client()
        .ok_or_else(|| CommandError::new("not_initialized", "请先选择后端服务地址"))?;
    if state.snapshot_auth().is_none() {
        return Err(CommandError::new("unauthorized", "未登录").with_status(401));
    }
    Ok(client.fetch_notices().await?)
}
