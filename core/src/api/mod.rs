//! Xboard API HTTP client.
//!
//! Centralizes Bearer-token injection, response envelope unwrapping, and the
//! V1/V2 split. UI code instantiates one [`HttpClient`] per backend instance
//! and shares it via `Arc`.
//!
//! Endpoints documented in the repo-root `Xboard-API.md`.

use std::sync::Arc;
use std::time::Duration;

use parking_lot::RwLock;
use reqwest::header::{
    HeaderMap, HeaderValue, ACCEPT, ACCEPT_LANGUAGE, AUTHORIZATION, CONTENT_TYPE,
};
use secrecy::{ExposeSecret, SecretString};
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::error::{Result, XboardError};

pub mod auth;
pub mod client;
pub mod guest;
pub mod notice;
pub mod order;
pub mod plan;
pub(crate) mod serde_helpers;
pub mod ticket;
pub mod types;
pub mod user;

pub use auth::{Captcha, LoginRequest, RegisterRequest};
pub use client::SubscribeFetch;
pub use guest::SiteConfig;
pub use notice::Notice;
pub use order::{CheckoutResponse, CouponCheckResult, Order, PaymentMethod};
pub use plan::Plan;
pub use ticket::{Ticket, TicketDetail, TicketMessage};
pub use types::{ApiEnvelope, ApiStatus, AuthResult};
pub use user::{CheckLoginResp, SubscribeInfo, UserInfo};

#[derive(Debug, Clone)]
pub struct HttpClient {
    inner: Arc<Inner>,
}

#[derive(Debug)]
struct Inner {
    base: url::Url,
    locale: String,
    bearer: RwLock<Option<SecretString>>,
    http: reqwest::Client,
}

impl HttpClient {
    pub fn new(base: &str, locale: &str) -> Result<Self> {
        let base = url::Url::parse(base)?;
        // UA must contain a clash.meta-family keyword (`meta`, `verge`,
        // `flclash`, `nekobox`, `clashmetaforandroid`) so Xboard panels'
        // protocol dispatcher routes our subscribe fetches to the ClashMeta
        // handler (mihomo-native YAML w/ full Hysteria2/VLESS/TUIC coverage)
        // rather than falling through to General (plain v2ray base64 links,
        // which mihomo can't fully consume). `clash.meta/<mihomo-version>`
        // is the cleanest match and aligns with the bundled mihomo binary.
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            .user_agent(concat!(
                "clash.meta/v1.18.7 xboard-client/",
                env!("CARGO_PKG_VERSION")
            ))
            .build()?;
        Ok(Self {
            inner: Arc::new(Inner {
                base,
                locale: locale.to_string(),
                bearer: RwLock::new(None),
                http,
            }),
        })
    }

    /// Set or clear the Sanctum bearer (the value of `auth_data` from
    /// `/passport/auth/login`, including the `"Bearer "` prefix).
    pub fn set_bearer(&self, token: Option<SecretString>) {
        *self.inner.bearer.write() = token;
    }

    /// Backend base URL — exposed so persistence layers can stamp
    /// snapshots with the host they belong to and invalidate cached
    /// credentials when the user repoints to a different deployment.
    pub fn backend_base_url(&self) -> &str {
        self.inner.base.as_str()
    }

    /// Backend host (no scheme, no path) — convenient for keychain
    /// account names. Falls back to the full URL if parsing somehow
    /// dropped the host (shouldn't happen for `https://...`).
    pub fn backend_host(&self) -> String {
        self.inner
            .base
            .host_str()
            .map(|h| h.to_string())
            .unwrap_or_else(|| self.inner.base.as_str().to_string())
    }

    /// Direct access to the underlying `reqwest::Client` for off-envelope
    /// fetches (subscribe text, manifest downloads).
    pub fn raw(&self) -> &reqwest::Client {
        &self.inner.http
    }

    fn endpoint(&self, path: &str) -> Result<url::Url> {
        Ok(self.inner.base.join(path)?)
    }

    fn default_headers(&self) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(ACCEPT, HeaderValue::from_static("application/json"));
        h.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        if let Ok(loc) = HeaderValue::from_str(&self.inner.locale) {
            h.insert(ACCEPT_LANGUAGE, loc);
        }
        if let Some(bearer) = self.inner.bearer.read().as_ref() {
            if let Ok(v) = HeaderValue::from_str(bearer.expose_secret()) {
                h.insert(AUTHORIZATION, v);
            }
        }
        h
    }

    async fn unwrap_envelope<T: DeserializeOwned>(resp: reqwest::Response) -> Result<T> {
        let status = resp.status();
        if status.as_u16() == 401 || status.as_u16() == 403 {
            return Err(XboardError::Unauthorized);
        }
        let body: serde_json::Value = resp.json().await?;
        if body.get("status").is_some() {
            let env: ApiEnvelope<T> = serde_json::from_value(body)?;
            match env.status {
                ApiStatus::Success => env.data.ok_or(XboardError::ApiFailure {
                    status_code: status.as_u16(),
                    message: "success but data missing".into(),
                }),
                ApiStatus::Fail => Err(XboardError::ApiFailure {
                    status_code: status.as_u16(),
                    message: env.message.unwrap_or_default(),
                }),
            }
        } else {
            // Some endpoints return a raw `{ data: ... }` shape.
            Ok(serde_json::from_value(body)?)
        }
    }

    pub async fn get_json<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        let url = self.endpoint(path)?;
        let resp = self
            .inner
            .http
            .get(url)
            .headers(self.default_headers())
            .send()
            .await?;
        Self::unwrap_envelope(resp).await
    }

    pub async fn post_json<B: Serialize, T: DeserializeOwned>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T> {
        let url = self.endpoint(path)?;
        let resp = self
            .inner
            .http
            .post(url)
            .headers(self.default_headers())
            .json(body)
            .send()
            .await?;
        Self::unwrap_envelope(resp).await
    }
}
