//! Subscription profile fetcher and converter.
//!
//! Backed by `<subscribe_url>?flag=...`; persists the raw text on disk for
//! offline restart and uses ETag for cheap freshness checks.

pub mod fetcher;
pub mod inject;
pub mod inject_singbox;

pub use fetcher::{ProfileFetcher, ProfileSnapshot};
pub use inject::{patch_mihomo, patch_mihomo_with_tun_fd, TunnelMode};
pub use inject_singbox::patch_singbox;
