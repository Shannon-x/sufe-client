//! Per-kernel subscription fetcher with ETag-based caching.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

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
        let target = self.cache_dir.join(kind_filename(kind));
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
            if let Some(p) = prev {
                return Ok(p.clone());
            }
            if let Some(snap) = self.cache_fallback(kind, prev, &target).await {
                return Ok(snap);
            }
        }
        // A 2xx with an empty body is not a usable subscription — refuse to
        // clobber a previously-good cache with emptiness, and surface the
        // condition rather than letting an empty config reach the kernel.
        if res.body.is_empty() {
            if let Some(snap) = self.cache_fallback(kind, prev, &target).await {
                tracing::warn!("subscribe returned empty body, using cached profile");
                return Ok(snap);
            }
            return Err(XboardError::SubscriptionUnavailable { status: res.status });
        }
        tokio::fs::write(&target, &res.body).await?;
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
        if let Some(p) = prev {
            return Some(p.clone());
        }
        match tokio::fs::metadata(target).await {
            Ok(meta) if meta.is_file() && meta.len() > 0 => {
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

fn kind_filename(kind: KernelKind) -> &'static str {
    match kind {
        KernelKind::Mihomo => "mihomo.profile",
        KernelKind::Xray => "xray.profile",
        // sing-box reuses the mihomo subscription (translated client-side
        // by `profile::inject_singbox`), so the on-disk cache name matches.
        KernelKind::SingBox => "mihomo.profile",
    }
}
