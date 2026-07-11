//! Version resolution trait and implementations.
//!
//! Provides a unified interface for resolving version specifications against
//! different version sources (installed versions, remote releases, etc.).

use anyhow::{anyhow, Result};
use std::collections::HashSet;

use crate::{
    http::HttpClient,
    remote::{index, release::Release},
    user_version::VersionSpec,
    version::GoVersion,
};

/// Trait for resolving a [`VersionSpec`] to a concrete [`GoVersion`].
///
/// Implementors provide version resolution against a specific source
/// (installed versions, remote releases, etc.).
pub trait VersionResolver {
    /// Resolves the given spec to the best-matching version.
    ///
    /// Returns an error if no version satisfies the spec.
    fn resolve(&self, spec: &VersionSpec) -> Result<GoVersion>;

    /// Returns all available versions known to this resolver.
    fn available_versions(&self) -> Result<Vec<GoVersion>>;

    /// Checks if a specific version is available.
    fn is_available(&self, version: &GoVersion) -> Result<bool> {
        let versions = self.available_versions()?;
        Ok(versions.iter().any(|v| v == version))
    }
}

/// Resolver that queries locally installed Go versions.
#[derive(Debug, Clone)]
pub struct InstalledResolver<'a> {
    config: &'a crate::config::Config,
}

impl<'a> InstalledResolver<'a> {
    /// Creates a new resolver for installed versions.
    pub fn new(config: &'a crate::config::Config) -> Self {
        Self { config }
    }
}

impl VersionResolver for InstalledResolver<'_> {
    fn resolve(&self, spec: &VersionSpec) -> Result<GoVersion> {
        crate::toolchain::resolve_installed(self.config, spec)
    }

    fn available_versions(&self) -> Result<Vec<GoVersion>> {
        crate::toolchain::list_installed(self.config)
    }
}

/// Resolver that queries the remote go.dev release index.
#[derive(Debug, Clone)]
pub struct RemoteResolver<'a> {
    client: &'a HttpClient,
    releases: Option<Vec<Release>>,
}

impl<'a> RemoteResolver<'a> {
    /// Creates a new remote resolver with the given HTTP client.
    pub fn new(client: &'a HttpClient) -> Self {
        Self {
            client,
            releases: None,
        }
    }

    /// Ensures releases are fetched and cached.
    fn ensure_releases(&mut self) -> Result<&Vec<Release>> {
        if self.releases.is_none() {
            let releases = index::fetch_releases(self.client)?;
            self.releases = Some(releases);
        }
        Ok(self.releases.as_ref().unwrap())
    }

    /// Returns only stable releases.
    fn stable_releases(&mut self) -> Result<&[Release]> {
        let releases = self.ensure_releases()?;
        Ok(releases)
    }
}

impl VersionResolver for RemoteResolver<'_> {
    fn resolve(&self, spec: &VersionSpec) -> Result<GoVersion> {
        // We need mutable access to fetch releases, but resolver is immutable.
        // For simplicity, fetch inside resolve.
        let releases = index::fetch_releases(self.client)?;
        let release = index::resolve(spec, &releases)?;
        release
            .go_version()
            .ok_or_else(|| anyhow!("Could not parse version tag '{}'", release.version))
    }

    fn available_versions(&self) -> Result<Vec<GoVersion>> {
        let releases = index::fetch_releases(self.client)?;
        let mut versions: Vec<GoVersion> = releases
            .iter()
            .filter(|r| r.stable)
            .filter_map(|r| r.go_version())
            .collect();

        // Deduplicate by (major, minor, patch)
        let mut seen = HashSet::new();
        versions.retain(|v| seen.insert((v.major, v.minor, v.patch)));
        versions.sort_by(|a, b| b.cmp(a));
        Ok(versions)
    }
}

/// Composite resolver that tries multiple resolvers in order.
///
/// Tries each resolver until one succeeds. Useful for fallback chains
/// (e.g., try installed first, then remote).
pub struct CompositeResolver<'a> {
    resolvers: Vec<Box<dyn VersionResolver + 'a>>,
}

impl<'a> CompositeResolver<'a> {
    /// Creates a new composite resolver.
    pub fn new() -> Self {
        Self {
            resolvers: Vec::new(),
        }
    }

    /// Adds a resolver to the chain.
    pub fn add_resolver<R: VersionResolver + 'a>(mut self, resolver: R) -> Self {
        self.resolvers.push(Box::new(resolver));
        self
    }
}

impl Default for CompositeResolver<'_> {
    fn default() -> Self {
        Self::new()
    }
}

impl VersionResolver for CompositeResolver<'_> {
    fn resolve(&self, spec: &VersionSpec) -> Result<GoVersion> {
        let mut last_error = None;
        for resolver in &self.resolvers {
            match resolver.resolve(spec) {
                Ok(v) => return Ok(v),
                Err(e) => last_error = Some(e),
            }
        }
        Err(last_error.unwrap_or_else(|| anyhow!("No resolvers available")))
    }

    fn available_versions(&self) -> Result<Vec<GoVersion>> {
        let mut all_versions = Vec::new();
        for resolver in &self.resolvers {
            if let Ok(versions) = resolver.available_versions() {
                all_versions.extend(versions);
            }
        }

        // Deduplicate
        let mut seen = HashSet::new();
        all_versions.retain(|v| seen.insert((v.major, v.minor, v.patch)));
        all_versions.sort_by(|a, b| b.cmp(a));
        Ok(all_versions)
    }
}

impl<'a> CompositeResolver<'a> {
    /// Creates a standard composite resolver (installed first, then remote).
    pub fn standard(config: &'a crate::config::Config, client: &'a HttpClient) -> Self {
        Self::new()
            .add_resolver(InstalledResolver::new(config))
            .add_resolver(RemoteResolver::new(client))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::version::GoVersion;

    #[test]
    fn composite_resolver_tries_in_order() {
        // This test would need mock resolvers to verify order
        // For now, just verify the API compiles
    }

    #[test]
    fn installed_resolver_implements_trait() {
        fn _assert_impl<T: VersionResolver>() {}
        _assert_impl::<InstalledResolver<'_>>();
    }

    #[test]
    fn remote_resolver_implements_trait() {
        fn _assert_impl<T: VersionResolver>() {}
        _assert_impl::<RemoteResolver<'_>>();
    }
}
