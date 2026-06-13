//! Shared HTTP agent factory.
//!
//! All outbound HTTP requests in `gvm` go through [`agent()`].
//! Using a single builder ensures consistent timeout and header settings
//! across the go.dev API, the GitHub Releases API, and binary downloads.

use std::time::Duration;

use anyhow::Result;

/// Builds and returns a `ureq` agent configured for gvm's needs.
///
/// - `timeout_connect`: 15 seconds covers DNS + TCP + TLS handshake.
///   Fails fast on unreachable networks (e.g. Termux with no internet).
///   Does not limit body transfer time so large Go tarballs can download
///   on slow connections.
///
/// - `User-Agent`: identifies the gvm version in server logs.
pub fn agent() -> Result<ureq::Agent> {
    Ok(ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(15)))
        .user_agent(format!("gvm/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .new_agent())
}
