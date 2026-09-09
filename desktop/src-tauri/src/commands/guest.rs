//! Public site configuration. Frontend caches the response for ~5 min
//! so multi-page navigations between `/login` / `/register` /
//! `/forget-password` don't keep refetching.

use once_cell::sync::Lazy;
use parking_lot::RwLock;
use std::sync::Arc;
use tauri::State;
use xboard_core::api::deployment::ClientConfig;
use xboard_core::api::runtime_config::DeploymentRuntime;
use xboard_core::api::SiteConfig;

use crate::error::{CommandError, CommandResult};
use crate::state::AppState;

#[tauri::command]
pub async fn fetch_site_config(state: State<'_, AppState>) -> CommandResult<SiteConfig> {
    refresh_client_config(&state).await?;
    let client = state
        .snapshot_client()
        .ok_or_else(|| CommandError::new("not_initialized", "后端尚未就绪"))?;
    let site = client.site_config().await?;
    let runtime = CLIENT_CONFIG
        .read()
        .clone()
        .ok_or_else(|| CommandError::new("not_initialized", "部署配置未初始化"))?;
    Ok(runtime.apply_captcha(site)?)
}

static CLIENT_CONFIG: Lazy<RwLock<Option<Arc<DeploymentRuntime>>>> =
    Lazy::new(|| RwLock::new(None));

pub(crate) fn initialize(config: &crate::config::RuntimeConfig) {
    *CLIENT_CONFIG.write() = Some(Arc::new(DeploymentRuntime::new(config.clone())));
}

pub fn client_config_snapshot() -> ClientConfig {
    if let Some(current) = CLIENT_CONFIG.read().as_ref() {
        return current.snapshot();
    }
    let mut flags = xboard_core::api::deployment::FeatureFlags::default();
    flags.purchase = false;
    flags.gift_card = false;
    flags.invite = false;
    flags.tickets = false;
    flags.notice = false;
    flags.custom_rules = false;
    ClientConfig {
        brand_name: "SUFE".into(),
        features: flags,
        chatwoot_base_url: None,
        chatwoot_inbox_identifier: None,
        config_source: "unavailable".into(),
        transport_mode: "https".into(),
        api_candidates: 0,
    }
}

pub(crate) async fn refresh_client_config(state: &State<'_, AppState>) -> CommandResult<()> {
    let runtime = CLIENT_CONFIG
        .read()
        .clone()
        .ok_or_else(|| CommandError::new("not_initialized", "部署配置未初始化"))?;
    let client = state
        .snapshot_client()
        .ok_or_else(|| CommandError::new("not_initialized", "后端尚未就绪"))?;
    Ok(runtime.refresh(&client).await?)
}

#[tauri::command]
pub async fn fetch_client_config(state: State<'_, AppState>) -> CommandResult<ClientConfig> {
    refresh_client_config(&state).await?;
    Ok(client_config_snapshot())
}

fn enabled(flag: bool) -> CommandResult<()> {
    if flag {
        Ok(())
    } else {
        Err(CommandError::new("feature_disabled", "此功能暂未开放"))
    }
}

#[tauri::command]
pub async fn gift_card_check(
    state: State<'_, AppState>,
    code: String,
) -> CommandResult<serde_json::Value> {
    refresh_client_config(&state).await?;
    if state.snapshot_auth().is_none() {
        return Err(CommandError::new("unauthorized", "未登录").with_status(401));
    }
    enabled(client_config_snapshot().features.gift_card)?;
    let client = state
        .snapshot_client()
        .ok_or_else(|| CommandError::new("not_initialized", "后端尚未就绪"))?;
    Ok(client.gift_card_check(&code).await?)
}

#[tauri::command]
pub async fn gift_card_redeem(
    state: State<'_, AppState>,
    code: String,
) -> CommandResult<serde_json::Value> {
    refresh_client_config(&state).await?;
    if state.snapshot_auth().is_none() {
        return Err(CommandError::new("unauthorized", "未登录").with_status(401));
    }
    enabled(client_config_snapshot().features.gift_card)?;
    let client = state
        .snapshot_client()
        .ok_or_else(|| CommandError::new("not_initialized", "后端尚未就绪"))?;
    Ok(client.gift_card_redeem(&code).await?)
}

#[tauri::command]
pub async fn gift_card_history(state: State<'_, AppState>) -> CommandResult<serde_json::Value> {
    refresh_client_config(&state).await?;
    if state.snapshot_auth().is_none() {
        return Err(CommandError::new("unauthorized", "未登录").with_status(401));
    }
    enabled(client_config_snapshot().features.gift_card)?;
    let client = state
        .snapshot_client()
        .ok_or_else(|| CommandError::new("not_initialized", "后端尚未就绪"))?;
    Ok(client.gift_card_history().await?)
}

#[tauri::command]
pub async fn fetch_invites(state: State<'_, AppState>) -> CommandResult<serde_json::Value> {
    refresh_client_config(&state).await?;
    if state.snapshot_auth().is_none() {
        return Err(CommandError::new("unauthorized", "未登录").with_status(401));
    }
    enabled(client_config_snapshot().features.invite)?;
    let client = state
        .snapshot_client()
        .ok_or_else(|| CommandError::new("not_initialized", "后端尚未就绪"))?;
    Ok(client.fetch_invites().await?)
}

#[tauri::command]
pub async fn create_invite(
    state: State<'_, AppState>,
    code: Option<String>,
) -> CommandResult<bool> {
    refresh_client_config(&state).await?;
    if state.snapshot_auth().is_none() {
        return Err(CommandError::new("unauthorized", "未登录").with_status(401));
    }
    enabled(client_config_snapshot().features.invite)?;
    let client = state
        .snapshot_client()
        .ok_or_else(|| CommandError::new("not_initialized", "后端尚未就绪"))?;
    Ok(client.create_invite(code.as_deref()).await?)
}
