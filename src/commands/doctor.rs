//! `gvm doctor` - diagnose the gvm environment.
//!
//! Runs a series of checks and reports the result of each one. Exits with
//! status code `1` if any check fails, making the command suitable for CI
//! health checks and pre-commit hooks.
//!
//! # Checks performed
//!
//! 1. The `gvm` binary directory is on `PATH`.
//! 2. A global Go version has been set.
//! 3. The global version is installed on disk.
//! 4. `GOROOT` resolves to the correct directory.
//! 5. The current directory's `.go-version` (if present) refers to an
//!    installed version.
//! 6. The `gvm env` hook is present in the detected shell's profile.
//! 7. Only one `go` binary is active in `PATH` and it is gvm-managed.

use anyhow::Result;
use colored::Colorize;

use crate::{config::Config, shell, toolchain};

/// Runs all environment checks and prints a summary.
///
/// When `shell_str` is provided it overrides shell auto-detection for the
/// profile check. The function exits the process with code `1` if any issue
/// is found.
///
/// # Errors
///
/// This function only returns `Err` for unexpected I/O failures. Diagnostic
/// failures are reported to stdout and tracked in the `issues` counter rather
/// than being propagated as errors.
pub fn run(config: &Config, shell_str: Option<&str>) -> Result<()> {
    println!("Checking gvm environment...\n");
    let mut issues = 0u32;
    let go_name = if cfg!(windows) { "go.exe" } else { "go" };

    // ---- Check 1: gvm binary on PATH ----------------------------------------
    if shell::gvm_in_path() {
        ok("gvm binary is in PATH");
    } else {
        let dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.display().to_string()))
            .unwrap_or_else(|| "<unknown>".to_string());
        fail(&format!("gvm is NOT in PATH  (add '{dir}' to your PATH)"));
        issues += 1;
    }

    // ---- Check 2 & 3: global version set and installed ----------------------
    match toolchain::global_version(config) {
        Err(_) => {
            fail("No global Go version set  (run 'gvm use <version>')");
            issues += 1;
        }
        Ok(v) => {
            ok(&format!("Global version: {}", v.tag().bold()));

            if toolchain::is_installed(config, &v) {
                ok(&format!("{} is installed", v.tag()));

                // ---- Check 4: GOROOT directory exists -----------------------
                let root = config.version_dir(&v.tag());
                ok(&format!("GOROOT -> {}", root.display()));
            } else {
                fail(&format!(
                    "{} is NOT installed  (run 'gvm install {}')",
                    v.tag(),
                    v.tag()
                ));
                issues += 1;
            }
        }
    }

    // ---- Check 5: ~/.gvm/current junction is set up -------------------------
    //
    // Users who installed gvm before v1.1.0 (which introduced the junction)
    // will not have ~/.gvm/current until they run `gvm use` with the new
    // binary. Warn them so `go` works in CMD, Git Bash and editors.
    let current_dir = config.current_dir();
    let current_go = current_dir.join("bin").join(go_name);
    if current_go.exists() {
        ok(&format!(
            "~/.gvm/current junction is configured ({})",
            current_dir.display()
        ));
    } else {
        fail("~/.gvm/current not set up  (run 'gvm use <version>' to enable universal shell support)");
        issues += 1;
    }

    // ---- Check 6: local .go-version consistency -----------------------------
    if let Ok(cwd) = std::env::current_dir() {
        let vf = cwd.join(".go-version");
        if vf.exists() {
            let raw = std::fs::read_to_string(&vf)
                .unwrap_or_default()
                .trim()
                .to_string();

            match crate::version::GoVersion::parse(&raw) {
                Ok(v) if toolchain::is_installed(config, &v) => {
                    ok(&format!(".go-version = {} (installed)", v.tag()));
                }
                Ok(v) => {
                    warn(&format!(
                        ".go-version = {} but NOT installed  (run 'gvm install {}')",
                        v.tag(),
                        v.tag()
                    ));
                    issues += 1;
                }
                Err(_) => {
                    warn(&format!(".go-version contains invalid version: '{raw}'"));
                    issues += 1;
                }
            }
        }
    }

    // ---- Check 7: shell profile contains the gvm init hook ------------------
    let sh = match shell_str {
        Some(s) => shell::from_str(s).ok(),
        None => shell::detect(),
    };

    if let Some(sh) = sh {
        match sh.profile_path() {
            None => warn(&format!("Cannot determine profile path for {}", sh.name())),
            Some(profile) => {
                if profile.exists() {
                    let content = std::fs::read_to_string(&profile).unwrap_or_default();
                    if content.contains("# gvm init") {
                        ok(&format!("Shell profile configured ({})", profile.display()));
                    } else {
                        fail(&format!(
                            "gvm init missing from {}  (run 'gvm setup')",
                            profile.display()
                        ));
                        issues += 1;
                    }
                } else {
                    fail(&format!(
                        "Profile not found: {}  (run 'gvm setup')",
                        profile.display()
                    ));
                    issues += 1;
                }
            }
        }
    }

    // ---- Check 8: single, gvm-managed `go` binary in PATH ------------------
    //
    // Scans every directory in PATH for a `go` (or `go.exe`) executable.
    // Reports a warning when multiple are found (shadowing is confusing) and
    // an error when the first - i.e. the active - one is not managed by gvm.
    // This is what `whereis go` surfaces on Linux: system-installed Go
    // alongside the gvm-managed one.
    let path_sep = if cfg!(windows) { ';' } else { ':' };
    let path_var = std::env::var("PATH").unwrap_or_default();
    let versions_dir = config.versions_dir();

    let go_paths: Vec<std::path::PathBuf> = path_var
        .split(path_sep)
        .map(std::path::Path::new)
        .filter_map(|dir| {
            let candidate = dir.join(go_name);
            if candidate.is_file() {
                Some(candidate)
            } else {
                None
            }
        })
        .collect();

    match go_paths.len() {
        0 => {
            // No Go in PATH at all - only relevant if a version is supposed to be active.
        }
        1 => {
            let go = &go_paths[0];
            // Accept both ~/.gvm/versions/<tag>/bin/go and ~/.gvm/current/bin/go
            // (the junction path introduced in v1.1.0 for universal shell support).
            if go.starts_with(&versions_dir) || go.starts_with(&current_dir) {
                ok(&format!("Active Go is gvm-managed ({})", go.display()));
            } else {
                fail(&format!(
                    "Active Go is NOT managed by gvm: {}",
                    go.display()
                ));
                println!(
                    "    Hint: remove the system Go package or run 'gvm setup' \
                          so gvm's PATH entry comes first."
                );
                issues += 1;
            }
        }
        n => {
            let first_is_gvm =
                go_paths[0].starts_with(&versions_dir) || go_paths[0].starts_with(&current_dir);
            if first_is_gvm {
                let non_gvm_shadowed: Vec<_> = go_paths
                    .iter()
                    .skip(1)
                    .filter(|p| !p.starts_with(&versions_dir) && !p.starts_with(&current_dir))
                    .collect();
                if non_gvm_shadowed.is_empty() {
                    // All extra entries are other gvm-managed paths (e.g. both the versioned
                    // directory and the ~/.gvm/current junction appear in PATH). Harmless.
                    ok(&format!(
                        "Active Go is gvm-managed ({})",
                        go_paths[0].display()
                    ));
                } else {
                    warn(&format!(
                        "{n} Go binaries found in PATH - {} non-gvm installation(s) are shadowed \
                         (consider removing them to avoid confusion):",
                        non_gvm_shadowed.len()
                    ));
                    for path in non_gvm_shadowed.iter() {
                        println!("    {} (non-gvm, shadowed)", path.display());
                    }
                }
            } else {
                fail(&format!(
                    "{n} Go binaries in PATH - gvm's version is being shadowed:"
                ));
                for (i, path) in go_paths.iter().enumerate() {
                    let label = if i == 0 {
                        " (active - NOT gvm-managed)"
                    } else if path.starts_with(&versions_dir) || path.starts_with(&current_dir) {
                        " (gvm-managed - shadowed)"
                    } else {
                        " (shadowed)"
                    };
                    println!("    {}{label}", path.display());
                }
                println!(
                    "    Hint: remove the system Go package or run 'gvm setup' \
                          to ensure gvm's PATH entry is first."
                );
                issues += 1;
            }
        }
    }

    // ---- Summary ------------------------------------------------------------
    println!();
    if issues == 0 {
        println!("{} Everything looks good!", "✓".green().bold());
    } else {
        println!("{} {} issue(s) found.", "!".yellow().bold(), issues);
        std::process::exit(1);
    }

    Ok(())
}

// --- Private helpers ---------------------------------------------------------

fn ok(msg: &str) {
    println!("  {} {msg}", "✓".green());
}

fn fail(msg: &str) {
    println!("  {} {msg}", "x".red());
}

fn warn(msg: &str) {
    println!("  {} {msg}", "!".yellow());
}
