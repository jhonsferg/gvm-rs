//! Shared HTTP agent factory, verbose-logging helpers, and download options.
//!
//! All outbound HTTP requests in `gvm` go through [`agent()`].
//! Using a single builder ensures consistent timeout and header settings
//! across the go.dev API, the GitHub Releases API, and binary downloads.
//!
//! When the `--verbose` / `-v` flag is passed, [`log_request`] and
//! [`log_response`] print HTTP negotiation details to stderr so the user
//! can diagnose connectivity, redirects, and server behaviour.
//!
//! [`set_retries`] configures the download engine's retry limit; its value
//! is read from the `--retries` CLI flag and applied globally before any
//! download begins.

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::time::Duration;

use anyhow::Result;
use colored::Colorize;

/// Set to `true` by `main` when the user passes `--verbose` / `-v`.
static VERBOSE: AtomicBool = AtomicBool::new(false);

/// Maximum retry attempts on network failure (default: 3).
static RETRIES: AtomicU8 = AtomicU8::new(3);

/// Activates (or deactivates) verbose HTTP logging for the process lifetime.
pub fn set_verbose(v: bool) {
    VERBOSE.store(v, Ordering::Relaxed);
}

/// Returns `true` when verbose mode is active.
pub fn is_verbose() -> bool {
    VERBOSE.load(Ordering::Relaxed)
}

/// Logs an outgoing HTTP request line to stderr when verbose mode is active.
pub fn log_request(method: &str, url: &str) {
    if is_verbose() {
        eprintln!("  {} > {} {}", "[v]".dimmed(), method.bold(), url);
    }
}

/// Logs an incoming HTTP response status and all headers to stderr when
/// verbose mode is active.
///
/// `headers` is the `http::HeaderMap` returned by `ureq::Response::headers()`.
pub fn log_response(status: u16, reason: &str, headers: &ureq::http::HeaderMap) {
    if is_verbose() {
        eprintln!(
            "  {} < {} {}",
            "[v]".dimmed(),
            status.to_string().bold(),
            reason
        );
        for (name, value) in headers {
            eprintln!(
                "  {} < {}: {}",
                "[v]".dimmed(),
                name,
                value.to_str().unwrap_or("<binary>")
            );
        }
        eprintln!();
    }
}

/// Sets the maximum number of retry attempts on network failure.
pub fn set_retries(n: u8) {
    RETRIES.store(n, Ordering::Relaxed);
}

/// Returns the configured retry limit.
pub fn retries() -> u8 {
    RETRIES.load(Ordering::Relaxed)
}

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
