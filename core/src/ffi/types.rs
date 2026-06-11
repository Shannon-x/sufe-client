//! Rust mirrors of every UDL `dictionary` plus conversions from the
//! corresponding `crate::api::*` and `crate::kernel::*` types.
//!
//! Why mirror instead of `#[derive(uniffi::Record)]` on the originals?
//! Two reasons:
//!
//! 1. The wire types carry shapes UniFFI doesn't model directly:
//!    `serde_json::Value` (CheckoutResponse), `chrono::DateTime` (state
//!    timestamps), Rust-only deserializers (`de_truthy`).
//! 2. The mirror lets us rename fields where the panel's wire-name doesn't
//!    match what the Compose / SwiftUI layers want (`SubscribeInfo::u/d`
//!    → `upload/download`).
//!
//! Conversions are infallible — every wire type either drops cleanly into
//! the FFI shape or carries enough fallback to avoid panicking.

use crate::api::{AuthResult as ApiAuthResult, CheckoutResponse as ApiCheckoutResponse};
use crate::api::{
    CouponCheckResult as ApiCouponCheckResult, Notice as ApiNotice, Order as ApiOrder,
    PaymentMethod as ApiPaymentMethod, Plan as ApiPlan, SiteConfig as ApiSiteConfig,
    SubscribeInfo as ApiSubscribeInfo, Ticket as ApiTicket, TicketDetail as ApiTicketDetail,
    TicketMessage as ApiTicketMessage, UserInfo as ApiUserInfo,
};
use crate::kernel::driver::{ProxyGroup as KernelProxyGroup, TrafficStats as KernelTrafficStats};
use crate::kernel::manager::{
    ConnectStage as KernelConnectStage, ConnectionState as KernelConnectionState,
    TunnelMode as KernelTunnelMode,
};

// ---------------------------------------------------------------------- //
// Auth                                                                    //
// ---------------------------------------------------------------------- //

#[derive(Debug, Clone)]
pub struct LoginSummary {
    pub email: String,
    pub is_admin: bool,
    pub subscribe_token: String,
}

impl LoginSummary {
    pub fn from_auth_result(email: String, auth: &ApiAuthResult) -> Self {
        Self {
            email,
            is_admin: auth.is_admin,
            subscribe_token: auth.token.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct LoginArgs {
    pub email: String,
    pub password: String,
    pub recaptcha: Option<String>,
    pub turnstile: Option<String>,
}

#[derive(Debug, Clone)]
pub struct RegisterArgs {
    pub email: String,
    pub password: String,
    pub email_code: String,
    pub invite_code: Option<String>,
    pub recaptcha: Option<String>,
    pub turnstile: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ForgetPasswordArgs {
    pub email: String,
    pub password: String,
    pub email_code: String,
    pub recaptcha: Option<String>,
    pub turnstile: Option<String>,
}

// ---------------------------------------------------------------------- //
// Site config                                                             //
// ---------------------------------------------------------------------- //

#[derive(Debug, Clone)]
pub struct SiteConfig {
    pub tos_url: String,
    pub is_email_verify: bool,
    pub is_invite_force: bool,
    pub email_whitelist_suffix: Vec<String>,
    pub is_captcha: bool,
    pub captcha_type: String,
    pub recaptcha_site_key: String,
    pub recaptcha_v3_site_key: String,
    /// `None` ⇒ use reCAPTCHA-recommended default. See API-side doc.
    pub recaptcha_v3_score_threshold: Option<f32>,
    pub turnstile_site_key: String,
    pub is_recaptcha: bool,
    pub app_description: String,
    pub app_url: String,
    pub logo: String,
}

impl From<ApiSiteConfig> for SiteConfig {
    fn from(c: ApiSiteConfig) -> Self {
        Self {
            tos_url: c.tos_url,
            is_email_verify: c.is_email_verify,
            is_invite_force: c.is_invite_force,
            email_whitelist_suffix: c.email_whitelist_suffix,
            is_captcha: c.is_captcha,
            captcha_type: c.captcha_type,
            recaptcha_site_key: c.recaptcha_site_key,
            recaptcha_v3_site_key: c.recaptcha_v3_site_key,
            recaptcha_v3_score_threshold: c.recaptcha_v3_score_threshold,
            turnstile_site_key: c.turnstile_site_key,
            is_recaptcha: c.is_recaptcha,
            app_description: c.app_description,
            app_url: c.app_url,
            logo: c.logo,
        }
    }
}

// ---------------------------------------------------------------------- //
// User                                                                    //
// ---------------------------------------------------------------------- //

#[derive(Debug, Clone)]
pub struct UserInfo {
    pub email: String,
    pub balance: i64,
    pub commission_balance: i64,
    pub plan_id: Option<i64>,
    pub expired_at: Option<i64>,
    pub uuid: Option<String>,
    pub avatar_url: Option<String>,
}

impl From<ApiUserInfo> for UserInfo {
    fn from(u: ApiUserInfo) -> Self {
        Self {
            email: u.email,
            balance: u.balance,
            commission_balance: u.commission_balance,
            plan_id: u.plan_id,
            expired_at: u.expired_at,
            uuid: u.uuid,
            avatar_url: u.avatar_url,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SubscribeInfo {
    pub plan_id: Option<i64>,
    pub token: String,
    pub expired_at: Option<i64>,
    pub upload: u64,
    pub download: u64,
    pub transfer_enable: u64,
    pub subscribe_url: String,
    /// Widened to `u16` to cover yearly-reset plans (~366 days legitimate).
    pub reset_day: Option<u16>,
}

impl From<ApiSubscribeInfo> for SubscribeInfo {
    fn from(s: ApiSubscribeInfo) -> Self {
        Self {
            plan_id: s.plan_id,
            token: s.token,
            expired_at: s.expired_at,
            upload: s.u,
            download: s.d,
            transfer_enable: s.transfer_enable,
            subscribe_url: s.subscribe_url,
            reset_day: s.reset_day,
        }
    }
}

// ---------------------------------------------------------------------- //
// Notice                                                                  //
// ---------------------------------------------------------------------- //

#[derive(Debug, Clone)]
pub struct Notice {
    pub id: i64,
    pub title: String,
    pub content: String,
    pub img_url: Option<String>,
    pub tags: Vec<String>,
    pub created_at: Option<i64>,
    pub updated_at: Option<i64>,
}

impl From<ApiNotice> for Notice {
    fn from(n: ApiNotice) -> Self {
        Self {
            id: n.id,
            title: n.title,
            content: n.content,
            img_url: n.img_url,
            tags: n.tags,
            created_at: n.created_at,
            updated_at: n.updated_at,
        }
    }
}

// ---------------------------------------------------------------------- //
// Plans / orders                                                          //
// ---------------------------------------------------------------------- //

#[derive(Debug, Clone)]
pub struct Plan {
    pub id: i64,
    pub name: String,
    pub content: String,
    pub group_id: Option<i64>,
    pub kind: Option<i32>,
    pub transfer_enable: u64,
    pub month_price: Option<i64>,
    pub quarter_price: Option<i64>,
    pub half_year_price: Option<i64>,
    pub year_price: Option<i64>,
    pub two_year_price: Option<i64>,
    pub three_year_price: Option<i64>,
    pub onetime_price: Option<i64>,
    pub reset_price: Option<i64>,
    pub reset_traffic_method: Option<i32>,
    pub show: bool,
    pub sell: bool,
    pub renew: bool,
    pub sort: Option<i64>,
    pub created_at: Option<i64>,
    pub updated_at: Option<i64>,
}

impl From<ApiPlan> for Plan {
    fn from(p: ApiPlan) -> Self {
        Self {
            id: p.id,
            name: p.name,
            content: p.content,
            group_id: p.group_id,
            kind: p.kind,
            transfer_enable: p.transfer_enable,
            month_price: p.month_price,
            quarter_price: p.quarter_price,
            half_year_price: p.half_year_price,
            year_price: p.year_price,
            two_year_price: p.two_year_price,
            three_year_price: p.three_year_price,
            onetime_price: p.onetime_price,
            reset_price: p.reset_price,
            reset_traffic_method: p.reset_traffic_method,
            show: p.show,
            sell: p.sell,
            renew: p.renew,
            sort: p.sort,
            created_at: p.created_at,
            updated_at: p.updated_at,
        }
    }
}

#[derive(Debug, Clone)]
pub struct PaymentMethod {
    pub id: i64,
    pub name: String,
    pub payment: String,
    pub icon: Option<String>,
    pub handling_fee_fixed: Option<i64>,
    pub handling_fee_percent: Option<f64>,
}

impl From<ApiPaymentMethod> for PaymentMethod {
    fn from(p: ApiPaymentMethod) -> Self {
        Self {
            id: p.id,
            name: p.name,
            payment: p.payment,
            icon: p.icon,
            handling_fee_fixed: p.handling_fee_fixed,
            handling_fee_percent: p.handling_fee_percent,
        }
    }
}

/// `data_json` is the upstream `data` field re-serialised to JSON, so the
/// caller can `JSON.parse` whatever shape it expects for the given `kind`
/// (URL, QR, Stripe form, etc.) without a UniFFI-side ADT for every payment
/// gateway.
#[derive(Debug, Clone)]
pub struct CheckoutResponse {
    pub kind: i32,
    pub data_json: String,
}

impl From<ApiCheckoutResponse> for CheckoutResponse {
    fn from(c: ApiCheckoutResponse) -> Self {
        let data_json = serde_json::to_string(&c.data).unwrap_or_else(|_| "null".to_string());
        Self {
            kind: c.kind,
            data_json,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SaveOrderArgs {
    pub plan_id: i64,
    pub period: String,
    pub coupon_code: Option<String>,
}

/// FFI mirror of `api::CouponCheckResult`. `kind` matches the upstream
/// `type` discriminator (1 = fixed-cents, 2 = percent); see the API-side
/// doc-comment for `value` semantics.
#[derive(Debug, Clone)]
pub struct CouponCheckResult {
    pub id: i64,
    pub code: String,
    pub kind: i32,
    pub value: Option<i64>,
}

impl From<ApiCouponCheckResult> for CouponCheckResult {
    fn from(c: ApiCouponCheckResult) -> Self {
        Self {
            id: c.id,
            code: c.code,
            kind: c.r#type,
            value: c.value,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Order {
    pub id: i64,
    pub trade_no: String,
    pub plan_id: Option<i64>,
    pub period: Option<String>,
    pub kind: Option<i32>,
    pub status: i32,
    pub commission_status: Option<i32>,
    pub total_amount: i64,
    pub balance_amount: Option<i64>,
    pub discount_amount: Option<i64>,
    pub surplus_amount: Option<i64>,
    pub refund_amount: Option<i64>,
    pub created_at: Option<i64>,
    pub updated_at: Option<i64>,
}

impl From<ApiOrder> for Order {
    fn from(o: ApiOrder) -> Self {
        Self {
            id: o.id,
            trade_no: o.trade_no,
            plan_id: o.plan_id,
            period: o.period,
            kind: o.kind,
            status: o.status,
            commission_status: o.commission_status,
            total_amount: o.total_amount,
            balance_amount: o.balance_amount,
            discount_amount: o.discount_amount,
            surplus_amount: o.surplus_amount,
            refund_amount: o.refund_amount,
            created_at: o.created_at,
            updated_at: o.updated_at,
        }
    }
}

// ---------------------------------------------------------------------- //
// Tickets                                                                 //
// ---------------------------------------------------------------------- //

#[derive(Debug, Clone)]
pub struct Ticket {
    pub id: i64,
    pub level: i32,
    pub reply_status: i32,
    pub status: i32,
    pub subject: String,
    pub last_reply_user_id: Option<i64>,
    pub created_at: Option<i64>,
    pub updated_at: Option<i64>,
}

impl From<ApiTicket> for Ticket {
    fn from(t: ApiTicket) -> Self {
        Self {
            id: t.id,
            level: t.level,
            reply_status: t.reply_status,
            status: t.status,
            subject: t.subject,
            last_reply_user_id: t.last_reply_user_id,
            created_at: t.created_at,
            updated_at: t.updated_at,
        }
    }
}

#[derive(Debug, Clone)]
pub struct TicketMessage {
    pub id: i64,
    pub ticket_id: i64,
    pub user_id: Option<i64>,
    pub message: String,
    pub is_me: bool,
    pub created_at: Option<i64>,
}

impl From<ApiTicketMessage> for TicketMessage {
    fn from(m: ApiTicketMessage) -> Self {
        Self {
            id: m.id,
            ticket_id: m.ticket_id,
            user_id: m.user_id,
            message: m.message,
            is_me: m.is_me,
            created_at: m.created_at,
        }
    }
}

#[derive(Debug, Clone)]
pub struct TicketDetail {
    pub id: i64,
    pub level: i32,
    pub reply_status: i32,
    pub status: i32,
    pub subject: String,
    pub message: Vec<TicketMessage>,
    pub created_at: Option<i64>,
    pub updated_at: Option<i64>,
}

impl From<ApiTicketDetail> for TicketDetail {
    fn from(d: ApiTicketDetail) -> Self {
        Self {
            id: d.id,
            level: d.level,
            reply_status: d.reply_status,
            status: d.status,
            subject: d.subject,
            message: d.message.into_iter().map(TicketMessage::from).collect(),
            created_at: d.created_at,
            updated_at: d.updated_at,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SaveTicketArgs {
    pub subject: String,
    pub level: i32,
    pub message: String,
}

// ---------------------------------------------------------------------- //
// Kernel control plane                                                    //
// ---------------------------------------------------------------------- //

#[derive(Debug, Clone)]
pub struct ProxyGroup {
    pub name: String,
    pub kind: String,
    pub now: Option<String>,
    pub all: Vec<String>,
}

impl From<KernelProxyGroup> for ProxyGroup {
    fn from(g: KernelProxyGroup) -> Self {
        Self {
            name: g.name,
            kind: g.kind,
            now: g.now,
            all: g.all,
        }
    }
}

#[derive(Debug, Clone)]
pub struct TrafficStats {
    pub up: u64,
    pub down: u64,
    pub up_total: u64,
    pub down_total: u64,
}

impl From<KernelTrafficStats> for TrafficStats {
    fn from(t: KernelTrafficStats) -> Self {
        Self {
            up: t.up,
            down: t.down,
            up_total: t.up_total,
            down_total: t.down_total,
        }
    }
}

// ---------------------------------------------------------------------- //
// Connection state                                                        //
// ---------------------------------------------------------------------- //

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectStage {
    Fetching,
    Writing,
    Elevating,
    Spawning,
    ApplyingRoute,
    FallbackProxy,
}

impl From<KernelConnectStage> for ConnectStage {
    fn from(s: KernelConnectStage) -> Self {
        match s {
            KernelConnectStage::Fetching => Self::Fetching,
            KernelConnectStage::Writing => Self::Writing,
            KernelConnectStage::Elevating => Self::Elevating,
            KernelConnectStage::Spawning => Self::Spawning,
            KernelConnectStage::ApplyingRoute => Self::ApplyingRoute,
            KernelConnectStage::FallbackProxy => Self::FallbackProxy,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TunnelMode {
    Tun,
    SystemProxy,
}

impl From<KernelTunnelMode> for TunnelMode {
    fn from(m: KernelTunnelMode) -> Self {
        match m {
            KernelTunnelMode::Tun => Self::Tun,
            KernelTunnelMode::SystemProxy => Self::SystemProxy,
        }
    }
}

impl From<TunnelMode> for KernelTunnelMode {
    fn from(m: TunnelMode) -> Self {
        match m {
            TunnelMode::Tun => Self::Tun,
            TunnelMode::SystemProxy => Self::SystemProxy,
        }
    }
}

/// FFI-shaped state mirror. The kernel manager's `Error` variant is renamed
/// to `Failed` to match the UDL — the UI side reads it as terminal-but-not-
/// disconnected, no upstream callers compare against the variant name.
#[derive(Debug, Clone)]
pub enum ConnectionState {
    Disconnected,
    Connecting {
        stage: ConnectStage,
        mode: TunnelMode,
    },
    Connected {
        /// RFC 3339 timestamp — formatted Rust-side so Kotlin / Swift only
        /// have to display it.
        since: String,
        mode: TunnelMode,
        mixed_port: u16,
    },
    Failed {
        message: String,
        mode: TunnelMode,
    },
}

impl From<KernelConnectionState> for ConnectionState {
    fn from(s: KernelConnectionState) -> Self {
        match s {
            KernelConnectionState::Disconnected => Self::Disconnected,
            KernelConnectionState::Connecting { stage, mode } => Self::Connecting {
                stage: stage.into(),
                mode: mode.into(),
            },
            KernelConnectionState::Connected {
                since,
                mode,
                mixed_port,
            } => Self::Connected {
                since: since.to_rfc3339(),
                mode: mode.into(),
                mixed_port,
            },
            KernelConnectionState::Error { message, mode } => Self::Failed {
                message,
                mode: mode.into(),
            },
        }
    }
}

// ---------------------------------------------------------------------- //
// TUN delegate config                                                     //
// ---------------------------------------------------------------------- //

#[derive(Debug, Clone)]
pub struct TunConfig {
    pub session: String,
    pub ipv4_addr: String,
    pub ipv4_prefix: u8,
    pub routes: Vec<String>,
    pub dns: Vec<String>,
    pub mtu: u32,
}
