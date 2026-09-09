//! Per-kernel subscription fetcher with ETag-based caching.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::api::HttpClient;
use crate::error::{Result, XboardError};
use crate::kernel::KernelKind;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileSnapshot {
    pub kind: KernelKind,
    pub etag: Option<String>,
    pub fetched_at: chrono::DateTime<chrono::Utc>,
    pub bytes_path: PathBuf,
}

#[derive(Debug)]
pub struct ProfileFetcher {
    pub http: HttpClient,
    pub cache_dir: PathBuf,
}

impl ProfileFetcher {
    pub fn new(http: HttpClient, cache_dir: PathBuf) -> Self {
        Self { http, cache_dir }
    }

    /// Fetch + cache. Returns the previous snapshot unchanged on a 304.
    ///
    /// When the upstream fetch fails (network error, DNS, CDN block) and we
    /// already have a cached profile on disk, fall back to it rather than
    /// surfacing a hard error to the user — the typical chicken-and-egg case
    /// is that the subscribe URL is fronted by a CDN that itself needs the
    /// proxy to reach. Logs a warning so the failure isn't silent.
    pub async fn fetch(
        &self,
        subscribe_url: &str,
        kind: KernelKind,
        prev: Option<&ProfileSnapshot>,
    ) -> Result<ProfileSnapshot> {
        tokio::fs::create_dir_all(&self.cache_dir).await?;
        // Subscription tokens identify accounts. A shared `mihomo.profile`
        // could replay the previous user's nodes after logout on a network
        // failure. Hash the opaque URL so tokens never appear in filenames.
        let target = self.cache_dir.join(cache_filename(kind, subscribe_url));
        let prev = prev.filter(|snapshot| snapshot.bytes_path == target);
        let prev_etag = prev.and_then(|s| s.etag.as_deref());
        let fetch_res = self
            .http
            .fetch_subscribe(subscribe_url, kind.flag(), prev_etag)
            .await;

        let res = match fetch_res {
            Ok(r) => r,
            Err(e) => {
                // A 4xx "subscription unavailable" is a definitive answer
                // from the backend (expired / suspended account), NOT a
                // transient transport failure. Never paper over it with a
                // stale cache — propagate so the UI can prompt a renewal.
                // (Overwriting the cache is impossible here too, since we
                // return before the write.)
                if matches!(e, XboardError::SubscriptionUnavailable { .. }) {
                    return Err(e);
                }
                // Genuine transport failure (DNS / CDN block / timeout):
                // the chicken-and-egg "subscribe URL needs the proxy" case.
                // Fall back to the last good cache if we have one.
                if let Some(snap) = self.cache_fallback(kind, prev, &target).await {
                    tracing::warn!(error=%e, "subscribe fetch failed, using cached profile");
                    return Ok(snap);
                }
                return Err(e);
            }
        };

        if res.status == 304 {
            if let Some(snap) = self.cache_fallback(kind, prev, &target).await {
                return Ok(snap);
            }
            return Err(XboardError::Config(
                "订阅缓存已丢失，请重试获取完整订阅".into(),
            ));
        }
        // A 2xx with an empty body is not a usable subscription — refuse to
        // clobber a previously-good cache with emptiness, and surface the
        // condition rather than letting an empty config reach the kernel.
        if !valid_profile(kind, &res.body) {
            return Err(XboardError::SubscriptionUnavailable { status: res.status });
        }
        let staging = target.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
        tokio::fs::write(&staging, &res.body).await?;
        if let Err(error) = tokio::fs::rename(&staging, &target).await {
            let _ = tokio::fs::remove_file(&staging).await;
            return Err(error.into());
        }
        Ok(ProfileSnapshot {
            kind,
            etag: res.etag,
            fetched_at: chrono::Utc::now(),
            bytes_path: target,
        })
    }

    /// If the on-disk cache file exists, synthesize a snapshot pointing at
    /// it. Used as a soft fallback when the upstream fetch fails or returns
    /// 304 without a `prev` to clone.
    async fn cache_fallback(
        &self,
        kind: KernelKind,
        prev: Option<&ProfileSnapshot>,
        target: &PathBuf,
    ) -> Option<ProfileSnapshot> {
        match tokio::fs::metadata(target).await {
            Ok(meta) if meta.is_file() && meta.len() > 0 => {
                let bytes = tokio::fs::read(target).await.ok()?;
                if !valid_profile(kind, &bytes) {
                    return None;
                }
                if let Some(p) = prev {
                    return Some(p.clone());
                }
                let fetched_at = meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .and_then(|d| chrono::DateTime::from_timestamp(d.as_secs() as i64, 0))
                    .unwrap_or_else(chrono::Utc::now);
                Some(ProfileSnapshot {
                    kind,
                    etag: None,
                    fetched_at,
                    bytes_path: target.clone(),
                })
            }
            _ => None,
        }
    }
}

fn cache_filename(kind: KernelKind, subscribe_url: &str) -> String {
    let family = match kind {
        KernelKind::Mihomo => "mihomo",
        KernelKind::Xray => "xray",
        // sing-box reuses the mihomo subscription (translated client-side
        // by `profile::inject_singbox`), so the on-disk cache name matches.
        KernelKind::SingBox => "mihomo",
    };
    format!(
        "{family}-{}.profile",
        hex::encode(Sha256::digest(subscribe_url.as_bytes()))
    )
}

fn valid_profile(kind: KernelKind, bytes: &[u8]) -> bool {
    if bytes.is_empty() {
        return false;
    }
    match kind {
        KernelKind::Mihomo | KernelKind::SingBox => {
            let Ok(doc) = serde_yaml::from_slice::<serde_yaml::Value>(bytes) else {
                return false;
            };
            doc.get("proxies")
                .and_then(|v| v.as_sequence())
                .is_some_and(|nodes| !nodes.is_empty())
        }
        KernelKind::Xray => serde_json::from_slice::<serde_json::Value>(bytes).is_ok(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[tokio::test]
    async fn invalid_response_does_not_poison_account_scoped_cache() {
        let dir = std::env::temp_dir().join(format!("sufe-profile-{}", uuid::Uuid::new_v4()));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!(
            "http://{}/subscribe?token=alice",
            listener.local_addr().unwrap()
        );
        let good = "proxies:\n  - { name: test, type: ss, server: example.com, port: 443 }\n";
        let server = tokio::spawn(async move {
            for body in [good, "<html>upstream error</html>"] {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut request = vec![0; 4096];
                let _ = stream.read(&mut request).await.unwrap();
                stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
            }
        });
        let fetcher = ProfileFetcher::new(
            HttpClient::new("https://api.example.com", "zh-CN").unwrap(),
            dir.clone(),
        );
        let snapshot = fetcher.fetch(&url, KernelKind::Mihomo, None).await.unwrap();
        assert!(fetcher
            .fetch(&url, KernelKind::Mihomo, Some(&snapshot))
            .await
            .is_err());
        assert_eq!(
            tokio::fs::read_to_string(&snapshot.bytes_path)
                .await
                .unwrap(),
            good
        );
        let bob = url.replace("alice", "bob");
        assert_ne!(
            cache_filename(KernelKind::Mihomo, &url),
            cache_filename(KernelKind::Mihomo, &bob)
        );
        assert!(!dir.join(cache_filename(KernelKind::Mihomo, &bob)).exists());
        server.await.unwrap();
        tokio::fs::remove_dir_all(dir).await.unwrap();
    }
}
