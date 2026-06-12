//! Canonical Go version representation.
//!
//! [`GoVersion`] holds a parsed `major.minor.patch` triple and provides
//! comparison, display, and the canonical release tag (`go1.22.4`).
//!
//! This type represents a *known, resolved* version. User-supplied input
//! that may be partial (`1.22`) or symbolic (`latest`) is handled by
//! [`crate::user_version::VersionSpec`] instead.

use anyhow::{bail, Result};
use std::fmt;

/// A fully resolved Go version with `major`, `minor`, and `patch` components.
///
/// Versions are ordered semantically: `go1.22.4 > go1.22.3 > go1.21.0`.
/// When `patch` is 0, the version represents the initial release of a minor
/// (e.g. `go1.22`), which is stored and displayed without the trailing `.0`.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct GoVersion {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl GoVersion {
    /// Parses a version string into a [`GoVersion`].
    ///
    /// Accepts both the bare numeric form (`1.22.4`, `1.22`) and the canonical
    /// Go tag form (`go1.22.4`, `go1.22`). The `go` prefix is stripped before
    /// parsing.
    ///
    /// # Errors
    ///
    /// Returns an error if the input does not match `X.Y` or `X.Y.Z` (after
    /// stripping the optional `go` prefix), or if any component is not a valid
    /// unsigned integer.
    pub fn parse(input: &str) -> Result<Self> {
        let s = input.trim().strip_prefix("go").unwrap_or(input.trim());
        let parts: Vec<&str> = s.split('.').collect();
        match parts.as_slice() {
            [major, minor] => Ok(Self {
                major: major.parse()?,
                minor: minor.parse()?,
                patch: 0,
            }),
            [major, minor, patch] => Ok(Self {
                major: major.parse()?,
                minor: minor.parse()?,
                patch: patch.parse()?,
            }),
            _ => bail!(
                "Invalid version '{}'. Use X.Y or X.Y.Z (e.g. 1.22 or 1.22.4)",
                input
            ),
        }
    }

    /// Returns the canonical Go release tag used by go.dev and the local store.
    ///
    /// When `patch` is 0 the tag omits the patch component (`go1.22`);
    /// otherwise all three components are included (`go1.22.4`).
    pub fn tag(&self) -> String {
        if self.patch == 0 {
            format!("go{}.{}", self.major, self.minor)
        } else {
            format!("go{}.{}.{}", self.major, self.minor, self.patch)
        }
    }
}

/// Displays the version without the `go` prefix (`1.22.4` or `1.22`).
impl fmt::Display for GoVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.patch == 0 {
            write!(f, "{}.{}", self.major, self.minor)
        } else {
            write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
        }
    }
}

impl PartialOrd for GoVersion {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// Compares versions by `major`, then `minor`, then `patch` (all ascending).
impl Ord for GoVersion {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.major
            .cmp(&other.major)
            .then(self.minor.cmp(&other.minor))
            .then(self.patch.cmp(&other.patch))
    }
}
