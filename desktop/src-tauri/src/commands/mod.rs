//! `tauri::command` surface. One file per logical area; keep these wrappers
//! thin — any non-trivial logic belongs in `xboard-core`.
//!
//! The submodules are `pub` so `tauri::generate_handler!` can resolve each
//! command's macro-generated `__cmd__<name>` helper at its defining path.

pub mod auth;
pub mod billing;
pub mod connection;
pub mod guest;
pub mod helper;
pub mod kernel;
pub mod meta;
pub mod notice;
pub mod session;
pub mod ticket;
pub mod user;
