//! Functions for fetching and querying the go.dev release index.
//!
//! This module is the single point of contact with the remote API. All network
//! requests made by `gvm` go through [`fetch_releases`].

use anyhow::{Context, Result};

use crate::{http::HttpClient, remote::release::Release, user_version::VersionSpec};

/// URL of the go.dev download API that lists all releases.
const GO_DL_API: &str = "https://go.dev/dl/?mode=json&include=all";

/// Downloads the complete list of Go releases from go.dev.
///
/// The API returns both stable and unstable (RC/beta) releases; callers are
/// responsible for filtering by [`Release::stable`] if needed.
///
/// When verbose mode is active, the request and response details are printed
/// to stderr before the JSON body is parsed.
///
/// # Errors
///
/// Returns an error if the HTTP request fails or if the response body cannot
/// be deserialised as JSON.
pub fn fetch_releases(client: &HttpClient) -> Result<Vec<Release>> {
    crate::http::log_request(client, "GET", GO_DL_API);

    let mut response = client
        .agent()
        .get(GO_DL_API)
        .call()
        .context("Failed to reach go.dev - check your internet connection")?;

    crate::http::log_response(
        client,
        response.status().as_u16(),
        response.status().canonical_reason().unwrap_or(""),
        response.headers(),
    );

    response
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::release::{Release, ReleaseFile};

    fn release(version: &str, stable: bool) -> Release {
        Release {
            version: version.to_string(),
            stable,
            files: vec![ReleaseFile {
                filename: format!("{version}.linux-amd64.tar.gz"),
                os: "linux".to_string(),
                arch: "amd64".to_string(),
                sha256: "deadbeef".to_string(),
                size: 123,
                kind: "archive".to_string(),
            }],
        }
    }

    #[test]
    fn resolve_latest_returns_first_stable_release() {
        let releases = vec![
            release("go1.23.0", true),
            release("go1.22.4", true),
            release("go1.22.3", true),
        ];
        let resolved = resolve(&VersionSpec::Latest, &releases).unwrap();
        assert_eq!(resolved.version, "go1.23.0");
    }

    #[test]
    fn resolve_latest_skips_unstable_releases() {
        let releases = vec![release("go1.24.0rc1", false), release("go1.23.0", true)];
        let resolved = resolve(&VersionSpec::Latest, &releases).unwrap();
        assert_eq!(resolved.version, "go1.23.0");
    }

    #[test]
    fn resolve_latest_errors_when_no_stable_releases() {
        let releases = vec![release("go1.24.0rc1", false)];
        let err = resolve(&VersionSpec::Latest, &releases).unwrap_err();
        assert!(err.to_string().contains("No stable releases"));
    }

    #[test]
    fn resolve_partial_matches_newest_patch() {
        let releases = vec![
            release("go1.23.0", true),
            release("go1.22.5", true),
            release("go1.22.4", true),
        ];
        let spec = VersionSpec::Partial {
            major: 1,
            minor: 22,
        };
        let resolved = resolve(&spec, &releases).unwrap();
        assert_eq!(resolved.version, "go1.22.5");
    }

    #[test]
    fn resolve_exact_requires_full_match() {
        let releases = vec![release("go1.22.4", true), release("go1.22.5", true)];
        let spec = VersionSpec::Exact {
            major: 1,
            minor: 22,
            patch: 4,
        };
        let resolved = resolve(&spec, &releases).unwrap();
        assert_eq!(resolved.version, "go1.22.4");
    }

    #[test]
    fn resolve_errors_with_helpful_message_when_not_found() {
        let releases = vec![release("go1.22.4", true)];
        let spec = VersionSpec::Exact {
            major: 9,
            minor: 9,
            patch: 9,
        };
        let err = resolve(&spec, &releases).unwrap_err();
        assert!(err.to_string().contains("not found"));
        assert!(err.to_string().contains("list-remote"));
    }

    #[test]
    fn resolve_ignores_unstable_release_for_partial_and_exact() {
        let releases = vec![release("go1.22.4", false)];
        let partial = VersionSpec::Partial {
            major: 1,
            minor: 22,
        };
        assert!(resolve(&partial, &releases).is_err());
    }

    #[test]
    fn download_url_builds_expected_link() {
        assert_eq!(
            download_url("go1.22.4.linux-amd64.tar.gz"),
            "https://go.dev/dl/go1.22.4.linux-amd64.tar.gz"
        );
    }

    #[test]
    fn host_os_matches_a_known_platform() {
        assert!(["windows", "darwin", "linux"].contains(&host_os()));
    }

    #[test]
    fn host_arch_matches_a_known_architecture() {
        assert!(
            ["amd64", "arm64", "arm", "386", "riscv64", "s390x", "ppc64le"].contains(&host_arch())
        );
    }
}
