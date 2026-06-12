//! Archive download and extraction.
//!
//! This module handles the two-phase process of acquiring a Go toolchain from
//! the network:
//!
//! 1. [`download`] - fetches the archive over HTTPS with a progress bar and
//!    verifies the SHA-256 checksum reported by go.dev.
//! 2. [`extract`] - unpacks `.tar.gz` (Linux / macOS) and `.zip` (Windows)
//!    archives with a spinner to indicate progress.

pub mod download;
pub mod extract;
