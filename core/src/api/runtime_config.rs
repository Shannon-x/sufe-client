//! Operator-owned build configuration. See docs/client-deployment.md.
use super::{
    deployment::{DeploymentDocument, FeatureFlags, OssBootstrap},
    HttpClient,
};
use crate::{Result, XboardError};
use serde::Deserialize;

pub const BACKEND_URL: &str = match option_env!("SUFE_BACKEND_URL") {
    Some(value) => {
        if value.is_empty() {
            "https://imitate.cnqq.de"
        } else {
            value
        }
    }
    None => "https://imitate.cnqq.de",
};
pub const DEFAULT_LOCALE: &str = "zh-CN";

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeConfig {
    pub deployment: DeploymentDocument,
    #[serde(default)]
    pub stealth_password: Option<String>,
    #[serde(default)]
    pub middleware_turnstile_site_key: Option<String>,
    #[serde(default)]
    pub oss: Option<OssBootstrap>,
}

impl std::fmt::Debug for RuntimeConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RuntimeConfig")
            .field("project_id", &self.deployment.project_id)
            .field("api_candidates", &self.deployment.api_endpoints.len())
            .field("stealth_configured", &self.stealth_password.is_some())
            .field("oss_configured", &self.oss.is_some())
            .finish_non_exhaustive()
    }
}

pub fn load() -> Result<RuntimeConfig> {
    let mut config = match option_env!("SUFE_DEPLOYMENT_JSON") {
        Some(json) if !json.trim().is_empty() => serde_json::from_str::<RuntimeConfig>(json)
            .map_err(|_| XboardError::Config("内嵌部署配置格式不正确".into()))?,
        _ => RuntimeConfig {
            deployment: DeploymentDocument {
                project_id: "sufe".into(),
                brand_name: "SUFE".into(),
                api_endpoints: vec![BACKEND_URL.into()],
                features: FeatureFlags::default(),
                chatwoot_base_url: None,
                chatwoot_inbox_identifier: None,
                issued_at: 0,
                expires_at: 0,
            },
            stealth_password: option_env!("SUFE_STEALTH_PASSWORD")
                .filter(|s| !s.is_empty())
                .map(str::to_string),
            middleware_turnstile_site_key: option_env!("SUFE_MIDDLEWARE_TURNSTILE_SITE_KEY")
                .filter(|s| !s.is_empty())
                .map(str::to_string),
            oss: None,
        },
    };
    config.deployment.validate()?;
    config.middleware_turnstile_site_key = config
        .middleware_turnstile_site_key
        .filter(|s| !s.trim().is_empty());
    Ok(config)
}

impl RuntimeConfig {
    pub fn create_http(&self, locale: &str) -> Result<HttpClient> {
        let client = HttpClient::with_endpoints(
            &self.deployment.api_endpoints,
            locale,
            self.stealth_password.as_deref(),
        )?;
        client.set_middleware_captcha(self.middleware_turnstile_site_key.is_some());
        Ok(client)
    }
}

/// Shared desktop/mobile refresh policy. Secrets stay inside the native core.
pub struct DeploymentRuntime {
    pub config: RuntimeConfig,
    current: parking_lot::RwLock<DeploymentDocument>,
    source: parking_lot::RwLock<&'static str>,
    refreshed: tokio::sync::Mutex<Option<std::time::Instant>>,
}
impl std::fmt::Debug for DeploymentRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeploymentRuntime")
            .field("public", &self.snapshot())
            .finish_non_exhaustive()
    }
}
impl DeploymentRuntime {
    pub fn new(config: RuntimeConfig) -> Self {
        Self {
            current: parking_lot::RwLock::new(config.deployment.clone()),
            config,
            source: parking_lot::RwLock::new("built_in"),
            refreshed: tokio::sync::Mutex::new(None),
        }
    }
    pub fn snapshot(&self) -> super::deployment::ClientConfig {
        self.current
            .read()
            .public_config(&self.source.read(), self.config.stealth_password.is_some())
    }
    pub async fn refresh(&self, http: &HttpClient) -> Result<()> {
        let mut refreshed = self.refreshed.lock().await;
        if refreshed
            .as_ref()
            .is_some_and(|t| t.elapsed() < std::time::Duration::from_secs(300))
        {
            return Ok(());
        }
        if let Some(oss) = &self.config.oss {
            let last = self.current.read().issued_at;
            match oss.fetch(&self.config.deployment.project_id, last).await {
                Ok(document) => {
                    http.set_endpoints(&document.api_endpoints)?;
                    *self.current.write() = document;
                    *self.source.write() = "encrypted_oss";
                }
                Err(_) => tracing::warn!(
                    "remote bootstrap unavailable; retaining trusted deployment configuration"
                ),
            }
        }
        *refreshed = Some(std::time::Instant::now());
        Ok(())
    }
    pub fn apply_captcha(&self, mut site: super::SiteConfig) -> Result<super::SiteConfig> {
        if let Some(key) = &self.config.middleware_turnstile_site_key {
            if site.is_captcha {
                return Err(XboardError::Config(
                    "中间件与后端不能重复校验同一个验证码，请在后端关闭重复验证".into(),
                ));
            }
            site.is_captcha = true;
            site.captcha_type = "turnstile".into();
            site.turnstile_site_key = key.clone();
        }
        Ok(site)
    }
}
