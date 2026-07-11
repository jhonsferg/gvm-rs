//! `gvm install` - download and install a Go version.
//!
//! The install process follows these steps:
//!
//! 1. Resolve the [`VersionSpec`] against the go.dev release index.
//! 2. Skip installation if the version is already present (unless `--force`).
//! 3. Download the appropriate archive for the current OS and architecture.
//! 4. Verify the SHA-256 checksum.
//! 5. Extract the archive to a staging directory.
//! 6. Move the extracted tree to its final location in the versions store.
//! 7. Install user-defined default packages (e.g. gopls, dlv) if configured.
//!
//! Temporary files (archive and extracted tree) are cleaned up on any error
//! so partial installs do not leave the store in a broken state.

mod default_packages;
mod download;
mod extract;

use anyhow::{anyhow, Context, Result};
use colored::Colorize;

use crate::{
    config::{Config, ConfigMut},
    fs as gvm_fs,
    http::HttpClient,
    lock,
    remote::index,
    tempdir::TempDir,
    toolchain,
    user_version::VersionSpec,
};

/// Downloads and installs the Go version described by `spec_str`.
///
/// When `force` is `true` the existing installation is removed first, allowing
/// a clean reinstall. When `force` is `false` and the version is already
/// installed, the function returns early with a hint to use `--force`.
///
/// # Errors
///
/// Returns an error if:
/// - `spec_str` is not a valid version spec.
/// - The go.dev release index cannot be fetched.
/// - No release matches the spec.
/// - The archive download fails or the checksum does not match.
/// - Extraction fails.
/// - The extracted directory cannot be moved to the versions store.
pub fn run(config: &Config, client: &HttpClient, spec_str: &str, force: bool) -> Result<()> {
    config.ensure_dirs()?;

    let spec = VersionSpec::parse(spec_str)?;

    println!("{} Fetching available Go versions...", "->".cyan());
    let releases = index::fetch_releases(client)?;
    let release = index::resolve(&spec, &releases)?;

    let version = release
        .go_version()
        .ok_or_else(|| anyhow!("Could not parse version tag '{}'", release.version))?;

    // Bail out early if already installed (unless --force).
    if toolchain::is_installed(config, &version) {
        if force {
            println!(
                "{} Reinstalling Go {}...",
                "->".cyan(),
                version.tag().bold()
            );
            let lock_path = config.root.join(".lock");
            lock::with_lock(&lock_path, || {
                Ok(std::fs::remove_dir_all(config.version_dir(&version.tag()))?)
            })?;
        } else {
            println!(
                "{} Go {} is already installed.",
                "✓".green(),
                version.tag().bold()
            );
            println!("  Use {} to reinstall.", "--force".cyan());
            return Ok(());
        }
    }

    // Download and verify the archive.
    let archive = download::download_archive(client, config, &release, &version)?;

    // Extract to staging directory.
    let source_root = extract::extract_archive(&archive.path, config, &version.tag())?;

    // Move the compiled tree to the versions store.
    let dest = config.version_dir(&version.tag());
    let lock_path = config.root.join(".lock");
    lock::with_lock(&lock_path, || crate::fs::move_dir(&source_root, &dest))?;

    // Cleanup
    let _ = std::fs::remove_file(&archive.path);

    println!(
        "{} Go {} installed successfully.",
        "✓".green(),
        version.tag().bold()
    );
    println!(
        "  Run {} to activate.",
        format!("gvm use {}", version).cyan()
    );

    // Install user-defined default packages (e.g. gopls, dlv) if configured.
    crate::commands::install::default_packages::install_default_packages(config, &version);

    Ok(())
}
