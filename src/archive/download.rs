//! Single-stream download engine with resume and retry support.
//!
//! [`fetch`] is the single entry point for all downloads in `gvm`. Every
//! download uses one HTTP connection end to end - no chunking, no
//! multi-bar coordination, nothing to race against. A large read buffer
//! keeps per-syscall overhead low so the stream can use as much of the
//! link's actual throughput as the OS scheduler allows.
//!
//! There is no separate HEAD probe: go.dev's download links 302-redirect to
//! a CDN host, and a HEAD request pays that redirect round-trip just to
//! learn the file size. The GET response carries the same size headers, so
//! [`try_fetch`] reads them off the real transfer instead of a throwaway
//! request.
//!
//! Resume is transparent: if a `.part` file already exists from a previous
//! attempt, the download picks up from the last written byte using a
//! `Range: bytes=N-` request. If the server ignores the Range header (no
//! `Accept-Ranges` support) the transfer restarts from scratch.
//!
//! On any network error the download retries up to [`http::retries()`]
//! times using exponential back-off (1 s, 2 s, 4 s, …).
//!
//! [`verify_sha256`] checks the integrity of a completed file against the
//! expected hex digest published by go.dev.

use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use colored::Colorize;
use indicatif::{ProgressBar, ProgressStyle};
use sha2::{Digest, Sha256};

use crate::http;

/// Size of the buffer used to read the response body. Large enough to keep
/// per-syscall overhead low and let the single stream use as much of the
/// link's throughput as possible.
const READ_BUF_SIZE: usize = 1024 * 1024;

// ── Public API ────────────────────────────────────────────────────────────────

/// Downloads `url` to `dest` over a single HTTP stream.
///
/// Reads the retry limit from the global populated by the `--retries` CLI
/// flag.
///
/// # Errors
///
/// Returns an error if the server is unreachable, all retries are
/// exhausted, or `dest` cannot be written to.
pub fn fetch(url: &str, dest: &Path) -> Result<()> {
    let retries = http::retries();
    fetch_single(url, dest, retries)
}

/// Verifies the SHA-256 digest of `file` against the `expected` hex string.
///
/// If `expected` is empty the check is skipped (some older go.dev entries do
/// not include a checksum). When the digests differ an informative error is
/// returned that includes both the expected and actual values.
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

// ── Download ─────────────────────────────────────────────────────────────────

/// Single-connection download with retry and transparent resume.
fn fetch_single(url: &str, dest: &Path, retries: u8) -> Result<()> {
    let part = part_path(dest);
    let mut attempt = 0u8;

    loop {
        let existing = part.metadata().map(|m| m.len()).unwrap_or(0);

        match try_fetch(url, &part, existing) {
            Ok(()) => {
                std::fs::rename(&part, dest)
                    .with_context(|| format!("Cannot finalise {}", dest.display()))?;
                return Ok(());
            }
            Err(e) => {
                if attempt >= retries {
                    let _ = std::fs::remove_file(&part);
                    return Err(e).context(format!("Download failed after {retries} retries"));
                }
                attempt += 1;
                eprintln!(
                    "  {} Network error, retrying ({}/{retries})...",
                    "!".yellow(),
                    attempt
                );
                thread::sleep(Duration::from_secs(backoff(attempt)));
            }
        }
    }
}

/// Performs a single GET request and streams the body to `part` with a
/// large read buffer ([`READ_BUF_SIZE`]) to keep per-syscall overhead low.
///
/// Sizes the progress bar from the GET response's own headers - no separate
/// HEAD request, so there is only ever one redirect round-trip per attempt
/// instead of two.
fn try_fetch(url: &str, part: &Path, offset: u64) -> Result<()> {
    http::log_request("GET", url);

    let mut req = http::agent()?
        .get(url)
        // Prevent transparent gzip so the raw binary body is never decoded.
        .header("Accept-Encoding", "identity");
    if offset > 0 {
        req = req.header("Range", &format!("bytes={offset}-"));
    }

    let mut response = req
        .call()
        .with_context(|| format!("Failed to connect to {url}"))?;

    http::log_response(
        response.status().as_u16(),
        response.status().canonical_reason().unwrap_or(""),
        response.headers(),
    );

    // 206 means the server honoured our Range header and we can append.
    // Any other status (200 for a server that ignores Range) means restart.
    let resuming = offset > 0 && response.status().as_u16() == 206;

    // go.dev's CDN always reports the full file size via
    // x-identity-content-length, even on a 206 response, so it doesn't need
    // adjusting for the current offset. Plain content-length is only the
    // size of the remaining bytes on a 206, so add offset back to get the
    // total for the progress bar.
    let headers = response.headers();
    let total_length = headers
        .get("x-identity-content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or_else(|| {
            let len = headers
                .get("content-length")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(0);
            if resuming {
                offset + len
            } else {
                len
            }
        });

    let pb = progress_bar(total_length, if resuming { offset } else { 0 });
    pb.enable_steady_tick(Duration::from_millis(120));

    let mut file: File = if resuming {
        OpenOptions::new()
            .append(true)
            .open(part)
            .with_context(|| format!("Cannot open {} for appending", part.display()))?
    } else {
        File::create(part).with_context(|| format!("Cannot create {}", part.display()))?
    };

    let mut reader = response.body_mut().as_reader();
    let mut buf = vec![0u8; READ_BUF_SIZE];
    let result = (|| -> Result<()> {
        loop {
            let n = reader.read(&mut buf).context("Download interrupted")?;
            if n == 0 {
                break;
            }
            file.write_all(&buf[..n]).context("Write failed")?;
            pb.inc(n as u64);
        }
        Ok(())
    })();

    pb.finish_and_clear();
    result
}

// ── Utilities ─────────────────────────────────────────────────────────────────

/// Returns the path used for the in-progress download: `"{dest}.part"`.
fn part_path(dest: &Path) -> PathBuf {
    PathBuf::from(format!("{}.part", dest.to_string_lossy()))
}

/// Builds the progress bar for the download.
///
/// When `content_length > 0` returns a determinate bar pre-positioned at
/// `existing` bytes (to reflect a resumed transfer). Otherwise returns a
/// spinner.
fn progress_bar(content_length: u64, existing: u64) -> ProgressBar {
    if content_length > 0 {
        let bar = ProgressBar::new(content_length);
        bar.set_position(existing);
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
    }
}

/// Exponential back-off in seconds for retry attempt `n` (1-based): 1, 2, 4, 8 …
fn backoff(n: u8) -> u64 {
    2u64.pow(n as u32 - 1)
}
