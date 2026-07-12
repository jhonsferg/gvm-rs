//! `gvm uninstall` - remove an installed Go version from disk.
//!
//! The command refuses to uninstall the currently active version to avoid
//! leaving the environment in a broken state.

use anyhow::{bail, Result};
use colored::Colorize;

use crate::{config::Config, lock, toolchain, user_version::VersionSpec};

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
    let lock_path = config.root.join(".lock");
    lock::with_lock(&lock_path, || Ok(std::fs::remove_dir_all(&dir)?))?;

    println!("{} Go {} uninstalled.", "✓".green(), version.tag().bold());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn install_version(config: &Config, tag: &str) {
        std::fs::create_dir_all(config.version_dir(tag)).unwrap();
    }

    #[test]
    fn run_removes_installed_version_directory() {
        let dir = tempdir().unwrap();
        let config = Config {
            root: dir.path().to_path_buf(),
        };
        // Use a tag distinct from any ambient locally-active version so this
        // test is not coupled to the real process working directory's
        // `.go-version` resolution.
        install_version(&config, "go1.19.9");

        run(&config, "1.19.9").unwrap();

        assert!(!config.version_dir("go1.19.9").exists());
    }

    #[test]
    fn run_errors_when_version_not_installed() {
        let dir = tempdir().unwrap();
        let config = Config {
            root: dir.path().to_path_buf(),
        };
        assert!(run(&config, "1.19.9").is_err());
    }

    #[test]
    fn run_errors_on_invalid_spec() {
        let dir = tempdir().unwrap();
        let config = Config {
            root: dir.path().to_path_buf(),
        };
        assert!(run(&config, "not-a-version").is_err());
    }
}
