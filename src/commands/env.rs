//! `gvm env` - emit shell initialisation commands.
//!
//! Generates the shell script that sets `GVM_DIR`, `PATH`, and `GOROOT` for
//! the active Go version, and registers the `cd` hook that keeps those
//! variables in sync as the user navigates the file system.
//!
//! The output is meant to be evaluated by the shell, not executed as a script:
//!
//! - Bash / Zsh: `eval "$(gvm env --shell bash)"`
//! - Fish: `gvm env --shell fish | source`
//! - PowerShell: `gvm env --shell powershell | Out-String | Invoke-Expression`

use anyhow::Result;

use crate::{
    config::Config,
    shell::{self, EnvContext},
    toolchain,
    toolchain::VersionSource,
};

/// Prints the shell initialisation script for the active Go version.
///
/// The target shell is taken from `shell_str` when provided, otherwise it is
/// auto-detected from the environment via [`shell::detect`]. If no version is
/// currently active, the script still sets `GVM_DIR` and installs the hook,
/// but omits the `PATH` and `GOROOT` assignments.
///
/// # Errors
///
/// Returns an error if `shell_str` names an unsupported shell or if no shell
/// can be detected and `shell_str` is `None`.
pub fn run(config: &Config, shell_str: Option<&str>) -> Result<()> {
    let shell = match shell_str {
        Some(s) => shell::from_str(s)?,
        None => shell::detect()
            .ok_or_else(|| anyhow::anyhow!("Could not detect shell. Use --shell <name>."))?,
    };

    let (active_bin, active_root) = match toolchain::active_version(config) {
        Ok((v, src)) => match toolchain::version_bin_path(config, &v) {
            Ok(bin) => (Some(bin), Some(config.version_dir(&v.tag()))),
            Err(_) => {
                if src == VersionSource::Local {
                    eprintln!(
                        "gvm: Go {} (from .go-version) is not installed. Run: gvm install {}",
                        v, v
                    );
                }
                (None, None)
            }
        },
        Err(_) => (None, None),
    };

    let ctx = EnvContext {
        gvm_dir: &config.root,
        active_bin: active_bin.as_deref(),
        active_root: active_root.as_deref(),
    };

    print!("{}", shell.env_script(&ctx));
    Ok(())
}
