//! Functions for fetching and querying the go.dev release index.
//!
//! This module is the single point of contact with the remote API. All network
//! requests made by `gvm` go through [`fetch_releases`].

use anyhow::{Context, Result};

use crate::{http, remote::release::Release, user_version::VersionSpec};

/// URL of the go.dev download API that lists all releases.
const GO_DL_API: &str = "https://go.dev/dl/?mode=json&include=all";

/// Downloads the complete list of Go releases from go.dev.
///
/// The API returns both stable and unstable (RC/beta) releases; callers are
/// responsible for filtering by [`Release::stable`] if needed.
///
/// # Errors
///
/// Returns an error if the HTTP request fails or if the response body cannot
/// be deserialised as JSON.
pub fn fetch_releases() -> Result<Vec<Release>> {
    http::agent()?
        .get(GO_DL_API)
        .call()
        .context("Failed to reach go.dev - check your internet connection")?
        .body_mut()
        .read_json::<Vec<Release>>()
        .context("Failed to parse Go releases JSON")
}

/// Resolves a [`VersionSpec`] to the best-matching stable [`Release`].
///
/// Only stable releases are considered. The API returns releases in
/// newest-first order, so the first match is always the most recent one that
/// satisfies the spec.
///
/// # Errors
///
/// Returns an error if no stable release satisfies the spec, or if the
/// release list is empty.
pub fn resolve<'a>(spec: &VersionSpec, releases: &'a [Release]) -> Result<&'a Release> {
    let mut stable = releases.iter().filter(|r| r.stable);

    match spec {
        VersionSpec::Latest => stable
            .next()
            .ok_or_else(|| anyhow::anyhow!("No stable releases found")),
        _ => stable
            .find(|r| r.go_version().is_some_and(|v| spec.matches(&v)))
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Go version '{}' not found. Run 'gvm list-remote' to see available versions.",
                    spec
                )
            }),
    }
}

/// Builds the download URL for a named archive file from go.dev.
pub fn download_url(filename: &str) -> String {
    format!("https://go.dev/dl/{filename}")
}

/// Returns the go.dev OS identifier for the current compilation target.
///
/// Possible return values: `"windows"`, `"darwin"`, `"linux"`.
pub fn host_os() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "darwin"
    } else {
        "linux"
    }
}

/// Returns the go.dev architecture identifier for the current compilation target.
///
/// Possible return values match Go's `GOARCH` naming: `"amd64"`, `"arm64"`,
/// `"arm"`, `"386"`, `"riscv64"`, `"s390x"`, `"ppc64le"`.
pub fn host_arch() -> &'static str {
    if cfg!(target_arch = "x86_64") {
        "amd64"
    } else if cfg!(target_arch = "aarch64") {
        "arm64"
    } else if cfg!(target_arch = "arm") {
        "arm"
    } else if cfg!(target_arch = "x86") {
        "386"
    } else if cfg!(target_arch = "riscv64") {
        "riscv64"
    } else if cfg!(target_arch = "s390x") {
        "s390x"
    } else if cfg!(target_arch = "powerpc64") {
        "ppc64le"
    } else {
        "386"
    }
}
