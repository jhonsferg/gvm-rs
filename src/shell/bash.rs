//! Bash shell integration.

use crate::shell::EnvContext;
use std::path::{Path, PathBuf};

pub fn profile_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".bashrc"))
}

/// Generates the Bash initialisation script.
///
/// The `_gvm_hook` defined here checks `GVM_SHELL_VERSION` first. When that
/// variable is set (by `gvm shell <version>`) the hook returns immediately so
/// that the session-scoped activation is not overridden by directory changes.
pub fn env_script(ctx: &EnvContext<'_>) -> String {
    let gvm_dir = ctx.gvm_dir.display().to_string();

    let path_stmt = ctx.active_bin.map_or_else(String::new, |bin| {
        format!("export PATH=\"{}:$PATH\"\n", bin.display())
    });

    let goroot_stmt = ctx.active_root.map_or_else(String::new, |root| {
        format!("export GOROOT=\"{}\"\n", root.display())
    });

    format!(
        r#"export GVM_DIR="{gvm_dir}"
{goroot_stmt}{path_stmt}
if ! declare -f _gvm_hook > /dev/null 2>&1; then
    _gvm_hook() {{
        [ -n "$GVM_SHELL_VERSION" ] && return
        [ "$PWD" = "${{_GVM_PREV_PWD-}}" ] && return
        export _GVM_PREV_PWD="$PWD"
        local p; p=$(gvm path 2>/dev/null)
        if [ -n "$p" ]; then
            export GOROOT="$(dirname "$p")"
            PATH="$p:$(printenv PATH | tr ':' '\n' | grep -v "$GVM_DIR/versions" | tr '\n' ':' | sed 's/:$//')"
            export PATH
        fi
    }}
    cd() {{ builtin cd "$@" && _gvm_hook; }}
    PROMPT_COMMAND="_gvm_hook${{PROMPT_COMMAND:+;$PROMPT_COMMAND}}"
fi"#,
        gvm_dir = gvm_dir,
        goroot_stmt = goroot_stmt,
        path_stmt = path_stmt,
    )
}

/// Returns the Bash wrapper function.
///
/// The wrapper handles three categories:
/// - `shell`: captures the command's stdout (which IS the env script) and evals it.
/// - `use|default|local`: runs the binary, then separately evals `gvm env` to refresh.
/// - everything else: pass-through.
pub fn wrapper_function() -> &'static str {
    r#"gvm() {
    local _gvm_exit
    case "$1" in
        shell)
            local _gvm_shell_script
            _gvm_shell_script="$(command gvm "$@")"
            _gvm_exit=$?
            if [ "$_gvm_exit" -eq 0 ]; then
                eval "$_gvm_shell_script"
            fi
            ;;
        use|default|local)
            command gvm "$@"
            _gvm_exit=$?
            eval "$(command gvm env --shell bash 2>/dev/null)"
            ;;
        *)
            command gvm "$@"
            _gvm_exit=$?
            ;;
    esac
    return $_gvm_exit
}"#
}

/// Returns the minimal env script that activates a specific version for this
/// session only (stdout, meant to be eval'd by the shell wrapper).
///
/// Sets `GVM_SHELL_VERSION` so `_gvm_hook` skips its normal version switching
/// while this override is active.
pub fn shell_version_script(tag: &str, bin: &Path, root: &Path) -> String {
    format!(
        r#"export GVM_SHELL_VERSION="{tag}"
export GOROOT="{root}"
export PATH="{bin}:$(printenv PATH | tr ':' '\n' | grep -v "$GVM_DIR/versions" | tr '\n' ':' | sed 's/:$//')"
"#,
        tag = tag,
        root = root.display(),
        bin = bin.display(),
    )
}

/// Returns the script that clears the session-scoped Go override.
pub fn shell_unset_script() -> &'static str {
    "unset GVM_SHELL_VERSION\n_gvm_hook 2>/dev/null || true\n"
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shell::EnvContext;
    use std::path::Path;

    fn ctx_with_version() -> (String, String) {
        let bin = Path::new("/home/user/.gvm/versions/go1.23.4/bin");
        let root = Path::new("/home/user/.gvm/versions/go1.23.4");
        let ctx = EnvContext {
            gvm_dir: Path::new("/home/user/.gvm"),
            active_bin: Some(bin),
            active_root: Some(root),
        };
        (
            env_script(&ctx),
            shell_version_script("go1.23.4", bin, root),
        )
    }

    #[test]
    fn hook_checks_gvm_shell_version() {
        let (script, _) = ctx_with_version();
        assert!(
            script.contains("GVM_SHELL_VERSION"),
            "env_script hook must check GVM_SHELL_VERSION"
        );
    }

    #[test]
    fn hook_returns_early_when_shell_version_set() {
        let (script, _) = ctx_with_version();
        // The guard must come BEFORE the `gvm path` call
        let hook_start = script.find("_gvm_hook()").unwrap_or(0);
        let guard_pos = script.find("GVM_SHELL_VERSION").unwrap_or(usize::MAX);
        let path_pos = script.find("gvm path").unwrap_or(usize::MAX);
        assert!(
            guard_pos > hook_start && guard_pos < path_pos,
            "GVM_SHELL_VERSION guard must appear before gvm path call inside the hook"
        );
    }

    #[test]
    fn bash_shell_version_script_includes_bin_path() {
        let bin = Path::new("/home/user/.gvm/versions/go1.23.4/bin");
        let root = Path::new("/home/user/.gvm/versions/go1.23.4");
        let ctx = EnvContext {
            gvm_dir: Path::new("/home/user/.gvm"),
            active_bin: Some(bin),
            active_root: Some(root),
        };
        let script = env_script(&ctx);
        assert!(
            script.contains(bin.to_str().unwrap()),
            "env_script must include the bin path in PATH export"
        );
    }

    #[test]
    fn bash_hook_registered_to_prompt_command() {
        let ctx = EnvContext {
            gvm_dir: Path::new("/home/user/.gvm"),
            active_bin: None,
            active_root: None,
        };
        let script = env_script(&ctx);
        assert!(
            script.contains("PROMPT_COMMAND"),
            "env_script must register hook via PROMPT_COMMAND for startup detection"
        );
    }

    #[test]
    fn wrapper_handles_shell_subcommand() {
        let w = wrapper_function();
        assert!(
            w.contains("shell)"),
            "wrapper must handle the shell subcommand"
        );
        assert!(w.contains("eval"), "wrapper must eval shell output");
    }

    #[test]
    fn shell_version_script_has_required_exports() {
        let bin = Path::new("/home/user/.gvm/versions/go1.23.4/bin");
        let root = Path::new("/home/user/.gvm/versions/go1.23.4");
        let script = shell_version_script("go1.23.4", bin, root);
        assert!(script.contains("GVM_SHELL_VERSION=\"go1.23.4\""));
        assert!(script.contains("GOROOT="));
        assert!(script.contains("PATH="));
    }

    #[test]
    fn shell_unset_clears_variable_and_triggers_hook() {
        let s = shell_unset_script();
        assert!(s.contains("unset GVM_SHELL_VERSION"));
        assert!(s.contains("_gvm_hook"));
    }
}
