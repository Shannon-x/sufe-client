//! Placeholder driver for Xray-core.
//!
//! The trait is fully implemented (with `NotImplemented` errors) so today's
//! callers can compile against the multi-kernel surface; swapping in a real
//! implementation will not require trait changes.

use async_trait::async_trait;
use futures::stream::{BoxStream, StreamExt};

use super::driver::{KernelConfig, KernelDriver, KernelKind, LogLine, ProxyGroup, TrafficStats};
use crate::error::{Result, XboardError};

#[derive(Debug, Default)]
pub struct XrayDriver;

impl XrayDriver {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl KernelDriver for XrayDriver {
    fn kind(&self) -> KernelKind {
        KernelKind::Xray
    }

    async fn version(&self) -> Result<String> {
        Err(XboardError::NotImplemented("XrayDriver::version"))
    }
    async fn start(&self, _: &KernelConfig) -> Result<()> {
        Err(XboardError::NotImplemented("XrayDriver::start"))
    }
    async fn stop(&self) -> Result<()> {
        Ok(())
    }
    async fn reload(&self, _: &KernelConfig) -> Result<()> {
        Err(XboardError::NotImplemented("XrayDriver::reload"))
    }
    async fn is_running(&self) -> bool {
        false
    }
    async fn proxies(&self) -> Result<Vec<ProxyGroup>> {
        Err(XboardError::NotImplemented("XrayDriver::proxies"))
    }
    async fn select_proxy(&self, _: &str, _: &str) -> Result<()> {
        Err(XboardError::NotImplemented("XrayDriver::select_proxy"))
    }
    async fn latency_test(&self, _: &str, _: &str, _: u32) -> Result<u32> {
        Err(XboardError::NotImplemented("XrayDriver::latency_test"))
    }
    async fn traffic(&self) -> Result<TrafficStats> {
        Err(XboardError::NotImplemented("XrayDriver::traffic"))
    }
    fn log_stream(&self) -> BoxStream<'static, LogLine> {
        futures::stream::empty().boxed()
    }
}
