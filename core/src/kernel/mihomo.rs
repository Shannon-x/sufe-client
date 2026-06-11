//! Driver for [mihomo](https://github.com/MetaCubeX/mihomo) (Clash.Meta).
//!
//! After the launcher refactor the driver is **attach-only**: it talks to
//! mihomo's External Controller HTTP API on `controller_addr` but does not
//! supervise the subprocess. The
//! [`crate::kernel::launcher::KernelLauncher`] owns the `Child`; the
//! manager calls `launcher.spawn(spec)` first and then `driver.start(cfg)`
//! to point this driver at the freshly-up controller.

use std::collections::HashMap;
use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use futures::stream::{BoxStream, StreamExt};
use parking_lot::{Mutex, RwLock};
use reqwest::{Client, RequestBuilder};
use serde::Deserialize;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tokio_stream::wrappers::BroadcastStream;
use tokio_util::io::StreamReader;

use super::driver::{
    ConnectionItem, KernelConfig, KernelDriver, KernelKind, LogLine, ProxyGroup, RuleItem,
    TrafficStats,
};
use crate::error::{Result, XboardError};

const LOG_CHANNEL_CAPACITY: usize = 256;

#[derive(Debug)]
pub struct MihomoDriver {
    controller_addr: RwLock<String>,
    secret: RwLock<Option<String>>,
    attached: RwLock<bool>,
    http: Client,
    log_tx: broadcast::Sender<LogLine>,
    log_task: Mutex<Option<JoinHandle<()>>>,
}

impl Default for MihomoDriver {
    fn default() -> Self {
        Self::new()
    }
}

impl MihomoDriver {
    pub fn new() -> Self {
        let (log_tx, _) = broadcast::channel(LOG_CHANNEL_CAPACITY);
        Self {
            controller_addr: RwLock::new("127.0.0.1:9090".to_string()),
            secret: RwLock::new(None),
            attached: RwLock::new(false),
            // No global timeout: /traffic and /logs are long-lived chunked
            // streams. Per-call timeouts are applied in the methods that
            // use this client.
            http: Client::builder().build().expect("reqwest client"),
            log_tx,
            log_task: Mutex::new(None),
        }
    }

    fn controller_url(&self, path: &str) -> String {
        format!("http://{}{}", self.controller_addr.read(), path)
    }

    fn auth_header(&self) -> Option<String> {
        self.secret.read().as_ref().map(|s| format!("Bearer {}", s))
    }

    fn with_auth(&self, mut req: RequestBuilder) -> RequestBuilder {
        if let Some(h) = self.auth_header() {
            req = req.header("Authorization", h);
        }
        req
    }

    /// Background task: long-poll mihomo `/logs`, deserialise each JSON line,
    /// fan out via the broadcast channel. Aborted by `stop()`.
    fn spawn_log_task(&self) {
        let mut guard = self.log_task.lock();
        if guard.as_ref().map(|h| !h.is_finished()).unwrap_or(false) {
            return;
        }

        let url = self.controller_url("/logs?level=info");
        let auth = self.auth_header();
        let http = self.http.clone();
        let tx = self.log_tx.clone();

        let handle = tokio::spawn(async move {
            loop {
                let mut req = http.get(&url);
                if let Some(h) = &auth {
                    req = req.header("Authorization", h.clone());
                }
                let resp = match req.send().await.and_then(|r| r.error_for_status()) {
                    Ok(r) => r,
                    Err(_) => {
                        // Kernel might still be coming up; wait and retry.
                        tokio::time::sleep(Duration::from_millis(500)).await;
                        continue;
                    }
                };

                let stream = resp
                    .bytes_stream()
                    .map(|chunk| chunk.map_err(std::io::Error::other));
                let reader = BufReader::new(StreamReader::new(stream));
                let mut lines = reader.lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    let line = line.trim();
                    if line.is_empty() {
                        continue;
                    }
                    if let Ok(raw) = serde_json::from_str::<RawLogLine>(line) {
                        let entry = LogLine {
                            level: raw.r#type.unwrap_or_else(|| "info".into()),
                            message: raw.payload.unwrap_or_default(),
                            at: Utc::now(),
                        };
                        let _ = tx.send(entry);
                    }
                }
                // EOF or error → reconnect after a short backoff.
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
        });

        *guard = Some(handle);
    }

    fn abort_log_task(&self) {
        if let Some(h) = self.log_task.lock().take() {
            h.abort();
        }
    }
}

#[derive(Debug, Deserialize)]
struct RawLogLine {
    /// mihomo uses `type` for the level ("info"/"warning"/"error"/...).
    r#type: Option<String>,
    payload: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawProxiesResp {
    proxies: HashMap<String, RawProxy>,
}

#[derive(Debug, Deserialize)]
struct RawProxy {
    /// "Selector" / "URLTest" / "Fallback" / "LoadBalance" / "Relay" mark a group.
    /// Plain proxies have types like "Trojan" / "Vmess" / "Direct" / "Reject".
    #[serde(rename = "type")]
    kind: String,
    now: Option<String>,
    #[serde(default)]
    all: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct RawDelayResp {
    delay: u32,
}

#[derive(Debug, Deserialize)]
struct RawTrafficLine {
    up: u64,
    down: u64,
}

#[derive(Debug, Deserialize)]
struct RawConnectionsResp {
    #[serde(rename = "downloadTotal", default)]
    download_total: u64,
    #[serde(rename = "uploadTotal", default)]
    upload_total: u64,
}

/// Full `/connections` payload including the live connection list.
#[derive(Debug, Deserialize)]
struct RawConnectionsList {
    #[serde(default)]
    connections: Vec<RawConnection>,
}

#[derive(Debug, Deserialize)]
struct RawConnection {
    #[serde(default)]
    id: String,
    #[serde(default)]
    upload: u64,
    #[serde(default)]
    download: u64,
    #[serde(default)]
    start: String,
    #[serde(default)]
    chains: Vec<String>,
    #[serde(default)]
    rule: String,
    #[serde(rename = "rulePayload", default)]
    rule_payload: String,
    #[serde(default)]
    metadata: RawConnMeta,
}

#[derive(Debug, Default, Deserialize)]
struct RawConnMeta {
    #[serde(default)]
    network: String,
    #[serde(rename = "type", default)]
    conn_type: String,
    #[serde(default)]
    host: String,
    #[serde(rename = "sourceIP", default)]
    source_ip: String,
    #[serde(rename = "destinationIP", default)]
    destination_ip: String,
    #[serde(rename = "destinationPort", default)]
    destination_port: String,
    #[serde(default)]
    process: String,
    #[serde(rename = "processPath", default)]
    process_path: String,
}

#[derive(Debug, Deserialize)]
struct RawRulesResp {
    #[serde(default)]
    rules: Vec<RawRule>,
}

#[derive(Debug, Deserialize)]
struct RawRule {
    #[serde(rename = "type", default)]
    kind: String,
    #[serde(default)]
    payload: String,
    #[serde(default)]
    proxy: String,
}

fn is_group_kind(kind: &str) -> bool {
    matches!(
        kind,
        "Selector" | "URLTest" | "Fallback" | "LoadBalance" | "Relay"
    )
}

#[async_trait]
impl KernelDriver for MihomoDriver {
    fn kind(&self) -> KernelKind {
        KernelKind::Mihomo
    }

    async fn version(&self) -> Result<String> {
        if !self.is_running().await {
            return Err(XboardError::KernelNotRunning);
        }
        let req = self
            .with_auth(self.http.get(self.controller_url("/version")))
            .timeout(Duration::from_secs(2));
        let resp: serde_json::Value = req.send().await?.error_for_status()?.json().await?;
        Ok(resp
            .get("version")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string())
    }

    async fn start(&self, cfg: &KernelConfig) -> Result<()> {
        let KernelConfig::Mihomo {
            controller_addr,
            controller_secret,
        } = cfg
        else {
            return Err(XboardError::Config(
                "mihomo driver got non-mihomo config".into(),
            ));
        };

        // Re-attach: tear down log task before retargeting.
        if *self.attached.read() {
            self.abort_log_task();
        }

        *self.controller_addr.write() = controller_addr.clone();
        *self.secret.write() = Some(controller_secret.clone());

        // Sanity probe — the launcher already waited for /version, but a
        // race against svc/helper restart isn't impossible. One quick hit
        // confirms our auth header is correct before we start streaming.
        let probe = self
            .with_auth(self.http.get(self.controller_url("/version")))
            .timeout(Duration::from_secs(2))
            .send()
            .await
            .and_then(|r| r.error_for_status());
        if let Err(e) = probe {
            *self.secret.write() = None;
            return Err(XboardError::Kernel(format!("controller probe failed: {e}")));
        }

        *self.attached.write() = true;
        self.spawn_log_task();
        Ok(())
    }

    async fn stop(&self) -> Result<()> {
        self.abort_log_task();
        *self.attached.write() = false;
        *self.secret.write() = None;
        Ok(())
    }

    async fn reload(&self, cfg: &KernelConfig) -> Result<()> {
        // Detach and re-attach. Real "hot reload" of the YAML happens at
        // the launcher layer (write new file → mihomo SIGHUP) — this only
        // covers the driver-side rebind.
        self.start(cfg).await
    }

    async fn is_running(&self) -> bool {
        *self.attached.read()
    }

    async fn proxies(&self) -> Result<Vec<ProxyGroup>> {
        if !self.is_running().await {
            return Err(XboardError::KernelNotRunning);
        }
        let req = self
            .with_auth(self.http.get(self.controller_url("/proxies")))
            .timeout(Duration::from_secs(3));
        let resp: RawProxiesResp = req.send().await?.error_for_status()?.json().await?;

        let mut groups: Vec<ProxyGroup> = resp
            .proxies
            .into_iter()
            .filter_map(|(name, raw)| {
                if !is_group_kind(&raw.kind) {
                    return None;
                }
                Some(ProxyGroup {
                    name,
                    kind: raw.kind,
                    now: raw.now,
                    all: raw.all,
                })
            })
            .collect();
        groups.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(groups)
    }

    async fn select_proxy(&self, group: &str, name: &str) -> Result<()> {
        if !self.is_running().await {
            return Err(XboardError::KernelNotRunning);
        }
        let url = self.controller_url(&format!("/proxies/{}", urlencoding(group)));
        let body = serde_json::json!({ "name": name });
        let resp = self
            .with_auth(self.http.put(url).json(&body))
            .timeout(Duration::from_secs(3))
            .send()
            .await?;
        let status = resp.status();
        if status.is_success() {
            Ok(())
        } else {
            let text = resp.text().await.unwrap_or_default();
            Err(XboardError::Kernel(format!(
                "select_proxy({group} → {name}) failed: {status} {text}"
            )))
        }
    }

    async fn latency_test(&self, name: &str, url: &str, timeout_ms: u32) -> Result<u32> {
        if !self.is_running().await {
            return Err(XboardError::KernelNotRunning);
        }
        let path = format!(
            "/proxies/{}/delay?url={}&timeout={}",
            urlencoding(name),
            urlencoding(url),
            timeout_ms
        );
        let req = self
            .with_auth(self.http.get(self.controller_url(&path)))
            // Give mihomo a little headroom over the requested timeout so the
            // kernel-reported timeout (504) wins over our HTTP timeout.
            .timeout(Duration::from_millis(timeout_ms as u64 + 1_000));
        let resp = req.send().await?;
        let status = resp.status();
        if status.is_success() {
            let body: RawDelayResp = resp.json().await?;
            Ok(body.delay)
        } else if status.as_u16() == 408 || status.as_u16() == 504 {
            // Kernel reports timeout — that's a result, not an error.
            Ok(u32::MAX)
        } else {
            let text = resp.text().await.unwrap_or_default();
            Err(XboardError::Kernel(format!(
                "latency_test({name}) failed: {status} {text}"
            )))
        }
    }

    async fn traffic(&self) -> Result<TrafficStats> {
        if !self.is_running().await {
            return Err(XboardError::KernelNotRunning);
        }

        let totals: RawConnectionsResp = self
            .with_auth(self.http.get(self.controller_url("/connections")))
            .timeout(Duration::from_secs(2))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        // Read the first chunked frame off /traffic, then drop the response.
        let resp = self
            .with_auth(self.http.get(self.controller_url("/traffic")))
            .send()
            .await?
            .error_for_status()?;
        let stream = resp
            .bytes_stream()
            .map(|chunk| chunk.map_err(std::io::Error::other));
        let mut reader = BufReader::new(StreamReader::new(stream));
        let mut buf = String::new();

        let read_first_line = async {
            // mihomo emits one JSON object per line, terminated by "\n".
            // Loop in case the kernel sends a heartbeat empty line first.
            loop {
                buf.clear();
                let n = reader.read_line(&mut buf).await?;
                if n == 0 {
                    return Err(XboardError::Kernel(
                        "traffic stream closed before first frame".into(),
                    ));
                }
                let line = buf.trim();
                if !line.is_empty() {
                    return serde_json::from_str::<RawTrafficLine>(line).map_err(XboardError::from);
                }
            }
        };

        let frame = tokio::time::timeout(Duration::from_secs(2), read_first_line)
            .await
            .map_err(|_| XboardError::Kernel("traffic snapshot timed out".into()))??;

        Ok(TrafficStats {
            up: frame.up,
            down: frame.down,
            up_total: totals.upload_total,
            down_total: totals.download_total,
        })
    }

    fn log_stream(&self) -> BoxStream<'static, LogLine> {
        // Multiple subscribers each get a fresh receiver from the broadcast
        // channel populated by `spawn_log_task` (started in `start`). Any
        // slow consumer that lags out is silently dropped; the BroadcastStream
        // adapter filters those errors away.
        let rx = self.log_tx.subscribe();
        BroadcastStream::new(rx)
            .filter_map(|res| async move { res.ok() })
            .boxed()
    }

    async fn connections(&self) -> Result<Vec<ConnectionItem>> {
        if !self.is_running().await {
            return Err(XboardError::KernelNotRunning);
        }
        let resp: RawConnectionsList = self
            .with_auth(self.http.get(self.controller_url("/connections")))
            .timeout(Duration::from_secs(3))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        Ok(resp
            .connections
            .into_iter()
            .map(|c| {
                let m = c.metadata;
                // Prefer the SNI/host; fall back to the destination IP so the
                // row is never blank.
                let host = if !m.host.is_empty() {
                    m.host.clone()
                } else {
                    m.destination_ip.clone()
                };
                // `process` is usually the basename already; if only a path
                // came back, take its last component.
                let process = if !m.process.is_empty() {
                    m.process
                } else {
                    m.process_path
                        .rsplit(['/', '\\'])
                        .next()
                        .unwrap_or_default()
                        .to_string()
                };
                ConnectionItem {
                    id: c.id,
                    upload: c.upload,
                    download: c.download,
                    start: c.start,
                    chains: c.chains,
                    rule: c.rule,
                    rule_payload: c.rule_payload,
                    network: m.network,
                    conn_type: m.conn_type,
                    host,
                    source_ip: m.source_ip,
                    destination_ip: m.destination_ip,
                    destination_port: m.destination_port,
                    process,
                }
            })
            .collect())
    }

    async fn close_connection(&self, id: &str) -> Result<()> {
        if !self.is_running().await {
            return Err(XboardError::KernelNotRunning);
        }
        self.with_auth(
            self.http
                .delete(self.controller_url(&format!("/connections/{}", urlencoding(id)))),
        )
        .timeout(Duration::from_secs(3))
        .send()
        .await?
        .error_for_status()?;
        Ok(())
    }

    async fn close_all_connections(&self) -> Result<()> {
        if !self.is_running().await {
            return Err(XboardError::KernelNotRunning);
        }
        self.with_auth(self.http.delete(self.controller_url("/connections")))
            .timeout(Duration::from_secs(3))
            .send()
            .await?
            .error_for_status()?;
        Ok(())
    }

    async fn rules(&self) -> Result<Vec<RuleItem>> {
        if !self.is_running().await {
            return Err(XboardError::KernelNotRunning);
        }
        let resp: RawRulesResp = self
            .with_auth(self.http.get(self.controller_url("/rules")))
            .timeout(Duration::from_secs(3))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        Ok(resp
            .rules
            .into_iter()
            .map(|r| RuleItem {
                kind: r.kind,
                payload: r.payload,
                proxy: r.proxy,
            })
            .collect())
    }
}

/// Tiny URL component encoder so we don't pull in `percent-encoding`. Only
/// needs to handle proxy names and URLs — anything outside the unreserved set
/// is hex-escaped.
fn urlencoding(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for &b in s.as_bytes() {
        let safe = b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~');
        if safe {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{:02X}", b));
        }
    }
    out
}
