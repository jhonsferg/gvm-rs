//! Source tarball download and verification for Go builds.

use anyhow::{anyhow, Context, Result};

use crate::{archive::download, config::Config, http::HttpClient, remote::{index, release::Release}, tempdir::TempDir};

/// Downloads and verifies the Go source tarball for the given release.
pub fn download_source(
    client: &HttpClient,
    config: &Config,
    release: &Release,
    version: &crate::version::GoVersion,
) -> Result<PathBuf> {
    let src_file = release.source_file().ok_or_else(|| {
        anyhow!(
            "No source tarball found for {}. \
             Source tarballs are only available for stable releases.",
            version.tag()
        )
    })?;

    let src_archive = config.tmp_dir().join(&src_file.filename);

    println!(
        "{} Downloading {}...",
        "->".cyan(),
        src_file.filename.bold()
    );

    download::fetch(client, &index::download_url(&src_file.filename), &src_archive)
        .with_context(|| format!("Failed to download source tarball {}", src_file.filename))?;

    // Verify SHA-256 checksum.
    if !src_file.sha256.is_empty() {
        println!("{} Verifying checksum...", "->".cyan());
        download::verify_sha256(&src_archive, &src_file.sha256)
            .context("Source tarball checksum mismatch")?;
    }

    Ok(src_archive)
}

/// Extracts the source tarball to a staging directory.
pub fn extract_source(
    archive_path: &Path,
    config: &Config,
    version_tag: &str,
) -> Result<PathBuf> {
    // Extract source into a unique staging dir using TempDir for auto-cleanup
    let staging_dir = TempDir::new_in(config.tmp_dir(), format!("src-{}", version_tag))?;

    crate::archive::extract::unpack(archive_path, staging_dir.path())
        .context("Failed to extract source tarball")?;

    let _ = std::fs::remove_file(archive_path);

    // The Go source tarball always extracts to a `go/` subdirectory.
    let source_root = staging_dir.path().join("go");
    if !source_root.exists() {
        anyhow::bail!(
            "Unexpected archive layout: expected 'go/' inside {}",
            staging_dir.path().display()
        );
    }

    // Prevent auto-cleanup since we want to keep the extracted source
    staging_dir.keep();
    Ok(source_root)
}