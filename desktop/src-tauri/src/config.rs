//! All native shells read the same operator-owned core build configuration.
pub use xboard_core::api::runtime_config::{load, RuntimeConfig, DEFAULT_LOCALE};
use xboard_core::api::HttpClient;
use xboard_core::Result;

pub fn create_client() -> Result<HttpClient> {
    let config = load()?;
    let client = config.create_http(DEFAULT_LOCALE)?;
    crate::commands::guest::initialize(&config);
    Ok(client)
}
