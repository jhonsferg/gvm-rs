//! User configuration and directory layout.
//!
//! [`Config`] is the single source of truth for every path that `gvm` reads
//! from or writes to. All other modules receive a `&Config` reference rather
//! than constructing paths themselves, which keeps the directory layout easy
//! to change in one place.
//!
//! The root directory defaults to `~/.gvm` and can be overridden with the
//! `GVM_DIR` environment variable.

use anyhow::{Context, Result};
use std::path::PathBuf;

/// Runtime configuration loaded at startup.
///
/// All path helpers are derived from the single [`root`](Config::root) field
/// so the entire directory tree moves atomically when `GVM_DIR` is set.
#[derive(Debug)]
pub struct Config {
    /// Root directory for all gvm data (default: `~/.gvm`).
    pub root: PathBuf,
}

impl Config {
    /// Loads configuration from the environment.
    ///
    /// The root directory is taken from `GVM_DIR` if set; otherwise it
    /// defaults to `~/.gvm`.
    ///
    /// # Errors
    ///
    /// Returns an error if the home directory cannot be determined and `GVM_DIR`
    /// is not set.
    pub fn load() -> Result<Self> {
        let root = match std::env::var("GVM_DIR") {
            Ok(dir) => PathBuf::from(dir),
            Err(_) => dirs::home_dir()
                .context("Cannot determine home directory")?
                .join(".gvm"),
        };
        Ok(Self { root })
    }

    /// Returns the directory that holds all installed Go versions.
    ///
    /// Layout: `<root>/versions/`
    pub fn versions_dir(&self) -> PathBuf {
        self.root.join("versions")
    }

    /// Returns the staging directory used during downloads and extraction.
    ///
    /// Layout: `<root>/tmp/`
    pub fn tmp_dir(&self) -> PathBuf {
        self.root.join("tmp")
    }

    /// Returns the path to the file that records the global default version.
    ///
    /// Layout: `<root>/version` (plain text, e.g. `go1.22.4`)
    pub fn version_file(&self) -> PathBuf {
        self.root.join("version")
    }

    /// Returns the installation directory for a specific Go version tag.
    ///
    /// Layout: `<root>/versions/<tag>/`  (e.g. `~/.gvm/versions/go1.22.4/`)
    pub fn version_dir(&self, tag: &str) -> PathBuf {
        self.versions_dir().join(tag)
    }

    /// Returns the `bin/` subdirectory for a specific Go version tag.
    ///
    /// Layout: `<root>/versions/<tag>/bin/`
    pub fn version_bin_dir(&self, tag: &str) -> PathBuf {
        self.version_dir(tag).join("bin")
    }

    /// Returns the path to the `current` junction/symlink that always points to
    /// the active Go version directory.
    ///
    /// Adding `<root>/current/bin` to the OS PATH once (during install) makes
    /// `go` available in every shell - CMD, Git Bash, editors - without session
    /// hooks. `gvm use` updates this link; the PATH entry never changes.
    ///
    /// Layout: `<root>/current` -> `<root>/versions/<active-tag>/`
    pub fn current_dir(&self) -> PathBuf {
        self.root.join("current")
    }

    /// Returns the path to the optional default-packages file.
    ///
    /// When this file exists, `gvm install` reads it after a successful
    /// installation and runs `go install <pkg>` for each non-blank, non-comment
    /// line. This mirrors mise's `MISE_GO_DEFAULT_PACKAGES_FILE` feature.
    ///
    /// Layout: `<root>/default-packages`
    pub fn default_packages_file(&self) -> PathBuf {
        self.root.join("default-packages")
    }

    /// Creates the [`versions_dir`](Self::versions_dir) and
    /// [`tmp_dir`](Self::tmp_dir) directories if they do not already exist.
    ///
    /// # Errors
    ///
    /// Returns an error if either directory cannot be created (e.g. permission
    /// denied).
    pub fn ensure_dirs(&self) -> Result<()> {
        std::fs::create_dir_all(self.versions_dir())
            .context("Failed to create versions directory")?;
        std::fs::create_dir_all(self.tmp_dir()).context("Failed to create tmp directory")?;
        Ok(())
    }
}
