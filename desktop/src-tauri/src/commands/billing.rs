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
    super::guest::refresh_client_config(&state).await?;
    if !crate::commands::guest::client_config_snapshot()
        .features
        .purchase
    {
        return Err(CommandError::new(
            "feature_disabled",
            "当前暂未开放新订阅购买，已有订单仍可在订单中心处理",
        ));
    }
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
    period: Option<String>,
) -> CommandResult<CouponCheckResult> {
    super::guest::refresh_client_config(&state).await?;
    if !crate::commands::guest::client_config_snapshot()
        .features
        .purchase
    {
        return Err(CommandError::new(
            "feature_disabled",
            "当前暂未开放订阅购买",
        ));
    }
    let trimmed = code.trim();
    if trimmed.is_empty() {
        return Err(CommandError::new("invalid_coupon", "请输入优惠码"));
    }
    let client = require_auth(&state)?;
    Ok(client
        .check_coupon_with_period(trimmed, plan_id, period.as_deref())
        .await?)
}

/// Preserve payment binding and exact discounts instead of discarding them
/// through the historical list's reduced Order type.
#[tauri::command]
pub async fn fetch_order(
    state: State<'_, AppState>,
    trade_no: String,
) -> CommandResult<serde_json::Value> {
    validate_trade_no(&trade_no)?;
    let client = require_auth(&state)?;
    Ok(client
        .get_json(&format!("/api/v1/user/order/detail?trade_no={trade_no}"))
        .await?)
}

fn validate_trade_no(trade: &str) -> CommandResult<()> {
    if trade.is_empty()
        || trade.len() > 128
        || !trade
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
    {
        return Err(CommandError::new("invalid_trade_no", "订单号格式无效"));
    }
    Ok(())
}

/// The remote gateway receives no Tauri IPC capabilities. Navigating back
/// to the panel only requests a status refresh; the server remains the
/// sole authority for whether payment actually succeeded.
#[tauri::command]
pub async fn open_payment_window(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    url: String,
    trade_no: String,
) -> CommandResult<()> {
    use tauri::{Emitter, Manager};
    let _ = require_auth(&state)?;
    validate_trade_no(&trade_no)?;
    let parsed =
        reqwest::Url::parse(&url).map_err(|_| CommandError::new("payment_url", "支付链接无效"))?;
    if parsed.scheme() != "https"
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
    {
        return Err(CommandError::new("payment_url", "支付链接必须使用 HTTPS"));
    }
    #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
    {
        if let Some(window) = app.get_webview_window("payment") {
            window
                .close()
                .map_err(|e| CommandError::new("payment_window", e.to_string()))?;
        }
        let callback_app = app.clone();
        let callback_trade = trade_no.clone();
        tauri::WebviewWindowBuilder::new(&app, "payment", tauri::WebviewUrl::External(parsed))
            .title("安全支付")
            .inner_size(520.0, 740.0)
            .on_navigation(move |destination| {
                let returned = destination
                    .fragment()
                    .map(|fragment| {
                        fragment == format!("/order/{callback_trade}")
                            || fragment.starts_with(&format!("/order/{callback_trade}?"))
                    })
                    .unwrap_or(false);
                if returned {
                    let _ = callback_app.emit("payment://returned", &callback_trade);
                    let handle = callback_app.clone();
                    tauri::async_runtime::spawn(async move {
                        if let Some(window) = handle.get_webview_window("payment") {
                            let _ = window.close();
                        }
                        if let Some(main) = handle.get_webview_window("main") {
                            let _ = main.set_focus();
                        }
                    });
                    return false;
                }
                matches!(destination.scheme(), "https" | "about")
            })
            .build()
            .map_err(|e| CommandError::new("payment_window", e.to_string()))?;
        Ok(())
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        let _ = (app, parsed, trade_no);
        Err(CommandError::new(
            "payment_window",
            "请使用系统浏览器完成支付，返回后会自动检查订单",
        ))
    }
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
