//! Local stand-in for `uniffi-bindgen-cli` — UniFFI 0.28 stopped publishing
//! the bindgen as a separate crate, so the canonical pattern is to ship a
//! one-line binary inside the same workspace that calls `uniffi_bindgen_main()`.
//!
//! Usage (matches the upstream CLI exactly):
//!
//! ```sh
//! cargo run -p xboard-core --bin uniffi-bindgen -- \
//!     generate src/ffi.udl --language kotlin --out-dir ./generated/kotlin
//! cargo run -p xboard-core --bin uniffi-bindgen -- \
//!     generate src/ffi.udl --language swift  --out-dir ./generated/swift
//! ```
//!
//! Wired into `justfile::core-android` and the iOS bindings build step.

fn main() {
    uniffi::uniffi_bindgen_main()
}
