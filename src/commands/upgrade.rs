//! `gvm upgrade` - self-update the gvm binary.
//!
//! Queries the GitHub Releases API for the latest `jhonsferg/gvm` release and
//! compares the published tag with the version embedded in this binary at
//! compile time. When a newer version is available the correct platform binary
//! is downloaded and the running executable is replaced in-place.
//!
//! # Replacement strategy
//!
//! - **Unix**: the new binary is downloaded to a temporary file, made
//!   executable, and then atomically moved over the current executable with
//!   `rename(2)`. The old file stays alive until the current process exits
//!   because the kernel keeps its inode open.
//! - **Windows**: the running executable cannot be overwritten directly. The
//!   current binary is first renamed to `gvm.exe.old` (which succeeds even
//!   while in use), then the new binary is moved to the original path. An
//!   immediate deletion of the `.old` file is attempted and silently ignored
//!   if it fails.

use anyhow::{Context, Result};
use colored::Colorize;
use std::path::Path;

use crate::{archive::download, http};

/// GitHub repository slug used to build the Releases API URL.
const REPO: &str = "jhonsferg/gvm";

/// Returns the GitHub API base URL, overridable via `GVM_TEST_API_BASE` for
/// local testing without a real GitHub release.
fn api_base() -> String {
    std::env::var("GVM_TEST_API_BASE").unwrap_or_else(|_| "https://api.github.com".to_owned())
}

/// Returns the GitHub download base URL, overridable via `GVM_TEST_DL_BASE`.
fn dl_base() -> String {
    std::env::var("GVM_TEST_DL_BASE").unwrap_or_else(|_| "https://github.com".to_owned())
}

/// Minimal shape of the GitHub Releases API response.
///
/// Only `tag_name` is needed; the rest of the payload is ignored.
#[derive(serde::Deserialize)]
struct GithubRelease {
    tag_name: String,
}

/// Checks for a newer gvm release and replaces the binary if one is found.
///
/// When `force` is `true` the version comparison is skipped and the latest
/// binary is always downloaded and installed.
///
/// # Errors
///
/// Returns an error if:
/// - The GitHub API cannot be reached or returns an unexpected response.
/// - The binary cannot be downloaded.
/// - The in-place replacement fails (e.g. permission denied).
pub fn run(force: bool) -> Result<()> {
    let current = env!("CARGO_PKG_VERSION");

    println!("{} Checking for updates...", "->".cyan());

    let api_url = format!("{}/repos/{REPO}/releases/latest", api_base());
    let response = http::client()?
        .get(&api_url)
        .send()
        .context("Failed to reach GitHub API - check your internet connection")?;

    if response.status() == reqwest::StatusCode::NOT_FOUND {
        anyhow::bail!(
            "No releases found for {REPO}. \
             The project may not have published a release yet."
        );
    }
    if !response.status().is_success() {
        anyhow::bail!("GitHub API returned HTTP {}", response.status());
    }

    let release: GithubRelease = response
        .json()
        .context("Failed to parse GitHub release response")?;

    // Strip the leading 'v' so we can parse the version components.
    let latest_tag = release.tag_name.trim_start_matches('v');

    let latest = parse_semver(latest_tag)
        .ok_or_else(|| anyhow::anyhow!("Cannot parse version tag '{}'", release.tag_name))?;
    let current_parsed = parse_semver(current)
        .ok_or_else(|| anyhow::anyhow!("Cannot parse current version '{current}'"))?;

    println!("  Current: {}", format!("v{current}").bold());
    println!("  Latest:  {}", format!("v{latest_tag}").bold());

    if !force && current_parsed >= latest {
        println!();
        println!("{} gvm is already up to date.", "✓".green());
        return Ok(());
    }

    let binary_name = release_binary_name();
    // Use the original tag (with 'v' prefix) for the download URL.
    let url = format!(
        "{}/{REPO}/releases/download/{}/{binary_name}",
        dl_base(),
        release.tag_name,
    );

    println!("{} Downloading {}...", "->".cyan(), binary_name.bold());

    let tmp = tmp_path()?;
    if let Err(e) = download::fetch(&url, &tmp) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }

    replace_binary(&tmp)?;

    println!();
    println!(
        "{} gvm upgraded to {}.",
        "✓".green(),
        format!("v{latest_tag}").bold(),
    );
    Ok(())
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Parses a `"MAJOR.MINOR.PATCH"` string into a comparable tuple.
///
/// Returns `None` when the string does not match the expected format.
fn parse_semver(s: &str) -> Option<(u32, u32, u32)> {
    let mut parts = s.split('.').filter_map(|p| p.parse::<u32>().ok());
    let major = parts.next()?;
    let minor = parts.next()?;
    let patch = parts.next()?;
    Some((major, minor, patch))
}

/// Returns the artifact name for the current platform as published on GitHub.
///
/// Naming convention matches the release workflow:
/// `gvm-{os}-{arch}` with an `.exe` suffix on Windows.
fn release_binary_name() -> String {
    let os = if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "darwin"
    } else {
        "linux"
    };

    let arch = if cfg!(target_arch = "x86_64") {
        "x86_64"
    } else {
        "aarch64"
    };

    let ext = if cfg!(windows) { ".exe" } else { "" };

    format!("gvm-{os}-{arch}{ext}")
}

/// Returns a path to a temporary file for writing the downloaded binary.
///
/// The file name includes the current process ID to avoid collisions when
/// multiple upgrade processes run concurrently.
fn tmp_path() -> Result<std::path::PathBuf> {
    Ok(std::env::temp_dir().join(format!("gvm-upgrade-{}", std::process::id())))
}

/// Replaces the current gvm executable with the file at `new_binary`.
///
/// On Unix the new binary is first copied to a hidden temp file inside the
/// same directory as the target executable (so that the final `rename(2)` is
/// guaranteed to be same-filesystem and therefore atomic). `/tmp` is often a
/// separate `tmpfs`, which would cause `rename(2)` to fail with `EXDEV` if
/// used directly. On Windows the running binary is renamed first (freeing the
/// name while keeping the file in use) and then the new binary takes the
/// original name.
///
/// # Errors
///
/// Returns an error if the executable path cannot be determined, if staging,
/// `chmod`, or `rename` fails, or if the Windows rename pair fails.
fn replace_binary(new_binary: &Path) -> Result<()> {
    let exe = std::env::current_exe().context("Cannot determine gvm binary location")?;

    // ── Unix: copy-to-same-fs then atomic rename ──────────────────────────────
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        // Stage the new binary next to the target so rename(2) stays on the
        // same filesystem.
        let staged = exe.with_file_name(format!(".gvm-upgrade-{}", std::process::id()));

        if let Err(e) = std::fs::copy(new_binary, &staged) {
            let _ = std::fs::remove_file(new_binary);
            return Err(e).context("Cannot stage new binary in install directory");
        }
        let _ = std::fs::remove_file(new_binary);

        // Set the executable bit before making the file visible at its final
        // path so it is never world-visible in a non-executable state.
        std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755))
            .context("Cannot set execute permission on new binary")?;

        if let Err(e) = std::fs::rename(&staged, &exe) {
            let _ = std::fs::remove_file(&staged);
            return Err(e).with_context(|| format!("Cannot replace {}", exe.display()));
        }
    }

    // ── Windows: two-step rename ──────────────────────────────────────────────
    #[cfg(windows)]
    {
        let old = exe.with_file_name("gvm.exe.old");
        // Rename the running binary to free its name (succeeds even while in use).
        std::fs::rename(&exe, &old)
            .with_context(|| format!("Cannot rename current binary {}", exe.display()))?;

        if let Err(e) = std::fs::rename(new_binary, &exe) {
            // Roll back so gvm keeps working.
            let _ = std::fs::rename(&old, &exe);
            let _ = std::fs::remove_file(new_binary);
            return Err(e).context("Cannot place new binary - rolled back to previous version");
        }

        // Best-effort cleanup; silently ignored if the file is still
        // memory-mapped by this process.
        let _ = std::fs::remove_file(&old);
    }

    Ok(())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::{parse_semver, release_binary_name};

    #[test]
    fn semver_parses_correctly() {
        assert_eq!(parse_semver("1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse_semver("0.1.0"), Some((0, 1, 0)));
        assert_eq!(parse_semver("10.0.0"), Some((10, 0, 0)));
        assert_eq!(parse_semver("bad"), None);
        assert_eq!(parse_semver("1.2"), None);
    }

    #[test]
    fn semver_ordering() {
        assert!(parse_semver("0.2.0") > parse_semver("0.1.9"));
        assert!(parse_semver("1.0.0") > parse_semver("0.99.99"));
        assert_eq!(parse_semver("1.0.0"), parse_semver("1.0.0"));
    }

    #[test]
    fn binary_name_has_correct_format() {
        let name = release_binary_name();
        assert!(name.starts_with("gvm-"));
        assert!(name.contains("x86_64") || name.contains("aarch64"));
        #[cfg(windows)]
        assert!(name.ends_with(".exe"));
        #[cfg(not(windows))]
        assert!(!name.ends_with(".exe"));
    }
}
