//! Parallel download engine with resume and retry support.
//!
//! [`fetch`] is the single entry point for all downloads in `gvm`. It probes
//! the server with a HEAD request and, when the server advertises
//! `Accept-Ranges: bytes` and a known `Content-Length`, splits the transfer
//! into [`http::connections()`] parallel chunks. Each chunk runs in its own
//! thread and writes to a temporary `.partN` file. Progress is reported
//! through an [`indicatif::MultiProgress`] panel: one bar per chunk plus a
//! summary bar at the bottom. When all chunks finish the parts are merged
//! in-order into the final destination file.
//!
//! When the server does not support ranges, the size is unknown, or
//! `--connections 1` is passed, a single-stream path is used instead.
//!
//! Both paths support transparent resume: if a `.part` or `.partN` file
//! already exists the download picks up from the last written byte using a
//! `Range: bytes=N-` request.
//!
//! On any network error the failing chunk (or the single stream) retries up
//! to [`http::retries()`] times using exponential back-off (1 s, 2 s, 4 s …).
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
use indicatif::{MultiProgress, ProgressBar, ProgressDrawTarget, ProgressStyle};
use sha2::{Digest, Sha256};

use crate::http;

/// Minimum byte size for a single chunk in parallel mode (4 MiB).
///
/// When `total / connections` is smaller than this, the connection count is
/// reduced so every chunk is at least `MIN_CHUNK` bytes. This avoids spinning
/// up many threads for tiny files.
const MIN_CHUNK: u64 = 4 * 1024 * 1024;

// ── Public API ────────────────────────────────────────────────────────────────

/// Downloads `url` to `dest`, using parallel chunks when possible.
///
/// Reads connection count and retry limit from the globals populated by the
/// `--connections` / `--retries` CLI flags. Falls back to single-stream when
/// the server does not support `Accept-Ranges: bytes`.
///
/// # Errors
///
/// Returns an error if the server is unreachable, all retries are exhausted,
/// or `dest` cannot be written to.
pub fn fetch(url: &str, dest: &Path) -> Result<()> {
    let want_conns = http::connections();
    let retries = http::retries();

    let (content_length, accepts_ranges) = probe(url)?;

    // Cap connections so no chunk is smaller than MIN_CHUNK.
    let effective = if accepts_ranges && content_length > 0 && want_conns > 1 {
        let max = (content_length / MIN_CHUNK).max(1) as usize;
        want_conns.min(max)
    } else {
        1
    };

    if effective > 1 {
        fetch_parallel(url, dest, content_length, effective, retries)
    } else {
        fetch_single(url, dest, content_length, retries)
    }
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

// ── Server probe ──────────────────────────────────────────────────────────────

/// Sends a HEAD request to discover `Content-Length` and `Accept-Ranges`.
///
/// Falls back to `(0, false)` on any error so callers degrade gracefully to
/// single-stream mode.
fn probe(url: &str) -> Result<(u64, bool)> {
    http::log_request("HEAD", url);
    match http::agent()?.head(url).call() {
        Ok(resp) => {
            http::log_response(
                resp.status().as_u16(),
                resp.status().canonical_reason().unwrap_or(""),
                resp.headers(),
            );
            // go.dev serves files with transfer-encoding:chunked and omits
            // content-length; fall back to x-identity-content-length which
            // Google's CDN always provides for binary assets.
            let len = resp
                .headers()
                .get("content-length")
                .or_else(|| resp.headers().get("x-identity-content-length"))
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.parse().ok())
                .unwrap_or(0u64);
            let ranges = resp
                .headers()
                .get("accept-ranges")
                .and_then(|v| v.to_str().ok())
                .is_some_and(|v| v.contains("bytes"));
            Ok((len, ranges))
        }
        // HEAD not supported or network error: fall back gracefully.
        Err(_) => Ok((0, false)),
    }
}

// ── Single-stream path ────────────────────────────────────────────────────────

/// Single-connection download with retry and transparent resume.
fn fetch_single(url: &str, dest: &Path, content_length: u64, retries: u8) -> Result<()> {
    let part = chunk_path(dest, None);
    let mut attempt = 0u8;

    loop {
        let existing = part.metadata().map(|m| m.len()).unwrap_or(0);
        let pb = single_bar(content_length, existing);
        pb.enable_steady_tick(Duration::from_millis(120));

        match try_single(url, &part, existing, &pb) {
            Ok(()) => {
                pb.finish_and_clear();
                std::fs::rename(&part, dest)
                    .with_context(|| format!("Cannot finalise {}", dest.display()))?;
                return Ok(());
            }
            Err(e) => {
                pb.finish_and_clear();
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

fn try_single(url: &str, part: &Path, offset: u64, pb: &ProgressBar) -> Result<()> {
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

    let mut file: File = if resuming {
        OpenOptions::new()
            .append(true)
            .open(part)
            .with_context(|| format!("Cannot open {} for appending", part.display()))?
    } else {
        pb.set_position(0);
        File::create(part).with_context(|| format!("Cannot create {}", part.display()))?
    };

    std::io::copy(
        &mut pb.wrap_read(response.body_mut().as_reader()),
        &mut file,
    )
    .context("Download interrupted")?;

    Ok(())
}

// ── Parallel path ─────────────────────────────────────────────────────────────

/// Parallel download: splits `total` into `connections` byte ranges and
/// fetches each range in a dedicated thread. Shows a per-chunk progress bar
/// above a summary bar at the bottom.
fn fetch_parallel(
    url: &str,
    dest: &Path,
    total: u64,
    connections: usize,
    retries: u8,
) -> Result<()> {
    let chunk_size = total.div_ceil(connections as u64);

    let mp = MultiProgress::new();
    // Cap redraws to 10 Hz. Without this, 8+ concurrent threads each calling
    // bar.inc() can flood the terminal with partial renders on Windows.
    mp.set_draw_target(ProgressDrawTarget::stderr_with_hz(10));

    // Build the styles once, shared across all bars.
    let chunk_style = ProgressStyle::default_bar()
        .template("  {prefix:.dim}  [{bar:30.blue/dim}] {bytes}/{total_bytes}  {bytes_per_sec}")
        .unwrap()
        .progress_chars("=>-");
    let total_style = ProgressStyle::default_bar()
        .template("  [{bar:40.cyan/blue}] {bytes}/{total_bytes}  {bytes_per_sec}  eta {eta}")
        .unwrap()
        .progress_chars("=>-");

    // Create ALL chunk bars with their styles set before any tick thread starts.
    // This is critical: if the tick thread fires between ProgressBar::new() and
    // set_style(), the bar renders with the default blocky style (░░░░).
    // Chunk bars are added first so they appear above the total bar.
    let chunk_bars: Vec<ProgressBar> = (0..connections)
        .map(|i| {
            let start = i as u64 * chunk_size;
            let end = (start + chunk_size).min(total) - 1;
            let size = end - start + 1;
            let bar = mp.add(ProgressBar::new(size));
            bar.set_style(chunk_style.clone());
            bar.set_prefix(format!("#{:>2}", i + 1));
            bar
        })
        .collect();

    // Total bar is added last so it always renders at the bottom.
    let total_bar = mp.add(ProgressBar::new(total));
    total_bar.set_style(total_style);
    // Enable steady_tick ONLY after all bars exist and have their styles set.
    // Starting it earlier would race with bar creation above.
    total_bar.enable_steady_tick(Duration::from_millis(200));

    // Spawn one thread per chunk. Chunk bars are moved into the threads; the
    // total_bar clone is Arc-backed, so sharing it is safe.
    let handles: Vec<_> = chunk_bars
        .into_iter()
        .enumerate()
        .map(|(i, bar)| {
            let start = i as u64 * chunk_size;
            let end = (start + chunk_size).min(total) - 1;
            let url = url.to_owned();
            let part = chunk_path(dest, Some(i));
            let total_bar = total_bar.clone();

            thread::spawn(move || -> Result<()> {
                download_chunk(&url, start, end, &part, retries, bar, total_bar)
            })
        })
        .collect();

    // Wait for every thread; keep the first error if any.
    let mut first_err: Option<anyhow::Error> = None;
    for h in handles {
        match h.join() {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                first_err.get_or_insert(e);
            }
            Err(_) => {
                first_err.get_or_insert_with(|| anyhow::anyhow!("A download thread panicked"));
            }
        }
    }

    // Clear the entire panel at once. finish_and_clear on individual bars
    // would leave orphaned lines on screen.
    mp.clear().ok();

    if let Some(e) = first_err {
        for i in 0..connections {
            let _ = std::fs::remove_file(chunk_path(dest, Some(i)));
        }
        return Err(e);
    }

    // Merge chunks in-order into the final destination file.
    let mut out =
        File::create(dest).with_context(|| format!("Cannot create {}", dest.display()))?;
    for i in 0..connections {
        let part = chunk_path(dest, Some(i));
        let mut f =
            File::open(&part).with_context(|| format!("Cannot open chunk {i} for merging"))?;
        std::io::copy(&mut f, &mut out).with_context(|| format!("Failed to merge chunk {i}"))?;
        std::fs::remove_file(&part)
            .with_context(|| format!("Failed to remove part file for chunk {i}"))?;
    }

    Ok(())
}

/// Downloads the byte range `[start, end]` to `part` with retry and resume.
fn download_chunk(
    url: &str,
    start: u64,
    end: u64,
    part: &Path,
    retries: u8,
    bar: ProgressBar,
    total_bar: ProgressBar,
) -> Result<()> {
    let mut attempt = 0u8;

    loop {
        // If a partial chunk file exists, resume from its current size.
        let existing = part.metadata().map(|m| m.len()).unwrap_or(0);
        let actual_start = start + existing;

        if actual_start > end {
            bar.finish_with_message("done");
            return Ok(());
        }

        bar.set_position(existing);

        match try_range(url, actual_start, end, part, existing > 0, &bar, &total_bar) {
            Ok(()) => {
                bar.finish();
                return Ok(());
            }
            Err(e) => {
                if attempt >= retries {
                    bar.finish();
                    return Err(e).context(format!(
                        "Chunk {start}-{end} failed after {retries} retries"
                    ));
                }
                attempt += 1;
                // Don't call bar.set_message() here: it triggers a redraw from
                // the chunk thread which competes with the total_bar tick thread.
                thread::sleep(Duration::from_secs(backoff(attempt)));
            }
        }
    }
}

/// Performs a single `Range` GET request, streaming the body to `part`.
fn try_range(
    url: &str,
    start: u64,
    end: u64,
    part: &Path,
    append: bool,
    bar: &ProgressBar,
    total_bar: &ProgressBar,
) -> Result<()> {
    let range_hdr = format!("bytes={start}-{end}");

    if http::is_verbose() {
        // Route through total_bar.println so the MultiProgress panel absorbs
        // the output. Direct eprintln!() would corrupt the bar rendering.
        total_bar.println(format!(
            "  {} > GET {} Range: {range_hdr}",
            "[v]".dimmed(),
            url
        ));
    }

    let mut response = http::agent()?
        .get(url)
        .header("Range", &range_hdr)
        // Disable transparent compression: a byte-range response is raw binary
        // and must not be gzip-decoded by the HTTP client.
        .header("Accept-Encoding", "identity")
        .call()
        .with_context(|| format!("Failed to fetch range {range_hdr}"))?;

    if http::is_verbose() {
        total_bar.println(format!(
            "  {} < {} {}",
            "[v]".dimmed(),
            response.status().as_u16().to_string().bold(),
            response.status().canonical_reason().unwrap_or("")
        ));
    }

    let status = response.status().as_u16();
    if status != 206 {
        bail!("Expected 206 Partial Content, got {status}");
    }

    let mut file: File = if append {
        OpenOptions::new()
            .append(true)
            .open(part)
            .context("Cannot open chunk file for appending")?
    } else {
        File::create(part).context("Cannot create chunk file")?
    };

    let mut reader = response.body_mut().as_reader();
    let mut buf = [0u8; 65_536];
    loop {
        let n = reader.read(&mut buf).context("Chunk read interrupted")?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n]).context("Chunk write failed")?;
        // Both bars are Arc-backed and safe to update from any thread.
        bar.inc(n as u64);
        total_bar.inc(n as u64);
    }

    Ok(())
}

// ── Utilities ─────────────────────────────────────────────────────────────────

/// Returns the path for a partial download file.
///
/// - `chunk = None`    → `"{dest}.part"`   (single-stream)
/// - `chunk = Some(i)` → `"{dest}.part{i}"` (parallel chunk i)
fn chunk_path(dest: &Path, chunk: Option<usize>) -> PathBuf {
    let base = dest.to_string_lossy();
    match chunk {
        None => PathBuf::from(format!("{base}.part")),
        Some(i) => PathBuf::from(format!("{base}.part{i}")),
    }
}

/// Builds the progress bar for a single-stream download.
///
/// When `content_length > 0` returns a determinate bar pre-positioned at
/// `existing` bytes (to reflect a resumed transfer). Otherwise returns a
/// spinner.
fn single_bar(content_length: u64, existing: u64) -> ProgressBar {
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
