//! `/api/v1/user/plan/fetch` — purchasable subscription plans.
//!
//! Period-price fields (`month_price`, `quarter_price`, ...) are individually
//! nullable: `null` means that billing cadence isn't available for the plan.
//! v2board / xboard panels store all amounts as **cents**.

use serde::{Deserialize, Serialize};

use super::types::de_truthy;
use super::HttpClient;
use crate::error::{Result, XboardError};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Plan {
    pub id: i64,
    #[serde(default)]
    pub name: String,
    /// Markdown / plain text — render as text on the client; the backend
    /// emits whatever an admin pasted into the panel.
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub group_id: Option<i64>,
    /// Plan flavor. v2board uses `type` to disambiguate flat-rate vs. usage
    /// plans; we surface it but don't currently switch on it client-side.
    #[serde(default, rename = "type")]
    pub kind: Option<i32>,
    /// Total monthly transfer in **GB**. (Multiply by 1024^3 for bytes.)
    #[serde(default)]
    pub transfer_enable: u64,

    // Periodic prices — cents, `None` ⇒ that billing cadence isn't sold.
    #[serde(default)]
    pub month_price: Option<i64>,
    #[serde(default)]
    pub quarter_price: Option<i64>,
    #[serde(default)]
    pub half_year_price: Option<i64>,
    #[serde(default)]
    pub year_price: Option<i64>,
    #[serde(default)]
    pub two_year_price: Option<i64>,
    #[serde(default)]
    pub three_year_price: Option<i64>,
    #[serde(default)]
    pub onetime_price: Option<i64>,

    /// Mid-period traffic reset (top-up) price.
    #[serde(default)]
    pub reset_price: Option<i64>,
    /// 0 = no auto reset; 1 = monthly; 2 = on first day of month.
    #[serde(default)]
    pub reset_traffic_method: Option<i32>,

    /// Visibility / availability flags. Each is `0/1` or bool depending on
    /// the panel build, so use the permissive boolean parser.
    #[serde(default, deserialize_with = "de_truthy")]
    pub show: bool,
    #[serde(default, deserialize_with = "de_truthy")]
    pub sell: bool,
    #[serde(default, deserialize_with = "de_truthy")]
    pub renew: bool,

    #[serde(default)]
    pub sort: Option<i64>,
    #[serde(default)]
    pub created_at: Option<i64>,
    #[serde(default)]
    pub updated_at: Option<i64>,
}

impl HttpClient {
    /// Fetches the catalog of plans the current user is allowed to buy. The
    /// backend already filters by group / sell flag — we don't refilter
    /// client-side beyond hiding `show=false` rows in the UI.
    pub async fn fetch_plans(&self) -> Result<Vec<Plan>> {
        let raw: serde_json::Value = self.get_json("/api/v1/user/plan/fetch").await?;
        if raw.is_array() {
            return serde_json::from_value(raw)
                .map_err(|e| XboardError::Other(anyhow::anyhow!("plan list parse: {e}")));
        }
        if let Some(inner) = raw.get("data").cloned() {
            return serde_json::from_value(inner).map_err(|e| {
                XboardError::Other(anyhow::anyhow!("plan paginate parse: {e}"))
            });
        }
        Err(XboardError::Other(anyhow::anyhow!(
            "unrecognised /user/plan/fetch payload shape"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_round_trip_with_partial_prices() {
        let raw = r#"{
            "id": 1,
            "name": "Basic",
            "transfer_enable": 100,
            "month_price": 1000,
            "year_price": 10800,
            "show": 1,
            "sell": 1,
            "renew": 1
        }"#;
        let p: Plan = serde_json::from_str(raw).unwrap();
        assert_eq!(p.id, 1);
        assert_eq!(p.transfer_enable, 100);
        assert_eq!(p.month_price, Some(1000));
        assert_eq!(p.quarter_price, None);
        assert!(p.show);
        assert!(p.sell);
    }

    #[test]
    fn plan_renew_accepts_string_flag() {
        let raw = r#"{"id": 1, "renew": "1"}"#;
        let p: Plan = serde_json::from_str(raw).unwrap();
        assert!(p.renew);
    }
}
