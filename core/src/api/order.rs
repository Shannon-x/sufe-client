//! `/api/v1/user/order/fetch` — historical orders (read-only this round).
//!
//! `status` is an integer in the v2board / xboard convention:
//!   0 = pending payment
//!   1 = activating (paid, awaiting fulfillment)
//!   2 = cancelled
//!   3 = completed
//!   4 = discounted / credited

use serde::{Deserialize, Serialize};

use super::HttpClient;
use crate::error::{Result, XboardError};

/// One row of `GET /user/order/getPaymentMethod`. Fees are unused in the
/// MVP — we surface them through the type so a future "show effective
/// total" affordance doesn't need a schema change.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PaymentMethod {
    pub id: i64,
    #[serde(default)]
    pub name: String,
    /// Backend-internal payment driver name, e.g. `"AlipayF2F"`. Only useful
    /// for diagnostic logs — the client always passes `id` to `/checkout`.
    #[serde(default)]
    pub payment: String,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub handling_fee_fixed: Option<i64>,
    #[serde(default)]
    pub handling_fee_percent: Option<f64>,
}

/// Result of `POST /user/order/checkout`.
///
/// `type` discriminates how the client should fulfill the payment:
///   - `-1` → balance settled the order in full; nothing else to do.
///   - `1` → redirect URL — open `data` (string) in the browser.
///   - `0` → QR code — `data` is typically the QR image URL or its content.
///   - `-2` → gateway-specific (e.g. Stripe form). UI should fall back to
///     opening `data` if it's a URL, or show the raw payload otherwise.
///
/// The `data` field varies in shape (string/object/null) across networks,
/// so we surface it as `serde_json::Value` and let the UI branch.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CheckoutResponse {
    #[serde(rename = "type")]
    pub kind: i32,
    #[serde(default)]
    pub data: serde_json::Value,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Order {
    pub id: i64,
    #[serde(default)]
    pub trade_no: String,
    #[serde(default)]
    pub plan_id: Option<i64>,
    /// Which periodic price this order paid for (e.g. `month_price`).
    #[serde(default)]
    pub period: Option<String>,
    /// 1 = new, 2 = renew, 3 = upgrade, 4 = traffic reset.
    #[serde(default, rename = "type")]
    pub kind: Option<i32>,
    #[serde(default)]
    pub status: i32,
    #[serde(default)]
    pub commission_status: Option<i32>,

    /// All amounts are cents.
    #[serde(default)]
    pub total_amount: i64,
    #[serde(default)]
    pub balance_amount: Option<i64>,
    #[serde(default)]
    pub discount_amount: Option<i64>,
    #[serde(default)]
    pub surplus_amount: Option<i64>,
    #[serde(default)]
    pub refund_amount: Option<i64>,

    #[serde(default)]
    pub created_at: Option<i64>,
    #[serde(default)]
    pub updated_at: Option<i64>,
}

impl HttpClient {
    pub async fn fetch_orders(&self) -> Result<Vec<Order>> {
        let raw: serde_json::Value = self.get_json("/api/v1/user/order/fetch").await?;
        if raw.is_array() {
            return serde_json::from_value(raw)
                .map_err(|e| XboardError::Other(anyhow::anyhow!("order list parse: {e}")));
        }
        if let Some(inner) = raw.get("data").cloned() {
            return serde_json::from_value(inner).map_err(|e| {
                XboardError::Other(anyhow::anyhow!("order paginate parse: {e}"))
            });
        }
        Err(XboardError::Other(anyhow::anyhow!(
            "unrecognised /user/order/fetch payload shape"
        )))
    }

    pub async fn fetch_payment_methods(&self) -> Result<Vec<PaymentMethod>> {
        let raw: serde_json::Value = self
            .get_json("/api/v1/user/order/getPaymentMethod")
            .await?;
        if raw.is_array() {
            return serde_json::from_value(raw).map_err(|e| {
                XboardError::Other(anyhow::anyhow!("payment method list parse: {e}"))
            });
        }
        if let Some(inner) = raw.get("data").cloned() {
            return serde_json::from_value(inner).map_err(|e| {
                XboardError::Other(anyhow::anyhow!("payment method data parse: {e}"))
            });
        }
        Err(XboardError::Other(anyhow::anyhow!(
            "unrecognised /user/order/getPaymentMethod payload shape"
        )))
    }

    /// Create an order and return the freshly-allocated `trade_no`.
    /// `period` must be one of the `*_price` keys advertised by the plan
    /// (e.g. `"month_price"`); the backend validates this and rejects
    /// mismatches against the chosen `plan_id`.
    pub async fn save_order(
        &self,
        plan_id: i64,
        period: &str,
        coupon_code: Option<&str>,
    ) -> Result<String> {
        #[derive(Serialize)]
        struct Body<'a> {
            plan_id: i64,
            period: &'a str,
            #[serde(skip_serializing_if = "Option::is_none")]
            coupon_code: Option<&'a str>,
        }
        let raw: serde_json::Value = self
            .post_json(
                "/api/v1/user/order/save",
                &Body {
                    plan_id,
                    period,
                    coupon_code,
                },
            )
            .await?;
        // Two shapes seen: a bare string (most builds) or `{ trade_no: "..." }`.
        if let Some(s) = raw.as_str() {
            return Ok(s.to_string());
        }
        if let Some(s) = raw.get("trade_no").and_then(|v| v.as_str()) {
            return Ok(s.to_string());
        }
        Err(XboardError::Other(anyhow::anyhow!(
            "unrecognised /user/order/save payload shape"
        )))
    }

    pub async fn checkout_order(
        &self,
        trade_no: &str,
        method: i64,
    ) -> Result<CheckoutResponse> {
        #[derive(Serialize)]
        struct Body<'a> {
            trade_no: &'a str,
            method: i64,
        }
        // The /checkout response is "raw" — it does NOT carry the `{status,
        // data}` envelope; `unwrap_envelope`'s no-status branch returns the
        // body verbatim, which already matches `CheckoutResponse`.
        let resp: CheckoutResponse = self
            .post_json(
                "/api/v1/user/order/checkout",
                &Body { trade_no, method },
            )
            .await?;
        Ok(resp)
    }

    /// Returns the order's status integer (see `Order::status` doc-comment
    /// for semantics). Used for lightweight polling after the user has been
    /// sent to a payment page in their browser.
    pub async fn check_order(&self, trade_no: &str) -> Result<i32> {
        // `trade_no` is backend-allocated ASCII alphanumeric (typically a
        // timestamp + suffix), so no percent-encoding is required.
        let raw: serde_json::Value = self
            .get_json(&format!("/api/v1/user/order/check?trade_no={trade_no}"))
            .await?;
        if let Some(n) = raw.as_i64() {
            return Ok(n as i32);
        }
        if let Some(n) = raw.get("status").and_then(|v| v.as_i64()) {
            return Ok(n as i32);
        }
        if let Some(n) = raw.get("data").and_then(|v| v.as_i64()) {
            return Ok(n as i32);
        }
        Err(XboardError::Other(anyhow::anyhow!(
            "unrecognised /user/order/check payload shape"
        )))
    }

    pub async fn cancel_order(&self, trade_no: &str) -> Result<()> {
        #[derive(Serialize)]
        struct Body<'a> {
            trade_no: &'a str,
        }
        let _: serde_json::Value = self
            .post_json("/api/v1/user/order/cancel", &Body { trade_no })
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn order_round_trip_with_minimal_payload() {
        let raw = r#"{
            "id": 42,
            "trade_no": "ORDER0042",
            "status": 3,
            "total_amount": 1000,
            "type": 1,
            "created_at": 1700000000
        }"#;
        let o: Order = serde_json::from_str(raw).unwrap();
        assert_eq!(o.id, 42);
        assert_eq!(o.trade_no, "ORDER0042");
        assert_eq!(o.status, 3);
        assert_eq!(o.kind, Some(1));
        assert_eq!(o.total_amount, 1000);
    }
}
