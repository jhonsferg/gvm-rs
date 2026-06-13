//! HTTPS download with progress reporting and checksum verification.
//!
//! [`fetch`] streams the response body to disk while displaying a
//! [`indicatif`] progress bar. [`verify_sha256`] computes the SHA-256 digest
//! of a local file and compares it against the expected hex string published
//! by go.dev.

use crate::http;
use anyhow::{bail, Context, Result};
use indicatif::{ProgressBar, ProgressStyle};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::Path;

/// Downloads the resource at `url` and writes it to `dest`.
///
/// A progress bar is displayed while the transfer is in progress. The bar
/// shows the number of bytes transferred, total size, and an ETA. It is
/// cleared from the terminal when the download completes.
///
/// # Errors
///
/// Returns an error if:
/// - The HTTP request cannot be made (network error).
/// - The server returns a non-2xx status code.
/// - `dest` cannot be created or written to.
/// - The connection is interrupted before the download completes.
pub fn fetch(url: &str, dest: &Path) -> Result<()> {
    let response = http::client()?
        .get(url)
        .send()
        .with_context(|| format!("Failed to connect to {url}"))?;

    if !response.status().is_success() {
        bail!("HTTP {} while downloading {url}", response.status());
    }

    let total = response.content_length().unwrap_or(0);
    let pb = ProgressBar::new(total);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("  {spinner:.cyan} [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})")
            .unwrap()
            .progress_chars("=->"),
    );

    let mut file = std::fs::File::create(dest)
        .with_context(|| format!("Cannot create file at {}", dest.display()))?;

    std::io::copy(&mut pb.wrap_read(response), &mut file).context("Download interrupted")?;

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
/// Returns an error if:
/// - `file` cannot be opened or read.
/// - The computed digest does not match `expected`.
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
