//! `/user/*` endpoints (Sanctum-authenticated user-center surfaces).

use serde::{Deserialize, Serialize};

use super::types::de_truthy;
use super::HttpClient;
use crate::error::Result;

/// Response of `GET /api/v1/user/checkLogin`. Backend returns `is_login`
/// truthily across forks (bool/0|1/"0"|"1"), so we use the permissive
/// deserializer.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CheckLoginResp {
    #[serde(default, deserialize_with = "de_truthy")]
    pub is_login: bool,
    #[serde(default, deserialize_with = "de_truthy")]
    pub is_admin: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct UserInfo {
    pub email: String,
    /// Cents.
    #[serde(default)]
    pub balance: i64,
    /// Cents.
    #[serde(default)]
    pub commission_balance: i64,
    #[serde(default)]
    pub plan_id: Option<i64>,
    #[serde(default)]
    pub expired_at: Option<i64>,
    #[serde(default)]
    pub uuid: Option<String>,
    #[serde(default)]
    pub avatar_url: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SubscribeInfo {
    pub plan_id: Option<i64>,
    /// Subscribe token — used as `?token=` on the subscription URL.
    pub token: String,
    #[serde(default)]
    pub expired_at: Option<i64>,
    /// Bytes uploaded this period.
    #[serde(default)]
    pub u: u64,
    /// Bytes downloaded this period.
    #[serde(default)]
    pub d: u64,
    /// Total bytes allowed this period.
    pub transfer_enable: u64,
    /// Full subscription URL — opaque to the client; pass it to the kernel
    /// after appending the right `?flag=` for the active driver.
    pub subscribe_url: String,
    #[serde(default)]
    pub reset_day: Option<u8>,
}

impl HttpClient {
    pub async fn user_info(&self) -> Result<UserInfo> {
        self.get_json("/api/v1/user/info").await
    }

    pub async fn user_subscribe(&self) -> Result<SubscribeInfo> {
        self.get_json("/api/v1/user/getSubscribe").await
    }

    /// Fast probe — does the bearer the client is currently holding still
    /// represent an authenticated session on the backend? Used for cold-start
    /// hydration and "user navigated back to /login" revalidation.
    pub async fn check_login(&self) -> Result<CheckLoginResp> {
        self.get_json("/api/v1/user/checkLogin").await
    }
}
