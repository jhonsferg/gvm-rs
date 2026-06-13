//! HTTPS download with progress reporting and checksum verification.
//!
//! [`fetch`] streams the response body to disk while displaying a real-time
//! [`indicatif`] progress bar. When the server sends a `Content-Length`
//! header a determinate bar shows bytes transferred, download speed, and ETA.
//! When the length is unknown a spinner shows bytes received and speed.
//!
//! [`verify_sha256`] computes the SHA-256 digest of a local file and compares
//! it against the expected hex string published by go.dev.

use std::io::Read;
use std::path::Path;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use indicatif::{ProgressBar, ProgressStyle};
use sha2::{Digest, Sha256};

use crate::http;

/// Downloads the resource at `url` and writes it to `dest`.
///
/// Displays a live progress bar while the transfer is in progress:
/// - **Known size**: filled bar with bytes transferred, total, speed, and ETA.
/// - **Unknown size**: spinner with bytes received and speed.
///
/// When `--verbose` is active, the request method, URL, response status, and
/// all response headers are printed to stderr before the bar appears.
///
/// # Errors
///
/// Returns an error if the HTTP request cannot be made, the server returns a
/// non-2xx status, `dest` cannot be written to, or the connection drops.
pub fn fetch(url: &str, dest: &Path) -> Result<()> {
    http::log_request("GET", url);

    let mut response = http::agent()?
        .get(url)
        .call()
        .with_context(|| format!("Failed to connect to {url}"))?;

    http::log_response(
        response.status().as_u16(),
        response.status().canonical_reason().unwrap_or(""),
        response.headers(),
    );

    let total = response.body().content_length().unwrap_or(0);

    let pb = if total > 0 {
        let bar = ProgressBar::new(total);
        bar.set_style(
            ProgressStyle::default_bar()
                .template(
                    "  [{bar:40.cyan/blue}] {bytes}/{total_bytes}  {bytes_per_sec}  eta {eta}",
                )
                .unwrap()
                .progress_chars("=>-"),
        );
        bar
    } else {
        let bar = ProgressBar::new_spinner();
        bar.set_style(
            ProgressStyle::default_spinner()
                .template("  {spinner:.cyan}  {bytes}  {bytes_per_sec}")
                .unwrap(),
        );
        bar
    };
    pb.enable_steady_tick(Duration::from_millis(120));

    let mut file = std::fs::File::create(dest)
        .with_context(|| format!("Cannot create file at {}", dest.display()))?;

    std::io::copy(
        &mut pb.wrap_read(response.body_mut().as_reader()),
        &mut file,
    )
    .context("Download interrupted")?;

    pb.finish_and_clear();
    Ok(())
}

/// Verifies the SHA-256 digest of `file` against the `expected` hex string.
///
/// If `expected` is empty the check is skipped (some older go.dev entries do
/// not include a checksum). When the digests differ, an informative error
/// message is returned that includes both the expected and actual values.
///
/// # Errors
///
/// Returns an error if `file` cannot be read or the digest does not match.
pub fn verify_sha256(file: &Path, expected: &str) -> Result<()> {
    if expected.is_empty() {
        return Ok(());
    }

    let mut hasher = Sha256::new();
    let mut f = std::fs::File::open(file)
        .with_context(|| format!("Cannot open {} for checksum", file.display()))?;
    let mut buf = [0u8; 65_536];
    loop {
        let n = f
            .read(&mut buf)
            .context("Failed to read file for checksum")?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }

    let actual: String = hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();

    if actual != expected {
        bail!(
            "Checksum mismatch for {}!\n  expected: {expected}\n  got:      {actual}",
            file.display()
        );
    }
    Ok(())
}
