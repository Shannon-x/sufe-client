//! Common HTTP envelope and authentication types.
//!
//! Mirrors `app/Helpers/ApiResponse.php` (success/fail) and
//! `app/Services/AuthService.php::generateAuthData` (login response).

use serde::{Deserialize, Deserializer, Serialize};

/// Wrapping envelope used by the Xboard backend.
///
/// - Success: `{ "status": "success", "message": "ok", "data": {...}, "error": null }`
/// - Failure: `{ "status": "fail",    "message": "...", "data": null, "error": null }`
#[derive(Debug, Deserialize)]
pub struct ApiEnvelope<T> {
    pub status: ApiStatus,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default = "Option::default")]
    pub data: Option<T>,
    #[serde(default)]
    pub error: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ApiStatus {
    Success,
    Fail,
}

/// Returned by `/passport/auth/login` and `/passport/auth/register`.
///
/// Note the **two distinct tokens**:
///
/// - `auth_data` — already includes `"Bearer "` prefix; goes into the
///   `Authorization` header for all `user`-middleware endpoints.
/// - `token` — the user's "subscribe token" (`users.token` column);
///   appended as `?token=<...>` on subscription URLs and `client/*` endpoints.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AuthResult {
    pub token: String,
    pub auth_data: String,
    /// Some Xboard forks (cedar2025) return this as `bool`, upstream
    /// returns `0`/`1` as integer, and ancient builds emit `"0"`/`"1"`
    /// strings. `de_truthy` accepts all three so the client doesn't
    /// break across deployments.
    #[serde(default, deserialize_with = "de_truthy")]
    pub is_admin: bool,
}

/// Permissive boolean deserializer for fields that may arrive as `bool`,
/// `0`/`1` integer, or `"0"`/`"1"` string. Anything truthy → `true`.
///
/// Crate-visible because `/api/v1/user/checkLogin` and
/// `/api/v1/guest/comm/config` carry the same backend quirk on multiple
/// fields each.
pub(crate) fn de_truthy<'de, D>(d: D) -> std::result::Result<bool, D::Error>
where
    D: Deserializer<'de>,
{
    use serde::de::Error as _;
    let v = serde_json::Value::deserialize(d)?;
    match v {
        serde_json::Value::Bool(b) => Ok(b),
        serde_json::Value::Number(n) => Ok(n.as_i64().map(|x| x != 0).unwrap_or(false)),
        serde_json::Value::String(s) => match s.as_str() {
            "0" | "" | "false" | "False" => Ok(false),
            _ => Ok(true),
        },
        serde_json::Value::Null => Ok(false),
        other => Err(D::Error::custom(format!(
            "expected bool/int/string, got {other:?}"
        ))),
    }
}
