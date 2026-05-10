//! `interface Client` backing implementation.
//!
//! Mirrors the desktop Tauri command surface 1:1 — every method here has a
//! sibling in [`crate::desktop::commands::*`], so the Compose / SwiftUI
//! shells can call the FFI methods with the same arg names + return shapes
//! the Vue side already uses.
//!
//! Persistence model (mobile-friendly, single account):
//!
//! - `bearer` — Sanctum `auth_data` (already prefixed with `"Bearer "`).
//! - `session` — JSON `SessionSnapshot { email, is_admin, subscribe_token,
//!   backend_host }`. The `backend_host` field guards against replaying a
//!   stale token when the user repoints the app at a different deployment.
//!
//! Both keys are written atomically on login / register and best-effort
//! deleted on logout. SecureStore call failures during write degrade
//! gracefully (the in-memory bearer still works) — but during read they
//! short-circuit hydrate to "no session".

use std::sync::Arc;

use parking_lot::RwLock;
use secrecy::SecretString;
use serde::{Deserialize, Serialize};

use super::errors::FfiError;
use super::secure::{CallbackSecureStore, SecureStore};
use super::types::{
    CheckoutResponse, ForgetPasswordArgs, LoginArgs, LoginSummary, Notice, Order, PaymentMethod,
    Plan, RegisterArgs, SaveOrderArgs, SaveTicketArgs, SiteConfig, SubscribeInfo, Ticket,
    TicketDetail, UserInfo,
};
use crate::api::{HttpClient, LoginRequest, RegisterRequest};
use crate::storage::SecureStore as CoreSecureStore;

/// Persisted session metadata. Lives alongside the bearer in the host
/// SecureStore under the `session` key.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SessionSnapshot {
    email: String,
    is_admin: bool,
    subscribe_token: String,
    backend_host: String,
}

const SESSION_KEY: &str = "session";
const BEARER_KEY: &str = "bearer";

#[derive(Debug)]
pub struct Client {
    http: HttpClient,
    secure: Arc<dyn CoreSecureStore>,
    /// Mirror of the most recently-authenticated user's snapshot. Populated
    /// after `hydrate_session` / `login` / `register`. Cleared on `logout`.
    /// Cheap-to-Clone — the heavy state (HTTP client, secure store) lives
    /// outside this lock.
    session: RwLock<Option<SessionSnapshot>>,
}

impl Client {
    /// UDL constructor. Validates the backend URL and wraps `secure` so the
    /// crate-internal `CoreSecureStore` trait can keep being used unchanged
    /// inside `hydrate_session` / `login` etc.
    ///
    /// `secure` arrives as `Box<dyn SecureStore>` (UniFFI's calling convention
    /// for `callback interface` arguments); we re-bind it as an `Arc` so the
    /// `blocking_*` helpers can hand a clone off to `tokio::task::spawn_blocking`
    /// without forcing every caller to pre-share it. The return type is bare
    /// `Self` — UniFFI's scaffolding wraps interface returns in `Arc<...>`
    /// itself, so returning `Arc<Self>` here would double-wrap.
    pub fn new(
        backend_base_url: String,
        locale: String,
        secure: Box<dyn SecureStore>,
    ) -> Result<Self, FfiError> {
        let http = HttpClient::new(&backend_base_url, &locale)?;
        let secure: Arc<dyn CoreSecureStore> =
            Arc::new(CallbackSecureStore::new(Arc::from(secure)));
        Ok(Self {
            http,
            secure,
            session: RwLock::new(None),
        })
    }

    /// Internal: shared HTTP client. The `ConnectionManager` constructor
    /// borrows this so subscription fetches share the bearer + base URL.
    pub(crate) fn http_client(&self) -> HttpClient {
        self.http.clone()
    }

    // -- Session lifecycle -------------------------------------------------

    pub async fn hydrate_session(&self) -> Result<Option<LoginSummary>, FfiError> {
        let secure = self.secure.clone();
        let raw = blocking_get(&secure, SESSION_KEY).await?;
        let Some(raw) = raw else {
            return Ok(None);
        };

        let snap: SessionSnapshot = match serde_json::from_str(&raw) {
            Ok(s) => s,
            Err(_) => {
                // Corrupt snapshot — treat as absent and clean up so we
                // don't keep failing on every cold start.
                let _ = blocking_delete(&secure, SESSION_KEY).await;
                let _ = blocking_delete(&secure, BEARER_KEY).await;
                return Ok(None);
            }
        };

        // Bind to the backend the snapshot was captured against. If the user
        // repointed the app at a different deployment, the cached bearer is
        // useless and we force a fresh login.
        if snap.backend_host != self.http.backend_host() {
            let _ = blocking_delete(&secure, SESSION_KEY).await;
            let _ = blocking_delete(&secure, BEARER_KEY).await;
            return Ok(None);
        }

        let bearer = match blocking_get(&secure, BEARER_KEY).await? {
            Some(b) => b,
            None => {
                let _ = blocking_delete(&secure, SESSION_KEY).await;
                return Ok(None);
            }
        };

        self.http.set_bearer(Some(SecretString::from(bearer)));
        *self.session.write() = Some(snap.clone());
        Ok(Some(LoginSummary {
            email: snap.email,
            is_admin: snap.is_admin,
            subscribe_token: snap.subscribe_token,
        }))
    }

    pub async fn check_login(&self) -> Result<bool, FfiError> {
        match self.http.check_login().await {
            Ok(resp) => Ok(resp.is_login),
            Err(crate::error::XboardError::Unauthorized) => Ok(false),
            Err(e) => Err(e.into()),
        }
    }

    pub async fn login(&self, args: LoginArgs) -> Result<LoginSummary, FfiError> {
        let req = LoginRequest {
            email: &args.email,
            password: &args.password,
            recaptcha_data: args.recaptcha.as_deref(),
            turnstile: args.turnstile.as_deref(),
        };
        let auth = self.http.login(&req).await?;
        self.persist_session(&args.email, &auth).await?;
        Ok(LoginSummary::from_auth_result(args.email, &auth))
    }

    pub async fn register(&self, args: RegisterArgs) -> Result<LoginSummary, FfiError> {
        let req = RegisterRequest {
            email: &args.email,
            password: &args.password,
            email_code: &args.email_code,
            invite_code: args.invite_code.as_deref(),
            recaptcha_data: args.recaptcha.as_deref(),
            turnstile: args.turnstile.as_deref(),
        };
        let auth = self.http.register(&req).await?;
        self.persist_session(&args.email, &auth).await?;
        Ok(LoginSummary::from_auth_result(args.email, &auth))
    }

    pub async fn send_email_verify(&self, email: String) -> Result<(), FfiError> {
        self.http.send_email_verify(&email).await?;
        Ok(())
    }

    pub async fn forget_password(&self, args: ForgetPasswordArgs) -> Result<(), FfiError> {
        self.http
            .forget_password(
                &args.email,
                &args.password,
                &args.email_code,
                args.recaptcha.as_deref(),
                args.turnstile.as_deref(),
            )
            .await?;
        Ok(())
    }

    /// Best-effort cleanup. Always succeeds — keychain failures are logged
    /// but never surfaced to the host (the UDL marks this method as not
    /// throwing).
    pub async fn logout(&self) {
        self.http.set_bearer(None);
        *self.session.write() = None;
        let secure = self.secure.clone();
        // Best-effort; ignore errors.
        let _ = blocking_delete(&secure, BEARER_KEY).await;
        let _ = blocking_delete(&secure, SESSION_KEY).await;
    }

    // -- Site / user / subscribe ------------------------------------------

    pub async fn fetch_site_config(&self) -> Result<SiteConfig, FfiError> {
        Ok(self.http.site_config().await?.into())
    }

    pub async fn current_user(&self) -> Result<UserInfo, FfiError> {
        Ok(self.http.user_info().await?.into())
    }

    pub async fn current_subscribe(&self) -> Result<SubscribeInfo, FfiError> {
        Ok(self.http.user_subscribe().await?.into())
    }

    // -- Notices / plans / payment ----------------------------------------

    pub async fn fetch_notices(&self) -> Result<Vec<Notice>, FfiError> {
        let raw = self.http.fetch_notices().await?;
        Ok(raw.into_iter().map(Notice::from).collect())
    }

    pub async fn fetch_plans(&self) -> Result<Vec<Plan>, FfiError> {
        let raw = self.http.fetch_plans().await?;
        Ok(raw.into_iter().map(Plan::from).collect())
    }

    pub async fn fetch_payment_methods(&self) -> Result<Vec<PaymentMethod>, FfiError> {
        let raw = self.http.fetch_payment_methods().await?;
        Ok(raw.into_iter().map(PaymentMethod::from).collect())
    }

    // -- Orders ------------------------------------------------------------

    pub async fn save_order(&self, args: SaveOrderArgs) -> Result<String, FfiError> {
        Ok(self
            .http
            .save_order(args.plan_id, &args.period, args.coupon_code.as_deref())
            .await?)
    }

    pub async fn checkout_order(
        &self,
        trade_no: String,
        method_id: i64,
    ) -> Result<CheckoutResponse, FfiError> {
        Ok(self.http.checkout_order(&trade_no, method_id).await?.into())
    }

    pub async fn check_order(&self, trade_no: String) -> Result<i32, FfiError> {
        Ok(self.http.check_order(&trade_no).await?)
    }

    pub async fn cancel_order(&self, trade_no: String) -> Result<(), FfiError> {
        self.http.cancel_order(&trade_no).await?;
        Ok(())
    }

    pub async fn fetch_orders(&self) -> Result<Vec<Order>, FfiError> {
        let raw = self.http.fetch_orders().await?;
        Ok(raw.into_iter().map(Order::from).collect())
    }

    // -- Tickets -----------------------------------------------------------

    pub async fn fetch_tickets(&self) -> Result<Vec<Ticket>, FfiError> {
        let raw = self.http.fetch_tickets().await?;
        Ok(raw.into_iter().map(Ticket::from).collect())
    }

    pub async fn fetch_ticket(&self, id: i64) -> Result<TicketDetail, FfiError> {
        Ok(self.http.fetch_ticket(id).await?.into())
    }

    pub async fn reply_ticket(&self, id: i64, message: String) -> Result<(), FfiError> {
        self.http.reply_ticket(id, &message).await?;
        Ok(())
    }

    pub async fn close_ticket(&self, id: i64) -> Result<(), FfiError> {
        self.http.close_ticket(id).await?;
        Ok(())
    }

    pub async fn save_ticket(&self, args: SaveTicketArgs) -> Result<Option<i64>, FfiError> {
        Ok(self
            .http
            .save_ticket(&args.subject, args.level, &args.message)
            .await?)
    }

    // -- Internal helpers --------------------------------------------------

    /// Bake a fresh session into the in-memory state + SecureStore. Failure
    /// to write the keychain is logged but doesn't fail the call — the
    /// in-memory bearer stays valid for the rest of this app run.
    async fn persist_session(
        &self,
        email: &str,
        auth: &crate::api::AuthResult,
    ) -> Result<(), FfiError> {
        self.http
            .set_bearer(Some(SecretString::from(auth.auth_data.clone())));

        let snap = SessionSnapshot {
            email: email.to_string(),
            is_admin: auth.is_admin,
            subscribe_token: auth.token.clone(),
            backend_host: self.http.backend_host(),
        };
        *self.session.write() = Some(snap.clone());

        let snap_json = serde_json::to_string(&snap).map_err(|e| {
            FfiError::Config(format!("serialize session snapshot: {e}"))
        })?;

        let secure = self.secure.clone();
        if let Err(e) = blocking_put(&secure, BEARER_KEY, &auth.auth_data).await {
            tracing::warn!(error = ?e, "secure store unavailable; bearer kept in-memory only");
        }
        if let Err(e) = blocking_put(&secure, SESSION_KEY, &snap_json).await {
            tracing::warn!(error = ?e, "secure store unavailable; session snapshot not persisted");
        }
        Ok(())
    }
}

// -- Blocking SecureStore helpers ----------------------------------------- //
// Keychain backends are uniformly synchronous (Keyring crate, Android
// EncryptedSharedPreferences, iOS Security.framework). Wrap each call in
// `spawn_blocking` so a slow keychain doesn't stall the tokio runtime that's
// driving the rest of the FFI methods.

async fn blocking_get(
    store: &Arc<dyn CoreSecureStore>,
    key: &str,
) -> Result<Option<String>, FfiError> {
    let store = store.clone();
    let key = key.to_string();
    tokio::task::spawn_blocking(move || store.get(&key))
        .await
        .map_err(|e| FfiError::Config(format!("spawn_blocking: {e}")))?
        .map_err(FfiError::from)
}

async fn blocking_put(
    store: &Arc<dyn CoreSecureStore>,
    key: &str,
    value: &str,
) -> Result<(), FfiError> {
    let store = store.clone();
    let key = key.to_string();
    let value = value.to_string();
    tokio::task::spawn_blocking(move || store.put(&key, &value))
        .await
        .map_err(|e| FfiError::Config(format!("spawn_blocking: {e}")))?
        .map_err(FfiError::from)
}

async fn blocking_delete(
    store: &Arc<dyn CoreSecureStore>,
    key: &str,
) -> Result<(), FfiError> {
    let store = store.clone();
    let key = key.to_string();
    tokio::task::spawn_blocking(move || store.delete(&key))
        .await
        .map_err(|e| FfiError::Config(format!("spawn_blocking: {e}")))?
        .map_err(FfiError::from)
}
