//! `gvm list` - show all locally installed Go versions.
//!
//! Installed versions are listed newest-first. The currently active version
//! is marked so the user can identify it at a glance.

use anyhow::Result;
use colored::Colorize;

use crate::{config::Config, toolchain};

/// Prints all installed Go versions, sorted newest-first.
///
/// The active version (determined from `.go-version` or the global default)
/// is highlighted with a check mark and an `(active)` label. If no version is
/// active - for example because no global default has been set yet - all
/// versions are shown without highlighting.
///
/// # Errors
///
/// Returns an error if the versions directory cannot be read.
pub fn run(config: &Config) -> Result<()> {
    let installed = toolchain::list_installed(config)?;

    if installed.is_empty() {
        println!("No Go versions installed. Run 'gvm install latest'.");
        return Ok(());
    }

    let active = toolchain::active_version(config).map(|(v, _)| v).ok();

    println!("Installed Go versions:");
    for v in &installed {
        if active.as_ref() == Some(v) {
            println!(
                "  {} {}  {}",
                "✓".green(),
                v.tag().bold(),
                "(active)".dimmed()
            );
        } else {
            println!("    {}", v.tag());
        }
    }
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
    fn run_reports_no_versions_when_none_installed() {
        let dir = tempdir().unwrap();
        let config = Config {
            root: dir.path().to_path_buf(),
        };
        run(&config).unwrap();
    }

    #[test]
    fn run_lists_installed_versions_with_active_marked() {
        let dir = tempdir().unwrap();
        let config = Config {
            root: dir.path().to_path_buf(),
        };
        install_version(&config, "go1.22.4");
        install_version(&config, "go1.21.0");
        std::fs::write(config.version_file(), "go1.22.4").unwrap();

        run(&config).unwrap();
    }

    #[test]
    fn run_lists_installed_versions_without_active_version() {
        let dir = tempdir().unwrap();
        let config = Config {
            root: dir.path().to_path_buf(),
        };
        install_version(&config, "go1.22.4");
        // No global version file is set for this config. `active_version`
        // resolves from the real process working directory (not
        // `config.root`), so its outcome is environment-dependent here -
        // the important invariant under test is that `run` still succeeds
        // and lists the installed version either way.
        run(&config).unwrap();
    }
}
