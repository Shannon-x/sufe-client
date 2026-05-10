//! Kernel and application self-updater.
//!
//! Both kernels and app releases are served from your own CDN as
//! ed25519-signed manifests; clients verify the signature with a built-in
//! public key before applying any swap.

pub mod app;
pub mod kernel;

pub use kernel::{KernelAsset, KernelManifest, KernelManifestPayload, KernelUpdater};
