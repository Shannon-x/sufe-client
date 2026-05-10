//! Kernel binary updater with signed manifests + atomic swap.

use std::path::{Path, PathBuf};

use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{Result, XboardError};

/// Manifest served at e.g. `https://cdn.<your-domain>/kernels/mihomo/manifest.json`.
///
/// `signature` is the hex-encoded ed25519 signature of `serde_json::to_vec(&payload)`,
/// produced by your release pipeline using an offline private key. Clients
/// reject any payload whose signature does not verify against the embedded
/// public key.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KernelManifest {
    pub payload: KernelManifestPayload,
    pub signature: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KernelManifestPayload {
    /// e.g. `"v1.18.7"`
    pub version: String,
    pub released_at: chrono::DateTime<chrono::Utc>,
    pub assets: Vec<KernelAsset>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KernelAsset {
    /// `"darwin-arm64"` / `"linux-amd64"` / `"windows-amd64"` / `"android-arm64"` / ...
    pub arch: String,
    pub url: String,
    pub sha256: String,
    pub size: u64,
}

#[derive(Debug)]
pub struct KernelUpdater {
    pub manifest_url: String,
    pub install_dir: PathBuf,
    pub verify_key: VerifyingKey,
    pub arch: String,
}

impl KernelUpdater {
    /// Fetch + verify the manifest. Errors out if the signature is missing,
    /// malformed, or fails verification against the embedded public key.
    pub async fn check(&self) -> Result<KernelManifest> {
        let manifest: KernelManifest = Client::new()
            .get(&self.manifest_url)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let payload_bytes = serde_json::to_vec(&manifest.payload)?;
        let sig_bytes =
            hex::decode(&manifest.signature).map_err(|_| XboardError::InvalidSignature)?;
        let sig = Signature::from_slice(&sig_bytes).map_err(|_| XboardError::InvalidSignature)?;
        self.verify_key
            .verify(&payload_bytes, &sig)
            .map_err(|_| XboardError::InvalidSignature)?;
        Ok(manifest)
    }

    /// Read the recorded version on disk, if any.
    pub fn current_version(&self) -> Option<String> {
        std::fs::read_to_string(self.install_dir.join("version.lock")).ok()
    }

    /// Download → SHA-256 verify → atomic rename → mark exec → record version.
    /// Caller MUST stop the running kernel before invoking this on the same
    /// path; the swap will fail on Windows otherwise (open-file lock).
    pub async fn apply(&self, manifest: &KernelManifest, target_binary: &Path) -> Result<()> {
        let asset = manifest
            .payload
            .assets
            .iter()
            .find(|a| a.arch == self.arch)
            .ok_or_else(|| XboardError::Config(format!("no asset for arch {}", self.arch)))?;

        let staging = target_binary.with_extension("staging");
        let backup = target_binary.with_extension("bak");

        // 1. Download
        let bytes = Client::new()
            .get(&asset.url)
            .send()
            .await?
            .error_for_status()?
            .bytes()
            .await?;

        // 2. SHA-256 verify
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        let actual = hex::encode(hasher.finalize());
        if !actual.eq_ignore_ascii_case(&asset.sha256) {
            return Err(XboardError::ChecksumMismatch {
                expected: asset.sha256.clone(),
                actual,
            });
        }

        if let Some(parent) = target_binary.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tokio::fs::write(&staging, &bytes).await?;

        // 3. Atomic swap
        if target_binary.exists() {
            let _ = tokio::fs::rename(target_binary, &backup).await;
        }
        tokio::fs::rename(&staging, target_binary).await?;

        // 4. POSIX exec bit
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perm = tokio::fs::metadata(target_binary).await?.permissions();
            perm.set_mode(0o755);
            tokio::fs::set_permissions(target_binary, perm).await?;
        }

        // 5. version.lock
        tokio::fs::write(
            self.install_dir.join("version.lock"),
            &manifest.payload.version,
        )
        .await?;

        // 6. Best-effort cleanup
        let _ = tokio::fs::remove_file(&backup).await;
        Ok(())
    }
}
