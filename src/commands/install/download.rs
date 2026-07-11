//! Download and verification logic for `gvm install`.

use anyhow::{anyhow, Context, Result};
use colored::Colorize;

use crate::{
    archive::download,
    config::Config,
    http::HttpClient,
    remote::{index, release::Release, release::ReleaseFile},
    tempdir::TempDir,
    toolchain,
    user_version::VersionSpec,
    version::GoVersion,
};

/// Downloads the archive for the specified version.
pub fn download_archive(
    client: &HttpClient,
    config: &Config,
    release: &Release,
    version: &GoVersion,
) -> Result<ReleaseArchive> {
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

    download::fetch(client, &url, &archive_path)
        .with_context(|| format!("Failed to download {}", file.filename))?;

    println!("{} Verifying checksum...", "->".cyan());
    download::verify_sha256(&archive_path, &file.sha256).context("Archive checksum mismatch")?;

    Ok(ReleaseArchive {
        path: archive_path,
        file: file.clone(),
    })
}

/// Archive information returned after download.
pub struct ReleaseArchive {
    pub path: std::path::PathBuf,
    pub file: ReleaseFile,
}
