//! Default packages installation for `gvm install`.

use anyhow::Result;
use colored::Colorize;

use crate::{config::Config, version::GoVersion};

/// Reads `~/.gvm/default-packages` and runs `go install <pkg>` for each entry.
///
/// Errors from individual package installs are printed as warnings rather than
/// propagated so they never block the overall `gvm install` flow.
pub fn install_default_packages(config: &Config, version: &GoVersion) {
    let pkg_file = config.default_packages_file();
    if !pkg_file.exists() {
        return;
    }

    let content = match std::fs::read_to_string(&pkg_file) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{} Could not read default-packages: {e}", "!".yellow());
            return;
        }
    };

    let packages: Vec<&str> = content
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect();

    if packages.is_empty() {
        return;
    }

    let go_exe = if cfg!(windows) { "go.exe" } else { "go" };
    let go_bin = config.version_bin_dir(&version.tag()).join(go_exe);

    println!("{} Installing default packages...", "->".cyan());

    let sep = if cfg!(windows) { ";" } else { ":" };
    let new_path = format!(
        "{}{}{}",
        config.version_bin_dir(&version.tag()).display(),
        sep,
        std::env::var("PATH").unwrap_or_default()
    );

    for pkg in &packages {
        use std::io::Write as _;
        print!("    {} {}... ", "→".cyan(), pkg);
        let _ = std::io::stdout().flush();

        let result = std::process::Command::new(&go_bin)
            .arg("install")
            .arg(pkg)
            .env("GOROOT", config.version_dir(&version.tag()))
            .env("PATH", &new_path)
            .output();

        match result {
            Ok(o) if o.status.success() => println!("{}", "✓".green()),
            Ok(o) => {
                println!("{}", "✗".red());
                let stderr = String::from_utf8_lossy(&o.stderr);
                eprintln!("      {}", stderr.trim());
            }
            Err(e) => {
                println!("{}", "✗".red());
                eprintln!("      {e}");
            }
        }
    }
}
