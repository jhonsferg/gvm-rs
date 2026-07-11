//! `gvm install` - download and install a Go version.
//!
//! The install process follows these steps:
//!
//! 1. Resolve the [`VersionSpec`] against the go.dev release index.
//! 2. Skip installation if the version is already present (unless `--force`).
//! 3. Download the appropriate archive for the current OS and architecture.
//! 4. Verify the SHA-256 checksum.
//! 5. Extract the archive to a staging directory.
//! 6. Move the extracted tree to its final location in the versions store.
//!
//! Temporary files (archive and extracted tree) are cleaned up on any error
//! so partial installs do not leave the store in a broken state.

use anyhow::{anyhow, Result};
use colored::Colorize;

use crate::{
    archive::{download, extract},
    config::Config,
    fs as gvm_fs,
    http::HttpClient,
    lock,
    remote::index,
    tempdir::TempDir,
    toolchain,
    user_version::VersionSpec,
};

/// Downloads and installs the Go version described by `spec_str`.
///
/// When `force` is `true` the existing installation is removed first, allowing
/// a clean reinstall. When `force` is `false` and the version is already
/// installed, the function returns early with a hint to use `--force`.
///
/// # Errors
///
/// Returns an error if:
/// - `spec_str` is not a valid version spec.
/// - The go.dev release index cannot be fetched.
/// - No release matches the spec.
/// - The archive download fails or the checksum does not match.
/// - Extraction fails.
/// - The extracted directory cannot be moved to the versions store.
pub fn run(config: &Config, client: &HttpClient, spec_str: &str, force: bool) -> Result<()> {
    config.ensure_dirs()?;

    let spec = VersionSpec::parse(spec_str)?;

    println!("{} Fetching available Go versions...", "->".cyan());
    let releases = index::fetch_releases(client)?;
    let release = index::resolve(&spec, &releases)?;

    let version = release
        .go_version()
        .ok_or_else(|| anyhow!("Could not parse version tag '{}'", release.version))?;

    if toolchain::is_installed(config, &version) {
        if force {
            println!(
                "{} Reinstalling Go {}...",
                "->".cyan(),
                version.tag().bold()
            );
            let lock_path = config.root.join(".lock");
            lock::with_lock(&lock_path, || {
                Ok(std::fs::remove_dir_all(config.version_dir(&version.tag()))?)
            })?;
        } else {
            println!(
                "{} Go {} is already installed.",
                "✓".green(),
                version.tag().bold()
            );
            println!("  Use {} to reinstall.", "--force".cyan());
            return Ok(());
        }
    }

    let file = release
        .archive_for(index::host_os(), index::host_arch())
        .ok_or_else(|| {
            anyhow!(
                "No binary found for {}/{}",
                index::host_os(),
                index::host_arch()
            )
        })?;

    let url = index::download_url(&file.filename);
    let archive_path = config.tmp_dir().join(&file.filename);

    println!("{} Downloading {}...", "->".cyan(), file.filename.bold());
    let dl_result = download::fetch(client, &url, &archive_path);
    if let Err(e) = dl_result {
        let _ = std::fs::remove_file(&archive_path);
        return Err(e);
    }

    println!("{} Verifying checksum...", "->".cyan());
    if let Err(e) = download::verify_sha256(&archive_path, &file.sha256) {
        let _ = std::fs::remove_file(&archive_path);
        return Err(e);
    }

    // Use TempDir for extraction - auto-cleanup on drop
    let staging_dir = TempDir::new_in(config.tmp_dir(), "gvm-install-")?;

    let extract_result = extract::unpack(&archive_path, staging_dir.path());
    if let Err(e) = extract_result {
        // staging_dir will auto-cleanup on drop
        return Err(e);
    }

    let extracted = staging_dir.path().join("go");
    let dest = config.version_dir(&version.tag());
    let lock_path = config.root.join(".lock");
    lock::with_lock(&lock_path, || gvm_fs::move_dir(&extracted, &dest))?;
    let _ = std::fs::remove_file(&archive_path);
    // staging_dir will auto-cleanup on drop

    println!(
        "{} Go {} installed successfully.",
        "✓".green(),
        version.tag().bold()
    );
    println!(
        "  Run {} to activate.",
        format!("gvm use {}", version).cyan()
    );

    // Install user-defined default packages (e.g. gopls, dlv) if configured.
    install_default_packages(config, &version);

    Ok(())
}

/// Reads `~/.gvm/default-packages` and runs `go install <pkg>` for every
/// non-blank, non-comment entry using the freshly installed Go toolchain.
///
/// Errors from individual package installs are printed as warnings rather than
/// propagated so they never block the overall `gvm install` flow.
fn install_default_packages(config: &crate::config::Config, version: &crate::version::GoVersion) {
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
        std::env::var("PATH").unwrap_or_default(),
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
