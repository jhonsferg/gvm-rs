//! `gvm build` - compile a Go version from source.
//!
//! Unlike `gvm install` which downloads a precompiled binary, this command
//! fetches the official Go source tarball from go.dev, locates or downloads a
//! suitable bootstrap compiler, and runs the platform build script
//! (`src/make.bash` on Unix, `src/make.bat` on Windows) to produce a fully
//! functional Go toolchain installed into `~/.gvm/versions/go<X>.<Y>.<Z>/`.
//!
//! # Steps
//!
//! 1. Resolve the requested version against the go.dev release index.
//! 2. Skip if already installed (unless `--force`).
//! 3. Find the source tarball (`kind == "source"`) in the release.
//! 4. Resolve the bootstrap compiler (explicit `--bootstrap`, highest installed,
//!    or a temporarily downloaded previous-minor release).
//! 5. Download and verify the source tarball.
//! 6. Extract to a unique staging directory.
//! 7. Run the build script with `GOROOT_BOOTSTRAP` and any user-supplied env vars.
//! 8. Move the compiled tree to `~/.gvm/versions/<tag>/`.
//! 9. Clean up staging and temporary bootstrap directories.

use anyhow::{anyhow, Context, Result};
use colored::Colorize;
use std::path::{Path, PathBuf};

use crate::{
    archive::{download, extract},
    config::Config,
    fs as gvm_fs,
    remote::{index, release::Release},
    toolchain,
    user_version::VersionSpec,
    version::GoVersion,
};

struct Bootstrap {
    /// Directory passed as `GOROOT_BOOTSTRAP` to `make.bash`.
    path: PathBuf,
    /// If set, this directory is removed after compilation (temp download).
    cleanup: Option<PathBuf>,
    label: String,
}

/// Compiles `version_str` from source and installs it to the gvm versions store.
pub fn run(
    config: &Config,
    version_str: &str,
    force: bool,
    no_cgo: bool,
    bootstrap_spec: Option<&str>,
    env_vars: &[String],
) -> Result<()> {
    config.ensure_dirs()?;

    // Resolve version against the remote index.
    let spec = VersionSpec::parse(version_str)?;
    println!("{} Fetching available Go versions...", "->".cyan());
    let releases = index::fetch_releases()?;
    let release = index::resolve(&spec, &releases)?;
    let version = release
        .go_version()
        .ok_or_else(|| anyhow!("Could not parse version tag '{}'", release.version))?;

    // Bail out early if already installed (unless --force).
    if toolchain::is_installed(config, &version) {
        if force {
            println!(
                "{} Removing existing Go {} installation...",
                "->".cyan(),
                version.tag().bold()
            );
            std::fs::remove_dir_all(config.version_dir(&version.tag()))
                .context("Failed to remove existing installation")?;
        } else {
            println!(
                "{} Go {} is already installed.",
                "✓".green(),
                version.tag().bold()
            );
            println!("  Use {} to rebuild from source.", "--force".cyan());
            return Ok(());
        }
    }

    // Locate the source tarball entry for this release.
    let src_file = release.source_file().ok_or_else(|| {
        anyhow!(
            "No source tarball found for {}. \
             Source tarballs are only available for stable releases.",
            version.tag()
        )
    })?;

    println!();
    println!(
        "  {} Building {} from source.",
        "->".cyan(),
        version.tag().bold()
    );
    println!(
        "  {} This will take 5-15 minutes and requires ~3 GB of disk space.",
        "!".yellow()
    );
    println!();

    // Resolve the bootstrap compiler before downloading source.
    let bootstrap = resolve_bootstrap(config, &version, bootstrap_spec, &releases)?;
    println!("{} Bootstrap: {}", "->".cyan(), bootstrap.label.bold());

    // Download source tarball.
    let src_archive = config.tmp_dir().join(&src_file.filename);
    println!(
        "{} Downloading {}...",
        "->".cyan(),
        src_file.filename.bold()
    );
    if let Err(e) = download::fetch(&index::download_url(&src_file.filename), &src_archive) {
        let _ = std::fs::remove_file(&src_archive);
        cleanup_bootstrap(&bootstrap);
        return Err(e).context("Failed to download source tarball");
    }

    // Verify SHA-256 checksum.
    if !src_file.sha256.is_empty() {
        println!("{} Verifying checksum...", "->".cyan());
        if let Err(e) = download::verify_sha256(&src_archive, &src_file.sha256) {
            let _ = std::fs::remove_file(&src_archive);
            cleanup_bootstrap(&bootstrap);
            return Err(e);
        }
    }

    // Extract source into a unique staging dir to avoid races with other commands.
    let staging = config.tmp_dir().join(format!("src-{}", version.tag()));
    if staging.exists() {
        std::fs::remove_dir_all(&staging)?;
    }
    std::fs::create_dir_all(&staging)?;

    if let Err(e) = extract::unpack(&src_archive, &staging) {
        let _ = std::fs::remove_file(&src_archive);
        let _ = std::fs::remove_dir_all(&staging);
        cleanup_bootstrap(&bootstrap);
        return Err(e).context("Failed to extract source tarball");
    }
    let _ = std::fs::remove_file(&src_archive);

    // The Go source tarball always extracts to a `go/` subdirectory.
    let source_root = staging.join("go");
    if !source_root.exists() {
        let _ = std::fs::remove_dir_all(&staging);
        cleanup_bootstrap(&bootstrap);
        anyhow::bail!(
            "Unexpected archive layout: expected 'go/' inside {}",
            staging.display()
        );
    }

    println!("{} Compiling Go {}...", "->".cyan(), version.tag().bold());
    println!(
        "  {} Build output follows. This will take several minutes.",
        "i".cyan()
    );
    println!();

    let compiled = compile(&source_root, &bootstrap.path, no_cgo, env_vars);
    cleanup_bootstrap(&bootstrap);

    if let Err(e) = compiled {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(e);
    }

    // Move the compiled tree to the versions store.
    let dest = config.version_dir(&version.tag());
    if let Err(e) = gvm_fs::move_dir(&source_root, &dest) {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(e).context("Failed to move compiled Go to versions directory");
    }

    let _ = std::fs::remove_dir_all(&staging);

    println!();
    println!(
        "{} Go {} built and installed successfully.",
        "✓".green(),
        version.tag().bold()
    );
    println!(
        "  Run {} to activate.",
        format!("gvm use {}", version).cyan()
    );

    Ok(())
}

/// Resolves the bootstrap Go compiler to use.
///
/// Priority:
/// 1. `--bootstrap VERSION` - must already be installed via gvm.
/// 2. Highest locally-installed gvm version (no download needed).
/// 3. Download the latest patch of the previous minor as a temporary bootstrap.
fn resolve_bootstrap(
    config: &Config,
    target: &GoVersion,
    bootstrap_spec: Option<&str>,
    releases: &[Release],
) -> Result<Bootstrap> {
    // Explicit --bootstrap flag.
    if let Some(spec_str) = bootstrap_spec {
        let spec = VersionSpec::parse(spec_str)?;
        let bv = toolchain::resolve_installed(config, &spec).map_err(|_| {
            anyhow!(
                "Bootstrap version '{}' is not installed. Run 'gvm install {}' first.",
                spec_str,
                spec_str
            )
        })?;
        return Ok(Bootstrap {
            path: config.version_dir(&bv.tag()),
            cleanup: None,
            label: bv.tag(),
        });
    }

    // Use the highest already-installed version.
    let installed = toolchain::list_installed(config)?;
    if let Some(bv) = installed.into_iter().next() {
        return Ok(Bootstrap {
            path: config.version_dir(&bv.tag()),
            cleanup: None,
            label: bv.tag(),
        });
    }

    // No local Go available - download the latest patch of the previous minor.
    let bootstrap_minor = target.minor.saturating_sub(1);
    let b_spec = VersionSpec::Partial {
        major: target.major,
        minor: bootstrap_minor,
    };

    let b_release = index::resolve(&b_spec, releases).with_context(|| {
        format!(
            "Could not find a bootstrap release for go{}.{}. \
             Install a Go version first with 'gvm install latest', \
             or specify one with --bootstrap.",
            target.major, bootstrap_minor
        )
    })?;

    let b_version = b_release
        .go_version()
        .ok_or_else(|| anyhow!("Cannot parse bootstrap version tag"))?;

    let b_archive = b_release
        .archive_for(index::host_os(), index::host_arch())
        .ok_or_else(|| {
            anyhow!(
                "No bootstrap binary available for {}/{}.",
                index::host_os(),
                index::host_arch()
            )
        })?;

    let bootstrap_staging = config
        .tmp_dir()
        .join(format!("bootstrap-{}", b_version.tag()));

    println!(
        "{} No local Go found. Downloading bootstrap compiler {}...",
        "->".cyan(),
        b_version.tag().bold()
    );

    let archive_path = config.tmp_dir().join(&b_archive.filename);
    if let Err(e) = download::fetch(&index::download_url(&b_archive.filename), &archive_path) {
        let _ = std::fs::remove_file(&archive_path);
        return Err(e).context("Failed to download bootstrap compiler");
    }
    if !b_archive.sha256.is_empty() {
        if let Err(e) = download::verify_sha256(&archive_path, &b_archive.sha256) {
            let _ = std::fs::remove_file(&archive_path);
            return Err(e);
        }
    }

    if bootstrap_staging.exists() {
        std::fs::remove_dir_all(&bootstrap_staging)?;
    }
    std::fs::create_dir_all(&bootstrap_staging)?;

    if let Err(e) = extract::unpack(&archive_path, &bootstrap_staging) {
        let _ = std::fs::remove_file(&archive_path);
        let _ = std::fs::remove_dir_all(&bootstrap_staging);
        return Err(e).context("Failed to extract bootstrap compiler");
    }
    let _ = std::fs::remove_file(&archive_path);

    // Bootstrap archive also extracts to a `go/` subdirectory.
    let bootstrap_root = bootstrap_staging.join("go");
    if !bootstrap_root.exists() {
        let _ = std::fs::remove_dir_all(&bootstrap_staging);
        anyhow::bail!("Bootstrap archive had an unexpected layout");
    }

    Ok(Bootstrap {
        path: bootstrap_root,
        cleanup: Some(bootstrap_staging),
        label: format!("{} (downloaded temporarily)", b_version.tag()),
    })
}

/// Removes the temporary bootstrap directory, if any.
fn cleanup_bootstrap(b: &Bootstrap) {
    if let Some(dir) = &b.cleanup {
        let _ = std::fs::remove_dir_all(dir);
    }
}

/// Runs the Go build script inside `source_root` with the supplied environment.
///
/// Uses `src/make.bash` on Unix and `src/make.bat` on Windows. Both scripts
/// must be invoked from the `src/` subdirectory. Build output is streamed
/// directly to the terminal. Returns an error if the script exits non-zero.
fn compile(
    source_root: &Path,
    bootstrap_path: &Path,
    no_cgo: bool,
    env_vars: &[String],
) -> Result<()> {
    let src_dir = source_root.join("src");

    #[cfg(windows)]
    let (script_name, mut cmd) = {
        let script = src_dir.join("make.bat");
        if !script.exists() {
            anyhow::bail!(
                "Build script not found at {}. The source archive may be corrupt.",
                script.display()
            );
        }
        let mut c = std::process::Command::new("cmd.exe");
        c.args(["/c", script.to_str().unwrap_or("make.bat")]);
        ("make.bat", c)
    };

    #[cfg(not(windows))]
    let (script_name, mut cmd) = {
        let script = src_dir.join("make.bash");
        if !script.exists() {
            anyhow::bail!(
                "Build script not found at {}. The source archive may be corrupt.",
                script.display()
            );
        }
        let mut c = std::process::Command::new("bash");
        c.arg(&script);
        ("make.bash", c)
    };

    // Both make.bash and make.bat check for sibling files to verify they are
    // running from the correct directory.
    cmd.current_dir(&src_dir);
    cmd.env("GOROOT_BOOTSTRAP", bootstrap_path);

    if no_cgo {
        cmd.env("CGO_ENABLED", "0");
    }

    for kv in env_vars {
        // Split on the first '=' only so values containing '=' are preserved.
        if let Some((key, val)) = kv.split_once('=') {
            cmd.env(key, val);
        }
    }

    // Stream build output directly to the terminal.
    cmd.stdout(std::process::Stdio::inherit());
    cmd.stderr(std::process::Stdio::inherit());

    let status = cmd
        .spawn()
        .with_context(|| format!("Failed to start {script_name}"))?
        .wait()
        .context("Build process was interrupted")?;

    if !status.success() {
        anyhow::bail!(
            "Build failed with exit code {}.",
            status.code().unwrap_or(-1)
        );
    }

    Ok(())
}
