//! UniFFI surface — Rust-side adapters for `core/src/ffi.udl`.
//!
//! Submodule layout:
//!
//! - [`errors`]   — `FfiError` / `StorageError` / `TunnelError` (closed enums
//!   that cross the FFI boundary; `XboardError → FfiError` mapping)
//! - [`types`]    — Rust mirrors of every UDL `dictionary` + `enum` plus
//!   `From` conversions from the corresponding `crate::api::*` and
//!   `crate::kernel::*` shapes
//! - [`secure`]   — host-supplied keychain bridge (UDL `callback interface
//!   SecureStore`) + the `CallbackSecureStore` adapter that lets legacy
//!   crate code keep using the sync `crate::storage::SecureStore` trait
//! - [`observer`] — `StateObserver` + `TunDelegate` callback traits and the
//!   `StateFanout` helper that multiplexes broadcast frames to N observers
//! - [`client`]   — `interface Client` backing struct (HTTP + session)
//! - [`manager`]  — `interface ConnectionManager` backing struct (kernel
//!   wrapper + state-fanout forwarder)
//!
//! Everything declared in this module tree is re-exported at the crate root
//! by `lib.rs` (`pub use ffi::*;`) — UniFFI's `include_scaffolding!` macro
//! looks up symbol names from there.

pub mod client;
pub mod errors;
pub mod manager;
pub mod observer;
pub mod secure;
pub mod types;

pub use client::Client;
pub use errors::{FfiError, StorageError, TunnelError};
pub use manager::ConnectionManager;
pub use observer::{StateObserver, TunDelegate};
pub use secure::SecureStore;
pub use types::{
    CheckoutResponse, ConnectStage, ConnectionState, ForgetPasswordArgs, LoginArgs, LoginSummary,
    Notice, Order, PaymentMethod, Plan, ProxyGroup, RegisterArgs, SaveOrderArgs, SaveTicketArgs,
    SiteConfig, SubscribeInfo, Ticket, TicketDetail, TicketMessage, TrafficStats, TunConfig,
    TunnelMode, UserInfo,
};
