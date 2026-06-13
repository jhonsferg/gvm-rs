//! `gvm upgrade` - self-update the gvm binary.
//!
//! Queries the GitHub Releases API for the latest `jhonsferg/gvm` release and
//! compares the published tag with the version embedded in this binary at
//! compile time. When a newer version is available the correct platform archive
//! is downloaded, the `gvm` binary is extracted from it, and the running
//! executable is replaced in-place.
//!
//! # Replacement strategy
//!
//! - **Unix**: the archive is downloaded to a temp file, `gvm` is extracted,
//!   staged next to the target executable, and then atomically moved over the
//!   current executable with `rename(2)`.
//! - **Windows**: the archive is downloaded to a temp `.zip`, `gvm.exe` is
//!   extracted, the running binary is renamed (freeing the name while it stays
//!   in use), then the new binary is moved to the original path.

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
/// - The archive cannot be downloaded or extracted.
/// - The in-place replacement fails (e.g. permission denied).
pub fn run(force: bool) -> Result<()> {
    let current = env!("CARGO_PKG_VERSION");

    println!("{} Checking for updates...", "->".cyan());

    let api_url = format!("{}/repos/{REPO}/releases/latest", api_base());
    let mut response = match http::agent()?.get(&api_url).call() {
        Ok(r) => r,
        Err(ureq::Error::StatusCode(404)) => anyhow::bail!(
            "No releases found for {REPO}. \
             The project may not have published a release yet."
        ),
        Err(e) => {
            return Err(anyhow::anyhow!(e))
                .context("Failed to reach GitHub API - check your internet connection")
        }
    };

    let release: GithubRelease = response
        .body_mut()
        .read_json()
        .context("Failed to parse GitHub release response")?;

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

    let archive_name = release_archive_name();
    let url = format!(
        "{}/{REPO}/releases/download/{}/{archive_name}",
        dl_base(),
        release.tag_name,
    );

    println!("{} Downloading {}...", "->".cyan(), archive_name.bold());

    let tmp_archive = tmp_archive_path()?;
    if let Err(e) = download::fetch(&url, &tmp_archive) {
        let _ = std::fs::remove_file(&tmp_archive);
        return Err(e);
    }

    let tmp_bin = extract_upgrade_binary(&tmp_archive);
    let _ = std::fs::remove_file(&tmp_archive);
    let tmp_bin = tmp_bin?;

    if let Err(e) = replace_binary(&tmp_bin) {
        let _ = std::fs::remove_file(&tmp_bin);
        return Err(e);
    }

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

/// Returns the release archive name for the current platform and architecture.
///
/// Convention: `gvm_{os}_{arch}.tar.gz` on Unix, `gvm_{os}_{arch}.zip` on
/// Windows. Arch names follow Go's `GOARCH` naming except that Linux/Darwin
/// use `aarch64` for ARM64 to match the binary names in the release matrix.
/// Windows ARM64 uses `arm64` to match Go's own naming.
fn release_archive_name() -> String {
    let os = if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "darwin"
    } else if cfg!(target_os = "android") {
        "android"
    } else {
        "linux"
    };

    let arch = if cfg!(target_arch = "x86_64") {
        "x86_64"
    } else if cfg!(target_arch = "aarch64") {
        if cfg!(target_os = "windows") {
            "arm64"
        } else {
            "aarch64"
        }
    } else if cfg!(target_arch = "arm") {
        "armv7"
    } else if cfg!(target_arch = "x86") {
        "386"
    } else if cfg!(target_arch = "riscv64") {
        "riscv64"
    } else if cfg!(target_arch = "s390x") {
        "s390x"
    } else if cfg!(target_arch = "powerpc64") {
        "ppc64le"
    } else {
        "x86_64"
    };

    let ext = if cfg!(windows) { ".zip" } else { ".tar.gz" };

    format!("gvm_{os}_{arch}{ext}")
}

/// Returns a temp path for the downloaded archive file.
fn tmp_archive_path() -> Result<std::path::PathBuf> {
    let ext = if cfg!(windows) { ".zip" } else { ".tar.gz" };
    Ok(std::env::temp_dir().join(format!("gvm-upgrade-{}{ext}", std::process::id())))
}

/// Extracts the `gvm` (or `gvm.exe`) binary from the downloaded archive into
/// a new temporary file and returns its path.
///
/// The extraction is self-contained: no system `tar`, `unzip`, or similar
/// tool is required. The `flate2`, `tar`, and `zip` crates do the work.
fn extract_upgrade_binary(archive: &Path) -> Result<std::path::PathBuf> {
    let out = std::env::temp_dir().join(format!("gvm-upgrade-bin-{}", std::process::id()));

    #[cfg(not(windows))]
    {
        let binary_filename = "gvm";

        let file = std::fs::File::open(archive)
            .with_context(|| format!("Cannot open archive {}", archive.display()))?;
        let gz = flate2::read::GzDecoder::new(file);
        let mut ar = tar::Archive::new(gz);

        for entry in ar.entries().context("Failed to read tar entries")? {
            let mut entry = entry.context("Failed to read tar entry")?;
            let name = entry
                .path()
                .context("Invalid tar entry path")?
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_owned();

            if name == binary_filename {
                entry
                    .unpack(&out)
                    .context("Failed to extract gvm binary from archive")?;

                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&out, std::fs::Permissions::from_mode(0o755))
                    .context("Cannot set execute permission on extracted binary")?;

                return Ok(out);
            }
        }

        anyhow::bail!("Archive did not contain a file named '{binary_filename}'");
    }

    #[cfg(windows)]
    {
        let binary_filename = "gvm.exe";

        let file = std::fs::File::open(archive)
            .with_context(|| format!("Cannot open archive {}", archive.display()))?;
        let mut zip = zip::ZipArchive::new(file).context("Failed to read zip archive")?;

        for i in 0..zip.len() {
            let mut entry = zip.by_index(i).context("Failed to read zip entry")?;
            if entry.name() == binary_filename {
                let mut dest =
                    std::fs::File::create(&out).context("Cannot create temp binary file")?;
                std::io::copy(&mut entry, &mut dest)
                    .context("Failed to extract gvm.exe from zip")?;
                return Ok(out);
            }
        }

        anyhow::bail!("Archive did not contain a file named '{binary_filename}'");
    }
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
fn replace_binary(new_binary: &Path) -> Result<()> {
    let exe = std::env::current_exe().context("Cannot determine gvm binary location")?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let staged = exe.with_file_name(format!(".gvm-upgrade-{}", std::process::id()));

        if let Err(e) = std::fs::copy(new_binary, &staged) {
            let _ = std::fs::remove_file(new_binary);
            return Err(e).context("Cannot stage new binary in install directory");
        }
        let _ = std::fs::remove_file(new_binary);

        std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755))
            .context("Cannot set execute permission on new binary")?;

        if let Err(e) = std::fs::rename(&staged, &exe) {
            let _ = std::fs::remove_file(&staged);
            return Err(e).with_context(|| format!("Cannot replace {}", exe.display()));
        }
    }

    #[cfg(windows)]
    {
        let old = exe.with_file_name("gvm.exe.old");
        std::fs::rename(&exe, &old)
            .with_context(|| format!("Cannot rename current binary {}", exe.display()))?;

        if let Err(e) = std::fs::rename(new_binary, &exe) {
            let _ = std::fs::rename(&old, &exe);
            let _ = std::fs::remove_file(new_binary);
            return Err(e).context("Cannot place new binary - rolled back to previous version");
        }

        let _ = std::fs::remove_file(&old);
    }

    Ok(())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::{parse_semver, release_archive_name};

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
    fn archive_name_has_correct_format() {
        let name = release_archive_name();
        assert!(name.starts_with("gvm_"));
        #[cfg(windows)]
        assert!(name.ends_with(".zip"));
        #[cfg(not(windows))]
        assert!(name.ends_with(".tar.gz"));
    }
}
