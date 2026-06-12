//! `gvm outdated` - check which installed Go versions have newer patch releases.
//!
//! Compares every locally installed version against the latest available patch
//! for that same major.minor line on go.dev and reports the status so the user
//! can decide which versions to update or remove.

use anyhow::Result;
use colored::Colorize;

use crate::{config::Config, remote::index, toolchain};

/// Checks installed Go versions against go.dev and prints their update status.
///
/// For each installed version the function finds the newest available patch
/// release with the same major and minor version. If a newer patch exists the
/// version is reported as behind; otherwise it is reported as up to date.
///
/// # Errors
///
/// Returns an error if the versions directory cannot be read or if the go.dev
/// release index cannot be fetched.
pub fn run(config: &Config) -> Result<()> {
    let installed = toolchain::list_installed(config)?;

    if installed.is_empty() {
        println!("No Go versions installed. Run 'gvm install latest'.");
        return Ok(());
    }

    println!("{} Fetching available Go versions...", "->".cyan());
    let releases = index::fetch_releases()?;

    // Only consider stable releases; collect into owned Vec so we can compare.
    let stable: Vec<_> = releases
        .iter()
        .filter(|r| r.stable)
        .filter_map(|r| r.go_version())
        .collect();

    println!();
    println!(
        "  {:<14} {:<14} {}",
        "Installed".bold(),
        "Latest patch".bold(),
        "Status".bold()
    );
    println!("  {}", "─".repeat(50));

    let mut any_outdated = false;

    for v in &installed {
        // Highest patch release for the same major.minor on go.dev.
        let latest = stable
            .iter()
            .filter(|rv| rv.major == v.major && rv.minor == v.minor)
            .max()
            .cloned();

        match latest {
            None => {
                // Probably a very old minor no longer listed on go.dev.
                println!("  {:<14} {:<14} {}", v.tag(), "-", "?  no data".dimmed());
            }
            Some(ref latest) if v >= latest => {
                println!(
                    "  {:<14} {:<14} {}",
                    v.tag(),
                    latest.tag(),
                    format!("{}  up to date", "✓").green()
                );
            }
            Some(ref latest) => {
                let diff = latest.patch.saturating_sub(v.patch);
                println!(
                    "  {:<14} {:<14} {}",
                    v.tag(),
                    latest.tag(),
                    format!(
                        "⚠  {} patch{} behind",
                        diff,
                        if diff == 1 { "" } else { "es" }
                    )
                    .yellow()
                );
                any_outdated = true;
            }
        }
    }

    println!();
    if any_outdated {
        println!(
            "  Run {} to update a version, or {} to remove it.",
            "gvm install <version>".cyan(),
            "gvm uninstall <version>".cyan()
        );
    } else {
        println!("  {} All installed versions are up to date.", "✓".green());
    }

    Ok(())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use crate::version::GoVersion;

    // Helper: parse a GoVersion, panicking if the string is invalid (test only).
    fn v(s: &str) -> GoVersion {
        GoVersion::parse(s).unwrap()
    }

    #[test]
    fn version_ordering_used_for_latest() {
        let mut versions = [v("go1.21.3"), v("go1.21.13"), v("go1.21.1")];
        versions.sort();
        assert_eq!(versions.last().unwrap(), &v("go1.21.13"));
    }

    #[test]
    fn patch_diff_calculation() {
        let installed = v("go1.21.0");
        let latest = v("go1.21.13");
        let diff = latest.patch.saturating_sub(installed.patch);
        assert_eq!(diff, 13);
    }

    #[test]
    fn up_to_date_detection() {
        let installed = v("go1.23.4");
        let latest = v("go1.23.4");
        assert!(installed >= latest);
    }

    #[test]
    fn outdated_detection() {
        let installed = v("go1.22.4");
        let latest = v("go1.22.12");
        assert!(installed < latest);
    }

    #[test]
    fn version_without_patch_compared_correctly() {
        // go1.22 (patch = 0) should be behind go1.22.1
        let installed = v("go1.22");
        let latest = v("go1.22.12");
        assert!(installed < latest);
        let diff = latest.patch.saturating_sub(installed.patch);
        assert_eq!(diff, 12);
    }
}
