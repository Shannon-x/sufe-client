//! Public site configuration. Frontend caches the response for ~5 min
//! so multi-page navigations between `/login` / `/register` /
//! `/forget-password` don't keep refetching.

use tauri::State;
use xboard_core::api::SiteConfig;

use crate::error::{CommandError, CommandResult};
use crate::state::AppState;

#[tauri::command]
pub async fn fetch_site_config(state: State<'_, AppState>) -> CommandResult<SiteConfig> {
    let client = state
        .snapshot_client()
        .ok_or_else(|| CommandError::new("not_initialized", "后端尚未就绪"))?;
    Ok(client.site_config().await?)
}
