//! `/guest/comm/config` — public site configuration the UI needs *before*
//! the user has authenticated. The login / register / forget-password pages
//! all key off these flags (captcha provider + site keys, brand text, TOS
//! link, invite-required flag, etc).
//!
//! Backend reference: `app/Http/Controllers/V1/Guest/CommController@config`.
//! Field naming on the wire follows the backend exactly — we don't rename
//! anything because too many forks tweak field shapes individually.

use serde::{Deserialize, Serialize};

use super::types::de_truthy;
use super::HttpClient;
use crate::error::Result;

/// Public site configuration. All fields default to empty / false so a
/// trimmed-down deployment that only returns a subset still deserialises.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct SiteConfig {
    #[serde(default)]
    pub tos_url: String,

    #[serde(default, deserialize_with = "de_truthy")]
    pub is_email_verify: bool,

    #[serde(default, deserialize_with = "de_truthy")]
    pub is_invite_force: bool,

    /// Backend may return this as either a JSON array of suffixes (e.g.
    /// `["@gmail.com"]`) or `null` / missing when no whitelist is set.
    /// We accept both.
    #[serde(default)]
    pub email_whitelist_suffix: Vec<String>,

    #[serde(default, deserialize_with = "de_truthy")]
    pub is_captcha: bool,

    /// `""` when disabled, otherwise one of: `"recaptcha"`, `"recaptcha-v3"`,
    /// `"turnstile"`. Some forks use `"hcaptcha"` too — UI should treat any
    /// unknown value as "captcha required, but provider unsupported" and
    /// surface a clear error rather than silently bypass.
    #[serde(default)]
    pub captcha_type: String,

    #[serde(default)]
    pub recaptcha_site_key: String,

    #[serde(default)]
    pub recaptcha_v3_site_key: String,

    #[serde(default)]
    pub recaptcha_v3_score_threshold: f32,

    #[serde(default)]
    pub turnstile_site_key: String,

    /// Some forks expose `is_recaptcha` separately from `captcha_type` for
    /// legacy reasons. UI should prefer `captcha_type` and ignore this
    /// unless `captcha_type` is empty.
    #[serde(default, deserialize_with = "de_truthy")]
    pub is_recaptcha: bool,

    #[serde(default)]
    pub app_description: String,

    #[serde(default)]
    pub app_url: String,

    #[serde(default)]
    pub logo: String,
}

impl HttpClient {
    /// Public, unauthenticated. Safe to call on every cold start; backend
    /// caches it heavily.
    pub async fn site_config(&self) -> Result<SiteConfig> {
        self.get_json("/api/v1/guest/comm/config").await
    }
}
