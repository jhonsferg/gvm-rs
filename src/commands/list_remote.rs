//! `gvm list-remote` - list Go versions available from go.dev.
//!
//! By default shows only the latest patch release per minor version (compact
//! view). Passing `--all` shows every patch release in the index.
//! Already-installed versions are marked so the user can see what is new
//! at a glance.

use anyhow::Result;
use colored::Colorize;

use crate::{config::Config, http::HttpClient, remote::index, toolchain};

/// Fetches the go.dev release index and prints available stable versions.
///
/// When `all` is `false` (the default), only the highest patch release for
/// each minor version is shown, keeping the list concise. When `all` is
/// `true`, every stable patch release is listed.
///
/// Versions already installed locally are prefixed with `✓` to distinguish
/// them from versions available for download.
///
/// # Errors
///
/// Returns an error if the remote release index cannot be fetched.
pub fn run(config: &Config, client: &HttpClient, all: bool) -> Result<()> {
    println!("{} Fetching available Go versions...", "->".cyan());

    let releases = index::fetch_releases(client)?;
    let installed = toolchain::list_installed(config)?;

    let stable: Vec<_> = releases.iter().filter(|r| r.stable).collect();

    let to_show: Vec<_> = if all {
        stable
    } else {
        // Deduplicate: keep only the first (newest) release per major.minor pair.
        let mut seen = std::collections::HashSet::new();
        stable
            .into_iter()
            .filter(|r| {
                r.go_version()
                    .is_some_and(|v| seen.insert((v.major, v.minor)))
            })
            .collect()
    };

    println!(
        "Available Go versions{}:",
        if all {
            " (all patches)"
        } else {
            " (latest per minor)"
        }
    );

    for r in &to_show {
        let is_installed = installed.iter().any(|v| v.tag() == r.version);
        let mark = if is_installed {
            format!("{}", "✓".green())
        } else {
            " ".to_string()
        };
        println!("  {mark} {}", r.version);
    }

    if !all {
        println!(
            "\n  Use {} to see all patch releases.",
            "gvm list-remote --all".cyan()
        );
    }
    Ok(())
}
