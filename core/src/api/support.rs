//! Chatwoot Client API adapter. Only configured HTTPS inboxes are reachable;
//! account/admin API tokens are never shipped to the app. The contact source
//! identifier is a session credential and is held in the OS keychain.
use crate::api::deployment::ClientConfig;
use crate::error::XboardError;
use crate::storage::SecureStore;
use once_cell::sync::Lazy;
use parking_lot::RwLock;
use serde_json::{json, Value};
use std::{collections::HashMap, sync::Arc, time::Duration};

static CONTACTS: Lazy<RwLock<HashMap<String, String>>> = Lazy::new(|| RwLock::new(HashMap::new()));
static REQUEST_LOCK: Lazy<tokio::sync::Mutex<()>> = Lazy::new(|| tokio::sync::Mutex::new(()));

fn source_for(key: &str, secure: Option<&Arc<dyn SecureStore>>) -> Option<String> {
    if let Some(source) = CONTACTS.read().get(key) {
        return Some(source.clone());
    }
    secure.and_then(|s| s.get(key).ok().flatten())
}
fn endpoint(base: &reqwest::Url, inbox: &str, tail: &[&str]) -> SupportResult<reqwest::Url> {
    let mut url = base.clone();
    let mut path = url
        .path_segments_mut()
        .map_err(|_| SupportError::new("support_config", "客服地址无效"))?;
    path.pop_if_empty();
    path.extend(["public", "api", "v1", "inboxes", inbox, "contacts"]);
    path.extend(tail.iter().copied());
    drop(path);
    Ok(url)
}
async fn send_request(
    http: &reqwest::Client,
    url: reqwest::Url,
    body: Option<Value>,
) -> SupportResult<Value> {
    let req = match body {
        Some(value) => http.post(url).json(&value),
        None => http.get(url),
    };
    let response = req
        .send()
        .await
        .map_err(|_| SupportError::new("support_network", "暂时无法连接客服，请检查网络后重试"))?;
    let status = response.status();
    if !status.is_success() {
        let message = match status.as_u16() {
            401 | 403 => "客服身份验证失败，请联系管理员核对 API 收件箱配置",
            404 => "客服会话不存在或已失效，请联系管理员核对收件箱配置",
            429 => "操作过于频繁，请稍后再试",
            _ => "客服服务暂时不可用，请稍后重试或提交工单",
        };
        return Err(SupportError::new("support_api", message).with_status(status.as_u16()));
    }
    if response.content_length().unwrap_or(0) > 4 * 1024 * 1024 {
        return Err(SupportError::new(
            "support_payload",
            "客服响应过大，请稍后重试",
        ));
    }
    let bytes = super::deployment::bounded_body(response, 4 * 1024 * 1024)
        .await
        .map_err(|_| SupportError::new("support_payload", "客服响应过大或无法读取"))?;
    serde_json::from_slice(&bytes)
        .map_err(|_| SupportError::new("support_payload", "客服返回了无法识别的数据"))
}

async fn perform(
    config: &ClientConfig,
    backend: &str,
    email: &str,
    secure: Option<Arc<dyn SecureStore>>,
    action: &str,
    conversation_id: Option<u64>,
    content: Option<String>,
) -> SupportResult<Value> {
    if !matches!(action, "restore" | "start" | "messages" | "send") {
        return Err(SupportError::new("support_action", "不支持的客服操作"));
    }
    if !config.features.chatwoot {
        return Err(SupportError::new(
            "support_disabled",
            "当前未开启在线客服，可通过工单联系客户支持",
        ));
    }
    let address = config
        .chatwoot_base_url
        .as_deref()
        .ok_or_else(|| SupportError::new("support_config", "尚未配置客服服务地址"))?;
    let inbox = config
        .chatwoot_inbox_identifier
        .as_deref()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| SupportError::new("support_config", "尚未配置 Chatwoot API 收件箱"))?;
    let base = reqwest::Url::parse(address)
        .map_err(|_| SupportError::new("support_config", "客服地址格式无效"))?;
    if base.scheme() != "https"
        || base.host_str().is_none()
        || !base.username().is_empty()
        || base.password().is_some()
        || base.query().is_some()
        || base.fragment().is_some()
    {
        return Err(SupportError::new(
            "support_config",
            "客服地址必须使用有效的 HTTPS 地址",
        ));
    }
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| SupportError::new("support_network", e.to_string()))?;
    let key = format!("chatwoot:{backend}:{}:{inbox}:{email}", base.as_str());
    // Serialise contact/conversation creation so repeated clicks cannot
    // create multiple contacts before the keychain write completes.
    let _guard = REQUEST_LOCK.lock().await;
    let mut source = source_for(&key, secure.as_ref());
    if action == "restore" && source.is_none() {
        return Ok(json!({"conversations": []}));
    }
    if source.is_none() {
        if action != "start" {
            return Err(SupportError::new("support_session", "请先开始客服会话"));
        }
        let contact = send_request(
            &http,
            endpoint(&base, inbox, &[])?,
            Some(json!({"email": email, "name": email.split('@').next().unwrap_or("客户")})),
        )
        .await?;
        let created = contact
            .get("source_id")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| SupportError::new("support_payload", "客服未返回有效的会话标识"))?
            .to_owned();
        CONTACTS.write().insert(key.clone(), created.clone());
        if let Some(store) = secure.as_ref() {
            let _ = store.put(&key, &created);
        }
        source = Some(created);
    }
    let source =
        source.ok_or_else(|| SupportError::new("support_session", "客服会话尚未初始化"))?;
    if action == "restore" {
        let conversations = send_request(
            &http,
            endpoint(&base, inbox, &[&source, "conversations"])?,
            None,
        )
        .await?;
        return Ok(json!({"conversations": conversations}));
    }
    if action == "start" {
        let existing = send_request(
            &http,
            endpoint(&base, inbox, &[&source, "conversations"])?,
            None,
        )
        .await?;
        if let Some(rows) = existing.as_array() {
            if let Some(open) = rows.iter().rev().find(|c| {
                c.get("status")
                    .and_then(Value::as_str)
                    .is_some_and(|s| s != "resolved")
            }) {
                return Ok(open.clone());
            }
        }
        return send_request(
            &http,
            endpoint(&base, inbox, &[&source, "conversations"])?,
            Some(json!({"custom_attributes": {"channel": "sufe-client"}})),
        )
        .await;
    }
    let id = conversation_id
        .filter(|id| *id > 0)
        .ok_or_else(|| SupportError::new("support_conversation", "客服会话不存在"))?
        .to_string();
    let url = endpoint(&base, inbox, &[&source, "conversations", &id, "messages"])?;
    if action == "messages" {
        return send_request(&http, url, None).await;
    }
    let text = content.unwrap_or_default().trim().to_string();
    if text.is_empty() || text.chars().count() > 4000 {
        return Err(SupportError::new(
            "support_message",
            "请输入 1 至 4000 字的消息",
        ));
    }
    send_request(&http, url, Some(json!({"content": text}))).await
}

/// Shared trusted Chatwoot Client API boundary. No caller-supplied URL or
/// administrator token is accepted. Contact source IDs remain in SecureStore.
pub async fn request(
    config: &ClientConfig,
    backend: &str,
    email: &str,
    secure: Option<Arc<dyn SecureStore>>,
    action: &str,
    conversation_id: Option<u64>,
    content: Option<String>,
) -> crate::Result<Value> {
    let mut response = perform(
        config,
        backend,
        email,
        secure,
        action,
        conversation_id,
        content,
    )
    .await
    .map_err(XboardError::from)?;
    redact_credentials(&mut response);
    Ok(response)
}

fn redact_credentials(value: &mut Value) {
    match value {
        Value::Object(map) => {
            map.remove("source_id");
            map.remove("pubsub_token");
            for item in map.values_mut() {
                redact_credentials(item);
            }
        }
        Value::Array(items) => {
            for item in items {
                redact_credentials(item);
            }
        }
        _ => {}
    }
}

type SupportResult<T> = std::result::Result<T, SupportError>;
#[derive(Debug)]
struct SupportError {
    status: u16,
    message: String,
}
impl SupportError {
    fn new(_kind: &str, message: impl Into<String>) -> Self {
        Self {
            status: 0,
            message: message.into(),
        }
    }
    fn with_status(mut self, status: u16) -> Self {
        self.status = status;
        self
    }
}
impl From<SupportError> for XboardError {
    fn from(error: SupportError) -> Self {
        if error.status == 0 {
            Self::Config(error.message)
        } else {
            Self::ApiFailure {
                status_code: error.status,
                message: error.message,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn contact_credentials_are_removed_recursively() {
        let mut value = json!({"id": 9, "contact_inbox": {"source_id":"secret", "pubsub_token":"token"}, "messages":[{"content":"hello","source_id":"also-secret"}]});
        redact_credentials(&mut value);
        assert_eq!(value["messages"][0]["content"], "hello");
        assert!(!value.to_string().contains("secret"));
        assert!(!value.to_string().contains("pubsub_token"));
    }
    #[test]
    fn untrusted_identifiers_cannot_escape_the_configured_origin() {
        let base = reqwest::Url::parse("https://support.example.invalid/").unwrap();
        let result = endpoint(&base, "inbox", &["bad/../source?token=x", "conversations"]).unwrap();
        assert_eq!(result.origin(), base.origin());
        assert!(result.path().contains("bad%2F..%2Fsource%3Ftoken=x"));
        assert!(result.query().is_none());
    }
}
