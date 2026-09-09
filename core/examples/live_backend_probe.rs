//! Explicit operator diagnostic. Credentials arrive only through stdin; output
//! contains counts and schema results, never account or subscription secrets.
use secrecy::SecretString;
use serde::Deserialize;
use serde_json::{json, Value};
use std::io::{self, Read};
use xboard_core::api::{Captcha, LoginRequest};
use xboard_core::error::XboardError;
use xboard_core::kernel::KernelKind;

#[derive(Deserialize)]
struct Input {
    email: String,
    password: String,
}

fn error_summary(error: &XboardError) -> Value {
    match error {
        XboardError::ApiFailure {
            status_code,
            message,
        } => {
            let reason = if message.contains("邮箱或密码错误")
                || message.contains("Incorrect email or password")
            {
                "incorrect_email_or_password"
            } else if message.contains("邮箱不存在") || message.contains("email is not registered")
            {
                "email_not_registered"
            } else if message.contains("验证码") || message.to_lowercase().contains("captcha") {
                "captcha"
            } else if message.contains("次数过多")
                || message.contains("频繁")
                || message.contains("Too Many")
            {
                "rate_limited"
            } else {
                "unclassified_server_rejection"
            };
            json!({"kind":"api", "status":status_code, "reason":reason})
        }
        XboardError::Unauthorized => json!({"kind":"unauthorized"}),
        XboardError::SubscriptionUnavailable { status } => {
            json!({"kind":"subscription", "status":status})
        }
        XboardError::Json(e) => {
            json!({"kind":"json_schema", "category":format!("{:?}",e.classify()), "line":e.line(), "column":e.column()})
        }
        XboardError::Network(e) => {
            json!({"kind":"network", "timeout":e.is_timeout(), "connect":e.is_connect()})
        }
        XboardError::Yaml(_) => json!({"kind":"yaml_schema"}),
        XboardError::Config(_) => json!({"kind":"configuration"}),
        _ => json!({"kind":"other"}),
    }
}

fn record<T>(
    report: &mut Value,
    key: &str,
    result: xboard_core::Result<T>,
    summarize: impl FnOnce(&T) -> Value,
) -> Option<T> {
    match result {
        Ok(value) => {
            report[key] = json!({"ok":true, "summary":summarize(&value)});
            Some(value)
        }
        Err(error) => {
            report[key] = json!({"ok":false, "error":error_summary(&error)});
            None
        }
    }
}

#[tokio::main]
async fn main() {
    let config = match xboard_core::api::runtime_config::load() {
        Ok(c) => c,
        Err(_) => std::process::exit(2),
    };
    let origin = config
        .deployment
        .api_endpoints
        .first()
        .map(String::as_str)
        .unwrap_or("");
    if origin != "https://www.isufe.me" || config.deployment.api_endpoints.len() != 1 {
        eprintln!("Expected operator deployment is not embedded");
        std::process::exit(2);
    }
    if std::env::args().nth(1).as_deref() == Some("--check-deployment") {
        println!(
            "{}",
            json!({"backend":origin,"embedded_deployment_verified":true})
        );
        return;
    }
    let mut input = String::new();
    if io::stdin().read_to_string(&mut input).is_err() {
        std::process::exit(2);
    }
    let Ok(credentials) = serde_json::from_str::<Input>(&input) else {
        std::process::exit(2);
    };
    input.clear();
    let http = config
        .create_http("zh-CN")
        .expect("valid operator HTTP client");
    let mut report = json!({"backend":origin,"checked_at":chrono::Utc::now().to_rfc3339(),"mutations":"login only; no orders, payments, messages or account changes"});
    record(
        &mut report,
        "site_config",
        http.site_config().await,
        |c| json!({"captcha_required":c.is_captcha}),
    );
    let auth = record(
        &mut report,
        "login",
        http.login(&LoginRequest {
            email: &credentials.email,
            password: &credentials.password,
            captcha: Captcha::default(),
        })
        .await,
        |_| json!({"authenticated":true}),
    );
    drop(credentials);
    if let Some(auth) = auth {
        http.set_bearer(Some(SecretString::new(auth.auth_data.into())));
        record(
            &mut report,
            "check_login",
            http.check_login().await,
            |x| json!({"is_login":x.is_login}),
        );
        record(
            &mut report,
            "user_info",
            http.user_info().await,
            |x| json!({"banned":x.banned,"has_plan":x.plan_id.is_some()}),
        );
        let subscribe = record(
            &mut report,
            "subscription_info",
            http.user_subscribe().await,
            |x| json!({"has_plan":x.plan_id.is_some(),"traffic_remaining":x.transfer_enable.saturating_sub(x.u.saturating_add(x.d))>0,"expired":x.expired_at.is_some_and(|t|t<chrono::Utc::now().timestamp())}),
        );
        record(
            &mut report,
            "plans",
            http.fetch_plans().await,
            |x| json!({"count":x.len()}),
        );
        record(
            &mut report,
            "orders",
            http.fetch_orders().await,
            |x| json!({"count":x.len()}),
        );
        record(
            &mut report,
            "payment_methods",
            http.fetch_payment_methods().await,
            |x| json!({"count":x.len()}),
        );
        record(
            &mut report,
            "notices",
            http.fetch_notices().await,
            |x| json!({"count":x.len()}),
        );
        record(
            &mut report,
            "tickets",
            http.fetch_tickets().await,
            |x| json!({"count":x.len()}),
        );
        record(
            &mut report,
            "invites",
            http.fetch_invites().await,
            |_| json!({"decoded":true}),
        );
        record(
            &mut report,
            "gift_card_history",
            http.gift_card_history().await,
            |_| json!({"decoded":true}),
        );
        if let Some(subscribe) = subscribe {
            let profile = record(
                &mut report,
                "subscription_download",
                http.fetch_subscribe(&subscribe.subscribe_url, KernelKind::Mihomo.flag(), None)
                    .await,
                |x| json!({"status":x.status,"bytes":x.body.len()}),
            );
            if let Some(profile) = profile {
                let yaml: xboard_core::Result<serde_yaml::Value> =
                    serde_yaml::from_slice(&profile.body).map_err(Into::into);
                record(&mut report, "subscription_yaml", yaml, |x| {
                    let nodes = x.get("proxies").and_then(|n| n.as_sequence());
                    let mut protocols = std::collections::BTreeMap::<String, usize>::new();
                    for node in nodes.into_iter().flatten() {
                        if let Some(protocol) = node.get("type").and_then(|t| t.as_str()) {
                            *protocols.entry(protocol.into()).or_default() += 1;
                        }
                    }
                    json!({"nodes":nodes.map_or(0,Vec::len),"protocols":protocols})
                });
                let source = String::from_utf8_lossy(&profile.body);
                let secret = uuid::Uuid::new_v4().simple().to_string();
                let patched = xboard_core::profile::patch_mihomo(
                    &source,
                    "127.0.0.1:39091",
                    &secret,
                    39092,
                    xboard_core::profile::TunnelMode::Tun,
                );
                if let Some(patched) = record(
                    &mut report,
                    "tun_config_patch",
                    patched,
                    |_| json!({"patched":true}),
                ) {
                    record(
                        &mut report,
                        "privileged_config",
                        xboard_core::profile::privileged::prepare_privileged_config(
                            &patched,
                            "127.0.0.1:39093",
                            &uuid::Uuid::new_v4().simple().to_string(),
                        ),
                        |_| json!({"accepted":true}),
                    );
                }
            }
        }
        http.set_bearer(None);
    }
    println!("{}", serde_json::to_string_pretty(&report).unwrap());
}
