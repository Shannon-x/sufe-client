//! Plans / orders / checkout surface. Read endpoints (`fetch_plans`,
//! `fetch_orders`, `fetch_payment_methods`) are auth-gated so a stale local
//! session can't reach a freshly-rotated catalog. The write endpoints
//! (`save_order`, `checkout_order`, `cancel_order`) plus the `check_order`
//! poll all share the same gate via `require_auth`.

use tauri::State;
use xboard_core::api::{
    CheckoutResponse, CouponCheckResult, HttpClient, Order, PaymentMethod, Plan,
};

use crate::error::{CommandError, CommandResult};
use crate::state::AppState;

fn require_auth(state: &State<'_, AppState>) -> CommandResult<HttpClient> {
    let client = state
        .snapshot_client()
        .ok_or_else(|| CommandError::new("not_initialized", "请先选择后端服务地址"))?;
    if state.snapshot_auth().is_none() {
        return Err(CommandError::new("unauthorized", "未登录").with_status(401));
    }
    Ok(client)
}

#[tauri::command]
pub async fn fetch_plans(state: State<'_, AppState>) -> CommandResult<Vec<Plan>> {
    let client = require_auth(&state)?;
    Ok(client.fetch_plans().await?)
}

#[tauri::command]
pub async fn fetch_orders(state: State<'_, AppState>) -> CommandResult<Vec<Order>> {
    let client = require_auth(&state)?;
    Ok(client.fetch_orders().await?)
}

#[tauri::command]
pub async fn fetch_payment_methods(
    state: State<'_, AppState>,
) -> CommandResult<Vec<PaymentMethod>> {
    let client = require_auth(&state)?;
    Ok(client.fetch_payment_methods().await?)
}

#[tauri::command]
pub async fn save_order(
    state: State<'_, AppState>,
    plan_id: i64,
    period: String,
    coupon_code: Option<String>,
) -> CommandResult<String> {
    if period.trim().is_empty() {
        return Err(CommandError::new("invalid_period", "请选择计费周期"));
    }
    let client = require_auth(&state)?;
    let coupon = coupon_code
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    Ok(client.save_order(plan_id, period.trim(), coupon).await?)
}

#[tauri::command]
pub async fn checkout_order(
    state: State<'_, AppState>,
    trade_no: String,
    method: i64,
) -> CommandResult<CheckoutResponse> {
    if trade_no.trim().is_empty() {
        return Err(CommandError::new("invalid_trade_no", "订单号缺失"));
    }
    let client = require_auth(&state)?;
    Ok(client.checkout_order(trade_no.trim(), method).await?)
}

#[tauri::command]
pub async fn check_order(state: State<'_, AppState>, trade_no: String) -> CommandResult<i32> {
    if trade_no.trim().is_empty() {
        return Err(CommandError::new("invalid_trade_no", "订单号缺失"));
    }
    let client = require_auth(&state)?;
    Ok(client.check_order(trade_no.trim()).await?)
}

/// Validate a coupon `code` against `plan_id`. The panel returns an
/// envelope error for invalid / expired / wrong-plan codes; we surface
/// that as `CommandError::Api` so the UI can pin the message under the
/// coupon field rather than firing a toast.
#[tauri::command]
pub async fn check_coupon(
    state: State<'_, AppState>,
    code: String,
    plan_id: i64,
) -> CommandResult<CouponCheckResult> {
    let trimmed = code.trim();
    if trimmed.is_empty() {
        return Err(CommandError::new("invalid_coupon", "请输入优惠码"));
    }
    let client = require_auth(&state)?;
    Ok(client.check_coupon(trimmed, plan_id).await?)
}

#[tauri::command]
pub async fn cancel_order(state: State<'_, AppState>, trade_no: String) -> CommandResult<()> {
    if trade_no.trim().is_empty() {
        return Err(CommandError::new("invalid_trade_no", "订单号缺失"));
    }
    let client = require_auth(&state)?;
    client.cancel_order(trade_no.trim()).await?;
    Ok(())
}
