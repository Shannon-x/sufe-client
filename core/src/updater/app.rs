//! Application release metadata served from the static update endpoint.
//!
//! The Tauri updater plugin reads its own format on desktop; this struct is
//! used by the Android updater (`UpdateService.kt`) and as a sanity check on
//! desktop.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppRelease {
    pub version: String,
    /// `"stable"` or `"beta"`.
    pub channel: String,
    pub published_at: chrono::DateTime<chrono::Utc>,
    pub assets: Vec<AppAsset>,
    pub notes_url: Option<String>,
    /// Hex-encoded ed25519 signature of `serde_json::to_vec(&self_with_signature_empty)`.
    /// (Verification helper lives in [`crate::updater::kernel`] — share once
    /// the desktop wires this up.)
    #[serde(default)]
    pub signature: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppAsset {
    /// `"windows"` / `"macos"` / `"linux"` / `"android"`.
    pub platform: String,
    /// `"amd64"` / `"arm64"` / `"universal"`.
    pub arch: String,
    /// `"msi"` / `"dmg"` / `"AppImage"` / `"apk"`.
    pub format: String,
    pub url: String,
    pub sha256: String,
    pub size: u64,
}
