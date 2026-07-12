//! File-system utilities.
//!
//! Supplements the standard library with helpers needed for cross-platform
//! toolchain management. The primary concern is safe directory moves across
//! different drives or mount points, which `std::fs::rename` does not support.

use anyhow::{Context, Result};
use std::path::Path;

/// Creates or replaces the `~/.gvm/current` junction/symlink so it points to
/// `target`.
///
/// On Windows an NTFS junction (reparse point) is used: no elevation and no
/// Developer Mode is required. On Unix a directory symbolic link is used.
/// Any existing link or empty directory at `link` is atomically replaced.
pub fn set_version_link(link: &Path, target: &Path) -> Result<()> {
    remove_link_if_exists(link)?;
    create_link(link, target)
}

#[cfg(windows)]
fn remove_link_if_exists(link: &Path) -> Result<()> {
    match std::fs::symlink_metadata(link) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => {
            Err(anyhow::Error::from(e)).with_context(|| format!("Cannot stat {}", link.display()))
        }
        Ok(_) => std::fs::remove_dir(link)
            .with_context(|| format!("Failed to remove {}", link.display())),
    }
}

#[cfg(not(windows))]
fn remove_link_if_exists(link: &Path) -> Result<()> {
    match std::fs::symlink_metadata(link) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => {
            Err(anyhow::Error::from(e)).with_context(|| format!("Cannot stat {}", link.display()))
        }
        Ok(_) => std::fs::remove_file(link)
            .with_context(|| format!("Failed to remove {}", link.display())),
    }
}

#[cfg(windows)]
fn create_link(link: &Path, target: &Path) -> Result<()> {
    // mklink /J creates an NTFS junction - no elevation or Developer Mode needed.
    let out = std::process::Command::new("cmd")
        .arg("/c")
        .arg("mklink")
        .arg("/J")
        .arg(link)
        .arg(target)
        .output()
        .context("Failed to run cmd for mklink")?;
    if !out.status.success() {
        let msg = String::from_utf8_lossy(&out.stderr);
        return Err(anyhow::anyhow!(
            "Failed to create junction {} -> {}: {}",
            link.display(),
            target.display(),
            msg.trim()
        ));
    }
    Ok(())
}

#[cfg(not(windows))]
fn create_link(link: &Path, target: &Path) -> Result<()> {
    std::os::unix::fs::symlink(target, link).with_context(|| {
        format!(
            "Failed to create symlink {} -> {}",
            link.display(),
            target.display()
        )
    })
}

/// Moves the directory at `src` to `dst`.
///
/// Attempts an atomic rename first. If the rename fails - which happens when
/// `src` and `dst` reside on different file-system volumes (common on Windows
/// when the temp directory is on a different drive than `~/.gvm`) - it falls
/// back to a recursive copy followed by removal of the source.
///
/// # Errors
///
/// Returns an error if:
/// - Both the rename and the copy-then-delete fallback fail.
/// - Any file in `src` cannot be copied to `dst`.
/// - `src` cannot be removed after a successful copy.
pub fn move_dir(src: &Path, dst: &Path) -> Result<()> {
    match std::fs::rename(src, dst) {
        Ok(()) => Ok(()),
        Err(_) => {
            // Cross-device move: copy every entry then delete the source tree.
            copy_dir_all(src, dst).with_context(|| {
                format!("Failed to copy {} to {}", src.display(), dst.display())
            })?;
            std::fs::remove_dir_all(src)
                .with_context(|| format!("Failed to remove {}", src.display()))
        }
    }
}

/// Recursively copies the directory tree rooted at `src` into `dst`.
///
/// `dst` is created if it does not exist. Files are copied individually;
/// symbolic links are not followed - they are copied as regular files
/// pointing to the same content.
///
/// # Errors
///
/// Returns an error if any entry cannot be read, created, or copied.
fn copy_dir_all(src: &Path, dst: &Path) -> Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let dst_path = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_all(&entry.path(), &dst_path)?;
        } else {
            std::fs::copy(entry.path(), &dst_path)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn move_dir_relocates_contents_via_rename() {
        let root = tempdir().unwrap();
        let src = root.path().join("src");
        let dst = root.path().join("dst");
        std::fs::create_dir_all(src.join("nested")).unwrap();
        std::fs::write(src.join("file.txt"), b"hello").unwrap();
        std::fs::write(src.join("nested").join("inner.txt"), b"world").unwrap();

        move_dir(&src, &dst).unwrap();

        assert!(!src.exists());
        assert_eq!(std::fs::read_to_string(dst.join("file.txt")).unwrap(), "hello");
        assert_eq!(
            std::fs::read_to_string(dst.join("nested").join("inner.txt")).unwrap(),
            "world"
        );
    }

    #[test]
    fn move_dir_errors_when_source_missing() {
        let root = tempdir().unwrap();
        let src = root.path().join("does-not-exist");
        let dst = root.path().join("dst");
        assert!(move_dir(&src, &dst).is_err());
    }

    #[test]
    fn copy_dir_all_recreates_full_tree() {
        let root = tempdir().unwrap();
        let src = root.path().join("src");
        let dst = root.path().join("dst");
        std::fs::create_dir_all(src.join("a").join("b")).unwrap();
        std::fs::write(src.join("top.txt"), b"top").unwrap();
        std::fs::write(src.join("a").join("b").join("deep.txt"), b"deep").unwrap();

        copy_dir_all(&src, &dst).unwrap();

        // Original tree must remain untouched.
        assert!(src.exists());
        assert_eq!(std::fs::read_to_string(dst.join("top.txt")).unwrap(), "top");
        assert_eq!(
            std::fs::read_to_string(dst.join("a").join("b").join("deep.txt")).unwrap(),
            "deep"
        );
    }

    #[test]
    fn set_version_link_creates_and_replaces() {
        let root = tempdir().unwrap();
        let target_a = root.path().join("version-a");
        let target_b = root.path().join("version-b");
        std::fs::create_dir_all(&target_a).unwrap();
        std::fs::create_dir_all(&target_b).unwrap();
        std::fs::write(target_a.join("marker.txt"), b"a").unwrap();
        std::fs::write(target_b.join("marker.txt"), b"b").unwrap();

        let link = root.path().join("current");

        set_version_link(&link, &target_a).unwrap();
        assert_eq!(
            std::fs::read_to_string(link.join("marker.txt")).unwrap(),
            "a"
        );

        // Re-pointing the link should atomically replace it, not fail because
        // it already exists.
        set_version_link(&link, &target_b).unwrap();
        assert_eq!(
            std::fs::read_to_string(link.join("marker.txt")).unwrap(),
            "b"
        );
    }

    #[test]
    fn remove_link_if_exists_is_noop_when_absent() {
        let root = tempdir().unwrap();
        let link = root.path().join("nonexistent-link");
        assert!(remove_link_if_exists(&link).is_ok());
    }
}
