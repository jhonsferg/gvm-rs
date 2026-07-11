//! Shared HTTP client, verbose-logging helpers, and download options.
//!
//! All outbound HTTP requests in `gvm` go through [`HttpClient`].
//! Using a single client ensures consistent timeout and header settings
//! across the go.dev API, the GitHub Releases API, and binary downloads.
//!
//! When the `--verbose` / `-v` flag is passed, [`log_request`] and
//! [`log_response`] print HTTP negotiation details to stderr so the user
//! can diagnose connectivity, redirects, and server behaviour.

use std::time::Duration;

use anyhow::Result;
use colored::Colorize;

/// HTTP client configuration shared across all requests.
#[derive(Debug, Clone)]
pub struct HttpClient {
    agent: ureq::Agent,
    verbose: bool,
    retries: u8,
}

impl HttpClient {
    /// Creates a new `HttpClient` with the given verbosity and retry settings.
    pub fn new(verbose: bool, retries: u8) -> Result<Self> {
        let agent = ureq::Agent::config_builder()
            .timeout_connect(Some(Duration::from_secs(15)))
            .user_agent(format!("gvm/{}", env!("CARGO_PKG_VERSION")))
            .build()
            .new_agent();
        Ok(Self {
            agent,
            verbose,
            retries,
        })
    }

    /// Returns the underlying `ureq` agent.
    pub fn agent(&self) -> &ureq::Agent {
        &self.agent
    }

    /// Returns `true` when verbose mode is active.
    pub fn is_verbose(&self) -> bool {
        self.verbose
    }

    /// Returns the configured retry limit.
    pub fn retries(&self) -> u8 {
        self.retries
    }
}

/// Logs an outgoing HTTP request line to stderr when verbose mode is active.
pub fn log_request(client: &HttpClient, method: &str, url: &str) {
    if client.is_verbose() {
        eprintln!("  {} > {} {}", "[v]".dimmed(), method.bold(), url);
    }
}

/// Logs an incoming HTTP response status and all headers to stderr when
/// verbose mode is active.
///
/// `headers` is the `http::HeaderMap` returned by `ureq::Response::headers()`.
pub fn log_response(
    client: &HttpClient,
    status: u16,
    reason: &str,
    headers: &ureq::http::HeaderMap,
) {
    if client.is_verbose() {
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
