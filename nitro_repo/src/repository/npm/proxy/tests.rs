#![allow(clippy::expect_used, clippy::panic, clippy::todo, clippy::unwrap_used)]
use super::*;
use crate::repository::proxy_indexing::{ProxyIndexing, ProxyIndexingError};
use async_trait::async_trait;
use nr_core::repository::project::{ProxyArtifactKey, ProxyArtifactMeta};
use std::sync::Arc;
use tokio::sync::Mutex;

#[test]
fn cache_path_for_scoped_package() {
    let path = StoragePath::from("@scope/package/-/package-1.0.0.tgz");
    let cache = cache_path_for_npm_proxy(&path).expect("cache path");
    assert_eq!(
        cache.to_string(),
        "packages/@scope/package/package-1.0.0.tgz"
    );
}

#[test]
fn cache_path_for_unscoped_package() {
    let path = StoragePath::from("left-pad/-/left-pad-1.3.0.tgz");
    let cache = cache_path_for_npm_proxy(&path).expect("cache path");
    assert_eq!(cache.to_string(), "packages/left-pad/left-pad-1.3.0.tgz");
}

#[test]
fn cache_path_requires_tarball_segment() {
    let path = StoragePath::from("left-pad/latest");
    assert!(cache_path_for_npm_proxy(&path).is_none());
}

#[test]
fn normalize_routes_adds_default_when_empty() {
    let routes = normalize_routes(Vec::new());
    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0], DEFAULT_ROUTE.clone());
}

#[test]
fn normalize_routes_keeps_existing_entries() {
    let custom = NpmProxyRoute {
        url: ProxyURL::try_from(String::from("https://mirror.npm.internal")).expect("valid url"),
        name: Some("mirror".to_string()),
    };
    let routes = normalize_routes(vec![custom.clone()]);
    assert_eq!(routes, vec![custom]);
}

fn npm_tarball_path() -> StoragePath {
    StoragePath::from("packages/@scope/package/package-1.2.3.tgz")
}

#[test]
fn npm_proxy_meta_from_cache_path_parses_scoped_tarball() {
    let path = npm_tarball_path();
    let meta = super::npm_proxy_meta_from_cache_path(&path, 4096, None).expect("metadata");
    assert_eq!(meta.package_name, "@scope/package");
    assert_eq!(meta.package_key, "@scope/package");
    assert_eq!(meta.version.as_deref(), Some("1.2.3"));
    assert_eq!(meta.cache_path, path.to_string());
    assert_eq!(meta.size, Some(4096));
}

#[test]
fn npm_proxy_meta_from_cache_path_handles_unscoped_tarball() {
    let path = StoragePath::from("packages/left-pad/left-pad-1.3.0.tgz");
    let meta = super::npm_proxy_meta_from_cache_path(&path, 1024, None).expect("metadata");
    assert_eq!(meta.package_name, "left-pad");
    assert_eq!(meta.package_key, "left-pad");
    assert_eq!(meta.version.as_deref(), Some("1.3.0"));
}

#[tokio::test]
async fn record_npm_proxy_cache_hit_invokes_indexer() {
    let path = npm_tarball_path();
    let indexer = RecordingIndexer::default();

    super::record_npm_proxy_cache_hit(&indexer, &path, 2048, None)
        .await
        .expect("indexing succeeds");
    let recorded = indexer.recorded().await;
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].package_key, "@scope/package");
    assert_eq!(recorded[0].version.as_deref(), Some("1.2.3"));
}

#[tokio::test]
async fn evict_npm_proxy_cache_entry_invokes_indexer() {
    let path = npm_tarball_path();
    let indexer = RecordingIndexer::default();

    super::evict_npm_proxy_cache_entry(&indexer, &path)
        .await
        .expect("eviction succeeds");
    let evicted = indexer.evicted().await;
    assert_eq!(evicted.len(), 1);
    assert_eq!(evicted[0].package_key, "@scope/package");
    assert_eq!(evicted[0].version.as_deref(), Some("1.2.3"));
}

#[tokio::test]
async fn metadata_cache_hit_indexes_versions() {
    let metadata = r#"{
        "name": "left-pad",
        "versions": {
            "1.0.0": {
                "version": "1.0.0",
                "dist": {
                    "tarball": "https://registry.npmjs.org/left-pad/-/left-pad-1.0.0.tgz",
                    "integrity": "sha512-deadbeef"
                }
            },
            "2.0.0": {
                "version": "2.0.0",
                "dist": {
                    "tarball": "https://registry.npmjs.org/left-pad/-/left-pad-2.0.0.tgz"
                }
            }
        }
    }"#;

    let indexer = RecordingIndexer::default();

    let mut tarballs = super::record_npm_metadata_cache_hit(&indexer, metadata.as_bytes())
        .await
        .expect("metadata indexed");
    tarballs.sort_by(|a, b| a.0.as_str().cmp(b.0.as_str()));

    let mut recorded = indexer.recorded().await;
    recorded.sort_by(|a, b| a.version.cmp(&b.version));

    assert_eq!(recorded.len(), 2);
    assert_eq!(recorded[0].package_key, "left-pad");
    assert_eq!(recorded[0].version.as_deref(), Some("1.0.0"));
    assert_eq!(
        recorded[0].cache_path,
        "packages/left-pad/left-pad-1.0.0.tgz"
    );
    assert_eq!(
        recorded[0].upstream_url.as_deref(),
        Some("https://registry.npmjs.org/left-pad/-/left-pad-1.0.0.tgz")
    );
    assert_eq!(
        recorded[0].upstream_digest.as_deref(),
        Some("sha512-deadbeef")
    );

    assert_eq!(recorded[1].version.as_deref(), Some("2.0.0"));
    assert_eq!(
        recorded[1].cache_path,
        "packages/left-pad/left-pad-2.0.0.tgz"
    );

    assert_eq!(tarballs.len(), 2);
    assert_eq!(
        tarballs[0].0.as_str(),
        "https://registry.npmjs.org/left-pad/-/left-pad-1.0.0.tgz"
    );
    assert_eq!(
        tarballs[0].1.to_string(),
        "packages/left-pad/left-pad-1.0.0.tgz"
    );
    assert_eq!(
        tarballs[1].1.to_string(),
        "packages/left-pad/left-pad-2.0.0.tgz"
    );
}

#[derive(Clone, Default)]
struct RecordingIndexer {
    recorded: Arc<Mutex<Vec<ProxyArtifactMeta>>>,
    evicted: Arc<Mutex<Vec<ProxyArtifactKey>>>,
}

impl RecordingIndexer {
    async fn recorded(&self) -> Vec<ProxyArtifactMeta> {
        self.recorded.lock().await.clone()
    }

    async fn evicted(&self) -> Vec<ProxyArtifactKey> {
        self.evicted.lock().await.clone()
    }
}

#[async_trait]
impl ProxyIndexing for RecordingIndexer {
    async fn record_cached_artifact(
        &self,
        meta: ProxyArtifactMeta,
    ) -> Result<(), ProxyIndexingError> {
        self.recorded.lock().await.push(meta);
        Ok(())
    }

    async fn evict_cached_artifact(&self, key: ProxyArtifactKey) -> Result<(), ProxyIndexingError> {
        self.evicted.lock().await.push(key);
        Ok(())
    }
}
