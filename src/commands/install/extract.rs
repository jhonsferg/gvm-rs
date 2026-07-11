//! Archive extraction for `gvm install`.

use anyhow::{Context, Result};

use crate::{archive::extract, config::Config, tempdir::TempDir};

/// Extracts the downloaded archive to a staging directory.
pub fn extract_archive(
    archive_path: &std::path::Path,
    config: &Config,
    version_tag: &str,
) -> Result<std::path::PathBuf> {
    // Use TempDir for auto-cleanup on error
    let staging_dir = TempDir::new_in(config.tmp_dir(), format!("src-{}", version_tag))?;

    extract::unpack(archive_path, staging_dir.path()).context("Failed to extract archive")?;

    // Verify expected layout: archive extracts to a "go/" subdirectory
    let source_root = staging_dir.path().join("go");
    if !source_root.exists() {
        anyhow::bail!(
            "Unexpected archive layout: expected 'go/' inside {}",
            staging_dir.path().display()
        );
    }

    // Prevent auto-cleanup since caller will handle the extracted source
    staging_dir.keep();
    Ok(source_root)
}
