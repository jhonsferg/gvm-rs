//! Local toolchain storage and version resolution.
//!
//! This module provides the interface between the rest of `gvm` and the
//! on-disk toolchain store at `~/.gvm/versions/`. It handles:
//!
//! - Listing and querying installed versions.
//! - Reading and writing the global default version file.
//! - Walking up the directory tree to find a project-local `.go-version`.
//! - Resolving a [`VersionSpec`] to the best-matching installed [`GoVersion`].

use anyhow::{bail, Context, Result};
use std::path::PathBuf;

use crate::{config::Config, fs as gvm_fs, lock, user_version::VersionSpec, version::GoVersion};

/// Indicates where the active version was determined from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VersionSource {
    /// Activated by a `.go-version` file found in the current directory tree.
    Local,
    /// Activated by the global default stored in `~/.gvm/version`.
    Global,
}

impl VersionSource {
    /// Returns a human-readable label suitable for display.
    pub fn label(self) -> &'static str {
        match self {
            Self::Local => "local (.go-version)",
            Self::Global => "global",
        }
    }
}

// --- Queries -----------------------------------------------------------------

/// Returns all installed Go versions, sorted newest-first.
///
/// Reads the entries of [`Config::versions_dir`] and parses each directory
/// name as a [`GoVersion`]. Non-parsable entries are silently skipped.
///
/// # Errors
///
/// Returns an error if the versions directory cannot be read.
pub fn list_installed(config: &Config) -> Result<Vec<GoVersion>> {
    let dir = config.versions_dir();
    if !dir.exists() {
        return Ok(vec![]);
    }

    let mut versions: Vec<GoVersion> = std::fs::read_dir(&dir)?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .filter_map(|e| {
            e.file_name()
                .into_string()
                .ok()
                .and_then(|n| GoVersion::parse(&n).ok())
        })
        .collect();

    versions.sort_by(|a, b| b.cmp(a));
    Ok(versions)
}

/// Returns `true` if `version` is present in the local toolchain store.
pub fn is_installed(config: &Config, version: &GoVersion) -> bool {
    config.version_dir(&version.tag()).exists()
}

/// Reads the global default Go version from `~/.gvm/version`.
///
/// # Errors
///
/// Returns an error if the file does not exist (i.e. no global version has
/// been set yet) or if its contents cannot be parsed as a valid version.
pub fn global_version(config: &Config) -> Result<GoVersion> {
    let path = config.version_file();
    if !path.exists() {
        bail!("No global Go version set. Run 'gvm use <version>'.");
    }
    let raw = std::fs::read_to_string(&path)?.trim().to_string();
    GoVersion::parse(&raw).context("Corrupted version file - run 'gvm use <version>'")
}

/// Returns the active Go version and the source that determined it.
///
/// The resolution order is:
///
/// 1. Walk up from the current working directory looking for `.go-version`
///    (up to 20 levels to avoid infinite loops on unusual file systems).
/// 2. Fall back to the global default stored in `~/.gvm/version`.
///
/// # Errors
///
/// Returns an error if the current directory cannot be read, if a
/// `.go-version` file contains an invalid version string, or if no global
/// version has been set.
pub fn active_version(config: &Config) -> Result<(GoVersion, VersionSource)> {
    let mut dir = std::env::current_dir()?;
    let mut depth = 0u8;

    loop {
        let vf = dir.join(".go-version");
        if vf.exists() {
            let raw = std::fs::read_to_string(&vf)?.trim().to_string();
            let v = GoVersion::parse(&raw).context("Corrupted .go-version")?;
            return Ok((v, VersionSource::Local));
        }
        // Stop if we have reached the file-system root or the depth limit.
        if depth >= 20 || !dir.pop() {
            break;
        }
        depth += 1;
    }

    global_version(config).map(|v| (v, VersionSource::Global))
}

/// Returns the path to the `bin/` directory of an installed version.
///
/// # Errors
///
/// Returns an error if the version is not installed on disk.
pub fn version_bin_path(config: &Config, version: &GoVersion) -> Result<PathBuf> {
    let bin = config.version_bin_dir(&version.tag());
    if !bin.exists() {
        bail!(
            "Go {} is not installed. Run 'gvm install {}'.",
            version,
            version
        );
    }
    Ok(bin)
}

// --- Mutations ---------------------------------------------------------------

/// Writes `version` to the global default version file (`~/.gvm/version`).
///
/// # Errors
///
/// Returns an error if the file cannot be written (e.g. permission denied).
pub fn set_global_version(config: &Config, version: &GoVersion) -> Result<()> {
    let lock_path = config.root.join(".lock");
    lock::with_lock(&lock_path, || {
        std::fs::write(config.version_file(), version.tag())
            .context("Failed to write global version file")
    })
}

/// Updates the `~/.gvm/current` junction/symlink to point to `version`.
///
/// This is what makes `go` visible to all shells (CMD, Git Bash, PowerShell)
/// and editors (VSCode, GoLand) without any per-shell hook. The PATH entry
/// `~/.gvm/current/bin` is added to the Windows registry once during install;
/// afterwards only the junction target needs to change on every `gvm use`.
///
/// # Errors
///
/// Returns an error if the junction/symlink cannot be created (e.g. the
/// version directory does not exist, or file-system permissions prevent it).
pub fn update_current_link(config: &Config, version: &GoVersion) -> Result<()> {
    let lock_path = config.root.join(".lock");
    lock::with_lock(&lock_path, || {
        let link = config.current_dir();
        let target = config.version_dir(&version.tag());
        gvm_fs::set_version_link(&link, &target)
            .with_context(|| format!("Failed to update current link to {}", version.tag()))
    })
}

// --- Resolution --------------------------------------------------------------

/// Resolves a [`VersionSpec`] to the best-matching installed [`GoVersion`].
///
/// - `Latest` returns the newest installed version.
/// - `Partial` returns the newest installed patch within that minor line.
/// - `Exact` returns the specific version if installed.
///
/// Versions are considered in newest-first order (as returned by
/// [`list_installed`]), so the highest matching patch is always chosen for
/// partial specs.
///
/// # Errors
///
/// Returns an error if no installed version satisfies the spec.
pub fn resolve_installed(config: &Config, spec: &VersionSpec) -> Result<GoVersion> {
    let installed = list_installed(config)?;

    match spec {
        VersionSpec::Latest => installed
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("No Go versions installed.")),
        _ => installed
            .into_iter()
            .find(|v| spec.matches(v))
            .ok_or_else(|| {
                anyhow::anyhow!("Go {} is not installed. Run 'gvm install {}'.", spec, spec)
            }),
    }
}
