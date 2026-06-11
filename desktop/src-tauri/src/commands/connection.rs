//! Tauri commands for the "Connect" toggle and live kernel readouts.
//!
//! All heavy lifting lives in `xboard_core::KernelManager`; these wrappers
//! only translate the JS-side arguments and surface `CommandError`s.

use std::collections::{HashMap, HashSet};
use std::net::IpAddr;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Manager, State};
use tokio::net::lookup_host;
use tokio::time::{sleep, Duration};
use xboard_core::kernel::{
    ConnectionItem, ConnectionState, ProxyGroup, RuleItem, TrafficStats, TunnelMode,
};

use crate::error::{CommandError, CommandResult};
use crate::state::AppState;

#[derive(Debug, Clone, Serialize)]
pub struct NodeGeo {
    pub node: String,
    pub ip: String,
    pub country: String,
    pub city: String,
    pub lat: f64,
    pub lon: f64,
    pub isp: Option<String>,
    pub source: String,
}

#[tauri::command]
pub async fn connect(state: State<'_, AppState>, app: AppHandle) -> CommandResult<ConnectionState> {
    let auth = state
        .snapshot_auth()
        .ok_or_else(|| CommandError::new("unauthorized", "未登录").with_status(401))?;

    let client = state
        .snapshot_client()
        .ok_or_else(|| CommandError::new("not_initialized", "请先选择后端服务地址"))?;

    // Pull the *current* subscribe URL — server might have rotated the token.
    let subscribe = client.user_subscribe().await?;
    if subscribe.token != auth.subscribe_token {
        // Refresh our cached session token; the bearer is still valid.
        // Guard against a concurrent logout having cleared `auth` while we
        // awaited above — an `unwrap()` here would panic the command thread
        // and abort the process (panic=abort), leaking TUN/system-proxy
        // state. If the session is gone, just skip the cache update.
        if let Some(session) = state.auth.write().as_mut() {
            session.subscribe_token = subscribe.token.clone();
        }
    }

    let manager = state.ensure_kernel(&app)?;
    manager.connect(&subscribe.subscribe_url).await?;
    Ok(manager.state())
}

#[tauri::command]
pub async fn disconnect(state: State<'_, AppState>) -> CommandResult<ConnectionState> {
    let manager = state
        .kernel
        .get()
        .cloned()
        .ok_or_else(|| CommandError::new("kernel_not_running", "内核未启动"))?;
    manager.disconnect().await?;
    Ok(manager.state())
}

#[tauri::command]
pub fn connection_state(state: State<'_, AppState>) -> CommandResult<ConnectionState> {
    Ok(state
        .kernel
        .get()
        .map(|m| m.state())
        .unwrap_or(ConnectionState::Disconnected))
}

#[tauri::command]
pub fn set_tunnel_mode(state: State<'_, AppState>, mode: TunnelMode) -> CommandResult<()> {
    *state.requested_mode.write() = mode;
    if let Some(manager) = state.kernel.get() {
        manager.set_requested_mode(mode);
    }
    Ok(())
}

#[tauri::command]
pub async fn proxies(state: State<'_, AppState>) -> CommandResult<Vec<ProxyGroup>> {
    let manager = state
        .kernel
        .get()
        .cloned()
        .ok_or_else(|| CommandError::new("kernel_not_running", "内核未启动"))?;
    Ok(manager.proxies().await?)
}

#[tauri::command]
pub async fn select_proxy(
    state: State<'_, AppState>,
    group: String,
    name: String,
) -> CommandResult<()> {
    let manager = state
        .kernel
        .get()
        .cloned()
        .ok_or_else(|| CommandError::new("kernel_not_running", "内核未启动"))?;
    manager.select_proxy(&group, &name).await?;
    Ok(())
}

#[tauri::command]
pub async fn latency_test(state: State<'_, AppState>, name: String) -> CommandResult<u32> {
    let manager = state
        .kernel
        .get()
        .cloned()
        .ok_or_else(|| CommandError::new("kernel_not_running", "内核未启动"))?;
    Ok(manager.latency_test(&name).await?)
}

#[tauri::command]
pub async fn node_geo_test(
    state: State<'_, AppState>,
    group: String,
    name: String,
) -> CommandResult<NodeGeo> {
    let manager = state
        .kernel
        .get()
        .cloned()
        .ok_or_else(|| CommandError::new("kernel_not_running", "内核未启动"))?;

    let before = manager
        .proxies()
        .await?
        .into_iter()
        .find(|g| g.name == group)
        .and_then(|g| g.now);

    manager.select_proxy(&group, &name).await?;
    sleep(Duration::from_millis(350)).await;

    let result = lookup_geo_via_mixed_proxy(manager.mixed_port(), &name).await;

    if let Some(prev) = before.as_deref().filter(|prev| *prev != name) {
        let _ = manager.select_proxy(&group, prev).await;
    }

    result
}

async fn lookup_geo_via_mixed_proxy(mixed_port: u16, node: &str) -> CommandResult<NodeGeo> {
    let proxy = reqwest::Proxy::all(format!("http://127.0.0.1:{mixed_port}"))
        .map_err(|e| CommandError::new("geo_proxy", e.to_string()))?;
    let client = reqwest::Client::builder()
        .proxy(proxy)
        .timeout(Duration::from_secs(10))
        .user_agent("xboard-client/0.1 node-geo")
        .build()
        .map_err(|e| CommandError::new("geo_client", e.to_string()))?;

    let services = [
        ("ipwho.is", "https://ipwho.is/"),
        ("ipapi.co", "https://ipapi.co/json/"),
    ];
    let mut last_error = String::new();
    for (source, url) in services {
        match client.get(url).send().await {
            Ok(resp) if resp.status().is_success() => match resp.json::<Value>().await {
                Ok(json) => match parse_geo_json(node, source, &json) {
                    Ok(geo) => return Ok(geo),
                    Err(e) => last_error = e.message,
                },
                Err(e) => last_error = e.to_string(),
            },
            Ok(resp) => last_error = format!("{source} returned {}", resp.status()),
            Err(e) => last_error = e.to_string(),
        }
    }

    Err(CommandError::new(
        "geo_lookup_failed",
        format!("节点出口 IP 定位失败：{last_error}"),
    ))
}

fn parse_geo_json(node: &str, source: &str, json: &Value) -> CommandResult<NodeGeo> {
    let ip = string_field(json, &["ip", "query"])
        .ok_or_else(|| CommandError::new("geo_parse", "定位结果缺少 IP"))?;
    let lat = number_field(json, &["latitude", "lat"])
        .ok_or_else(|| CommandError::new("geo_parse", "定位结果缺少纬度"))?;
    let lon = number_field(json, &["longitude", "lon"])
        .ok_or_else(|| CommandError::new("geo_parse", "定位结果缺少经度"))?;
    let country = string_field(json, &["country", "country_name"]).unwrap_or_default();
    let city = string_field(json, &["city"]).unwrap_or_default();
    let isp = string_field(json, &["connection.isp", "org", "isp"]);

    Ok(NodeGeo {
        node: node.to_string(),
        ip,
        country,
        city,
        lat,
        lon,
        isp,
        source: source.to_string(),
    })
}

fn string_field(json: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .filter_map(|key| {
            let value = dotted_value(json, key)?;
            value.as_str().map(str::trim).filter(|s| !s.is_empty())
        })
        .map(ToOwned::to_owned)
        .next()
}

fn number_field(json: &Value, keys: &[&str]) -> Option<f64> {
    keys.iter().find_map(|key| dotted_value(json, key)?.as_f64())
}

fn dotted_value<'a>(json: &'a Value, key: &str) -> Option<&'a Value> {
    key.split('.').try_fold(json, |current, part| current.get(part))
}

#[tauri::command]
pub async fn current_traffic(state: State<'_, AppState>) -> CommandResult<TrafficStats> {
    let manager = state
        .kernel
        .get()
        .cloned()
        .ok_or_else(|| CommandError::new("kernel_not_running", "内核未启动"))?;
    Ok(manager.current_traffic().await?)
}

/// Batch-resolve geo for many nodes at once.
///
/// Reads the live mihomo `config.yaml` (the patched profile written by core),
/// extracts each `proxies[]` entry's `server`, resolves hostnames to IPs in
/// parallel, deduplicates, and asks ip-api.com's free batch endpoint for the
/// geo of each unique IP. The lookup is direct (not through the tunnel) — we
/// want the *server's* coordinates, not what the server's outbound IP looks
/// like to the world.
///
/// Returns a `node_name -> NodeGeo` map. Nodes whose server we can't resolve
/// or geo are simply omitted; the UI degrades to name-based aliases for those.
#[tauri::command]
pub async fn resolve_node_geo_batch(app: AppHandle) -> CommandResult<HashMap<String, NodeGeo>> {
    let work_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| CommandError::new("app_data_dir", e.to_string()))?
        .join("kernel");
    let cfg_path = work_dir.join("config.yaml");

    let yaml_text = match tokio::fs::read_to_string(&cfg_path).await {
        Ok(text) => text,
        Err(_) => {
            // No live config yet (kernel never started). Treat as empty result —
            // the UI falls back to name-based geo aliases.
            return Ok(HashMap::new());
        }
    };

    let proxies = extract_proxy_servers(&yaml_text)?;
    if proxies.is_empty() {
        return Ok(HashMap::new());
    }

    // Resolve hostnames → IPs. We cap to ~8 concurrent lookups so a flaky
    // resolver doesn't fan out into thousands of in-flight DNS queries.
    let mut node_to_ip: HashMap<String, IpAddr> = HashMap::new();
    let mut unique_ips: HashSet<IpAddr> = HashSet::new();
    for (name, host) in &proxies {
        if let Some(ip) = resolve_host(host).await {
            node_to_ip.insert(name.clone(), ip);
            unique_ips.insert(ip);
        }
    }

    if unique_ips.is_empty() {
        return Ok(HashMap::new());
    }

    let ip_to_geo = batch_lookup_geo(&unique_ips).await;

    let mut out = HashMap::new();
    for (node, ip) in node_to_ip {
        if let Some(geo) = ip_to_geo.get(&ip) {
            let mut geo = geo.clone();
            geo.node = node.clone();
            out.insert(node, geo);
        }
    }
    Ok(out)
}

/// Pull `proxies[].name` + `proxies[].server` out of a mihomo YAML config.
/// Ignores entries without both fields. Returns `(name, server_host)` pairs.
fn extract_proxy_servers(yaml: &str) -> CommandResult<Vec<(String, String)>> {
    let doc: serde_yaml::Value = serde_yaml::from_str(yaml)
        .map_err(|e| CommandError::new("yaml_parse", format!("config.yaml: {e}")))?;
    let proxies = match doc.get("proxies").and_then(|v| v.as_sequence()) {
        Some(seq) => seq,
        None => return Ok(Vec::new()),
    };
    let mut out = Vec::with_capacity(proxies.len());
    for entry in proxies {
        let Some(map) = entry.as_mapping() else {
            continue;
        };
        let name = map
            .get(serde_yaml::Value::String("name".into()))
            .and_then(|v| v.as_str());
        let server = map
            .get(serde_yaml::Value::String("server".into()))
            .and_then(|v| v.as_str());
        if let (Some(n), Some(s)) = (name, server) {
            if !n.is_empty() && !s.is_empty() {
                out.push((n.to_string(), s.to_string()));
            }
        }
    }
    Ok(out)
}

async fn resolve_host(host: &str) -> Option<IpAddr> {
    if let Ok(ip) = host.parse::<IpAddr>() {
        return Some(ip);
    }
    // Use port 0 so the resolver does an A/AAAA query without bothering with
    // service mapping. Prefer the first IPv4 we see — ip-api.com's free tier
    // is IPv4-only, and most server addresses end up resolving to v4 anyway.
    let probe = format!("{host}:0");
    let addrs = lookup_host(&probe).await.ok()?;
    let mut first: Option<IpAddr> = None;
    for sock in addrs {
        let ip = sock.ip();
        if ip.is_ipv4() {
            return Some(ip);
        }
        first.get_or_insert(ip);
    }
    first
}

#[derive(Debug, Deserialize)]
struct IpApiBatchRow {
    status: Option<String>,
    query: Option<String>,
    country: Option<String>,
    #[serde(rename = "countryCode")]
    country_code: Option<String>,
    city: Option<String>,
    lat: Option<f64>,
    lon: Option<f64>,
    isp: Option<String>,
    org: Option<String>,
}

async fn batch_lookup_geo(ips: &HashSet<IpAddr>) -> HashMap<IpAddr, NodeGeo> {
    let mut out = HashMap::new();
    if ips.is_empty() {
        return out;
    }
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .user_agent("xboard-client/0.1 geo-batch")
        .build()
    {
        Ok(c) => c,
        Err(_) => return out,
    };

    // ip-api free batch endpoint accepts up to 100 IPs per call, 15 calls/min
    // unlimited burst budget. Chunk to be safe.
    let ip_vec: Vec<IpAddr> = ips.iter().copied().collect();
    for chunk in ip_vec.chunks(100) {
        let payload: Vec<serde_json::Value> = chunk
            .iter()
            .map(|ip| serde_json::json!({ "query": ip.to_string() }))
            .collect();
        let url = "https://ip-api.com/batch?fields=status,country,countryCode,city,lat,lon,isp,org,query";
        let resp = match client.post(url).json(&payload).send().await {
            Ok(r) if r.status().is_success() => r,
            _ => continue,
        };
        let rows: Vec<IpApiBatchRow> = match resp.json().await {
            Ok(v) => v,
            Err(_) => continue,
        };
        for row in rows {
            if row.status.as_deref() != Some("success") {
                continue;
            }
            let Some(query) = row.query.as_deref() else { continue };
            let Ok(ip) = query.parse::<IpAddr>() else { continue };
            let (Some(lat), Some(lon)) = (row.lat, row.lon) else { continue };
            out.insert(
                ip,
                NodeGeo {
                    node: String::new(), // filled in by caller per node
                    ip: query.to_string(),
                    country: row.country.unwrap_or_else(|| {
                        row.country_code.clone().unwrap_or_default()
                    }),
                    city: row.city.unwrap_or_default(),
                    lat,
                    lon,
                    isp: row.isp.or(row.org),
                    source: "ip-api.com".into(),
                },
            );
        }
    }
    out
}

// ─── Observability: connections / rules ─────────────────────────────────

/// Snapshot of active connections (for the Connections monitor page).
#[tauri::command]
pub async fn connections(state: State<'_, AppState>) -> CommandResult<Vec<ConnectionItem>> {
    let manager = state
        .kernel
        .get()
        .cloned()
        .ok_or_else(|| CommandError::new("kernel_not_running", "内核未启动"))?;
    Ok(manager.connections().await?)
}

#[tauri::command]
pub async fn close_connection(state: State<'_, AppState>, id: String) -> CommandResult<()> {
    let manager = state
        .kernel
        .get()
        .cloned()
        .ok_or_else(|| CommandError::new("kernel_not_running", "内核未启动"))?;
    manager.close_connection(&id).await?;
    Ok(())
}

#[tauri::command]
pub async fn close_all_connections(state: State<'_, AppState>) -> CommandResult<()> {
    let manager = state
        .kernel
        .get()
        .cloned()
        .ok_or_else(|| CommandError::new("kernel_not_running", "内核未启动"))?;
    manager.close_all_connections().await?;
    Ok(())
}

/// The kernel's active routing rules (for the Rules page).
#[tauri::command]
pub async fn rules(state: State<'_, AppState>) -> CommandResult<Vec<RuleItem>> {
    let manager = state
        .kernel
        .get()
        .cloned()
        .ok_or_else(|| CommandError::new("kernel_not_running", "内核未启动"))?;
    Ok(manager.rules().await?)
}

// ─── Reliability: reconnect + sysproxy guard toggle ─────────────────────

/// Re-establish the connection with the last subscribe URL — used by the
/// frontend's bounded auto-reconnect after a `connection://kernel-failure`.
#[tauri::command]
pub async fn reconnect(state: State<'_, AppState>) -> CommandResult<ConnectionState> {
    let manager = state
        .kernel
        .get()
        .cloned()
        .ok_or_else(|| CommandError::new("kernel_not_running", "内核未启动"))?;
    manager.reconnect().await?;
    Ok(manager.state())
}

#[tauri::command]
pub fn set_proxy_guard_enabled(state: State<'_, AppState>, enabled: bool) -> CommandResult<()> {
    if let Some(manager) = state.kernel.get() {
        manager.set_proxy_guard_enabled(enabled);
    }
    // Cache so the choice applies even if the kernel isn't built yet.
    *state.proxy_guard_enabled.write() = Some(enabled);
    Ok(())
}

#[tauri::command]
pub fn proxy_guard_enabled(state: State<'_, AppState>) -> CommandResult<bool> {
    Ok(state
        .kernel
        .get()
        .map(|m| m.proxy_guard_enabled())
        .unwrap_or_else(|| state.proxy_guard_enabled.read().unwrap_or(true)))
}
