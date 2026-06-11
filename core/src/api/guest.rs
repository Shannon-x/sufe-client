//! `/guest/comm/config` — public site configuration the UI needs *before*
//! the user has authenticated. The login / register / forget-password pages
//! all key off these flags (captcha provider + site keys, brand text, TOS
//! link, invite-required flag, etc).
//!
//! Backend reference: `app/Http/Controllers/V1/Guest/CommController@config`.
//! Field naming on the wire follows the backend exactly — we don't rename
//! anything because too many forks tweak field shapes individually.

use serde::{Deserialize, Serialize};

use super::serde_helpers::{
    de_string_or_null, de_vec_string_or_empty, deserialize_opt_f32_lenient,
};
use super::types::de_truthy;
use super::HttpClient;
use crate::error::Result;

/// Public site configuration. All fields default to empty / false so a
/// trimmed-down deployment that only returns a subset still deserialises.
///
/// ### Wire-shape hazards
///
/// Most string-typed fields here are backed by `admin_setting('foo')` on the
/// PHP side with **no fallback default**. Laravel returns `null` for any
/// admin key that has never been written, which serde refuses to coerce
/// into a non-Option `String` — so every fresh panel with TOS / captcha /
/// branding unconfigured (i.e. nearly every fresh panel) would have its
/// `/guest/comm/config` response fail to deserialize, breaking cold-start
/// of the login / register / splash screens. We route these through
/// `de_string_or_null` which maps null → "".
///
/// Similarly, `email_whitelist_suffix` is an array when set, but
/// `CommController` emits the literal int `0` (not `null`, not `[]`) when
/// the whitelist toggle is off — `#[serde(default)]` alone does not rescue
/// an explicit non-array value, so we route through `de_vec_string_or_empty`.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct SiteConfig {
    #[serde(default, deserialize_with = "de_string_or_null")]
    pub tos_url: String,

    #[serde(default, deserialize_with = "de_truthy")]
    pub is_email_verify: bool,

    #[serde(default, deserialize_with = "de_truthy")]
    pub is_invite_force: bool,

    /// Backend may return this as either a JSON array of suffixes (e.g.
    /// `["@gmail.com"]`), `null` / missing when the key is unset, or the
    /// literal int `0` when the whitelist toggle is explicitly off. The
    /// custom deserializer collapses all non-array cases to `Vec::new()`.
    #[serde(default, deserialize_with = "de_vec_string_or_empty")]
    pub email_whitelist_suffix: Vec<String>,

    #[serde(default, deserialize_with = "de_truthy")]
    pub is_captcha: bool,

    /// `""` when disabled, otherwise one of: `"recaptcha"`, `"recaptcha-v3"`,
    /// `"turnstile"`. Some forks use `"hcaptcha"` too — UI should treat any
    /// unknown value as "captcha required, but provider unsupported" and
    /// surface a clear error rather than silently bypass.
    #[serde(default)]
    pub captcha_type: String,

    #[serde(default, deserialize_with = "de_string_or_null")]
    pub recaptcha_site_key: String,

    #[serde(default, deserialize_with = "de_string_or_null")]
    pub recaptcha_v3_site_key: String,

    /// reCAPTCHA v3 score threshold. The controller supplies a default of
    /// `0.5` so a missing key is safe, but `Setting::load()` `json_decode`s
    /// stored values: an admin who saved a non-numeric string (whitespace,
    /// a locale-comma like `"0,5"`, etc.) flows through as a raw string
    /// and would break plain `f32` parsing. Promote to `Option<f32>` and
    /// use the lenient parser; consumers should treat `None` as "use the
    /// reCAPTCHA-recommended default".
    #[serde(default, deserialize_with = "deserialize_opt_f32_lenient")]
    pub recaptcha_v3_score_threshold: Option<f32>,

    #[serde(default, deserialize_with = "de_string_or_null")]
    pub turnstile_site_key: String,

    /// Some forks expose `is_recaptcha` separately from `captcha_type` for
    /// legacy reasons. UI should prefer `captcha_type` and ignore this
    /// unless `captcha_type` is empty.
    #[serde(default, deserialize_with = "de_truthy")]
    pub is_recaptcha: bool,

    #[serde(default, deserialize_with = "de_string_or_null")]
    pub app_description: String,

    #[serde(default, deserialize_with = "de_string_or_null")]
    pub app_url: String,

    #[serde(default, deserialize_with = "de_string_or_null")]
    pub logo: String,
}

impl HttpClient {
    /// Public, unauthenticated. Safe to call on every cold start; backend
    /// caches it heavily.
    pub async fn site_config(&self) -> Result<SiteConfig> {
        self.get_json("/api/v1/guest/comm/config").await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fresh panel: TOS / captcha / logo all unset; whitelist toggle off.
    /// This is the exact shape that previously broke cold start.
    #[test]
    fn site_config_accepts_null_strings_and_zero_whitelist() {
        let raw = r#"{
            "tos_url": null,
            "is_email_verify": 0,
            "is_invite_force": 0,
            "email_whitelist_suffix": 0,
            "is_captcha": 0,
            "captcha_type": "recaptcha",
            "recaptcha_site_key": null,
            "recaptcha_v3_site_key": null,
            "recaptcha_v3_score_threshold": 0.5,
            "turnstile_site_key": null,
            "is_recaptcha": 0,
            "app_description": null,
            "app_url": null,
            "logo": null
        }"#;
        let c: SiteConfig = serde_json::from_str(raw).unwrap();
        assert_eq!(c.tos_url, "");
        assert_eq!(c.recaptcha_site_key, "");
        assert_eq!(c.app_url, "");
        assert_eq!(c.logo, "");
        assert!(c.email_whitelist_suffix.is_empty());
        assert_eq!(c.recaptcha_v3_score_threshold, Some(0.5));
    }

    #[test]
    fn site_config_accepts_string_score_threshold() {
        let raw = r#"{
            "tos_url": "",
            "captcha_type": "recaptcha",
            "recaptcha_v3_score_threshold": "0.7",
            "email_whitelist_suffix": ["@example.com"]
        }"#;
        let c: SiteConfig = serde_json::from_str(raw).unwrap();
        assert_eq!(c.recaptcha_v3_score_threshold, Some(0.7));
        assert_eq!(c.email_whitelist_suffix, vec!["@example.com"]);
    }

    #[test]
    fn site_config_round_trips_fully_configured_payload() {
        let raw = r#"{
            "tos_url": "https://example.com/tos",
            "is_email_verify": true,
            "is_invite_force": true,
            "email_whitelist_suffix": ["@gmail.com", "@outlook.com"],
            "is_captcha": true,
            "captcha_type": "turnstile",
            "recaptcha_site_key": "rk",
            "recaptcha_v3_site_key": "rk3",
            "recaptcha_v3_score_threshold": 0.6,
            "turnstile_site_key": "ts",
            "is_recaptcha": false,
            "app_description": "desc",
            "app_url": "https://app.example.com",
            "logo": "https://cdn.example.com/logo.png"
        }"#;
        let c: SiteConfig = serde_json::from_str(raw).unwrap();
        assert!(c.is_email_verify);
        assert!(c.is_invite_force);
        assert_eq!(c.captcha_type, "turnstile");
        assert_eq!(c.email_whitelist_suffix.len(), 2);
        assert_eq!(c.recaptcha_v3_score_threshold, Some(0.6));
        assert_eq!(c.logo, "https://cdn.example.com/logo.png");
    }
}
