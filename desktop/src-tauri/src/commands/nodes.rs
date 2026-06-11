//! Subscribe-only node preview.
//!
//! Lets the UI render the full node list before the kernel has ever run —
//! the X-Tunnel-style "open box, see countries" experience. Pulls the user's
//! subscribe URL via the existing `HttpClient`, asks the backend for the
//! Clash flavour, and walks `proxies[]` for `{ name, type, server, port }`.
//! No kernel state is touched; the live `proxies` command keeps owning the
//! kernel-bound view.

use serde::Serialize;
use tauri::State;
use xboard_core::kernel::KernelKind;
use xboard_core::XboardError;

use crate::error::{CommandError, CommandResult};
use crate::state::AppState;

#[derive(Debug, Clone, Serialize)]
pub struct NodePreview {
    pub name: String,
    /// Lowercase protocol family as it appears in the Clash YAML
    /// (`vless`, `vmess`, `trojan`, `ss`, `hysteria2`, ...).
    pub kind: String,
    pub server: String,
    pub port: u16,
}

#[tauri::command]
pub async fn preview_subscribe_nodes(
    state: State<'_, AppState>,
) -> CommandResult<Vec<NodePreview>> {
    let auth = state
        .snapshot_auth()
        .ok_or_else(|| CommandError::new("unauthorized", "未登录").with_status(401))?;

    let client = state
        .snapshot_client()
        .ok_or_else(|| CommandError::new("not_initialized", "请先选择后端服务地址"))?;

    // Re-issue the subscribe lookup — the user may have just registered and
    // we want the freshest token even if `state.auth.subscribe_token` is
    // stale. Falls back to the cached token if the API errors out.
    let subscribe_url = match client.user_subscribe().await {
        Ok(s) => s.subscribe_url,
        Err(_) => {
            return Err(CommandError::new(
                "subscribe_lookup_failed",
                "获取订阅链接失败，稍后重试",
            ));
        }
    };
    let _ = &auth; // keep the unauthorized guard above without clippy whining

    let fetch = match client
        .fetch_subscribe(&subscribe_url, KernelKind::Mihomo.flag(), None)
        .await
    {
        Ok(f) => f,
        // Expired / suspended / out-of-traffic account: tell the UI so the
        // node rail can show a "renew your plan" affordance instead of a
        // generic empty state.
        Err(XboardError::SubscriptionUnavailable { status }) => {
            return Err(CommandError::new(
                "subscription_unavailable",
                "订阅当前不可用——套餐可能已到期或流量耗尽，请续费后重试",
            )
            .with_status(status));
        }
        Err(e) => return Err(CommandError::new("subscribe_fetch", e.to_string())),
    };

    if fetch.body.is_empty() {
        return Ok(Vec::new());
    }

    Ok(parse_clash_proxies(&fetch.body))
}

fn parse_clash_proxies(body: &[u8]) -> Vec<NodePreview> {
    // The Xboard backend's Clash flavour is YAML with a top-level
    // `proxies:` sequence. Bail quietly on anything that isn't —
    // base64 / Surge / etc. flavours would need their own parsers
    // and we can layer them on later.
    let Ok(doc) = serde_yaml::from_slice::<serde_yaml::Value>(body) else {
        return Vec::new();
    };
    let Some(seq) = doc.get("proxies").and_then(|v| v.as_sequence()) else {
        return Vec::new();
    };

    let mut out = Vec::with_capacity(seq.len());
    for entry in seq {
        let Some(map) = entry.as_mapping() else { continue };
        let name = str_field(map, "name");
        let server = str_field(map, "server");
        let kind = str_field(map, "type")
            .map(|s| s.to_ascii_lowercase())
            .unwrap_or_default();
        let port = map
            .get(serde_yaml::Value::String("port".into()))
            .and_then(|v| v.as_u64())
            .and_then(|p| u16::try_from(p).ok())
            .unwrap_or(0);

        let (Some(name), Some(server)) = (name, server) else { continue };
        if name.is_empty() || server.is_empty() || kind.is_empty() {
            continue;
        }
        out.push(NodePreview { name, kind, server, port });
    }
    out
}

fn str_field(map: &serde_yaml::Mapping, key: &str) -> Option<String> {
    map.get(serde_yaml::Value::String(key.into()))
        .and_then(|v| v.as_str())
        .map(str::to_string)
}
