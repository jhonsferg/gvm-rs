//! `gvm implode` - completely remove gvm from the system.
//!
//! Removes, in order:
//! 1. The entire gvm data directory (`~/.gvm`) containing all installed Go
//!    versions.
//! 2. The `gvm` binary itself.
//! 3. Every gvm-managed line from the detected shell's profile file.
//!
//! A summary of what will be deleted is always printed before any action is
//! taken. When `--force` is omitted the user must type `yes` to confirm.

use anyhow::{bail, Context, Result};
use colored::Colorize;
use std::path::{Path, PathBuf};

use crate::{config::Config, shell};

/// Completely removes gvm and all associated data from the system.
///
/// `force` skips the interactive confirmation prompt; everything else is
/// identical between the two modes.
///
/// # Errors
///
/// Returns an error if:
/// - The confirmation prompt cannot be read from stdin.
/// - The data directory or binary cannot be removed.
///
/// Profile cleanup failures are emitted as warnings rather than errors so the
/// rest of the removal always completes.
pub fn run(config: &Config, force: bool) -> Result<()> {
    let versions_dir = config.versions_dir();
    let version_count = count_versions(&versions_dir);
    let data_mb = dir_size_mb(&config.root);

    let exe_path = std::env::current_exe().context("Cannot determine gvm binary location")?;

    let sh = shell::detect();
    let profile_path = sh.as_deref().and_then(|s| s.profile_path());

    // ── Print removal plan ────────────────────────────────────────────────────
    println!();
    println!(
        "{} This will permanently remove:",
        "gvm implode".red().bold()
    );
    println!();
    println!(
        "  {} {} ({} version{}, {:.1} MB)",
        "->".cyan(),
        config.root.display(),
        version_count,
        if version_count == 1 { "" } else { "s" },
        data_mb,
    );
    println!("  {} {}", "->".cyan(), exe_path.display());
    if let Some(ref p) = profile_path {
        println!("  {} {} (gvm lines removed)", "->".cyan(), p.display());
    }
    println!();

    // ── Confirm ───────────────────────────────────────────────────────────────
    if !force {
        eprint!("  Type {} to confirm: ", "yes".bold());
        let mut input = String::new();
        std::io::stdin()
            .read_line(&mut input)
            .context("Failed to read confirmation")?;
        if input.trim() != "yes" {
            bail!("Aborted.");
        }
        println!();
    }

    // ── Remove data directory ─────────────────────────────────────────────────
    if config.root.exists() {
        std::fs::remove_dir_all(&config.root)
            .with_context(|| format!("Failed to remove {}", config.root.display()))?;
        println!("  {} Removed {}", "✓".green(), config.root.display());
    }

    // ── Clean shell profile ───────────────────────────────────────────────────
    if let Some(ref profile) = profile_path {
        match clean_profile(profile) {
            Ok(true) => println!("  {} Cleaned {}", "✓".green(), profile.display()),
            Ok(false) => {}
            Err(e) => eprintln!(
                "  {} Could not clean {}: {e}",
                "!".yellow(),
                profile.display()
            ),
        }
    }

    // ── Remove the binary (last, so earlier errors still have gvm available) ──
    remove_binary(&exe_path)?;
    println!("  {} Removed {}", "✓".green(), exe_path.display());

    println!();
    println!("{}", "gvm has been completely removed.".green().bold());
    println!("  Open a new terminal to apply the profile changes.");
    println!();

    Ok(())
}

// ── Directory helpers ─────────────────────────────────────────────────────────

/// Counts the number of installed Go versions in `versions_dir`.
fn count_versions(versions_dir: &Path) -> usize {
    if !versions_dir.exists() {
        return 0;
    }
    std::fs::read_dir(versions_dir)
        .map(|rd| rd.filter_map(|e| e.ok()).count())
        .unwrap_or(0)
}

/// Returns the approximate on-disk size of `root` in megabytes.
///
/// Symlinks are counted by their own metadata only (not the target) to avoid
/// loops and double-counting.
fn dir_size_mb(root: &Path) -> f64 {
    fn walk(dir: &Path) -> u64 {
        let Ok(rd) = std::fs::read_dir(dir) else {
            return 0;
        };
        rd.filter_map(|e| e.ok())
            .fold(0u64, |acc, entry| match entry.metadata() {
                Ok(m) if m.is_dir() => acc + walk(&entry.path()),
                Ok(m) => acc + m.len(),
                Err(_) => acc,
            })
    }
    if !root.exists() {
        return 0.0;
    }
    walk(root) as f64 / (1024.0 * 1024.0)
}

// ── Profile cleaning ──────────────────────────────────────────────────────────

/// Removes all gvm-managed lines from `profile`.
///
/// Returns `Ok(true)` when the file was modified, `Ok(false)` when it
/// contained no gvm entries (or did not exist).
///
/// # Errors
///
/// Returns an error if the file cannot be read or written.
fn clean_profile(profile: &PathBuf) -> Result<bool> {
    if !profile.exists() {
        return Ok(false);
    }
    let content = std::fs::read_to_string(profile)
        .with_context(|| format!("Cannot read {}", profile.display()))?;
    let cleaned = remove_gvm_lines(&content);
    if cleaned == content {
        return Ok(false);
    }
    std::fs::write(profile, &cleaned)
        .with_context(|| format!("Cannot write {}", profile.display()))?;
    Ok(true)
}

/// Strips every line that belongs to a gvm-managed block.
///
/// Three block types are recognised:
///
/// - `# gvm init` - written by `gvm setup`. The marker and every subsequent
///   line up to (and including) the first blank line are removed. For a
///   single-line block (e.g. the `eval "$(gvm env …)"` one-liner) this means
///   the marker + one content line.
/// - `# gvm: binary location` - written by `install.sh`. Same format: marker
///   + one content line (`export PATH=…`).
/// - `# gvm wrapper` - written by `gvm setup`. Covers a multi-line shell
///   function definition; all lines from the marker until the first following
///   blank line are removed.
///
/// After removal, runs of more than one consecutive blank line are collapsed
/// to a single blank line so the file remains tidy.
fn remove_gvm_lines(content: &str) -> String {
    const MARKERS: &[&str] = &["# gvm init", "# gvm: binary location", "# gvm wrapper"];

    // `in_block` is true while we are skipping lines belonging to a marker
    // block. A blank line (or end-of-file) terminates the block.
    let mut in_block = false;
    let mut out: Vec<&str> = Vec::new();

    for line in content.lines() {
        if in_block {
            if line.trim().is_empty() {
                // Blank line ends the block; skip the blank itself so the
                // surrounding content re-joins cleanly after collapsing.
                in_block = false;
            }
            // Skip every line inside the block (content and terminating blank).
            continue;
        }
        if MARKERS.iter().any(|m| line.contains(m)) {
            in_block = true;
            continue; // drop the marker line itself
        }
        out.push(line);
    }

    // Collapse consecutive blank lines down to one.
    let mut result = String::with_capacity(content.len());
    let mut prev_blank = false;
    for line in &out {
        let blank = line.trim().is_empty();
        if blank && prev_blank {
            continue;
        }
        result.push_str(line);
        result.push('\n');
        prev_blank = blank;
    }

    let trimmed = result.trim_end().to_string();
    if trimmed.is_empty() {
        trimmed
    } else {
        trimmed + "\n"
    }
}

// ── Binary removal ────────────────────────────────────────────────────────────

/// Removes the gvm executable.
///
/// On Unix the file is unlinked directly. On Windows a running executable
/// cannot be deleted while in use, so the binary is first renamed to
/// `gvm.exe.old` (which succeeds even for a running process) and then an
/// immediate deletion is attempted. If the deletion fails the `.old` file is
/// left behind - it is inert and can be deleted manually.
///
/// # Errors
///
/// Returns an error if the file cannot be renamed or removed.
fn remove_binary(exe: &Path) -> Result<()> {
    #[cfg(windows)]
    {
        let old = exe.with_file_name("gvm.exe.old");
        std::fs::rename(exe, &old).with_context(|| format!("Cannot rename {}", exe.display()))?;
        let _ = std::fs::remove_file(&old); // best-effort; ignored if still locked
    }
    #[cfg(not(windows))]
    {
        std::fs::remove_file(exe).with_context(|| format!("Cannot remove {}", exe.display()))?;
    }
    Ok(())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::remove_gvm_lines;

    #[test]
    fn removes_init_block() {
        let input = "source ~/.bashrc\n\n# gvm init\neval \"$(gvm env --shell bash)\"\n";
        let got = remove_gvm_lines(input);
        assert!(!got.contains("gvm init"));
        assert!(!got.contains("eval"));
        assert!(got.contains("source ~/.bashrc"));
    }

    #[test]
    fn removes_binary_location_block() {
        let input = "# existing line\n\n# gvm: binary location\nexport PATH=\"/home/user/.local/bin:$PATH\"\n";
        let got = remove_gvm_lines(input);
        assert!(!got.contains("gvm: binary location"));
        assert!(!got.contains("export PATH"));
        assert!(got.contains("# existing line"));
    }

    #[test]
    fn removes_wrapper_block() {
        let input = concat!(
            "source ~/.bashrc\n\n",
            "# gvm wrapper\n",
            "gvm() {\n",
            "    command gvm \"$@\"\n",
            "    local _gvm_exit=$?\n",
            "    return $_gvm_exit\n",
            "}\n",
        );
        let got = remove_gvm_lines(input);
        assert!(!got.contains("gvm wrapper"));
        assert!(!got.contains("gvm()"));
        assert!(!got.contains("_gvm_exit"));
        assert!(got.contains("source ~/.bashrc"));
    }

    #[test]
    fn removes_both_blocks() {
        let input = concat!(
            "# user config\n\n",
            "# gvm: binary location\nexport PATH=\"/bin:$PATH\"\n\n",
            "# gvm init\neval \"$(gvm env --shell bash)\"\n\n",
            "# gvm wrapper\n",
            "gvm() {\n",
            "    command gvm \"$@\"\n",
            "}\n",
        );
        let got = remove_gvm_lines(input);
        assert!(!got.contains("gvm init"));
        assert!(!got.contains("gvm: binary location"));
        assert!(!got.contains("gvm wrapper"));
        assert!(!got.contains("gvm()"));
        assert!(got.contains("# user config"));
    }

    #[test]
    fn idempotent_on_clean_file() {
        let input = "export FOO=bar\nalias ll='ls -la'\n";
        assert_eq!(remove_gvm_lines(input), input);
    }

    #[test]
    fn collapses_extra_blank_lines() {
        let input = "line1\n\n\n\nline2\n";
        let got = remove_gvm_lines(input);
        assert!(!got.contains("\n\n\n"));
    }
}
