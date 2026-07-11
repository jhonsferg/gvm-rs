//! Bootstrap compiler resolution for building Go from source.

use crate::{
    archive::{download, extract},
    config::Config,
    http::HttpClient,
    remote::{index, release::Release},
    tempdir::TempDir,
    toolchain,
    user_version::VersionSpec,
    version::GoVersion,
};
use anyhow::{anyhow, Context, Result};
use colored::Colorize;
use std::path::PathBuf;

/// Represents a resolved bootstrap compiler.
#[derive(Debug)]
pub struct Bootstrap {
    /// Directory passed as `GOROOT_BOOTSTRAP` to `make.bash`.
    pub path: PathBuf,
    /// If set, this temporary directory is removed after compilation (temp download).
    cleanup: Option<TempDir>,
    pub label: String,
}

/// Resolves the bootstrap Go compiler to use.
///
/// Priority:
/// 1. `--bootstrap VERSION` - must already be installed via gvm.
/// 2. Previous version (patch-1, or latest of minor-1 when patch==0) if installed locally.
/// 3. Download that same previous version temporarily; removed after compilation.
pub fn resolve_bootstrap(
    config: &Config,
    client: &HttpClient,
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

    // Compute the closest older version spec:
    //   patch > 0 → exact previous patch (e.g. 1.25.10 for target 1.25.11)
    //   patch == 0 → latest patch of previous minor (e.g. 1.24.x for target 1.25.0)
    let prev_spec = if target.patch > 0 {
        VersionSpec::Exact {
            major: target.major,
            minor: target.minor,
            patch: target.patch - 1,
        }
    } else {
        VersionSpec::Partial {
            major: target.major,
            minor: target.minor.saturating_sub(1),
        }
    };

    // Check if the previous version is already installed - use it without downloading.
    if let Ok(bv) = toolchain::resolve_installed(config, &prev_spec) {
        return Ok(Bootstrap {
            path: config.version_dir(&bv.tag()),
            cleanup: None,
            label: bv.tag(),
        });
    }

    // Not installed locally - download that specific version as a temporary bootstrap.
    let b_release = index::resolve(&prev_spec, releases).with_context(|| {
        format!(
            "Could not find a bootstrap release for {}. \
             Install a Go version first with 'gvm install latest', \
             or specify one with --bootstrap.",
            prev_spec
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

    // Create a TempDir for the bootstrap staging - it will auto-cleanup on drop
    let staging_dir = TempDir::new_in(config.tmp_dir(), format!("bootstrap-{}", b_version.tag()))?;

    println!(
        "{} Downloading bootstrap {} (temporary, removed after build)...",
        "->".cyan(),
        b_version.tag().bold()
    );

    let archive_path = config.tmp_dir().join(&b_archive.filename);
    if let Err(e) = download::fetch(
        client,
        &index::download_url(&b_archive.filename),
        &archive_path,
    ) {
        let _ = std::fs::remove_file(&archive_path);
        return Err(e).context("Failed to download bootstrap compiler");
    }
    if !b_archive.sha256.is_empty() {
        if let Err(e) = download::verify_sha256(&archive_path, &b_archive.sha256) {
            let _ = std::fs::remove_file(&archive_path);
            return Err(e);
        }
    }

    extract::unpack(&archive_path, staging_dir.path())
        .context("Failed to extract bootstrap compiler")?;
    let _ = std::fs::remove_file(&archive_path);

    // Bootstrap archive also extracts to a `go/` subdirectory.
    let bootstrap_root = staging_dir.path().join("go");
    if !bootstrap_root.exists() {
        anyhow::bail!("Bootstrap archive had an unexpected layout");
    }

    // Keep the TempDir so it cleans up when Bootstrap is dropped
    Ok(Bootstrap {
        path: bootstrap_root,
        cleanup: Some(staging_dir),
        label: format!("{} (downloaded temporarily)", b_version.tag()),
    })
}

/// No explicit cleanup needed - TempDir cleans up automatically on drop.
#[allow(dead_code)]
pub fn cleanup_bootstrap(_b: &Bootstrap) {
    // TempDir handles cleanup automatically when Bootstrap is dropped.
}
