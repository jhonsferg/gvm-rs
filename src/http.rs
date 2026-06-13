//! Shared HTTP agent factory and verbose-logging helpers.
//!
//! All outbound HTTP requests in `gvm` go through [`agent()`].
//! Using a single builder ensures consistent timeout and header settings
//! across the go.dev API, the GitHub Releases API, and binary downloads.
//!
//! When the `--verbose` / `-v` flag is passed, [`log_request`] and
//! [`log_response`] print HTTP negotiation details to stderr so the user
//! can diagnose connectivity, redirects, and server behaviour.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::Result;
use colored::Colorize;

/// Set to `true` by `main` when the user passes `--verbose` / `-v`.
static VERBOSE: AtomicBool = AtomicBool::new(false);

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
