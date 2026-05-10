//! Support ticket surface — list / detail / reply / close. Each call sits
//! behind the same auth gate as `notice.rs` / `billing.rs` so a stale local
//! session can't surface another user's ticket thread.

use tauri::State;
use xboard_core::api::{Ticket, TicketDetail};

use crate::error::{CommandError, CommandResult};
use crate::state::AppState;

fn require_auth(state: &State<'_, AppState>) -> CommandResult<xboard_core::api::HttpClient> {
    let client = state
        .snapshot_client()
        .ok_or_else(|| CommandError::new("not_initialized", "请先选择后端服务地址"))?;
    if state.snapshot_auth().is_none() {
        return Err(CommandError::new("unauthorized", "未登录").with_status(401));
    }
    Ok(client)
}

#[tauri::command]
pub async fn fetch_tickets(state: State<'_, AppState>) -> CommandResult<Vec<Ticket>> {
    let client = require_auth(&state)?;
    Ok(client.fetch_tickets().await?)
}

#[tauri::command]
pub async fn fetch_ticket(state: State<'_, AppState>, id: i64) -> CommandResult<TicketDetail> {
    let client = require_auth(&state)?;
    Ok(client.fetch_ticket(id).await?)
}

#[tauri::command]
pub async fn reply_ticket(
    state: State<'_, AppState>,
    id: i64,
    message: String,
) -> CommandResult<()> {
    let trimmed = message.trim();
    if trimmed.is_empty() {
        return Err(CommandError::new("empty_message", "回复内容不能为空"));
    }
    let client = require_auth(&state)?;
    client.reply_ticket(id, trimmed).await?;
    Ok(())
}

#[tauri::command]
pub async fn close_ticket(state: State<'_, AppState>, id: i64) -> CommandResult<()> {
    let client = require_auth(&state)?;
    client.close_ticket(id).await?;
    Ok(())
}

#[tauri::command]
pub async fn save_ticket(
    state: State<'_, AppState>,
    subject: String,
    level: i32,
    message: String,
) -> CommandResult<Option<i64>> {
    let subj = subject.trim();
    let body = message.trim();
    if subj.is_empty() {
        return Err(CommandError::new("empty_subject", "工单主题不能为空"));
    }
    if body.is_empty() {
        return Err(CommandError::new("empty_message", "工单内容不能为空"));
    }
    if !(0..=2).contains(&level) {
        return Err(CommandError::new("invalid_level", "工单等级取值为 0/1/2"));
    }
    let client = require_auth(&state)?;
    Ok(client.save_ticket(subj, level, body).await?)
}
