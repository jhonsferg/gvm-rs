//! `gvm uninstall` - remove an installed Go version from disk.
//!
//! The command refuses to uninstall the currently active version to avoid
//! leaving the environment in a broken state.

use anyhow::{bail, Result};
use colored::Colorize;

use crate::{config::Config, toolchain, user_version::VersionSpec};

/// Removes the Go version described by `spec_str` from the local store.
///
/// The function first checks whether the target version is the currently
/// active one (via `.go-version` or global default). If it is, the command
/// aborts with an error to protect the user from an unbootable environment.
///
/// # Errors
///
/// Returns an error if:
/// - `spec_str` is not a valid version spec.
/// - No installed version matches the spec.
/// - The target is the currently active version.
/// - The version directory cannot be removed.
pub fn run(config: &Config, spec_str: &str) -> Result<()> {
    let spec = VersionSpec::parse(spec_str)?;
    let version = toolchain::resolve_installed(config, &spec)?;

    // Protect against uninstalling the active version to avoid leaving
    // the environment pointing at a non-existent toolchain.
    if let Ok((active, _)) = toolchain::active_version(config) {
        if active == version {
            bail!(
                "Cannot uninstall the currently active version ({}). \
                 Switch first with 'gvm use <version>'.",
                version.tag()
            );
        }
    }

    let dir = config.version_dir(&version.tag());
    std::fs::remove_dir_all(&dir)?;

    println!("{} Go {} uninstalled.", "✓".green(), version.tag().bold());
    Ok(())
}
