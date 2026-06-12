//! `gvm current` - print the active Go version and its source.
//!
//! The source indicates whether the version was determined from a local
//! `.go-version` file (`local (.go-version)`) or from the global default
//! (`global`).

use anyhow::Result;
use colored::Colorize;

use crate::{config::Config, toolchain};

/// Prints the active Go version tag and the source that determined it.
///
/// Example output:
///
/// ```text
/// go1.22.4  (local (.go-version))
/// go1.23.0  (global)
/// ```
///
/// # Errors
///
/// Returns an error if no global default has been set and no `.go-version`
/// file is found in the current directory tree.
pub fn run(config: &Config) -> Result<()> {
    let (version, source) = toolchain::active_version(config)?;
    println!(
        "{} {}",
        version.tag().bold(),
        format!("({})", source.label()).dimmed()
    );
    Ok(())
}
