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
