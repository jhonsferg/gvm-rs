//! Shell integration layer.
//!
//! This module defines the [`ShellConfig`] trait and provides concrete
//! implementations for every supported shell. It also exposes helpers for
//! runtime shell detection and idempotent profile injection.
//!
//! # Design
//!
//! Each supported shell is a zero-sized type (e.g. [`Bash`], [`PowerShell`])
//! that implements [`ShellConfig`]. Adding support for a new shell requires
//! only a new file and a new variant - no existing code needs to change
//! (Open/Closed principle).

mod bash;
mod fish;
mod powershell;
mod zsh;

use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};

/// Context passed to [`ShellConfig::env_script`] when generating the shell
/// initialisation script for the current session.
pub struct EnvContext<'a> {
    /// Path to the gvm root directory (value of `GVM_DIR`).
    pub gvm_dir: &'a Path,

    /// Path to the `bin/` directory of the currently active Go version.
    /// `None` when no version is active.
    pub active_bin: Option<&'a Path>,

    /// Path to the root directory of the currently active Go version.
    /// Used to set `GOROOT`. `None` when no version is active.
    pub active_root: Option<&'a Path>,
}

/// Behaviour that every supported shell must implement.
///
/// Implementors must be `Debug` so they can be logged and inspected.
/// The trait is object-safe and is used throughout `gvm` as
/// `Box<dyn ShellConfig>` or `&dyn ShellConfig`.
pub trait ShellConfig: std::fmt::Debug {
    /// Short, lowercase identifier for the shell (e.g. `"bash"`, `"powershell"`).
    fn name(&self) -> &'static str;

    /// Generates the shell script that sets `GVM_DIR`, `PATH`, `GOROOT`,
    /// and installs the `cd` hook for automatic version switching.
    fn env_script(&self, ctx: &EnvContext<'_>) -> String;

    /// Returns the path to the shell's user-level startup file where the
    /// `gvm env` hook line should be appended, or `None` if the path cannot
    /// be determined.
    fn profile_path(&self) -> Option<PathBuf>;

    /// Returns the path to the shell's login startup file where static PATH
    /// entries should be injected so they are visible to GUI applications and
    /// non-interactive login shells (e.g. VSCode, display managers).
    ///
    /// Returns `None` for shells that have no separate login profile (Fish,
    /// PowerShell) or on Windows where the registry PATH is used instead.
    fn login_profile_path(&self) -> Option<PathBuf> {
        None
    }

    /// Returns the one-liner that should be added to the shell profile so
    /// `gvm env` is evaluated on every new session.
    fn init_line(&self) -> &'static str;

    /// Returns the shell function definition that wraps the `gvm` binary.
    ///
    /// When sourced, this function calls the real `gvm` binary and then
    /// immediately re-evaluates `gvm env` after `use`, `default`, or `local`
    /// commands so that `PATH` and `GOROOT` are updated in the current shell
    /// session without opening a new terminal.
    fn wrapper_function(&self) -> &'static str;

    /// Returns a minimal shell script that activates `version_tag` for the
    /// current session only (sets `GVM_SHELL_VERSION`, `GOROOT`, and `PATH`).
    ///
    /// This script is emitted by `gvm shell <version>` and evaluated by the
    /// shell wrapper function. The `_gvm_hook` checks `GVM_SHELL_VERSION` and
    /// skips its normal version switching while this override is active, so the
    /// activation persists across `cd` calls until `gvm shell --unset` is run.
    fn shell_version_script(&self, version_tag: &str, bin: &Path, root: &Path) -> String;

    /// Returns the shell script that clears the session-scoped override.
    ///
    /// The script unsets `GVM_SHELL_VERSION` and calls `_gvm_hook` so that
    /// `PATH` and `GOROOT` are immediately restored to whatever `.go-version`
    /// or the global default says.
    fn shell_unset_script(&self) -> &'static str;

    /// Returns the executable name used to check whether this shell is
    /// installed on the current system (e.g. `"bash"`, `"zsh"`, `"pwsh"`).
    ///
    /// The default implementation returns `name()`, which is correct for
    /// bash, zsh, and fish. PowerShell overrides this to return `"pwsh"`.
    fn binary_name(&self) -> &'static str {
        self.name()
    }

    /// Returns `true` if the `# gvm init` block should prepend an explicit
    /// `export PATH` line for the gvm binary directory.
    ///
    /// Needed for bash and zsh: on Linux/macOS, the login profile (`~/.profile`,
    /// `~/.zprofile`) sources the interactive profile (`~/.bashrc`, `~/.zshrc`)
    /// BEFORE adding `~/.local/bin` to PATH, so `gvm env` would fail with
    /// "command not found" on every SSH login. Prepending the install dir inside
    /// the `# gvm init` block makes it self-sufficient regardless of order.
    ///
    /// Fish guards its init line with `command -q gvm` (silent skip), so it
    /// never emits errors. PowerShell uses the Windows registry, where PATH is
    /// fully set before any shell starts.
    fn needs_bin_path_in_init(&self) -> bool {
        false
    }
}

// --- Concrete implementations ------------------------------------------------

#[derive(Debug)]
pub struct Bash;
#[derive(Debug)]
pub struct Zsh;
#[derive(Debug)]
pub struct Fish;
#[derive(Debug)]
pub struct PowerShell;

impl ShellConfig for Bash {
    fn name(&self) -> &'static str {
        "bash"
    }
    fn env_script(&self, ctx: &EnvContext<'_>) -> String {
        bash::env_script(ctx)
    }
    fn profile_path(&self) -> Option<PathBuf> {
        bash::profile_path()
    }
    fn login_profile_path(&self) -> Option<PathBuf> {
        // ~/.profile is sourced by login shells and display managers on Linux.
        // It is not blocked by the interactive-only guard in ~/.bashrc, so
        // entries here are visible to VSCode and other GUI applications.
        #[cfg(not(target_os = "windows"))]
        return dirs::home_dir().map(|h| h.join(".profile"));
        #[cfg(target_os = "windows")]
        return None;
    }
    fn init_line(&self) -> &'static str {
        r#"eval "$(gvm env --shell bash)""#
    }
    fn wrapper_function(&self) -> &'static str {
        bash::wrapper_function()
    }
    fn shell_version_script(&self, tag: &str, bin: &Path, root: &Path) -> String {
        bash::shell_version_script(tag, bin, root)
    }
    fn shell_unset_script(&self) -> &'static str {
        bash::shell_unset_script()
    }
    fn needs_bin_path_in_init(&self) -> bool {
        true
    }
}

impl ShellConfig for Zsh {
    fn name(&self) -> &'static str {
        "zsh"
    }
    fn env_script(&self, ctx: &EnvContext<'_>) -> String {
        zsh::env_script(ctx)
    }
    fn profile_path(&self) -> Option<PathBuf> {
        zsh::profile_path()
    }
    fn login_profile_path(&self) -> Option<PathBuf> {
        // ~/.zprofile is sourced for zsh login shells (display managers, ssh).
        #[cfg(not(target_os = "windows"))]
        return dirs::home_dir().map(|h| h.join(".zprofile"));
        #[cfg(target_os = "windows")]
        return None;
    }
    fn init_line(&self) -> &'static str {
        r#"eval "$(gvm env --shell zsh)""#
    }
    fn wrapper_function(&self) -> &'static str {
        zsh::wrapper_function()
    }
    fn shell_version_script(&self, tag: &str, bin: &Path, root: &Path) -> String {
        zsh::shell_version_script(tag, bin, root)
    }
    fn shell_unset_script(&self) -> &'static str {
        zsh::shell_unset_script()
    }
    fn needs_bin_path_in_init(&self) -> bool {
        true
    }
}

impl ShellConfig for Fish {
    fn name(&self) -> &'static str {
        "fish"
    }
    fn env_script(&self, ctx: &EnvContext<'_>) -> String {
        fish::env_script(ctx)
    }
    fn profile_path(&self) -> Option<PathBuf> {
        fish::profile_path()
    }
    fn init_line(&self) -> &'static str {
        "if command -q gvm; gvm env --shell fish | source; end"
    }
    fn wrapper_function(&self) -> &'static str {
        fish::wrapper_function()
    }
    fn shell_version_script(&self, tag: &str, bin: &Path, root: &Path) -> String {
        fish::shell_version_script(tag, bin, root)
    }
    fn shell_unset_script(&self) -> &'static str {
        fish::shell_unset_script()
    }
}

impl ShellConfig for PowerShell {
    fn name(&self) -> &'static str {
        "powershell"
    }
    fn binary_name(&self) -> &'static str {
        // The executable is `pwsh` (PowerShell 7+) on all platforms.
        // On older Windows installations it may be `powershell.exe`, but
        // `is_available` checks both when running on Windows.
        "pwsh"
    }
    fn env_script(&self, ctx: &EnvContext<'_>) -> String {
        powershell::env_script(ctx)
    }
    fn profile_path(&self) -> Option<PathBuf> {
        powershell::profile_path()
    }
    fn init_line(&self) -> &'static str {
        "gvm env --shell powershell | Out-String | Invoke-Expression"
    }
    fn wrapper_function(&self) -> &'static str {
        powershell::wrapper_function()
    }
    fn shell_version_script(&self, tag: &str, bin: &Path, root: &Path) -> String {
        powershell::shell_version_script(tag, bin, root)
    }
    fn shell_unset_script(&self) -> &'static str {
        powershell::shell_unset_script()
    }
}

// --- Factory -----------------------------------------------------------------

/// Detects the running shell from the environment at runtime.
///
/// Detection order (most to least specific):
///
/// 1. `PSModulePath` environment variable - present in every PowerShell child
///    process, including nested ones.
/// 2. `SHELL` environment variable - standard on Unix systems.
/// 3. Compile-time `cfg!(target_os = "windows")` as a last resort when
///    neither variable is available.
///
/// Returns `None` if the shell cannot be identified.
pub fn detect() -> Option<Box<dyn ShellConfig>> {
    if std::env::var("PSModulePath").is_ok() {
        return Some(Box::new(PowerShell));
    }
    if let Ok(shell) = std::env::var("SHELL") {
        if shell.contains("zsh") {
            return Some(Box::new(Zsh));
        }
        if shell.contains("fish") {
            return Some(Box::new(Fish));
        }
        if shell.contains("bash") {
            return Some(Box::new(Bash));
        }
    }
    if cfg!(target_os = "windows") {
        return Some(Box::new(PowerShell));
    }
    None
}

/// Constructs a [`ShellConfig`] from a shell name string.
///
/// Accepted values (case-insensitive, hyphens ignored):
/// `powershell`, `pwsh`, `bash`, `zsh`, `fish`.
///
/// # Errors
///
/// Returns an error if the name does not match any supported shell.
pub fn from_str(s: &str) -> Result<Box<dyn ShellConfig>> {
    match s.to_lowercase().replace('-', "").as_str() {
        "powershell" | "pwsh" => Ok(Box::new(PowerShell)),
        "bash" => Ok(Box::new(Bash)),
        "zsh" => Ok(Box::new(Zsh)),
        "fish" => Ok(Box::new(Fish)),
        _ => bail!(
            "Unknown shell '{}'. Supported: powershell, bash, zsh, fish",
            s
        ),
    }
}

// --- Shell availability -------------------------------------------------------

/// Returns `true` if the shell binary is present somewhere in `PATH`.
///
/// PowerShell on Windows is always considered available since it is the
/// host process. On other platforms the `pwsh` binary is searched in PATH.
/// For bash, zsh, and fish the binary name matches `shell.name()`.
pub fn is_available(shell: &dyn ShellConfig) -> bool {
    // On Windows, PowerShell is always the host - no binary search needed.
    #[cfg(windows)]
    if shell.name() == "powershell" {
        return true;
    }
    find_binary(shell.binary_name())
}

/// Returns the names of every supported shell that is currently installed.
///
/// The list is ordered: powershell, bash, zsh, fish. Only shells whose
/// binary is found in PATH (or that are natively available) are included.
pub fn available_shells() -> Vec<&'static str> {
    let candidates: &[(&dyn ShellConfig, &'static str)] = &[
        (&PowerShell, "powershell"),
        (&Bash, "bash"),
        (&Zsh, "zsh"),
        (&Fish, "fish"),
    ];
    candidates
        .iter()
        .filter(|(sh, _)| is_available(*sh))
        .map(|(_, name)| *name)
        .collect()
}

/// Searches `PATH` for an executable with the given name.
///
/// On Windows also checks for `<name>.exe` since many Unix-style tools
/// are distributed without extension aliases.
fn find_binary(name: &str) -> bool {
    let sep = if cfg!(windows) { ';' } else { ':' };
    let Ok(path_var) = std::env::var("PATH") else {
        return false;
    };
    for dir in path_var.split(sep).filter(|s| !s.is_empty()) {
        let base = Path::new(dir).join(name);
        if base.exists() {
            return true;
        }
        #[cfg(windows)]
        if Path::new(dir).join(format!("{name}.exe")).exists() {
            return true;
        }
    }
    false
}

// --- Profile injection -------------------------------------------------------

/// Builds the content body of the `# gvm init` block.
///
/// For bash and zsh on non-Windows systems, prepends an `export PATH` line
/// for the gvm binary directory so the block is self-sufficient even when
/// sourced before the install dir is on PATH (e.g. Debian's `~/.profile`
/// sources `~/.bashrc` before adding `~/.local/bin`).
fn build_init_content(shell: &dyn ShellConfig, gvm_bin_dir: Option<&Path>) -> String {
    if shell.needs_bin_path_in_init() {
        #[cfg(not(target_os = "windows"))]
        if let Some(dir) = gvm_bin_dir {
            let path_expr = home_relative_path(dir);
            return format!("export PATH=\"{path_expr}:$PATH\"\n{}", shell.init_line());
        }
    }
    let _ = gvm_bin_dir;
    shell.init_line().to_string()
}

/// Converts an absolute path to a `$HOME`-relative expression when the path
/// is inside the user's home directory (e.g. `/home/jhon/.local/bin` becomes
/// `$HOME/.local/bin`). Falls back to the absolute path string otherwise.
#[cfg(not(target_os = "windows"))]
fn home_relative_path(path: &Path) -> String {
    if let Some(home) = dirs::home_dir() {
        if let Ok(rel) = path.strip_prefix(&home) {
            return format!("$HOME/{}", rel.display());
        }
    }
    path.display().to_string()
}

/// Appends the `gvm env` hook and the shell wrapper function to the shell's
/// profile file.
///
/// Two independent markers are used so that each block can be injected
/// separately and both can be detected by `gvm implode` for clean removal:
///
/// - `# gvm init` - guards the `eval "$(gvm env ...)"` one-liner that sets
///   `PATH`/`GOROOT` on every new shell session.
/// - `# gvm wrapper` - guards the `gvm()` / `function gvm` definition that
///   immediately refreshes the current session after `gvm use`, `gvm default`,
///   or `gvm local` without requiring a new terminal.
///
/// Re-running `gvm setup` is safe: each block is only appended when its
/// marker is absent, so existing installations receive the wrapper function
/// on upgrade without duplicating the init hook.
///
/// Creates the profile file (and any parent directories) if necessary.
///
/// # Errors
///
/// Returns an error if the profile path cannot be determined, the file cannot
/// be read, or the file cannot be written.
pub fn inject_profile(shell: &dyn ShellConfig, gvm_bin_dir: Option<&Path>) -> Result<()> {
    use anyhow::Context;

    const INIT_MARKER: &str = "# gvm init";
    const WRAPPER_MARKER: &str = "# gvm wrapper";

    let init_content = build_init_content(shell, gvm_bin_dir);

    let profile = shell
        .profile_path()
        .ok_or_else(|| anyhow::anyhow!("Cannot determine profile path for {}", shell.name()))?;

    if let Some(parent) = profile.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let existing = if profile.exists() {
        std::fs::read_to_string(&profile)?
    } else {
        String::new()
    };

    let has_init = existing.contains(INIT_MARKER);
    let has_wrapper = existing.contains(WRAPPER_MARKER);

    // If both markers are present, check whether the blocks are up to date.
    // When gvm is upgraded its init_line or wrapper may change; we replace
    // stale blocks rather than silently leaving old ones in place.
    if has_init && has_wrapper {
        let expected_init = format!("{INIT_MARKER}\n{init_content}\n");
        let expected_wrapper = format!("{WRAPPER_MARKER}\n{}\n", shell.wrapper_function());
        if existing.contains(&expected_init) && existing.contains(&expected_wrapper) {
            println!("  gvm hook already configured in {}", profile.display());
            return Ok(());
        }

        let mut content = existing.clone();
        let mut any_updated = false;

        // Replace stale init block (marker to next blank line or next marker).
        if !existing.contains(&expected_init) {
            if let Some(marker_pos) = content.find(INIT_MARKER) {
                let after_marker = &content[marker_pos + INIT_MARKER.len()..];
                let block_len = after_marker
                    .find("\n# gvm ")
                    .map(|i| i + 1)
                    .unwrap_or(after_marker.len());
                let end = marker_pos + INIT_MARKER.len() + block_len;
                let new_block = format!("{INIT_MARKER}\n{init_content}\n");
                content = format!("{}{}{}", &content[..marker_pos], new_block, &content[end..]);
            }
            println!("  Updated init hook in {}", profile.display());
            any_updated = true;
        }

        // Replace stale wrapper block (always last in the file).
        if !content.contains(&expected_wrapper) {
            let marker_pos = content
                .rfind(WRAPPER_MARKER)
                .expect("WRAPPER_MARKER must exist when has_wrapper is true");
            let before = content[..marker_pos].trim_end().to_string();
            content = format!(
                "{before}\n\n{WRAPPER_MARKER}\n{}\n",
                shell.wrapper_function()
            );
            println!("  Updated wrapper function in {}", profile.display());
            any_updated = true;
        }

        if any_updated {
            std::fs::write(&profile, &content)
                .with_context(|| format!("Failed to write to {}", profile.display()))?;
        }
        return Ok(());
    }

    let mut content = existing.trim_end().to_string();
    let mut changed = false;

    if !has_init {
        if !content.is_empty() {
            content.push_str("\n\n");
        }
        content.push_str(&format!("{INIT_MARKER}\n{init_content}\n"));
        println!("  Added init hook to {}", profile.display());
        changed = true;
    } else {
        println!("  Init hook already present in {}", profile.display());
    }

    if !has_wrapper {
        content.push_str(&format!(
            "\n{WRAPPER_MARKER}\n{}\n",
            shell.wrapper_function()
        ));
        println!("  Added wrapper function to {}", profile.display());
        changed = true;
    }

    if changed {
        std::fs::write(&profile, content)
            .with_context(|| format!("Failed to write to {}", profile.display()))?;
    }

    Ok(())
}

/// Injects a static `~/.gvm/current/bin` PATH entry into the shell's login
/// profile (e.g. `~/.profile` for bash, `~/.zprofile` for zsh).
///
/// This entry uses the `# gvm path` marker and is evaluated even in
/// non-interactive shells, making `go` visible to GUI applications such as
/// VSCode and GoLand that do not source `~/.bashrc`.
///
/// Does nothing for shells that have no login profile. Not compiled on Windows
/// where the registry PATH is used instead.
///
/// # Errors
///
/// Returns an error if the login profile cannot be read or written.
#[cfg(not(target_os = "windows"))]
pub fn inject_login_profile(shell: &dyn ShellConfig) -> Result<()> {
    let Some(profile) = shell.login_profile_path() else {
        return Ok(());
    };

    const MARKER: &str = "# gvm path";
    const EXPORT_LINE: &str = r#"export PATH="$HOME/.gvm/current/bin:$PATH""#;

    if let Some(parent) = profile.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let existing = if profile.exists() {
        std::fs::read_to_string(&profile)
            .with_context(|| format!("Cannot read {}", profile.display()))?
    } else {
        String::new()
    };

    let expected_block = format!("{MARKER}\n{EXPORT_LINE}\n");

    if existing.contains(&expected_block) {
        println!(
            "  gvm PATH entry already configured in {}",
            profile.display()
        );
        return Ok(());
    }

    if existing.contains(MARKER) {
        // Stale block - remove and re-add with updated content.
        let cleaned = remove_gvm_lines(&existing);
        let new_content = format!("{}\n\n{expected_block}", cleaned.trim_end());
        std::fs::write(&profile, &new_content)
            .with_context(|| format!("Failed to write to {}", profile.display()))?;
        println!("  Updated gvm PATH entry in {}", profile.display());
        return Ok(());
    }

    // No marker yet - append.
    let mut content = existing.trim_end().to_string();
    if !content.is_empty() {
        content.push_str("\n\n");
    }
    content.push_str(&expected_block);
    std::fs::write(&profile, content)
        .with_context(|| format!("Failed to write to {}", profile.display()))?;
    println!("  Added gvm PATH entry to {}", profile.display());
    Ok(())
}

/// Strips all gvm-managed blocks from `profile`.
///
/// Returns `Ok(true)` when the file was modified, `Ok(false)` when it
/// contained no gvm entries or did not exist.
///
/// Used by `gvm implode` and `gvm setup --reset`.
///
/// # Errors
///
/// Returns an error if the file cannot be read or written.
pub fn strip_profile(path: &Path) -> Result<bool> {
    if !path.exists() {
        return Ok(false);
    }
    let content =
        std::fs::read_to_string(path).with_context(|| format!("Cannot read {}", path.display()))?;
    let cleaned = remove_gvm_lines(&content);
    if cleaned == content {
        return Ok(false);
    }
    std::fs::write(path, &cleaned).with_context(|| format!("Cannot write {}", path.display()))?;
    Ok(true)
}

/// Strips all gvm-managed lines from a profile file's content string.
///
/// Recognised block markers (each starts a block terminated by the next
/// blank line):
///
/// - `# gvm init`             - eval hook added by `gvm setup`
/// - `# gvm wrapper`          - shell wrapper function added by `gvm setup`
/// - `# gvm path`             - static PATH entry added by `gvm setup` to the login profile
/// - `# gvm: binary location` - legacy PATH entry written by old install scripts
///
/// After removal, runs of more than one consecutive blank line are collapsed
/// to a single blank line so the file remains tidy.
pub fn remove_gvm_lines(content: &str) -> String {
    const MARKERS: &[&str] = &[
        "# gvm init",
        "# gvm wrapper",
        "# gvm path",
        "# gvm: binary location",
    ];

    let mut in_block = false;
    let mut out: Vec<&str> = Vec::new();

    for line in content.lines() {
        if in_block {
            if line.trim().is_empty() {
                in_block = false;
            }
            continue;
        }
        if MARKERS.iter().any(|m| line.contains(m)) {
            in_block = true;
            continue;
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

// --- Helpers -----------------------------------------------------------------

/// Returns `true` if the directory containing the current `gvm` executable
/// is listed in the `PATH` environment variable.
///
/// Used by `gvm setup` and `gvm doctor` to warn the user when the binary
/// itself is not reachable from the shell.
pub fn gvm_in_path() -> bool {
    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    let Some(dir) = exe.parent() else {
        return false;
    };
    let Ok(path_var) = std::env::var("PATH") else {
        return false;
    };
    let sep = if cfg!(windows) { ';' } else { ':' };
    path_var.split(sep).any(|p| Path::new(p) == dir)
}

// --- Tests -------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn run_inject(shell: &dyn ShellConfig, existing: &str) -> String {
        const INIT_MARKER: &str = "# gvm init";
        const WRAPPER_MARKER: &str = "# gvm wrapper";

        let dir = tempdir().unwrap();
        let path = dir.path().join("profile");
        fs::write(&path, existing).unwrap();

        let src = fs::read_to_string(&path).unwrap();
        let has_init = src.contains(INIT_MARKER);
        let has_wrapper = src.contains(WRAPPER_MARKER);

        let init_content = build_init_content(shell, None);

        if has_init && has_wrapper {
            let expected_init = format!("{INIT_MARKER}\n{init_content}\n");
            let expected_wrapper = format!("{WRAPPER_MARKER}\n{}\n", shell.wrapper_function());
            if src.contains(&expected_init) && src.contains(&expected_wrapper) {
                return src;
            }
            let mut content = src.clone();
            if !content.contains(&expected_init) {
                if let Some(pos) = content.find(INIT_MARKER) {
                    let after = &content[pos + INIT_MARKER.len()..];
                    let end = pos
                        + INIT_MARKER.len()
                        + after.find("\n# gvm ").map(|i| i + 1).unwrap_or(after.len());
                    let new_block = format!("{INIT_MARKER}\n{init_content}\n");
                    content = format!("{}{}{}", &content[..pos], new_block, &content[end..]);
                }
            }
            if !content.contains(&expected_wrapper) {
                let pos = content.rfind(WRAPPER_MARKER).unwrap();
                let before = content[..pos].trim_end().to_string();
                content = format!(
                    "{before}\n\n{WRAPPER_MARKER}\n{}\n",
                    shell.wrapper_function()
                );
            }
            fs::write(&path, &content).unwrap();
            return content;
        }

        let mut content = src.trim_end().to_string();
        if !has_init {
            if !content.is_empty() {
                content.push_str("\n\n");
            }
            content.push_str(&format!("{INIT_MARKER}\n{init_content}\n"));
        }
        if !has_wrapper {
            content.push_str(&format!(
                "\n{WRAPPER_MARKER}\n{}\n",
                shell.wrapper_function()
            ));
        }
        fs::write(&path, &content).unwrap();
        content
    }

    #[test]
    fn setup_injects_both_blocks_into_empty_profile() {
        let result = run_inject(&Bash, "");
        assert!(result.contains("# gvm init"));
        assert!(result.contains("# gvm wrapper"));
        assert!(result.contains("shell)"));
    }

    #[test]
    fn setup_is_idempotent_when_wrapper_is_current() {
        let sh = Bash;
        let first = run_inject(&sh, "");
        let second = run_inject(&sh, &first);
        assert_eq!(first, second, "second run must not change the file");
    }

    #[test]
    fn setup_updates_stale_bash_wrapper() {
        let stale = "# gvm init\neval \"$(gvm env --shell bash)\"\n\n# gvm wrapper\ngvm() { command gvm \"$@\"; }\n";
        let result = run_inject(&Bash, stale);
        assert!(result.contains("shell)"), "shell case must be injected");
        assert!(
            !result.contains("command gvm \"$@\"; }"),
            "old stub must be removed"
        );
        assert!(result.contains("# gvm init"), "init block must survive");
    }

    #[test]
    fn setup_updates_stale_zsh_wrapper() {
        let stale = "# gvm init\neval \"$(gvm env --shell zsh)\"\n\n# gvm wrapper\ngvm() { command gvm \"$@\"; }\n";
        let result = run_inject(&Zsh, stale);
        assert!(result.contains("shell)"));
        assert!(result.contains("--shell zsh"));
    }

    #[test]
    fn setup_updates_stale_fish_wrapper() {
        let stale = "# gvm init\ngvm env --shell fish | source\n\n# gvm wrapper\nfunction gvm\n    command gvm $argv\nend\n";
        let result = run_inject(&Fish, stale);
        assert!(result.contains("contains -- $argv[1] shell"));
        assert!(
            result.contains("string join"),
            "updated fish wrapper must use string join"
        );
    }

    #[test]
    fn setup_does_not_duplicate_init_block() {
        let existing = "# gvm init\neval \"$(gvm env --shell bash)\"\n";
        let result = run_inject(&Bash, existing);
        let count = result.matches("# gvm init").count();
        assert_eq!(count, 1, "init marker must appear exactly once");
    }

    #[test]
    fn setup_updates_stale_fish_init_line() {
        let stale = format!(
            "# gvm init\ngvm env --shell fish | source\n\n# gvm wrapper\n{}\n",
            Fish.wrapper_function()
        );
        let result = run_inject(&Fish, &stale);
        assert!(
            result.contains("command -q gvm"),
            "new guard must be present"
        );
        assert!(
            !result.contains("\ngvm env --shell fish | source\n"),
            "bare unguarded line must be replaced"
        );
        let count = result.matches("# gvm init").count();
        assert_eq!(
            count, 1,
            "init marker must appear exactly once after update"
        );
    }

    #[test]
    fn setup_is_idempotent_for_fish_after_update() {
        let sh = Fish;
        let first = run_inject(&sh, "");
        let second = run_inject(&sh, &first);
        assert_eq!(first, second, "fish: second run must not change the file");
    }

    // --- remove_gvm_lines tests -----------------------------------------------

    #[test]
    fn removes_init_block() {
        let input = "source ~/.bashrc\n\n# gvm init\neval \"$(gvm env --shell bash)\"\n";
        let got = remove_gvm_lines(input);
        assert!(!got.contains("gvm init"));
        assert!(!got.contains("eval"));
        assert!(got.contains("source ~/.bashrc"));
    }

    #[test]
    fn removes_legacy_binary_location_block() {
        let input =
            "# existing line\n\n# gvm: binary location\nexport PATH=\"/home/user/.local/bin:$PATH\"\n";
        let got = remove_gvm_lines(input);
        assert!(!got.contains("gvm: binary location"));
        assert!(!got.contains("export PATH"));
        assert!(got.contains("# existing line"));
    }

    #[test]
    fn removes_path_block() {
        let input = "# existing line\n\n# gvm path\nexport PATH=\"$HOME/.gvm/current/bin:$PATH\"\n";
        let got = remove_gvm_lines(input);
        assert!(!got.contains("gvm path"));
        assert!(!got.contains(".gvm/current/bin"));
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
        assert!(got.contains("source ~/.bashrc"));
    }

    #[test]
    fn removes_all_blocks() {
        let input = concat!(
            "# user config\n\n",
            "# gvm: binary location\nexport PATH=\"/bin:$PATH\"\n\n",
            "# gvm init\neval \"$(gvm env --shell bash)\"\n\n",
            "# gvm wrapper\n",
            "gvm() {\n",
            "    command gvm \"$@\"\n",
            "}\n\n",
            "# gvm path\nexport PATH=\"$HOME/.gvm/current/bin:$PATH\"\n",
        );
        let got = remove_gvm_lines(input);
        assert!(!got.contains("gvm init"));
        assert!(!got.contains("gvm: binary location"));
        assert!(!got.contains("gvm wrapper"));
        assert!(!got.contains("gvm path"));
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
