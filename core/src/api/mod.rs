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
pub mod benefits;
pub mod client;
pub mod deployment;
pub mod guest;
pub mod runtime_config;
pub mod support;
pub mod notice;
pub mod order;
pub mod plan;
pub(crate) mod serde_helpers;
pub mod ticket;
pub mod transport;
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
    endpoints: RwLock<Vec<url::Url>>,
    active: std::sync::atomic::AtomicUsize,
    stealth: Option<transport::Stealth>,
    middleware_captcha: std::sync::atomic::AtomicBool,
    locale: String,
    bearer: RwLock<Option<SecretString>>,
    http: reqwest::Client,
}

impl HttpClient {
    pub fn new(base: &str, locale: &str) -> Result<Self> {
        Self::with_endpoints(&[base.to_string()], locale, None)
    }

    pub fn with_endpoints(
        endpoints: &[String],
        locale: &str,
        stealth_password: Option<&str>,
    ) -> Result<Self> {
        if endpoints.is_empty() || endpoints.len() > 16 {
            return Err(XboardError::Config("API 地址数量无效".into()));
        }
        let endpoints = endpoints
            .iter()
            .map(|s| deployment::validate_api_url(s))
            .collect::<Result<Vec<_>>>()?;
        let base = endpoints[0].clone();
        // UA must contain a clash.meta-family keyword (`meta`, `verge`,
        // `flclash`, `nekobox`, `clashmetaforandroid`) so Xboard panels'
        // protocol dispatcher routes our subscribe fetches to the ClashMeta
        // handler (mihomo-native YAML w/ full Hysteria2/VLESS/TUIC coverage)
        // rather than falling through to General (plain v2ray base64 links,
        // which mihomo can't fully consume). `clash.meta/<mihomo-version>`
        // is the cleanest match and aligns with the bundled mihomo binary.
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            .connect_timeout(Duration::from_secs(5))
            // Never replay credentials or encrypted bodies through redirects.
            .redirect(reqwest::redirect::Policy::none())
            .user_agent(concat!(
                "clash.meta/v1.18.7 xboard-client/",
                env!("CARGO_PKG_VERSION")
            ))
            .build()?;
        Ok(Self {
            inner: Arc::new(Inner {
                base,
                endpoints: RwLock::new(endpoints),
                active: std::sync::atomic::AtomicUsize::new(0),
                stealth: stealth_password.map(transport::Stealth::new).transpose()?,
                middleware_captcha: std::sync::atomic::AtomicBool::new(false),
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

    pub fn set_middleware_captcha(&self, enabled: bool) {
        self.inner
            .middleware_captcha
            .store(enabled, std::sync::atomic::Ordering::Relaxed);
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

    fn endpoint_at(base: &url::Url, path: &str) -> Result<url::Url> {
        // Commands can only select an API path, never an arbitrary credential recipient.
        if !path.starts_with("/api/") || path.contains('#') || path.contains('\\') {
            return Err(XboardError::Config("无效 API 路径".into()));
        }
        let url = base.join(path)?;
        if url.origin() != base.origin() {
            return Err(XboardError::Config("禁止跨站 API 请求".into()));
        }
        Ok(url)
    }

    /// Replace only from the trusted signed bootstrap; base identity stays fixed
    /// so keychain/session storage is stable across failover hosts.
    pub fn set_endpoints(&self, endpoints: &[String]) -> Result<()> {
        if endpoints.is_empty() || endpoints.len() > 16 {
            return Err(XboardError::Config("API 地址数量无效".into()));
        }
        let parsed = endpoints
            .iter()
            .map(|s| deployment::validate_api_url(s))
            .collect::<Result<Vec<_>>>()?;
        *self.inner.endpoints.write() = parsed;
        self.inner
            .active
            .store(0, std::sync::atomic::Ordering::Relaxed);
        Ok(())
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

    fn decode_envelope<T: DeserializeOwned>(status: u16, bytes: &[u8]) -> Result<T> {
        if status == 401 {
            return Err(XboardError::Unauthorized);
        }
        let body: serde_json::Value =
            serde_json::from_slice(bytes).map_err(|_| XboardError::ApiFailure {
                status_code: status,
                message: "服务返回了无法识别的响应，请稍后重试".into(),
            })?;
        if status == 403
            && matches!(
                body.get("message").and_then(|v| v.as_str()),
                Some(
                    "未登录或登陆已过期"
                        | "未登录或登录已过期"
                        | "Unauthenticated."
                        | "Unauthenticated"
                        | "Unauthorized"
                )
            )
        {
            return Err(XboardError::Unauthorized);
        }
        if !(200..300).contains(&status) {
            return Err(XboardError::ApiFailure {
                status_code: status,
                message: body
                    .get("message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("服务暂时不可用，请稍后重试")
                    .to_string(),
            });
        }
        if body.get("status").and_then(|v| v.as_str()).is_some() {
            let env: ApiEnvelope<T> = serde_json::from_value(body)?;
            match env.status {
                ApiStatus::Success => env.data.ok_or(XboardError::ApiFailure {
                    status_code: status,
                    message: "success but data missing".into(),
                }),
                ApiStatus::Fail => Err(XboardError::ApiFailure {
                    status_code: status,
                    message: env.message.unwrap_or_default(),
                }),
            }
        } else {
            // Some endpoints return a raw `{ data: ... }` shape.
            Ok(serde_json::from_value(body)?)
        }
    }

    pub async fn get_json<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        self.request_json(reqwest::Method::GET, path, None).await
    }

    pub async fn post_json<B: Serialize, T: DeserializeOwned>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T> {
        self.request_json(
            reqwest::Method::POST,
            path,
            Some(serde_json::to_value(body)?),
        )
        .await
    }

    async fn request_json<T: DeserializeOwned>(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<serde_json::Value>,
    ) -> Result<T> {
        let endpoints = self.inner.endpoints.read().clone();
        let start = self.inner.active.load(std::sync::atomic::Ordering::Relaxed) % endpoints.len();
        // Xboard has legacy GET mutations. They must never be automatically replayed.
        let route = path.split('?').next().unwrap_or(path);
        let readonly = method == reqwest::Method::GET
            && matches!(
                route,
                "/api/v1/guest/comm/config"
                    | "/api/v1/guest/plan/fetch"
                    | "/api/v1/user/info"
                    | "/api/v1/user/getSubscribe"
                    | "/api/v1/user/getStat"
                    | "/api/v1/user/checkLogin"
                    | "/api/v1/user/getActiveSession"
                    | "/api/v1/user/order/fetch"
                    | "/api/v1/user/order/detail"
                    | "/api/v1/user/order/check"
                    | "/api/v1/user/order/getPaymentMethod"
                    | "/api/v1/user/plan/fetch"
                    | "/api/v1/user/invite/fetch"
                    | "/api/v1/user/invite/details"
                    | "/api/v1/user/invite/users"
                    | "/api/v1/user/notice/fetch"
                    | "/api/v1/user/ticket/fetch"
                    | "/api/v1/user/server/fetch"
                    | "/api/v1/user/comm/config"
                    | "/api/v1/user/stat/getTrafficLog"
                    | "/api/v1/user/knowledge/fetch"
                    | "/api/v1/user/knowledge/getCategory"
                    | "/api/v1/user/gift-card/history"
                    | "/api/v1/user/gift-card/detail"
                    | "/api/v1/user/gift-card/types"
                    | "/api/v1/user/withdraw/config"
                    | "/api/v1/user/withdraw/fetch"
                    | "/api/v1/user/traffic/advance-cycle/preview"
                    | "/api/v1/user/telegram/getBotInfo"
            );
        for attempt in 0..endpoints.len() {
            let index = (start + attempt) % endpoints.len();
            let mut url = Self::endpoint_at(&endpoints[index], path)?;
            let mut headers = self.default_headers();
            // Middleware captcha reads headers; Xboard itself reads these JSON fields.
            // The same proof is forwarded only to the explicitly trusted deployment.
            if let Some(body) = body.as_ref().filter(|_| {
                self.inner
                    .middleware_captcha
                    .load(std::sync::atomic::Ordering::Relaxed)
            }) {
                for (key, provider) in [
                    ("turnstile_token", "turnstile"),
                    ("recaptcha_v3_token", "recaptcha-v3"),
                    ("recaptcha_data", "recaptcha"),
                ] {
                    if let Some(token) = body
                        .get(key)
                        .and_then(|v| v.as_str())
                        .filter(|t| !t.is_empty())
                    {
                        headers.insert("x-captcha-provider", HeaderValue::from_static(provider));
                        if let Ok(value) = HeaderValue::from_str(token) {
                            headers.insert("x-captcha-token", value);
                        }
                        break;
                    }
                }
            }
            let wire = if let Some(crypto) = self.inner.stealth.as_ref() {
                crypto.prepare(&mut url, &mut headers, body.as_ref())?
            } else {
                body.as_ref().map(serde_json::to_string).transpose()?
            };
            let mut request = self
                .inner
                .http
                .request(method.clone(), url)
                .headers(headers);
            if let Some(wire) = wire {
                request = request.body(wire);
            }
            let response = match request.send().await {
                Ok(r) => r,
                Err(e) if (readonly || e.is_connect()) && attempt + 1 < endpoints.len() => continue,
                Err(e) => return Err(XboardError::Network(e.without_url())),
            };
            let status = response.status().as_u16();
            if readonly && matches!(status, 502 | 503 | 504) && attempt + 1 < endpoints.len() {
                continue;
            }
            let nonce = response
                .headers()
                .get("x-request-id")
                .and_then(|v| v.to_str().ok())
                .map(str::to_string);
            let bytes = deployment::bounded_body(response, 4 * 1024 * 1024).await?;
            let bytes = if let Some(crypto) = self.inner.stealth.as_ref() {
                // Never accept an unauthenticated plaintext success on a secure channel.
                let nonce = nonce.ok_or_else(transport::crypto_error)?;
                crypto.decrypt_response(&nonce, &bytes)?
            } else {
                bytes
            };
            self.inner
                .active
                .store(index, std::sync::atomic::Ordering::Relaxed);
            return Self::decode_envelope(status, &bytes);
        }
        Err(XboardError::Config("所有 API 地址均暂时不可用".into()))
    }
}

#[cfg(test)]
mod transport_tests {
    use super::*;
    #[test]
    fn http_failure_cannot_be_disguised_as_success_envelope() {
        assert!(HttpClient::decode_envelope::<serde_json::Value>(
            500,
            br#"{"status":"success","data":true}"#
        )
        .is_err());
        assert!(
            HttpClient::decode_envelope::<serde_json::Value>(302, br#"{"data":true}"#).is_err()
        );
        assert!(matches!(
            HttpClient::decode_envelope::<serde_json::Value>(
                403,
                "{\"message\":\"未登录或登陆已过期\"}".as_bytes()
            ),
            Err(XboardError::Unauthorized)
        ));
        assert!(matches!(
            HttpClient::decode_envelope::<serde_json::Value>(
                403,
                br#"{"message":"captcha required"}"#
            ),
            Err(XboardError::ApiFailure {
                status_code: 403,
                ..
            })
        ));
    }
    #[test]
    fn raw_numeric_status_is_not_mistaken_for_envelope() {
        let value: serde_json::Value =
            HttpClient::decode_envelope(200, br#"{"status":3}"#).unwrap();
        assert_eq!(value["status"], 3);
    }
    #[test]
    fn endpoint_cannot_redirect_bearer_to_external_host() {
        let base = url::Url::parse("https://panel.example").unwrap();
        for path in [
            "https://evil.example",
            "//evil.example/api/foo",
            "/api/\\evil",
            "/api/foo#fragment",
        ] {
            assert!(HttpClient::endpoint_at(&base, path).is_err());
        }
    }

    async fn server(status: u16, body: &'static str) -> (String, tokio::task::JoinHandle<String>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buffer = Vec::new();
            loop {
                let mut chunk = [0; 4096];
                let count = stream.read(&mut chunk).await.unwrap();
                if count == 0 {
                    break;
                }
                buffer.extend_from_slice(&chunk[..count]);
                let text = String::from_utf8_lossy(&buffer);
                if let Some(header_end) = text.find("\r\n\r\n") {
                    let length: usize = text[..header_end]
                        .lines()
                        .find_map(|line| {
                            let (key, value) = line.split_once(':')?;
                            if key.eq_ignore_ascii_case("content-length") {
                                value.trim().parse().ok()
                            } else {
                                None
                            }
                        })
                        .unwrap_or(0);
                    if buffer.len() >= header_end + 4 + length {
                        break;
                    }
                }
            }
            let response = format!("HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
            stream.write_all(response.as_bytes()).await.unwrap();
            String::from_utf8_lossy(&buffer).into_owned()
        });
        (address, task)
    }
    #[tokio::test]
    async fn readonly_get_fails_over_but_financial_post_never_replays_on_gateway_error() {
        let (bad, bad_task) = server(503, "{}").await;
        let (good, good_task) = server(200, r#"{"status":"success","data":{"ok":true}}"#).await;
        let client = HttpClient::with_endpoints(&[bad, good], "zh-CN", None).unwrap();
        let result: serde_json::Value = client.get_json("/api/v1/user/info").await.unwrap();
        assert_eq!(result["ok"], true);
        bad_task.await.unwrap();
        good_task.await.unwrap();

        let (bad, bad_task) = server(503, "{}").await;
        let (good, good_task) = server(200, r#"{"status":"success","data":true}"#).await;
        let client = HttpClient::with_endpoints(&[bad, good], "zh-CN", None).unwrap();
        assert!(client
            .post_json::<_, serde_json::Value>(
                "/api/v1/user/order/save",
                &serde_json::json!({"plan_id":1})
            )
            .await
            .is_err());
        bad_task.await.unwrap();
        assert!(!good_task.is_finished());
        good_task.abort();
    }
    #[tokio::test]
    async fn secure_transport_rejects_plaintext_success_and_coupon_preserves_period() {
        let (url, task) = server(200, r#"{"status":"success","data":true}"#).await;
        let client = HttpClient::with_endpoints(&[url], "zh-CN", Some("secret")).unwrap();
        assert!(client
            .get_json::<serde_json::Value>("/api/v1/user/info")
            .await
            .is_err());
        let request = task.await.unwrap();
        assert!(request.starts_with("GET /v2/"));
        assert!(!request.contains("/api/v1/user/info"));

        let (url, task) = server(
            200,
            r#"{"status":"success","data":{"id":1,"type":2,"value":10}}"#,
        )
        .await;
        let client = HttpClient::new(&url, "zh-CN").unwrap();
        client
            .check_coupon_with_period("CODE", 1, Some("year_price"))
            .await
            .unwrap();
        assert!(task.await.unwrap().contains("\"period\":\"year_price\""));
    }
}
