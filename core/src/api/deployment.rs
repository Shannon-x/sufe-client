//! Operator configuration. Remote configuration must be signed AND encrypted.
//! OSS cannot introduce a new transport policy or downgrade Stealth to plaintext.
use crate::error::{Result, XboardError};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use ed25519_dalek::{Signature, VerifyingKey};
use ring::aead;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct FeatureFlags {
    pub purchase: bool,
    pub recharge: bool,
    pub gift_card: bool,
    pub checkin: bool,
    pub notice: bool,
    pub invite: bool,
    pub tickets: bool,
    pub chatwoot: bool,
    pub custom_rules: bool,
}

impl Default for FeatureFlags {
    fn default() -> Self {
        Self {
            purchase: true,
            recharge: false,
            gift_card: true,
            checkin: false,
            notice: true,
            invite: true,
            tickets: true,
            chatwoot: false,
            custom_rules: true,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DeploymentDocument {
    pub project_id: String,
    #[serde(default = "default_brand")]
    pub brand_name: String,
    pub api_endpoints: Vec<String>,
    #[serde(default)]
    pub features: FeatureFlags,
    #[serde(default)]
    pub chatwoot_base_url: Option<String>,
    #[serde(default)]
    pub chatwoot_inbox_identifier: Option<String>,
    /// Unix seconds. Required and authenticated in remote documents.
    #[serde(default)]
    pub issued_at: i64,
    #[serde(default)]
    pub expires_at: i64,
}

fn default_brand() -> String {
    "SUFE".into()
}

#[derive(Debug, Clone, Serialize)]
pub struct ClientConfig {
    pub brand_name: String,
    pub features: FeatureFlags,
    pub chatwoot_base_url: Option<String>,
    pub chatwoot_inbox_identifier: Option<String>,
    pub config_source: String,
    pub transport_mode: String,
    pub api_candidates: usize,
}

impl DeploymentDocument {
    pub fn validate(&mut self) -> Result<()> {
        if self.project_id.trim().is_empty()
            || self.brand_name.trim().is_empty()
            || self.brand_name.len() > 80
        {
            return Err(XboardError::Config("品牌配置无效".into()));
        }
        if self.api_endpoints.is_empty() || self.api_endpoints.len() > 16 {
            return Err(XboardError::Config("每个配置需要 1–16 个 API 地址".into()));
        }
        for endpoint in &self.api_endpoints {
            validate_api_url(endpoint)?;
        }
        // These routes do not exist in Xboard-sh-1. Never advertise fake operations.
        self.features.recharge = false;
        self.features.checkin = false;
        if self.features.chatwoot {
            let base = self
                .chatwoot_base_url
                .as_deref()
                .ok_or_else(|| XboardError::Config("客服地址未配置".into()))?;
            validate_api_url(base)?;
            let inbox = self.chatwoot_inbox_identifier.as_deref().unwrap_or("");
            if inbox.is_empty()
                || !inbox
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
            {
                return Err(XboardError::Config(
                    "客服 Public API Inbox identifier 无效".into(),
                ));
            }
        }
        Ok(())
    }

    pub fn public_config(&self, source: &str, secure: bool) -> ClientConfig {
        ClientConfig {
            brand_name: self.brand_name.clone(),
            features: self.features.clone(),
            chatwoot_base_url: self.chatwoot_base_url.clone(),
            chatwoot_inbox_identifier: self.chatwoot_inbox_identifier.clone(),
            config_source: source.into(),
            transport_mode: if secure { "stealth-v1" } else { "https" }.into(),
            api_candidates: self.api_endpoints.len(),
        }
    }
}

pub fn validate_api_url(value: &str) -> Result<url::Url> {
    let parsed = validate_https_url(value)?;
    if parsed.path() != "/" || parsed.query().is_some() {
        return Err(XboardError::Config(
            "API 地址必须是 HTTPS 站点根地址".into(),
        ));
    }
    Ok(parsed)
}

pub fn validate_https_url(value: &str) -> Result<url::Url> {
    let parsed = url::Url::parse(value)?;
    let loopback = matches!(parsed.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
    let local_development = cfg!(debug_assertions) && loopback && parsed.scheme() == "http";
    if (parsed.scheme() != "https" && !local_development)
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.fragment().is_some()
    {
        return Err(XboardError::Config(
            "配置仅接受不含凭据的 HTTPS 地址".into(),
        ));
    }
    Ok(parsed)
}

/// Secret fields intentionally have no Debug or Serialize implementation.
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OssBootstrap {
    pub urls: Vec<String>,
    pub password: String,
    /// Standard base64 of the 32 byte Ed25519 public key. Never a private key.
    pub public_key: String,
}

impl std::fmt::Debug for OssBootstrap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("OssBootstrap([redacted])")
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EncryptedEnvelope {
    version: u32,
    nonce: String,
    ciphertext: String,
    signature: String,
}

impl OssBootstrap {
    pub fn decode(
        &self,
        body: &[u8],
        project_id: &str,
        now: i64,
        last_issued_at: i64,
    ) -> Result<DeploymentDocument> {
        if body.len() > 256 * 1024 || self.password.is_empty() {
            return Err(super::transport::crypto_error());
        }
        let envelope: EncryptedEnvelope = serde_json::from_slice(body)?;
        if envelope.version != 1 {
            return Err(super::transport::crypto_error());
        }
        let key: [u8; 32] = B64
            .decode(&self.public_key)
            .map_err(|_| super::transport::crypto_error())?
            .try_into()
            .map_err(|_| super::transport::crypto_error())?;
        let signature = B64
            .decode(&envelope.signature)
            .map_err(|_| super::transport::crypto_error())?;
        let signature =
            Signature::from_slice(&signature).map_err(|_| super::transport::crypto_error())?;
        let signed = format!(
            "sufe-bootstrap-v1\n{}\n{}",
            envelope.nonce, envelope.ciphertext
        );
        VerifyingKey::from_bytes(&key)
            .map_err(|_| super::transport::crypto_error())?
            .verify_strict(signed.as_bytes(), &signature)
            .map_err(|_| XboardError::InvalidSignature)?;
        let nonce: [u8; 12] = B64
            .decode(&envelope.nonce)
            .map_err(|_| super::transport::crypto_error())?
            .try_into()
            .map_err(|_| super::transport::crypto_error())?;
        let mut keys = super::transport::derive(
            &self.password,
            b"sufe-bootstrap-v1",
            b"sufe-bootstrap-config/v1",
        )?;
        let cipher = aead::LessSafeKey::new(
            aead::UnboundKey::new(&aead::AES_256_GCM, &keys[..32])
                .map_err(|_| super::transport::crypto_error())?,
        );
        keys.fill(0);
        let mut ciphertext = B64
            .decode(&envelope.ciphertext)
            .map_err(|_| super::transport::crypto_error())?;
        let plain = cipher
            .open_in_place(
                aead::Nonce::assume_unique_for_key(nonce),
                aead::Aad::from(project_id.as_bytes()),
                &mut ciphertext,
            )
            .map_err(|_| super::transport::crypto_error())?;
        let mut document: DeploymentDocument = serde_json::from_slice(plain)?;
        if document.project_id != project_id
            || document.issued_at < last_issued_at
            || document.issued_at > now + 300
            || document.expires_at <= now
            || document.expires_at <= document.issued_at
            || document.expires_at - document.issued_at > 90 * 86400
        {
            return Err(XboardError::Config("远程配置已过期或版本无效".into()));
        }
        document.validate()?;
        Ok(document)
    }

    pub async fn fetch(&self, project_id: &str, last_issued_at: i64) -> Result<DeploymentDocument> {
        if self.urls.is_empty() || self.urls.len() > 8 {
            return Err(XboardError::Config("OSS 地址数量须为 1–8".into()));
        }
        let http = reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(3))
            .timeout(std::time::Duration::from_secs(5))
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        // Race independent object stores, accepting only a verified document. No bearer is attached.
        use futures::{stream::FuturesUnordered, StreamExt};
        let mut pending = FuturesUnordered::new();
        for address in &self.urls {
            let url = validate_https_url(address)?;
            let http = http.clone();
            pending.push(async move {
                let response = http
                    .get(url)
                    .send()
                    .await
                    .map_err(|e| XboardError::Network(e.without_url()))?;
                if !response.status().is_success() {
                    return Err(super::transport::crypto_error());
                }
                let body = bounded_body(response, 256 * 1024).await?;
                self.decode(
                    &body,
                    project_id,
                    chrono::Utc::now().timestamp(),
                    last_issued_at,
                )
            });
        }
        let mut best: Option<DeploymentDocument> = None;
        while let Some(result) = pending.next().await {
            if let Ok(doc) = result {
                if best
                    .as_ref()
                    .map(|b| b.issued_at < doc.issued_at)
                    .unwrap_or(true)
                {
                    best = Some(doc);
                }
            }
        }
        best.ok_or_else(|| XboardError::Config("加密配置暂时不可达，已保留最近可信配置".into()))
    }
}

pub(crate) async fn bounded_body(response: reqwest::Response, limit: usize) -> Result<Vec<u8>> {
    use futures::StreamExt;
    if response
        .content_length()
        .map(|n| n > limit as u64)
        .unwrap_or(false)
    {
        return Err(XboardError::Config("响应超过允许大小".into()));
    }
    let mut bytes = Vec::new();
    let mut chunks = response.bytes_stream();
    while let Some(chunk) = chunks.next().await {
        let chunk = chunk.map_err(|e| XboardError::Network(e.without_url()))?;
        if bytes.len().saturating_add(chunk.len()) > limit {
            return Err(XboardError::Config("响应超过允许大小".into()));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_credential_leak_and_downgrade_urls() {
        for url in [
            "http://api.example",
            "https://user:pass@api.example",
            "file:///tmp/config",
            "https://api.example/#key",
            "https://api.example/api/v1",
        ] {
            assert!(validate_api_url(url).is_err(), "{url}");
        }
        assert!(validate_api_url("https://api.example").is_ok());
    }
    #[test]
    fn unsupported_routes_remain_disabled_even_if_operator_enables_them() {
        let mut doc: DeploymentDocument = serde_json::from_value(serde_json::json!({"project_id":"sufe", "api_endpoints":["https://api.example"], "features":{"recharge":true,"checkin":true}})).unwrap();
        doc.validate().unwrap();
        assert!(!doc.features.recharge && !doc.features.checkin);
        assert!(doc.features.gift_card && doc.features.purchase);
    }
    #[test]
    fn unsigned_or_malformed_config_is_never_accepted() {
        let source = OssBootstrap {
            urls: vec![],
            password: "test".into(),
            public_key: B64.encode([0u8; 32]),
        };
        assert!(source
            .decode(
                br#"{"project_id":"sufe","api_endpoints":["https://evil.example"]}"#,
                "sufe",
                1,
                0
            )
            .is_err());
        assert!(source
            .decode(
                br#"{"version":1,"nonce":"bad","ciphertext":"","signature":""}"#,
                "sufe",
                1,
                0
            )
            .is_err());
    }

    #[test]
    fn python_signed_encrypted_fixture_validates_expiry_project_signature_and_rollback() {
        // Fixed public test material generated by scripts/pack-client-config.py;
        // never use fixture credentials for a deployment.
        let source = OssBootstrap {
            urls: vec![],
            password: "fixture-only-password".into(),
            public_key: "A6EHv/POEL4dcN0Y50vAmWfk1jCbpQ1fHdyGZBJVMbg=".into(),
        };
        let bytes = include_bytes!("fixtures/bootstrap-v1.json");
        let doc = source.decode(bytes, "sufe-fixture", 1700000300, 0).unwrap();
        assert_eq!(doc.api_endpoints.len(), 2);
        assert!(!doc.features.recharge);
        assert!(source
            .decode(bytes, "other-project", 1700000300, 0)
            .is_err());
        assert!(source.decode(bytes, "sufe-fixture", 1700086401, 0).is_err());
        assert!(source
            .decode(bytes, "sufe-fixture", 1700000300, 1700000001)
            .is_err());
        let mut wrong = source.clone();
        wrong.password = "wrong".into();
        assert!(wrong.decode(bytes, "sufe-fixture", 1700000300, 0).is_err());
        let mut envelope: serde_json::Value = serde_json::from_slice(bytes).unwrap();
        envelope["ciphertext"] = serde_json::Value::String("A".repeat(100));
        assert!(matches!(
            source.decode(
                &serde_json::to_vec(&envelope).unwrap(),
                "sufe-fixture",
                1700000300,
                0
            ),
            Err(XboardError::InvalidSignature)
        ));
    }
}
