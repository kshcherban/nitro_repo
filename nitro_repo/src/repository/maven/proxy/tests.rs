#![allow(clippy::panic, clippy::unwrap_used)]

use super::*;
use async_trait::async_trait;
use axum::{Router, routing::get};
use http::StatusCode;
use nr_core::repository::project::{ProxyArtifactKey, ProxyArtifactMeta};
use nr_storage::{
    StaticStorageFactory, Storage, StorageConfig, StorageConfigInner, StorageTypeConfig,
    local::LocalConfig, local::LocalStorageFactory,
};
use std::sync::Arc;
use tempfile::tempdir;
use tokio::sync::Mutex;

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

#[test]
fn maven_proxy_meta_parses_coordinates() {
    let path = StoragePath::from("com/example/app/1.2.3/app-1.2.3.jar");
    let meta = maven_proxy_meta_from_cache_path(&path, 2048).expect("meta");
    assert_eq!(meta.package_name, "app");
    assert_eq!(meta.package_key, "com.example:app");
    assert_eq!(meta.version.as_deref(), Some("1.2.3"));
    assert_eq!(meta.cache_path, path.to_string());
    assert_eq!(meta.size, Some(2048));
}

#[tokio::test]
async fn record_maven_proxy_cache_hit_invokes_indexer() {
    let path = StoragePath::from("com/example/app/1.2.3/app-1.2.3.pom");
    let indexer = Arc::new(RecordingIndexer::default());
    record_maven_proxy_cache_hit(Some(indexer.clone().as_ref()), &path, 1024)
        .await
        .expect("recording succeeds");
    let recorded = indexer.recorded().await;
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].version.as_deref(), Some("1.2.3"));
}

#[tokio::test]
async fn evict_maven_proxy_cache_entry_invokes_indexer() {
    let path = StoragePath::from("com/example/app/1.2.3/app-1.2.3.pom");
    let indexer = Arc::new(RecordingIndexer::default());
    evict_maven_proxy_cache_entry(Some(indexer.clone().as_ref()), &path)
        .await
        .expect("eviction succeeds");
    let evicted = indexer.evicted().await;
    assert_eq!(evicted.len(), 1);
    assert_eq!(evicted[0].version.as_deref(), Some("1.2.3"));
}

#[test]
fn snapshot_routes_releases_read_lock() {
    let routes = vec![MavenProxyRepositoryRoute {
        url: ProxyURL::try_from(String::from("https://repo.example.com")).unwrap(),
        name: None,
        priority: Some(1),
    }];
    let lock = RwLock::new(MavenProxyConfig { routes });

    let cloned = snapshot_routes(&lock);
    assert_eq!(cloned.len(), 1);
    assert!(
        lock.try_write().is_some(),
        "read lock should not be held after snapshot"
    );
}

#[tokio::test]
async fn stream_response_to_tempfile_streams_full_body() {
    let payload = vec![7u8; 64 * 1024];
    let chunks: Vec<Bytes> = payload
        .chunks(4096)
        .map(|chunk| Bytes::copy_from_slice(chunk))
        .collect();
    let chunks = Arc::new(chunks);

    let app = Router::new().route(
        "/artifact",
        get({
            let chunks = chunks.clone();
            move || {
                let chunks = chunks.clone();
                async move {
                    let owned_chunks: Vec<Bytes> = chunks.iter().cloned().collect();
                    let stream = futures::stream::iter(
                        owned_chunks
                            .into_iter()
                            .map(|chunk| Ok::<_, std::convert::Infallible>(chunk)),
                    );
                    Response::builder()
                        .status(StatusCode::OK)
                        .body(Body::from_stream(stream))
                        .unwrap()
                }
            }
        }),
    );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind listener");
    let addr = listener.local_addr().expect("local addr");
    let server = tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("axum server should run");
    });

    let url = format!("http://{}/artifact", addr);
    let response = reqwest::Client::new()
        .get(&url)
        .send()
        .await
        .expect("http request");

    let streamed = stream_response_to_tempfile(response)
        .await
        .expect("stream to tempfile");
    let stored = tokio::fs::read(&streamed.path)
        .await
        .expect("read tempfile");

    assert_eq!(stored, payload);
    assert_eq!(streamed.size, payload.len() as u64);

    server.abort();
}

#[tokio::test]
async fn persist_streamed_download_records_size_and_writes_file() -> anyhow::Result<()> {
    let repository_id = Uuid::new_v4();
    let tempdir = tempdir()?;
    let storage = DynStorage::Local(
        LocalStorageFactory::create_storage_from_config(StorageConfig {
            storage_config: StorageConfigInner::test_config(),
            type_config: StorageTypeConfig::Local(LocalConfig {
                path: tempdir.path().to_path_buf(),
            }),
        })
        .await?,
    );

    let content = b"artifact-bytes".to_vec();
    let file = tempfile::Builder::new()
        .prefix("maven-proxy-test-")
        .tempfile()?;
    std::fs::write(file.path(), &content)?;
    let streamed = StreamedDownload {
        path: file.into_temp_path(),
        size: content.len() as u64,
    };

    let dest = StoragePath::from("com/example/app/1.0.0/app-1.0.0.jar");
    let indexer = Arc::new(RecordingIndexer::default());

    persist_streamed_download(
        &storage,
        repository_id,
        Some(indexer.as_ref()),
        &streamed,
        &dest,
    )
    .await?;

    let recorded = indexer.recorded().await;
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].size, Some(content.len() as u64));

    let saved = storage
        .open_file(repository_id, &dest)
        .await?
        .expect("file exists");
    assert!(saved.is_file());

    Ok(())
}
