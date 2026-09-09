//! Verified Xboard-sh-1 gift-card and invite endpoints. Values preserve the
//! upstream reward variants and pagination without losing unfamiliar rewards.
use super::HttpClient;
use crate::error::{Result, XboardError};
use serde_json::{json, Value};

fn validate_code(code: &str) -> Result<&str> {
    let code = code.trim();
    if code.is_empty() || code.len() > 255 {
        return Err(XboardError::Config("请输入有效兑换码".into()));
    }
    Ok(code)
}

impl HttpClient {
    pub async fn gift_card_check(&self, code: &str) -> Result<Value> {
        self.post_json(
            "/api/v1/user/gift-card/check",
            &json!({"code":validate_code(code)?}),
        )
        .await
    }
    pub async fn gift_card_redeem(&self, code: &str) -> Result<Value> {
        self.post_json(
            "/api/v1/user/gift-card/redeem",
            &json!({"code":validate_code(code)?}),
        )
        .await
    }
    pub async fn gift_card_history(&self) -> Result<Value> {
        self.gift_card_history_page(1).await
    }
    pub async fn gift_card_history_page(&self, page: i64) -> Result<Value> {
        if !(1..=100_000).contains(&page) {
            return Err(XboardError::Config("页码无效".into()));
        }
        self.get_json(&format!(
            "/api/v1/user/gift-card/history?per_page=30&page={page}"
        ))
        .await
    }
    pub async fn fetch_invites(&self) -> Result<Value> {
        self.get_json("/api/v1/user/invite/fetch").await
    }
    pub async fn create_invite(&self, code: Option<&str>) -> Result<bool> {
        self.post_json(
            "/api/v1/user/invite/save",
            &json!({"code":code.unwrap_or("").trim()}),
        )
        .await
    }
}
