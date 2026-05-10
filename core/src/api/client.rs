//! Subscription text fetcher.
//!
//! Clients must treat `subscribe_url` as opaque. We append `?flag=` so the
//! Xboard backend's `App\Support\ProtocolManager` matches the right protocol
//! handler and serializes the kernel-specific config.

use bytes::Bytes;
use reqwest::header::IF_NONE_MATCH;
use url::Url;

use super::HttpClient;
use crate::error::Result;

#[derive(Debug, Clone)]
pub struct SubscribeFetch {
    pub body: Bytes,
    pub etag: Option<String>,
    /// 200 (full body) or 304 (unchanged — body will be empty).
    pub status: u16,
    /// Standard `Subscription-Userinfo` header (upload/download/total/expire).
    pub user_info_header: Option<String>,
}

impl HttpClient {
    /// Fetch the raw subscription text. The caller picks the `flag` value from
    /// the active kernel (e.g. [`crate::kernel::KernelKind::flag`]).
    pub async fn fetch_subscribe(
        &self,
        subscribe_url: &str,
        flag: &str,
        if_none_match: Option<&str>,
    ) -> Result<SubscribeFetch> {
        let mut url = Url::parse(subscribe_url)?;
        url.query_pairs_mut().append_pair("flag", flag);

        let mut req = self.raw().get(url);
        if let Some(etag) = if_none_match {
            req = req.header(IF_NONE_MATCH, etag);
        }
        let resp = req.send().await?;
        let status = resp.status().as_u16();
        let etag = resp
            .headers()
            .get("etag")
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        let user_info_header = resp
            .headers()
            .get("subscription-userinfo")
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        let body = resp.bytes().await?;
        Ok(SubscribeFetch {
            body,
            etag,
            status,
            user_info_header,
        })
    }
}
