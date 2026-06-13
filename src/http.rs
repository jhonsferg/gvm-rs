//! Shared HTTP client factory.
//!
//! All outbound HTTP requests in `gvm` go through [`client()`].
//! Using a single builder ensures consistent timeout and header settings
//! across the go.dev API, the GitHub Releases API, and binary downloads.

use std::time::Duration;

use anyhow::{Context, Result};

/// Builds and returns a blocking `reqwest` client configured for gvm's needs.
///
/// Settings applied to every request made through this client:
///
/// - `connect_timeout`: 15 seconds - covers DNS resolution + TCP handshake +
///   TLS handshake.  Fails fast when the network is unreachable or DNS is
///   broken (common in constrained environments such as Termux on Android).
///   Does **not** limit the body transfer time so large Go tarballs can still
///   download on slow connections.
///
/// - `User-Agent`: identifies the gvm version so server logs can distinguish
///   automated traffic.
pub fn client() -> Result<reqwest::blocking::Client> {
    reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .user_agent(format!("gvm/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .context("Failed to initialise HTTP client")
}
