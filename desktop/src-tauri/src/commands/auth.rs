use secrecy::SecretString;
use serde::Serialize;
use tauri::State;
use xboard_core::api::{Captcha, LoginRequest, RegisterRequest};

use crate::commands::session::{clear_session, store_session_after_auth};
use crate::error::{CommandError, CommandResult};
use crate::persistence::SessionSnapshot;
use crate::state::{AppState, AuthSession};

#[derive(Debug, Serialize)]
pub struct LoginSummary {
    pub email: String,
    pub is_admin: bool,
    pub subscribe_token: String,
}

#[tauri::command]
pub async fn login(
    state: State<'_, AppState>,
    email: String,
    password: String,
    captcha_type: Option<String>,
    captcha_token: Option<String>,
) -> CommandResult<LoginSummary> {
    let client = state
        .snapshot_client()
        .ok_or_else(|| CommandError::new("not_initialized", "后端尚未就绪"))?;

    let req = LoginRequest {
        email: &email,
        password: &password,
        captcha: Captcha::from_type(captcha_type.as_deref(), captcha_token.as_deref()),
    };
    let auth = client.login(&req).await?;

    client.set_bearer(Some(SecretString::from(auth.auth_data.clone())));

    let session = AuthSession {
        email: email.clone(),
        is_admin: auth.is_admin,
        subscribe_token: auth.token.clone(),
    };
    *state.auth.write() = Some(session.clone());

    let snapshot = SessionSnapshot {
        backend_base_url: client.backend_base_url().to_string(),
        email: session.email.clone(),
        is_admin: session.is_admin,
        subscribe_token: session.subscribe_token.clone(),
        last_check_login_at: Some(now_ms()),
    };
    store_session_after_auth(&state, &auth.auth_data, snapshot)?;

    Ok(LoginSummary {
        email: session.email,
        is_admin: session.is_admin,
        subscribe_token: session.subscribe_token,
    })
}

#[tauri::command]
pub async fn register(
    state: State<'_, AppState>,
    email: String,
    password: String,
    email_code: String,
    invite_code: Option<String>,
    captcha_type: Option<String>,
    captcha_token: Option<String>,
) -> CommandResult<LoginSummary> {
    let client = state
        .snapshot_client()
        .ok_or_else(|| CommandError::new("not_initialized", "请先选择后端服务地址"))?;

    let req = RegisterRequest {
        email: &email,
        password: &password,
        email_code: &email_code,
        invite_code: invite_code.as_deref(),
        captcha: Captcha::from_type(captcha_type.as_deref(), captcha_token.as_deref()),
    };
    let auth = client.register(&req).await?;

    client.set_bearer(Some(SecretString::from(auth.auth_data.clone())));

    let session = AuthSession {
        email: email.clone(),
        is_admin: auth.is_admin,
        subscribe_token: auth.token.clone(),
    };
    *state.auth.write() = Some(session.clone());

    let snapshot = SessionSnapshot {
        backend_base_url: client.backend_base_url().to_string(),
        email: session.email.clone(),
        is_admin: session.is_admin,
        subscribe_token: session.subscribe_token.clone(),
        last_check_login_at: Some(now_ms()),
    };
    store_session_after_auth(&state, &auth.auth_data, snapshot)?;

    Ok(LoginSummary {
        email: session.email,
        is_admin: session.is_admin,
        subscribe_token: session.subscribe_token,
    })
}

#[tauri::command]
pub async fn send_email_verify(
    state: State<'_, AppState>,
    email: String,
    captcha_type: Option<String>,
    captcha_token: Option<String>,
) -> CommandResult<()> {
    let client = state
        .snapshot_client()
        .ok_or_else(|| CommandError::new("not_initialized", "请先选择后端服务地址"))?;
    client
        .send_email_verify(
            &email,
            Captcha::from_type(captcha_type.as_deref(), captcha_token.as_deref()),
        )
        .await?;
    Ok(())
}

#[tauri::command]
pub async fn forget_password(
    state: State<'_, AppState>,
    email: String,
    password: String,
    email_code: String,
    captcha_type: Option<String>,
    captcha_token: Option<String>,
) -> CommandResult<()> {
    let client = state
        .snapshot_client()
        .ok_or_else(|| CommandError::new("not_initialized", "请先选择后端服务地址"))?;
    client
        .forget_password(
            &email,
            &password,
            &email_code,
            Captcha::from_type(captcha_type.as_deref(), captcha_token.as_deref()),
        )
        .await?;
    Ok(())
}

#[tauri::command]
pub fn logout(app: tauri::AppHandle, state: State<'_, AppState>) -> CommandResult<()> {
    // Reuse the session-expired teardown so logout and "token revoked"
    // converge on a single code path. The frontend listens for the same
    // `xboard://session-expired` event in both cases.
    clear_session(&app, &state);
    Ok(())
}

fn now_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
