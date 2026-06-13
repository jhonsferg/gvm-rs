//! `gvm setup` - configure the shell environment for gvm.
//!
//! Responsible for ALL environment configuration: shell hooks, static PATH
//! entries in login profiles (Linux/macOS), and registry PATH entries
//! (Windows). The install script places the binary and then delegates here.
//!
//! # What this command does
//!
//! 1. Injects `# gvm init` + `# gvm wrapper` into the shell's interactive
//!    profile (e.g. `~/.bashrc`).
//! 2. On Linux/macOS: injects `# gvm path` into the login profile
//!    (`~/.profile` for bash, `~/.zprofile` for zsh) so `~/.gvm/current/bin`
//!    is on PATH for GUI applications (VSCode, GoLand, etc.) that do not
//!    source the interactive profile.
//! 3. On Windows: adds the gvm binary directory and `~\.gvm\current\bin` to
//!    the user PATH in the registry so all applications see them.
//!
//! # `--reset` flag
//!
//! Strips every `# gvm ...` block from the interactive and login profiles
//! (and the Windows registry on Windows), then re-applies clean configuration.
//! Only gvm-managed blocks are touched; all other content is preserved.

use anyhow::Result;
use colored::Colorize;

use crate::{shell, shell::ShellConfig};

/// Runs `gvm setup`, optionally with a full reset.
///
/// `shell_str` overrides auto-detection. `reset` strips all previous gvm
/// configuration before re-applying.
///
/// # Errors
///
/// Returns an error if the shell cannot be detected, profiles cannot be
/// written, or (on Windows) the registry cannot be accessed.
pub fn run(shell_str: Option<&str>, reset: bool) -> Result<()> {
    let sh: Box<dyn ShellConfig> = match shell_str {
        Some(s) => {
            let sh = shell::from_str(s)?;
            if !shell::is_available(sh.as_ref()) {
                let available = shell::available_shells();
                let hint = if available.is_empty() {
                    "No supported shells found in PATH.".to_string()
                } else {
                    format!("Shells available on this system: {}", available.join(", "))
                };
                anyhow::bail!(
                    "Shell '{}' is not installed or not found in PATH.\n  {}",
                    s,
                    hint
                );
            }
            sh
        }
        None => match shell::detect() {
            Some(sh) => {
                // Sanity-check: the detected shell should always be available,
                // but guard against a stale $SHELL pointing to a removed binary.
                if !shell::is_available(sh.as_ref()) {
                    let available = shell::available_shells();
                    let hint = if available.is_empty() {
                        "No supported shells found in PATH. Install bash, zsh, or fish first."
                            .to_string()
                    } else {
                        format!(
                            "Try: gvm setup --shell {}",
                            available.first().copied().unwrap_or("bash")
                        )
                    };
                    anyhow::bail!(
                        "Detected shell '{}' but its binary was not found in PATH.\n  {}",
                        sh.name(),
                        hint
                    );
                }
                sh
            }
            None => {
                let available = shell::available_shells();
                let hint = if available.is_empty() {
                    "No supported shells found. Install bash, zsh, fish, or PowerShell first."
                        .to_string()
                } else {
                    format!(
                        "Detected shells: {}. Use --shell <name> to select one.",
                        available.join(", ")
                    )
                };
                anyhow::bail!("Could not detect current shell.\n  {}", hint);
            }
        },
    };

    println!("Setting up gvm for {}...", sh.name().bold());

    // ---- Optional reset: strip all previous gvm config ----------------------
    if reset {
        strip_all(sh.as_ref())?;
        println!();
        println!("  Previous configuration removed. Re-applying...");
        println!();
    }

    // ---- Interactive profile: eval hook + wrapper ---------------------------
    let gvm_bin_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(std::path::PathBuf::from));
    shell::inject_profile(sh.as_ref(), gvm_bin_dir.as_deref())?;

    // ---- Login profile (Linux/macOS): static PATH for GUI apps --------------
    #[cfg(not(target_os = "windows"))]
    shell::inject_login_profile(sh.as_ref())?;

    // ---- Windows registry: PATH entries for gvm binary + current/bin --------
    #[cfg(target_os = "windows")]
    inject_windows_registry()?;

    // ---- Warn if gvm binary itself is not yet on PATH -----------------------
    if !shell::gvm_in_path() {
        if let Ok(exe) = std::env::current_exe() {
            let dir = exe
                .parent()
                .map(|p| p.display().to_string())
                .unwrap_or_default();
            println!();
            println!("{} gvm is not in PATH yet.", "!".yellow());
            println!(
                "  Add {} to your PATH so the shell hook can call 'gvm path'.",
                dir.cyan()
            );
        }
    }

    println!();
    println!(
        "{} Restart your shell or run: {}",
        "✓".green(),
        sh.init_line().cyan()
    );
    Ok(())
}

// --- Reset helpers -----------------------------------------------------------

/// Strips all gvm-managed blocks from the interactive profile and, on
/// non-Windows, from the login profile as well.
fn strip_all(sh: &dyn ShellConfig) -> Result<()> {
    // Interactive profile (e.g. ~/.bashrc)
    if let Some(p) = sh.profile_path() {
        match shell::strip_profile(&p) {
            Ok(true) => println!("  {} Cleaned {}", "✓".green(), p.display()),
            Ok(false) => println!("  No gvm config found in {}", p.display()),
            Err(e) => println!("  {} Could not clean {}: {e}", "!".yellow(), p.display()),
        }
    }

    // Login profile (e.g. ~/.profile or ~/.zprofile)
    #[cfg(not(target_os = "windows"))]
    if let Some(p) = sh.login_profile_path() {
        match shell::strip_profile(&p) {
            Ok(true) => println!("  {} Cleaned {}", "✓".green(), p.display()),
            Ok(false) => println!("  No gvm config found in {}", p.display()),
            Err(e) => println!("  {} Could not clean {}: {e}", "!".yellow(), p.display()),
        }
    }

    // Windows registry
    #[cfg(target_os = "windows")]
    strip_windows_registry()?;

    Ok(())
}

// --- Windows registry --------------------------------------------------------

/// Adds the gvm binary directory and `~\.gvm\current\bin` to the user PATH
/// in the Windows registry (HKCU\Environment).
///
/// Idempotent: entries that are already present are not duplicated.
#[cfg(target_os = "windows")]
fn inject_windows_registry() -> Result<()> {
    use anyhow::Context;
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE};
    use winreg::RegKey;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let env = hkcu
        .open_subkey_with_flags("Environment", KEY_READ | KEY_WRITE)
        .context("Cannot open HKCU\\Environment registry key")?;

    let current_path: String = env.get_value("PATH").unwrap_or_default();
    let mut entries: Vec<String> = current_path
        .split(';')
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect();

    let mut changed = false;

    // gvm binary directory
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let dir_str = dir.to_string_lossy().to_string();
            if !entries.iter().any(|e| std::path::Path::new(e) == dir) {
                entries.insert(0, dir_str);
                println!("  Added {} to user PATH (registry)", dir.display());
                changed = true;
            } else {
                println!("  {} already in user PATH", dir.display());
            }
        }
    }

    // ~/.gvm/current/bin
    if let Some(home) = dirs::home_dir() {
        let current_bin = home.join(".gvm").join("current").join("bin");
        let current_bin_str = current_bin.to_string_lossy().to_string();
        if !entries
            .iter()
            .any(|e| std::path::Path::new(e) == current_bin)
        {
            entries.insert(0, current_bin_str);
            println!("  Added {} to user PATH (registry)", current_bin.display());
            changed = true;
        } else {
            println!("  {} already in user PATH", current_bin.display());
        }
    }

    if changed {
        let new_path = entries.join(";");
        env.set_value("PATH", &new_path)
            .context("Cannot write PATH to HKCU\\Environment")?;
        println!("  {} User PATH updated in registry", "✓".green());
        println!("  Restart your terminal or log out/in for the change to take effect.");
    }

    Ok(())
}

/// Removes all gvm-managed entries from the Windows user PATH in the registry.
#[cfg(target_os = "windows")]
fn strip_windows_registry() -> Result<()> {
    use anyhow::Context;
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE};
    use winreg::RegKey;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let env = hkcu
        .open_subkey_with_flags("Environment", KEY_READ | KEY_WRITE)
        .context("Cannot open HKCU\\Environment registry key")?;

    let current_path: String = env.get_value("PATH").unwrap_or_default();

    // Remove any entry that is inside the user's .gvm directory.
    let home = dirs::home_dir().unwrap_or_default();
    let gvm_root = home.join(".gvm");

    let filtered: Vec<&str> = current_path
        .split(';')
        .filter(|e| {
            let p = std::path::Path::new(e);
            !p.starts_with(&gvm_root)
        })
        .collect();

    let new_path = filtered.join(";");
    if new_path != current_path {
        env.set_value("PATH", &new_path)
            .context("Cannot write PATH to HKCU\\Environment")?;
        println!(
            "  {} Removed gvm entries from user PATH (registry)",
            "✓".green()
        );
    }

    Ok(())
}
