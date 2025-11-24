//! Docker proxy (pull-through cache) support.
//!
//! The proxy repository is read-only and forwards GET/HEAD requests to an
//! upstream Docker registry, caching responses locally.

use std::{
    num::NonZeroUsize,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    task::{Context, Poll},
    time::Duration,
};

use axum::body::Body;
use chrono::{DateTime, Utc};
use futures::StreamExt;
use http::{
    HeaderMap, StatusCode,
    header::{ACCEPT, CONTENT_LENGTH, CONTENT_TYPE},
};
use lru::LruCache;
use nr_core::{
    repository::Visibility, repository::config::RepositoryConfigType, storage::StoragePath,
    utils::base64_utils,
};
use nr_storage::{DynStorage, FileContent, FileType, Storage, StorageFile, StorageFileReader};
use parking_lot::RwLock;
use reqwest::{Client, Response};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::Digest;
use std::sync::OnceLock;
use tempfile::Builder;
use tokio::io::{AsyncRead, AsyncWriteExt};
use tokio::time::sleep;
use tracing::{instrument, warn};
use url::Url;
use uuid::Uuid;

use super::{DockerError, types::MediaType};
use crate::repository::docker::DockerRegistryConfigType;
use crate::{
    app::NitroRepo,
    repository::{
        RepoResponse, Repository, RepositoryAuthConfigType, RepositoryFactoryError,
        RepositoryRequest, utils::can_read_repository_with_auth,
    },
    utils::ResponseBuilder,
};

/// Docker proxy configuration for upstream registries
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DockerProxyConfig {
    /// Upstream registry URL (e.g., "https://registry-1.docker.io")
    pub upstream_url: String,

    /// Optional authentication for upstream registry
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upstream_auth: Option<DockerProxyAuth>,

    /// Enable caching of pulled images
    #[serde(default = "default_cache_enabled")]
    pub cache_enabled: bool,

    /// How often to revalidate mutable tag manifests against upstream (seconds).
    /// Set to 0 to always revalidate; large values reduce HEAD traffic.
    #[serde(default = "default_revalidation_ttl")]
    pub revalidation_ttl_seconds: u64,

    /// Disable tag revalidation (not recommended; for air-gapped deployments only).
    #[serde(default)]
    pub skip_tag_revalidation: bool,
}

fn default_cache_enabled() -> bool {
    true
}

fn default_revalidation_ttl() -> u64 {
    300
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DockerProxyAuth {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Clone)]
pub struct ProxyUpstream {
    base: Url,
    client: Client,
    cache_enabled: bool,
    revalidation_ttl: u64,
    skip_tag_revalidation: bool,
}

impl ProxyUpstream {
    pub(crate) fn new(config: &DockerProxyConfig) -> Result<Self, DockerError> {
        let base = Url::parse(&config.upstream_url)?;
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(300))
            .build()?;
        Ok(Self {
            base,
            client,
            cache_enabled: config.cache_enabled,
            revalidation_ttl: config.revalidation_ttl_seconds,
            skip_tag_revalidation: config.skip_tag_revalidation,
        })
    }

    #[instrument(skip(self))]
    pub(crate) async fn fetch(
        &self,
        path: &str,
        accept: Option<&str>,
    ) -> Result<Response, DockerError> {
        let url = self.base.clone();
        let joined = url.join(path)?;
        self.fetch_with_optional_token(joined, accept, None).await
    }

    async fn fetch_with_optional_token(
        &self,
        url: Url,
        accept: Option<&str>,
        bearer: Option<String>,
    ) -> Result<Response, DockerError> {
        const MAX_ATTEMPTS: usize = 3;

        let mut current_bearer = bearer;
        for _ in 0..2 {
            let response = Self::send_with_retries(
                &self.client,
                url.clone(),
                accept,
                current_bearer.as_deref(),
                MAX_ATTEMPTS,
            )
            .await?;

            if response.status() != StatusCode::UNAUTHORIZED {
                return Ok(response);
            }

            let Some(auth_header) = response
                .headers()
                .get(http::header::WWW_AUTHENTICATE)
                .and_then(|v| v.to_str().ok())
            else {
                return Ok(response);
            };

            let Some(challenge) = parse_bearer_challenge(auth_header) else {
                return Ok(response);
            };

            // already tried with bearer; avoid infinite loop
            if current_bearer.is_some() {
                return Ok(response);
            }
            let token = self.obtain_token(&challenge).await?;
            current_bearer = Some(token);
            // retry with token in next loop iteration
        }

        // last attempt if loop exits unexpectedly
        Self::send_with_retries(
            &self.client,
            url,
            accept,
            current_bearer.as_deref(),
            MAX_ATTEMPTS,
        )
        .await
    }

    async fn send_with_retries(
        client: &Client,
        url: Url,
        accept: Option<&str>,
        bearer: Option<&str>,
        max_attempts: usize,
    ) -> Result<Response, DockerError> {
        for attempt in 0..max_attempts {
            let mut request = client.get(url.clone());
            if let Some(value) = accept {
                request = request.header(ACCEPT, value);
            }
            if let Some(token) = bearer {
                request = request.header(http::header::AUTHORIZATION, format!("Bearer {token}"));
            }

            match request.send().await {
                Ok(resp) => {
                    if resp.status().is_server_error() && attempt + 1 < max_attempts {
                        let backoff = 200 * (attempt as u64 + 1);
                        sleep(Duration::from_millis(backoff)).await;
                        continue;
                    }
                    return Ok(resp);
                }
                Err(err)
                    if err.is_timeout() || err.is_connect() || err.is_body() || err.is_decode() =>
                {
                    if attempt + 1 < max_attempts {
                        let backoff = 200 * (attempt as u64 + 1);
                        sleep(Duration::from_millis(backoff)).await;
                        continue;
                    }
                    return Err(DockerError::InvalidManifest(format!(
                        "Upstream fetch error after {} attempts: {}",
                        max_attempts, err
                    )));
                }
                Err(err) => {
                    return Err(DockerError::InvalidManifest(format!(
                        "Upstream fetch error: {}",
                        err
                    )));
                }
            }
        }

        Err(DockerError::InvalidManifest(
            "Exhausted upstream retries".to_string(),
        ))
    }

    async fn obtain_token(&self, challenge: &BearerChallenge) -> Result<String, DockerError> {
        let mut realm = Url::parse(&challenge.realm)?;
        {
            let mut query = realm.query_pairs_mut();
            if let Some(service) = &challenge.service {
                query.append_pair("service", service);
            }
            if let Some(scope) = &challenge.scope {
                query.append_pair("scope", scope);
            }
        }
        let token_resp = self.client.get(realm).send().await?;
        if !token_resp.status().is_success() {
            return Err(DockerError::InvalidManifest(format!(
                "Upstream auth failed with status {}",
                token_resp.status()
            )));
        }
        let value: serde_json::Value = token_resp.json().await?;
        let token = value
            .get("token")
            .or_else(|| value.get("access_token"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                DockerError::InvalidManifest("Upstream auth response missing token".to_string())
            })?
            .to_string();
        Ok(token)
    }
}

#[derive(Debug, Clone)]
struct BearerChallenge {
    realm: String,
    service: Option<String>,
    scope: Option<String>,
}

fn parse_bearer_challenge(header: &str) -> Option<BearerChallenge> {
    // Example: Bearer realm="https://auth.docker.io/token",service="registry.docker.io",scope="repository:library/nginx:pull"
    if !header.to_ascii_lowercase().starts_with("bearer ") {
        return None;
    }
    let params = header["Bearer ".len()..].trim();
    let mut realm = None;
    let mut service = None;
    let mut scope = None;
    for part in params.split(',') {
        let mut kv = part.trim().splitn(2, '=');
        let key = kv.next()?.trim();
        let val = kv.next()?.trim().trim_matches('"');
        match key {
            "realm" => realm = Some(val.to_string()),
            "service" => service = Some(val.to_string()),
            "scope" => scope = Some(val.to_string()),
            _ => {}
        }
    }
    realm.map(|realm| BearerChallenge {
        realm,
        service,
        scope,
    })
}

fn manifest_media_type(bytes: &[u8], headers: Option<&HeaderMap>) -> String {
    if let Some(headers) = headers {
        if let Some(value) = headers.get(CONTENT_TYPE).and_then(|v| v.to_str().ok()) {
            return value.to_string();
        }
    }

    if let Ok(value) = serde_json::from_slice::<serde_json::Value>(bytes) {
        if let Some(media_type) = value.get("mediaType").and_then(|v| v.as_str()) {
            return media_type.to_string();
        }
        if value
            .get("schemaVersion")
            .and_then(|v| v.as_u64())
            .map(|v| v == 1)
            .unwrap_or(false)
        {
            return "application/vnd.docker.distribution.manifest.v1+json".to_string();
        }
    }

    MediaType::OCI_IMAGE_MANIFEST.to_string()
}

fn compute_sha256_hex(bytes: &[u8]) -> String {
    format!("sha256:{:x}", sha2::Sha256::digest(bytes))
}

fn digest_from_header(headers: &HeaderMap) -> Option<String> {
    headers
        .get("Docker-Content-Digest")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
}

fn digest_from_hash(hash: &str) -> Option<String> {
    let trimmed = hash.trim();
    if trimmed.len() == 64 && trimmed.chars().all(|c| c.is_ascii_hexdigit()) {
        return Some(format!("sha256:{trimmed}"));
    }
    if let Ok(bytes) = base64_utils::decode(trimmed) {
        if bytes.len() == 32 {
            let mut hex = String::with_capacity(64);
            for byte in bytes {
                use std::fmt::Write;
                let _ = write!(&mut hex, "{:02x}", byte);
            }
            return Some(format!("sha256:{hex}"));
        }
    }
    None
}

fn manifest_meta_path(manifest_path: &StoragePath) -> StoragePath {
    let mut as_string = manifest_path.to_string();
    as_string.push_str(".nr-docker-tagmeta");
    StoragePath::from(as_string)
}

async fn load_manifest_meta(
    storage: &DynStorage,
    repository_id: Uuid,
    manifest_path: &StoragePath,
) -> Result<Option<ManifestMeta>, DockerError> {
    let meta_path = manifest_meta_path(manifest_path);
    let Some(StorageFile::File { content, .. }) =
        storage.open_file(repository_id, &meta_path).await?
    else {
        return Ok(None);
    };

    let bytes = content.read_to_vec(4096).await?;
    let meta: ManifestMeta = serde_json::from_slice(&bytes).map_err(|err| {
        DockerError::InvalidManifest(format!("Invalid manifest meta json: {err}"))
    })?;
    Ok(Some(meta))
}

async fn save_manifest_meta(
    storage: &DynStorage,
    repository_id: Uuid,
    manifest_path: &StoragePath,
    digest: &str,
) -> Result<(), DockerError> {
    let meta = ManifestMeta {
        digest: digest.to_string(),
        last_checked: Utc::now(),
    };
    let bytes = serde_json::to_vec(&meta)?;
    let meta_path = manifest_meta_path(manifest_path);
    storage
        .save_file(repository_id, FileContent::Bytes(bytes.into()), &meta_path)
        .await?;
    Ok(())
}

const DEFAULT_UPSTREAM_ACCEPT: &str = "\
application/vnd.docker.distribution.manifest.list.v2+json, \
application/vnd.docker.distribution.manifest.v2+json, \
application/vnd.oci.image.index.v1+json, \
application/vnd.oci.image.manifest.v1+json, \
application/vnd.docker.distribution.manifest.v1+json";

const MODERN_UPSTREAM_ACCEPT: &str = "\
application/vnd.docker.distribution.manifest.list.v2+json, \
application/vnd.oci.image.index.v1+json, \
application/vnd.docker.distribution.manifest.v2+json, \
application/vnd.oci.image.manifest.v1+json";

fn reorder_accept_header(raw: Option<&str>) -> Option<String> {
    let cleaned = raw.and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    });

    let header = match cleaned {
        Some(value) => value,
        None => return Some(DEFAULT_UPSTREAM_ACCEPT.to_string()),
    };

    let mut entries: Vec<(usize, &str, u8)> = header
        .split(',')
        .enumerate()
        .filter_map(|(idx, token)| {
            let trimmed = token.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some((idx, trimmed, accept_priority(trimmed)))
            }
        })
        .collect();

    if entries.is_empty() {
        return Some(DEFAULT_UPSTREAM_ACCEPT.to_string());
    }

    entries.sort_by(|a, b| a.2.cmp(&b.2).then(a.0.cmp(&b.0)));
    let reordered = entries
        .into_iter()
        .map(|(_, token, _)| token)
        .collect::<Vec<_>>()
        .join(", ");
    Some(reordered)
}

fn accept_priority(token: &str) -> u8 {
    let media_type = token
        .split(';')
        .next()
        .map(|value| value.trim().to_ascii_lowercase())
        .unwrap_or_default();
    match media_type.as_str() {
        "application/vnd.docker.distribution.manifest.list.v2+json" => 0,
        "application/vnd.docker.distribution.manifest.v2+json" => 1,
        "application/vnd.oci.image.index.v1+json" => 2,
        "application/vnd.oci.image.manifest.v1+json" => 3,
        "application/vnd.docker.distribution.manifest.v1+json" => 4,
        "*/*" => 5,
        _ => 10,
    }
}

struct StreamedDownload {
    path: tempfile::TempPath,
    size: u64,
    digest: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ManifestMeta {
    digest: String,
    last_checked: DateTime<Utc>,
}

async fn ensure_manifest_cached(
    storage: &DynStorage,
    repository_id: Uuid,
    manifest_path: &StoragePath,
    repository_name: &str,
    reference: &str,
    streamed: &StreamedDownload,
    computed_digest: &str,
) -> Result<(), DockerError> {
    let digest_path = if reference.starts_with("sha256:") {
        None
    } else {
        Some(StoragePath::from(format!(
            "v2/{}/manifests/{}",
            repository_name, computed_digest
        )))
    };

    save_manifest_atomically(
        storage,
        repository_id,
        manifest_path,
        digest_path.as_ref(),
        streamed,
        reference,
        computed_digest,
    )
    .await
}

async fn save_manifest_atomically(
    storage: &DynStorage,
    repository_id: Uuid,
    tag_path: &StoragePath,
    digest_path: Option<&StoragePath>,
    streamed: &StreamedDownload,
    reference: &str,
    computed_digest: &str,
) -> Result<(), DockerError> {
    write_manifest_file(
        storage,
        repository_id,
        tag_path,
        reference,
        computed_digest,
        streamed,
    )
    .await?;

    if let Some(digest_path) = digest_path {
        if let Err(err) = write_manifest_file(
            storage,
            repository_id,
            digest_path,
            computed_digest,
            computed_digest,
            streamed,
        )
        .await
        {
            let _ = storage.delete_file(repository_id, tag_path).await;
            return Err(err);
        }
    }

    Ok(())
}

async fn verify_cached_digest(
    storage: &DynStorage,
    repository_id: Uuid,
    path: &StoragePath,
    reference: &str,
    expected_digest: &str,
) -> Result<(), DockerError> {
    let cached = load_cached_manifest(storage, repository_id, path, reference).await?;
    if let Some(cached) = cached {
        if cached.digest == expected_digest {
            return Ok(());
        }
    }
    Err(DockerError::DigestMismatch {
        expected: expected_digest.to_string(),
        actual: cached_digest_string(storage, repository_id, path, reference).await,
    })
}

async fn write_manifest_file(
    storage: &DynStorage,
    repository_id: Uuid,
    path: &StoragePath,
    reference: &str,
    expected_digest: &str,
    streamed: &StreamedDownload,
) -> Result<(), DockerError> {
    if storage.file_exists(repository_id, path).await? {
        match load_cached_manifest(storage, repository_id, path, reference).await? {
            Some(existing) if existing.digest == expected_digest => return Ok(()),
            _ => {
                let _ = storage.delete_file(repository_id, path).await;
            }
        }
    }

    match storage
        .save_file(
            repository_id,
            FileContent::Path(streamed.path.to_path_buf()),
            path,
        )
        .await
    {
        Ok(_) => Ok(()),
        Err(nr_storage::StorageError::PathCollision(_)) => {
            verify_cached_digest(storage, repository_id, path, reference, expected_digest).await
        }
        Err(err) => Err(err.into()),
    }
}

async fn cached_digest_string(
    storage: &DynStorage,
    repository_id: Uuid,
    path: &StoragePath,
    reference: &str,
) -> String {
    match load_cached_manifest(storage, repository_id, path, reference).await {
        Ok(Some(cached)) => cached.digest,
        _ => "unknown".to_string(),
    }
}

const LOCK_CACHE_CAPACITY: usize = 100_000;

fn fetch_lock_map() -> &'static tokio::sync::Mutex<LruCache<String, Arc<tokio::sync::Mutex<()>>>> {
    static LOCKS: OnceLock<tokio::sync::Mutex<LruCache<String, Arc<tokio::sync::Mutex<()>>>>> =
        OnceLock::new();
    LOCKS.get_or_init(|| {
        tokio::sync::Mutex::new(LruCache::new(
            NonZeroUsize::new(LOCK_CACHE_CAPACITY).unwrap(),
        ))
    })
}

#[cfg(test)]
fn fetch_lock_len() -> usize {
    futures::executor::block_on(async { fetch_lock_map().lock().await.len() })
}

async fn with_fetch_lock<F, T>(key: &str, f: F) -> T
where
    F: std::future::Future<Output = T>,
{
    let lock = {
        let mut map = fetch_lock_map().lock().await;
        if let Some(existing) = map.get(key) {
            existing.clone()
        } else {
            let arc = Arc::new(tokio::sync::Mutex::new(()));
            map.put(key.to_string(), arc.clone());
            arc
        }
    };
    let _guard = lock.lock().await;
    f.await
}

async fn stream_response_to_tempfile(response: Response) -> Result<StreamedDownload, DockerError> {
    let named = Builder::new().prefix("docker-proxy-").tempfile()?;
    let (std_file, path) = named.into_parts();
    let mut file = tokio::fs::File::from_std(std_file);
    let mut hasher = sha2::Sha256::new();
    let mut total = 0u64;

    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        total += chunk.len() as u64;
        hasher.update(&chunk);
        file.write_all(&chunk).await?;
    }
    file.flush().await?;
    file.sync_all().await?;

    let digest = format!("sha256:{:x}", hasher.finalize());
    Ok(StreamedDownload {
        path,
        size: total,
        digest,
    })
}

struct TempFileReader {
    file: tokio::fs::File,
    _path: tempfile::TempPath,
}

impl TempFileReader {
    fn new(file: tokio::fs::File, path: tempfile::TempPath) -> Self {
        Self { file, _path: path }
    }
}

impl AsyncRead for TempFileReader {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.file).poll_read(cx, buf)
    }
}

fn upstream_image_name(repository_name: &str, upstream: &ProxyUpstream) -> String {
    let mut segments: Vec<&str> = repository_name.split('/').collect();

    // Strip storage/repository prefix (first two segments) if present
    if segments.len() >= 3 {
        segments.drain(0..2);
    }

    // If nothing left, fall back to original
    if segments.is_empty() {
        return repository_name.to_string();
    }

    // Docker Hub requires implicit "library/" for top-level images (e.g., "nginx")
    let host = upstream
        .base
        .host_str()
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    let is_docker_hub = host.contains("docker.io");
    if is_docker_hub && segments.len() == 1 {
        return format!("library/{}", segments[0]);
    }

    segments.join("/")
}

#[derive(Debug)]
pub struct DockerProxyInner {
    pub id: Uuid,
    pub name: String,
    pub active: AtomicBool,
    pub visibility: RwLock<Visibility>,
    pub storage: DynStorage,
    pub site: NitroRepo,
    pub upstream: ProxyUpstream,
}

#[derive(Debug, Clone)]
pub struct DockerProxy(Arc<DockerProxyInner>);

impl DockerProxy {
    async fn serve_manifest(
        &self,
        repository_name: &str,
        reference: &str,
        accept: Option<&str>,
        head_only: bool,
    ) -> Result<RepoResponse, DockerError> {
        let manifest = fetch_and_cache_manifest(
            self.upstream(),
            &self.0.storage,
            self.id(),
            repository_name,
            reference,
            accept,
        )
        .await?;

        Ok(if head_only {
            manifest_head_response(&manifest)
        } else {
            manifest_get_response(manifest)
        })
    }

    async fn serve_blob(
        &self,
        repository_name: &str,
        digest: &str,
        head_only: bool,
    ) -> Result<RepoResponse, DockerError> {
        if head_only {
            let blob_path = StoragePath::from(format!("v2/{repository_name}/blobs/{digest}"));

            if let Some(meta) = self
                .0
                .storage
                .get_file_information(self.id(), &blob_path)
                .await?
            {
                if let FileType::File(file_meta) = meta.file_type() {
                    let digest_value = file_meta
                        .file_hash
                        .sha2_256
                        .as_deref()
                        .and_then(digest_from_hash)
                        .unwrap_or_else(|| digest.to_string());
                    return Ok(blob_head_response(&digest_value, file_meta.file_size));
                }
            }
        }

        let blob = fetch_and_cache_blob(
            self.upstream(),
            &self.0.storage,
            self.id(),
            repository_name,
            digest,
        )
        .await?;

        Ok(if head_only {
            blob_head_response(&blob.digest, blob.length)
        } else {
            blob_get_response(blob)
        })
    }

    async fn proxy_catalog(
        &self,
        query: Option<&str>,
        accept: Option<&str>,
    ) -> Result<RepoResponse, DockerError> {
        let path = match query {
            Some(q) => format!("/v2/_catalog?{q}"),
            None => "/v2/_catalog".to_string(),
        };
        proxy_passthrough(self.upstream(), &path, accept).await
    }

    async fn proxy_tags(
        &self,
        repository_name: &str,
        query: Option<&str>,
        accept: Option<&str>,
    ) -> Result<RepoResponse, DockerError> {
        let upstream_repo = upstream_image_name(repository_name, self.upstream());
        let upstream_path = match query {
            Some(q) => format!("/v2/{upstream_repo}/tags/list?{q}"),
            None => format!("/v2/{upstream_repo}/tags/list"),
        };
        proxy_passthrough(self.upstream(), &upstream_path, accept).await
    }
    pub async fn load(
        repository: nr_core::database::entities::repository::DBRepository,
        storage: DynStorage,
        site: NitroRepo,
        config: DockerProxyConfig,
    ) -> Result<Self, RepositoryFactoryError> {
        let upstream = ProxyUpstream::new(&config)
            .map_err(|err| RepositoryFactoryError::InvalidConfig("docker", err.to_string()))?;

        Ok(Self(Arc::new(DockerProxyInner {
            id: repository.id,
            name: repository.name.into(),
            active: AtomicBool::new(repository.active),
            visibility: RwLock::new(repository.visibility),
            storage,
            site,
            upstream,
        })))
    }

    fn upstream(&self) -> &ProxyUpstream {
        &self.0.upstream
    }
}

#[derive(Debug)]
pub(crate) struct CachedManifest {
    pub reader: StorageFileReader,
    pub digest: String,
    pub content_type: String,
    pub length: u64,
}

#[derive(Debug)]
pub(crate) struct CachedBlob {
    pub reader: StorageFileReader,
    pub digest: String,
    pub length: u64,
}

async fn load_cached_manifest(
    storage: &DynStorage,
    repository_id: Uuid,
    manifest_path: &StoragePath,
    reference: &str,
) -> Result<Option<CachedManifest>, DockerError> {
    let Some(file) = storage.open_file(repository_id, manifest_path).await? else {
        return Ok(None);
    };

    let (reader, meta) = file
        .file()
        .ok_or_else(|| DockerError::InvalidManifest("Expected file, got directory".into()))?;
    let length = meta.file_type.file_size;
    let digest_from_meta = meta
        .file_type
        .file_hash
        .sha2_256
        .as_deref()
        .and_then(digest_from_hash);
    let content_type_from_meta = meta
        .file_type
        .mime_type
        .as_ref()
        .map(|v| v.to_string())
        .and_then(|value| {
            // application/octet-stream is a placeholder for unknown types; treat it as missing
            if value == "application/octet-stream" {
                None
            } else {
                Some(value)
            }
        });

    if let (Some(digest), Some(content_type)) =
        (digest_from_meta.clone(), content_type_from_meta.clone())
    {
        if reference.starts_with("sha256:") && reference != digest {
            return Err(DockerError::DigestMismatch {
                expected: reference.to_string(),
                actual: digest,
            });
        }
        return Ok(Some(CachedManifest {
            reader,
            digest,
            content_type,
            length,
        }));
    }

    let length_usize: usize = length
        .try_into()
        .map_err(|_| DockerError::InvalidManifest("manifest size overflow".to_string()))?;
    let bytes = reader.read_to_vec(length_usize).await?;
    let computed_digest = digest_from_meta.unwrap_or_else(|| compute_sha256_hex(&bytes));
    if reference.starts_with("sha256:") && reference != computed_digest {
        return Err(DockerError::DigestMismatch {
            expected: reference.to_string(),
            actual: computed_digest,
        });
    }
    // S3 sets content-type to application/octet-stream; prefer detecting from content
    let content_type = content_type_from_meta
        .clone()
        .unwrap_or_else(|| manifest_media_type(&bytes, None));
    let reopened = storage
        .open_file(repository_id, manifest_path)
        .await?
        .ok_or_else(|| DockerError::ManifestNotFound(reference.to_string()))?;
    let (reader, _) = reopened
        .file()
        .ok_or_else(|| DockerError::InvalidManifest("Expected file, got directory".into()))?;

    Ok(Some(CachedManifest {
        reader,
        digest: computed_digest,
        content_type,
        length,
    }))
}

async fn download_manifest_from_upstream(
    upstream: &ProxyUpstream,
    storage: &DynStorage,
    repository_id: Uuid,
    repository_name: &str,
    reference: &str,
    accept: Option<&str>,
    manifest_path: &StoragePath,
) -> Result<CachedManifest, DockerError> {
    let upstream_repo = upstream_image_name(repository_name, upstream);
    let path = format!("/v2/{}/manifests/{}", upstream_repo, reference);
    let upstream_accept = reorder_accept_header(accept);
    let response = upstream.fetch(&path, upstream_accept.as_deref()).await?;
    let status = response.status();
    if status == StatusCode::NOT_FOUND {
        return Err(DockerError::ManifestNotFound(reference.to_string()));
    }
    if !status.is_success() {
        return Err(DockerError::InvalidManifest(format!(
            "Upstream returned status {}",
            status
        )));
    }
    let headers = response.headers().clone();
    let streamed = stream_response_to_tempfile(response).await?;
    let manifest_bytes = tokio::fs::read(streamed.path.to_path_buf()).await?;
    let mut content_type = manifest_media_type(&manifest_bytes, Some(&headers));
    let mut streamed_download = streamed;

    // Reject schema1 in favor of modern manifests; re-fetch with modern-only Accept.
    if is_schema1_manifest(&content_type) {
        let modern_accept = Some(MODERN_UPSTREAM_ACCEPT);
        let modern_response = upstream.fetch(&path, modern_accept).await?;
        let modern_status = modern_response.status();
        if modern_status.is_success() {
            let modern_headers = modern_response.headers().clone();
            let modern_streamed = stream_response_to_tempfile(modern_response).await?;
            let modern_bytes = tokio::fs::read(modern_streamed.path.to_path_buf()).await?;
            let modern_content_type = manifest_media_type(&modern_bytes, Some(&modern_headers));
            let modern_digest = modern_streamed.digest.clone();

            if let Some(expected) = digest_from_header(&modern_headers) {
                if expected != modern_digest {
                    return Err(DockerError::DigestMismatch {
                        expected,
                        actual: modern_digest,
                    });
                }
            }

            content_type = modern_content_type;
            streamed_download = modern_streamed;
        } else {
            warn!(
                %reference,
                repository = repository_name,
                status = %modern_status,
                "Upstream returned schema1; modern re-fetch failed"
            );
        }
    }
    let computed_digest = streamed_download.digest.clone();

    if let Some(expected) = digest_from_header(&headers) {
        if expected != computed_digest {
            return Err(DockerError::DigestMismatch {
                expected,
                actual: computed_digest,
            });
        }
    }

    if reference.starts_with("sha256:") && reference != computed_digest {
        return Err(DockerError::DigestMismatch {
            expected: reference.to_string(),
            actual: computed_digest,
        });
    }

    if upstream.cache_enabled {
        ensure_manifest_cached(
            storage,
            repository_id,
            manifest_path,
            repository_name,
            reference,
            &streamed_download,
            &computed_digest,
        )
        .await?;

        if !reference.starts_with("sha256:") {
            let digest_path = StoragePath::from(format!(
                "v2/{}/manifests/{}",
                repository_name, &computed_digest
            ));
            ensure_manifest_cached(
                storage,
                repository_id,
                &digest_path,
                repository_name,
                &computed_digest,
                &streamed_download,
                &computed_digest,
            )
            .await?;
        }
    }

    if !reference.starts_with("sha256:") {
        save_manifest_meta(storage, repository_id, manifest_path, &computed_digest).await?;
    }

    Ok(CachedManifest {
        reader: StorageFileReader::AsyncReader(Box::pin(TempFileReader::new(
            tokio::fs::File::open(streamed_download.path.to_path_buf()).await?,
            streamed_download.path,
        ))),
        digest: computed_digest,
        content_type,
        length: streamed_download.size,
    })
}

enum RevalidationOutcome {
    Unchanged(CachedManifest),
    Refetched(CachedManifest),
}

async fn revalidate_manifest_tag(
    upstream: &ProxyUpstream,
    storage: &DynStorage,
    repository_id: Uuid,
    repository_name: &str,
    reference: &str,
    manifest_path: &StoragePath,
    cached: CachedManifest,
) -> Result<RevalidationOutcome, DockerError> {
    if upstream.skip_tag_revalidation || reference.starts_with("sha256:") {
        return Ok(RevalidationOutcome::Unchanged(cached));
    }

    if let Some(meta) = load_manifest_meta(storage, repository_id, manifest_path).await? {
        let age = Utc::now().signed_duration_since(meta.last_checked);
        if age.num_seconds() >= 0 && (age.num_seconds() as u64) < upstream.revalidation_ttl {
            if meta.digest == cached.digest {
                return Ok(RevalidationOutcome::Unchanged(cached));
            }
        }
    }

    let upstream_repo = upstream_image_name(repository_name, upstream);
    let path = format!("/v2/{}/manifests/{}", upstream_repo, reference);
    let head = upstream.fetch(&path, Some(MODERN_UPSTREAM_ACCEPT)).await?;
    if head.status() == StatusCode::NOT_FOUND {
        // Upstream tag disappeared; drop cache so next request triggers 404.
        let _ = storage.delete_file(repository_id, manifest_path).await;
        return Err(DockerError::ManifestNotFound(reference.to_string()));
    }
    if !head.status().is_success() {
        return Ok(RevalidationOutcome::Unchanged(cached));
    }

    let maybe_digest = digest_from_header(head.headers());
    if let Some(digest) = maybe_digest {
        if digest == cached.digest {
            save_manifest_meta(storage, repository_id, manifest_path, &digest).await?;
            return Ok(RevalidationOutcome::Unchanged(cached));
        }
    }

    // Upstream moved; fetch the new manifest and cache it.
    let refreshed = download_manifest_from_upstream(
        upstream,
        storage,
        repository_id,
        repository_name,
        reference,
        Some(MODERN_UPSTREAM_ACCEPT),
        manifest_path,
    )
    .await?;

    save_manifest_meta(storage, repository_id, manifest_path, &refreshed.digest).await?;
    Ok(RevalidationOutcome::Refetched(refreshed))
}

pub(crate) async fn fetch_and_cache_manifest(
    upstream: &ProxyUpstream,
    storage: &DynStorage,
    repository_id: Uuid,
    repository_name: &str,
    reference: &str,
    accept: Option<&str>,
) -> Result<CachedManifest, DockerError> {
    let manifest_path =
        StoragePath::from(format!("v2/{}/manifests/{}", repository_name, reference));
    let lock_key = format!(
        "manifest:{}:{}:{}",
        repository_id, repository_name, reference
    );

    with_fetch_lock(&lock_key, async {
        if let Some(cached) =
            load_cached_manifest(storage, repository_id, &manifest_path, reference).await?
        {
            let prefer_modern = client_prefers_modern_manifest(accept);
            let cached_is_schema1 = is_schema1_manifest(&cached.content_type);

            if cached_is_schema1 {
                // Purge schema1 cache to force a modern re-fetch. For digest requests, fail fast so
                // the client retries the tag and learns the modern digest.
                let _ = storage.delete_file(repository_id, &manifest_path).await;
                if !reference.starts_with("sha256:") {
                    let digest_path = StoragePath::from(format!(
                        "v2/{}/manifests/{}",
                        repository_name, cached.digest
                    ));
                    let _ = storage.delete_file(repository_id, &digest_path).await;
                } else {
                    return Err(DockerError::ManifestNotFound(reference.to_string()));
                }
            } else {
                match revalidate_manifest_tag(
                    upstream,
                    storage,
                    repository_id,
                    repository_name,
                    reference,
                    &manifest_path,
                    cached,
                )
                .await?
                {
                    RevalidationOutcome::Unchanged(cached) => {
                        if accept_allows_media_type(accept, &cached.content_type) {
                            return Ok(cached);
                        }
                    }
                    RevalidationOutcome::Refetched(new_manifest) => return Ok(new_manifest),
                }
            }

            // Re-download with the client's Accept header (or modern-only) to honor content negotiation.
            let override_accept = if prefer_modern {
                Some(MODERN_UPSTREAM_ACCEPT)
            } else {
                accept
            };
            match download_manifest_from_upstream(
                upstream,
                storage,
                repository_id,
                repository_name,
                reference,
                override_accept,
                &manifest_path,
            )
            .await
            {
                Ok(manifest) => return Ok(manifest),
                Err(err) => {
                    warn!(
                        %reference,
                        repository = repository_name,
                        %err,
                        "Failed to refresh manifest; cache miss path will retry"
                    );
                }
            }
        }

        download_manifest_from_upstream(
            upstream,
            storage,
            repository_id,
            repository_name,
            reference,
            Some(MODERN_UPSTREAM_ACCEPT),
            &manifest_path,
        )
        .await
    })
    .await
}

fn is_schema1_manifest(media_type: &str) -> bool {
    media_type.to_ascii_lowercase().contains("manifest.v1+json")
}

fn client_prefers_modern_manifest(accept: Option<&str>) -> bool {
    let Some(accept) = accept else {
        return true;
    };
    accept.split(',').any(|token| {
        let kind = token
            .split(';')
            .next()
            .map(|v| v.trim().to_ascii_lowercase())
            .unwrap_or_default();
        matches!(
            kind.as_str(),
            "application/vnd.docker.distribution.manifest.list.v2+json"
                | "application/vnd.docker.distribution.manifest.v2+json"
                | "application/vnd.oci.image.index.v1+json"
                | "application/vnd.oci.image.manifest.v1+json"
        )
    })
}

#[derive(Debug, PartialEq, Eq)]
enum MediaTypeFamily {
    ManifestV2,
    ManifestListV2,
    Other(String),
}

impl MediaTypeFamily {
    fn from_str(value: &str) -> Self {
        match value {
            "application/vnd.docker.distribution.manifest.v2+json"
            | "application/vnd.oci.image.manifest.v1+json" => MediaTypeFamily::ManifestV2,
            "application/vnd.docker.distribution.manifest.list.v2+json"
            | "application/vnd.oci.image.index.v1+json" => MediaTypeFamily::ManifestListV2,
            _ => MediaTypeFamily::Other(value.to_string()),
        }
    }

    fn matches(&self, other: &Self) -> bool {
        match (self, other) {
            (MediaTypeFamily::ManifestV2, MediaTypeFamily::ManifestV2) => true,
            (MediaTypeFamily::ManifestListV2, MediaTypeFamily::ManifestListV2) => true,
            (MediaTypeFamily::Other(a), MediaTypeFamily::Other(b)) => a == b,
            _ => false,
        }
    }
}

fn accept_allows_media_type(accept: Option<&str>, media_type: &str) -> bool {
    let Some(accept) = accept else {
        return true;
    };
    // Simple, conservative parser: split on commas, strip parameters, handle wildcards and exact matches.
    accept
        .split(',')
        .map(|part| part.trim())
        .any(|part| match part.split_once(';') {
            Some((kind, _)) => accept_token_matches(kind.trim(), media_type),
            None => accept_token_matches(part, media_type),
        })
}

fn accept_token_matches(token: &str, media_type: &str) -> bool {
    let token = token.trim();
    if token == "*/*" {
        return true;
    }
    let media_type = media_type.trim();
    let token_lower = token.to_ascii_lowercase();
    let media_lower = media_type.to_ascii_lowercase();
    if token_lower == "*/*" {
        return true;
    }
    if let Some((token_type, token_sub)) = token_lower.split_once('/') {
        if token_sub == "*" {
            if let Some((media_type_type, _)) = media_lower.split_once('/') {
                return token_type == media_type_type;
            }
            return false;
        }
    }
    let token_family = MediaTypeFamily::from_str(&token_lower);
    let media_family = MediaTypeFamily::from_str(&media_lower);
    token_family.matches(&media_family)
}

async fn load_cached_blob(
    storage: &DynStorage,
    repository_id: Uuid,
    blob_path: &StoragePath,
    digest: &str,
) -> Result<Option<CachedBlob>, DockerError> {
    let Some(existing) = storage.open_file(repository_id, blob_path).await? else {
        return Ok(None);
    };
    let (reader, meta) = existing
        .file()
        .ok_or_else(|| DockerError::BlobNotFound(digest.to_string()))?;
    let length = meta.file_type.file_size;
    let digest_from_meta = meta
        .file_type
        .file_hash
        .sha2_256
        .as_deref()
        .and_then(digest_from_hash);

    if let Some(found) = digest_from_meta.clone() {
        if digest.starts_with("sha256:") && digest != found {
            return Err(DockerError::DigestMismatch {
                expected: digest.to_string(),
                actual: found,
            });
        }
        return Ok(Some(CachedBlob {
            reader,
            digest: found,
            length,
        }));
    }

    let length_usize: usize = length
        .try_into()
        .map_err(|_| DockerError::InvalidManifest("blob size overflow".to_string()))?;
    let bytes = reader.read_to_vec(length_usize).await?;
    let computed_digest = compute_sha256_hex(&bytes);
    if digest.starts_with("sha256:") && digest != computed_digest {
        return Err(DockerError::DigestMismatch {
            expected: digest.to_string(),
            actual: computed_digest,
        });
    }
    let reopened = storage
        .open_file(repository_id, blob_path)
        .await?
        .ok_or_else(|| DockerError::BlobNotFound(digest.to_string()))?;
    let (reader, meta) = reopened
        .file()
        .ok_or_else(|| DockerError::BlobNotFound(digest.to_string()))?;

    Ok(Some(CachedBlob {
        reader,
        digest: computed_digest,
        length: meta.file_type.file_size,
    }))
}

async fn download_blob_from_upstream(
    upstream: &ProxyUpstream,
    storage: &DynStorage,
    repository_id: Uuid,
    repository_name: &str,
    digest: &str,
    blob_path: &StoragePath,
) -> Result<CachedBlob, DockerError> {
    let upstream_repo = upstream_image_name(repository_name, upstream);
    let path = format!("/v2/{}/blobs/{}", upstream_repo, digest);
    let response = upstream.fetch(&path, None).await?;
    let status = response.status();
    if status == StatusCode::NOT_FOUND {
        return Err(DockerError::BlobNotFound(digest.to_string()));
    }
    if !status.is_success() {
        return Err(DockerError::InvalidManifest(format!(
            "Upstream returned status {}",
            status
        )));
    }
    let headers = response.headers().clone();
    let streamed = stream_response_to_tempfile(response).await?;
    let computed_digest = streamed.digest.clone();

    if let Some(expected) = digest_from_header(&headers) {
        if expected != computed_digest {
            return Err(DockerError::DigestMismatch {
                expected,
                actual: computed_digest,
            });
        }
    }
    if digest.starts_with("sha256:") && digest != computed_digest {
        return Err(DockerError::DigestMismatch {
            expected: digest.to_string(),
            actual: computed_digest,
        });
    }

    if upstream.cache_enabled {
        if let Err(err) = storage
            .save_file(
                repository_id,
                FileContent::Path(streamed.path.to_path_buf()),
                blob_path,
            )
            .await
        {
            if matches!(err, nr_storage::StorageError::PathCollision(_)) {
                warn!(
                    ?blob_path,
                    "Blob cache write hit path collision; another request likely wrote it first"
                );
            } else {
                return Err(err.into());
            }
        }
    }

    Ok(CachedBlob {
        reader: StorageFileReader::AsyncReader(Box::pin(TempFileReader::new(
            tokio::fs::File::open(streamed.path.to_path_buf()).await?,
            streamed.path,
        ))),
        digest: computed_digest,
        length: streamed.size,
    })
}

pub(crate) async fn fetch_and_cache_blob(
    upstream: &ProxyUpstream,
    storage: &DynStorage,
    repository_id: Uuid,
    repository_name: &str,
    digest: &str,
) -> Result<CachedBlob, DockerError> {
    let blob_path = StoragePath::from(format!("v2/{}/blobs/{}", repository_name, digest));
    let lock_key = format!("blob:{}:{}:{}", repository_id, repository_name, digest);

    with_fetch_lock(&lock_key, async {
        if let Some(cached) = load_cached_blob(storage, repository_id, &blob_path, digest).await? {
            return Ok(cached);
        }

        download_blob_from_upstream(
            upstream,
            storage,
            repository_id,
            repository_name,
            digest,
            &blob_path,
        )
        .await
    })
    .await
}

fn manifest_get_response(manifest: CachedManifest) -> RepoResponse {
    let builder = ResponseBuilder::ok()
        .header("Docker-Distribution-API-Version", "registry/2.0")
        .header("Docker-Content-Digest", manifest.digest.clone())
        .header(CONTENT_TYPE, manifest.content_type)
        .header(CONTENT_LENGTH, manifest.length.to_string());
    let length_usize: usize = manifest.length.try_into().unwrap_or(usize::MAX); // length already validated earlier; usize::MAX only on overflow
    let body = Body::new(manifest.reader.into_body(length_usize));

    RepoResponse::Other(builder.body(body))
}

fn manifest_head_response(manifest: &CachedManifest) -> RepoResponse {
    RepoResponse::Other(
        ResponseBuilder::ok()
            .header("Docker-Distribution-API-Version", "registry/2.0")
            .header("Docker-Content-Digest", manifest.digest.clone())
            .header(CONTENT_TYPE, manifest.content_type.clone())
            .header(CONTENT_LENGTH, manifest.length.to_string())
            .body(Body::empty()),
    )
}

fn blob_get_response(blob: CachedBlob) -> RepoResponse {
    let builder = ResponseBuilder::ok()
        .header("Docker-Distribution-API-Version", "registry/2.0")
        .header("Docker-Content-Digest", blob.digest.clone())
        .header(CONTENT_TYPE, "application/octet-stream")
        .header(CONTENT_LENGTH, blob.length.to_string());
    let length_usize: usize = blob.length.try_into().unwrap_or(usize::MAX);
    let body = Body::new(blob.reader.into_body(length_usize));

    RepoResponse::Other(builder.body(body))
}

fn blob_head_response(digest: &str, length: u64) -> RepoResponse {
    RepoResponse::Other(
        ResponseBuilder::ok()
            .header("Docker-Distribution-API-Version", "registry/2.0")
            .header("Docker-Content-Digest", digest)
            .header(CONTENT_TYPE, "application/octet-stream")
            .header(CONTENT_LENGTH, length.to_string())
            .body(Body::empty()),
    )
}

fn read_only_response(method: &str) -> RepoResponse {
    RepoResponse::basic_text_response(
        StatusCode::METHOD_NOT_ALLOWED,
        format!("{method} not allowed for docker proxy repositories"),
    )
}

fn docker_v2_ok() -> RepoResponse {
    RepoResponse::Other(
        ResponseBuilder::ok()
            .header("Docker-Distribution-API-Version", "registry/2.0")
            .header(CONTENT_TYPE, "application/json")
            .body("{}"),
    )
}

async fn proxy_passthrough(
    upstream: &ProxyUpstream,
    path: &str,
    accept: Option<&str>,
) -> Result<RepoResponse, DockerError> {
    let response = upstream.fetch(path, accept).await?;
    let status = response.status();
    let headers = response.headers().clone();
    let body = response.bytes().await?;
    let mut builder = ResponseBuilder::default().status(status);
    builder = builder.header("Docker-Distribution-API-Version", "registry/2.0");
    if let Some(content_type) = headers.get(CONTENT_TYPE) {
        builder = builder.header(CONTENT_TYPE, content_type.clone());
    }
    if let Some(content_length) = headers.get(CONTENT_LENGTH) {
        builder = builder.header(CONTENT_LENGTH, content_length.clone());
    } else {
        builder = builder.header(CONTENT_LENGTH, body.len().to_string());
    }

    Ok(RepoResponse::Other(builder.body(Body::from(body))))
}

impl Repository for DockerProxy {
    type Error = DockerError;

    fn get_storage(&self) -> DynStorage {
        self.0.storage.clone()
    }

    fn get_type(&self) -> &'static str {
        super::REPOSITORY_TYPE_ID
    }

    fn full_type(&self) -> &'static str {
        "docker/proxy"
    }

    fn config_types(&self) -> Vec<&str> {
        vec![
            DockerRegistryConfigType::get_type_static(),
            RepositoryAuthConfigType::get_type_static(),
        ]
    }

    fn name(&self) -> String {
        self.0.name.clone()
    }

    fn id(&self) -> Uuid {
        self.0.id
    }

    fn visibility(&self) -> Visibility {
        *self.0.visibility.read()
    }

    fn is_active(&self) -> bool {
        self.0.active.load(Ordering::Relaxed)
    }

    fn site(&self) -> NitroRepo {
        self.0.site.clone()
    }

    async fn handle_get(&self, request: RepositoryRequest) -> Result<RepoResponse, Self::Error> {
        if !can_read_repository_with_auth(
            &request.authentication,
            self.visibility(),
            self.id(),
            self.site().as_ref(),
            &request.auth_config,
        )
        .await?
        {
            return Ok(RepoResponse::forbidden());
        }

        let path_str = request.path.to_string();
        let parts: Vec<&str> = path_str.trim_start_matches('/').split('/').collect();
        let accept = request
            .parts
            .headers
            .get(ACCEPT)
            .and_then(|v| v.to_str().ok());

        match parts.as_slice() {
            ["v2"] => Ok(docker_v2_ok()),
            ["v2", "_catalog"] => self.proxy_catalog(request.parts.uri.query(), accept).await,
            ["v2", name @ .., "tags", "list"] if !name.is_empty() => {
                let repo_name = name.join("/");
                self.proxy_tags(&repo_name, request.parts.uri.query(), accept)
                    .await
            }
            ["v2", name @ .., "manifests", reference] if !name.is_empty() => {
                let repository_name = name.join("/");
                self.serve_manifest(&repository_name, reference, accept, false)
                    .await
            }
            ["v2", name @ .., "blobs", digest] if !name.is_empty() => {
                let repository_name = name.join("/");
                self.serve_blob(&repository_name, digest, false).await
            }
            _ => Ok(RepoResponse::basic_text_response(
                StatusCode::NOT_FOUND,
                "Not Found",
            )),
        }
    }

    async fn handle_head(&self, request: RepositoryRequest) -> Result<RepoResponse, Self::Error> {
        if !can_read_repository_with_auth(
            &request.authentication,
            self.visibility(),
            self.id(),
            self.site().as_ref(),
            &request.auth_config,
        )
        .await?
        {
            return Ok(RepoResponse::forbidden());
        }

        let path_str = request.path.to_string();
        let parts: Vec<&str> = path_str.trim_start_matches('/').split('/').collect();
        let accept = request
            .parts
            .headers
            .get(ACCEPT)
            .and_then(|v| v.to_str().ok());

        match parts.as_slice() {
            ["v2"] => Ok(docker_v2_ok()),
            ["v2", name @ .., "manifests", reference] if !name.is_empty() => {
                let repository_name = name.join("/");
                self.serve_manifest(&repository_name, reference, accept, true)
                    .await
            }
            ["v2", name @ .., "blobs", digest] if !name.is_empty() => {
                let repository_name = name.join("/");
                self.serve_blob(&repository_name, digest, true).await
            }
            _ => Ok(RepoResponse::basic_text_response(
                StatusCode::NOT_FOUND,
                "Not Found",
            )),
        }
    }

    async fn handle_put(&self, _request: RepositoryRequest) -> Result<RepoResponse, Self::Error> {
        Ok(read_only_response("PUT"))
    }

    async fn handle_post(&self, _request: RepositoryRequest) -> Result<RepoResponse, Self::Error> {
        Ok(read_only_response("POST"))
    }

    async fn handle_patch(&self, _request: RepositoryRequest) -> Result<RepoResponse, Self::Error> {
        Ok(read_only_response("PATCH"))
    }

    async fn handle_delete(
        &self,
        _request: RepositoryRequest,
    ) -> Result<RepoResponse, Self::Error> {
        Ok(read_only_response("DELETE"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repository::test_helpers::test_storage;
    use axum::{Router, body::Body as AxumBody, routing::get};
    use bytes::Bytes;
    use futures::stream;
    use http::{HeaderValue, StatusCode};
    use nr_core::storage::StoragePath;
    use sha2::Digest;
    use std::convert::Infallible;
    use tokio::net::TcpListener;
    use tokio::task::JoinHandle;

    async fn reader_bytes(
        reader: nr_storage::StorageFileReader,
        len: u64,
    ) -> anyhow::Result<Vec<u8>> {
        let len: usize = len.try_into().expect("length fits in usize for tests");
        Ok(reader.read_to_vec(len).await?)
    }

    async fn start_upstream_server(
        manifest_body: &'static [u8],
        blob_body: &'static [u8],
    ) -> anyhow::Result<(String, JoinHandle<()>)> {
        let manifest_digest = format!("sha256:{:x}", sha2::Sha256::digest(manifest_body));
        let blob_digest = format!("sha256:{:x}", sha2::Sha256::digest(blob_body));
        let manifest_digest_header =
            HeaderValue::from_str(&manifest_digest).expect("valid digest header");
        let manifest_content_type =
            HeaderValue::from_static("application/vnd.docker.distribution.manifest.v2+json");

        let app = Router::new()
            .route(
                "/v2/library/alpine/manifests/latest",
                get({
                    let digest_header = manifest_digest_header.clone();
                    let content_type = manifest_content_type.clone();
                    move || {
                        let digest_header = digest_header.clone();
                        let content_type = content_type.clone();
                        async move {
                            (
                                StatusCode::OK,
                                [
                                    ("Docker-Content-Digest", digest_header),
                                    ("Content-Type", content_type),
                                ],
                                manifest_body,
                            )
                        }
                    }
                }),
            )
            .route(
                &format!("/v2/library/alpine/blobs/{blob_digest}"),
                get(move || async move {
                    (
                        StatusCode::OK,
                        [("Content-Type", "application/octet-stream")],
                        blob_body,
                    )
                }),
            );

        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let server = tokio::spawn(async move {
            if let Err(err) = axum::serve(listener, app).await {
                eprintln!("upstream server error: {err}");
            }
        });
        Ok((format!("http://{}", addr), server))
    }

    async fn start_negotiating_upstream_server(
        v1_manifest: &'static [u8],
        v2_manifest: &'static [u8],
    ) -> anyhow::Result<(String, JoinHandle<()>)> {
        let v1_digest = format!("sha256:{:x}", sha2::Sha256::digest(v1_manifest));
        let v2_digest = format!("sha256:{:x}", sha2::Sha256::digest(v2_manifest));
        let v1_ct =
            HeaderValue::from_static("application/vnd.docker.distribution.manifest.v1+json");
        let v2_ct =
            HeaderValue::from_static("application/vnd.docker.distribution.manifest.v2+json");

        let app = Router::new().route(
            "/v2/library/alpine/manifests/latest",
            get(move |headers: HeaderMap| {
                let v1_ct = v1_ct.clone();
                let v2_ct = v2_ct.clone();
                let v1_manifest = v1_manifest;
                let v2_manifest = v2_manifest;
                let v1_digest = v1_digest.clone();
                let v2_digest = v2_digest.clone();
                async move {
                    let accept = headers
                        .get(http::header::ACCEPT)
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or("");
                    if accept.contains("application/vnd.docker.distribution.manifest.v2+json") {
                        (
                            StatusCode::OK,
                            [
                                (
                                    "Docker-Content-Digest",
                                    HeaderValue::from_str(&v2_digest).unwrap(),
                                ),
                                ("Content-Type", v2_ct),
                            ],
                            v2_manifest,
                        )
                    } else {
                        (
                            StatusCode::OK,
                            [
                                (
                                    "Docker-Content-Digest",
                                    HeaderValue::from_str(&v1_digest).unwrap(),
                                ),
                                ("Content-Type", v1_ct),
                            ],
                            v1_manifest,
                        )
                    }
                }
            }),
        );

        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let server = tokio::spawn(async move {
            if let Err(err) = axum::serve(listener, app).await {
                eprintln!("upstream server error: {err}");
            }
        });
        Ok((format!("http://{}", addr), server))
    }

    async fn start_accept_filtered_upstream_server(
        manifest_list: &'static [u8],
    ) -> anyhow::Result<(String, JoinHandle<()>)> {
        let list_digest = format!("sha256:{:x}", sha2::Sha256::digest(manifest_list));
        let content_length = HeaderValue::from_str(&manifest_list.len().to_string())?;
        let digest_header = HeaderValue::from_str(&list_digest)?;
        let app = Router::new().route(
            "/v2/library/alpine/manifests/latest",
            get(move |headers: HeaderMap| {
                let manifest_list = manifest_list;
                let digest_header = digest_header.clone();
                let content_length = content_length.clone();
                async move {
                    let accept = headers
                        .get(ACCEPT)
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or_default();
                    if accept.contains("application/vnd.docker.distribution.manifest.v2+json")
                        && !accept
                            .contains("application/vnd.docker.distribution.manifest.list.v2+json")
                    {
                        axum::response::Response::builder()
                            .status(StatusCode::NOT_FOUND)
                            .header(CONTENT_TYPE, "application/json")
                            .body(AxumBody::from(
                                r#"{"errors":[{"code":"MANIFEST_UNKNOWN","message":"filtered"}]}"#,
                            ))
                            .unwrap()
                    } else {
                        axum::response::Response::builder()
                            .status(StatusCode::OK)
                            .header("Docker-Content-Digest", digest_header.clone())
                            .header(
                                CONTENT_TYPE,
                                "application/vnd.docker.distribution.manifest.list.v2+json",
                            )
                            .header(CONTENT_LENGTH, content_length.clone())
                            .body(AxumBody::from(manifest_list))
                            .unwrap()
                    }
                }
            }),
        );

        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let server = tokio::spawn(async move {
            if let Err(err) = axum::serve(listener, app).await {
                eprintln!("upstream server error: {err}");
            }
        });
        Ok((format!("http://{}", addr), server))
    }

    async fn start_revalidating_upstream_server(
        manifest_bytes: Vec<u8>,
    ) -> anyhow::Result<(String, Arc<RwLock<Vec<u8>>>, JoinHandle<()>)> {
        let state = Arc::new(RwLock::new(manifest_bytes));
        let state_clone = state.clone();
        let state_for_return = state.clone();

        let app = Router::new().route(
            "/v2/library/test/manifests/latest",
            get({
                move || {
                    let state = state_clone.clone();
                    async move {
                        let bytes = state.read().clone();
                        let digest = format!("sha256:{:x}", sha2::Sha256::digest(&bytes));
                        (
                            StatusCode::OK,
                            [
                                (
                                    "Docker-Content-Digest",
                                    HeaderValue::from_str(&digest).unwrap(),
                                ),
                                (
                                    "Content-Type",
                                    HeaderValue::from_static(
                                        "application/vnd.docker.distribution.manifest.v2+json",
                                    ),
                                ),
                            ],
                            bytes,
                        )
                    }
                }
            })
            .head({
                move || {
                    let state = state.clone();
                    async move {
                        let bytes = state.read().clone();
                        let digest = format!("sha256:{:x}", sha2::Sha256::digest(&bytes));
                        (
                            StatusCode::OK,
                            [
                                (
                                    "Docker-Content-Digest",
                                    HeaderValue::from_str(&digest).unwrap(),
                                ),
                                (
                                    "Content-Type",
                                    HeaderValue::from_static(
                                        "application/vnd.docker.distribution.manifest.v2+json",
                                    ),
                                ),
                            ],
                        )
                    }
                }
            }),
        );

        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let server = tokio::spawn(async move {
            if let Err(err) = axum::serve(listener, app).await {
                eprintln!("upstream server error: {err}");
            }
        });

        Ok((format!("http://{}", addr), state_for_return, server))
    }

    #[tokio::test]
    async fn fetch_manifest_caches_locally() -> anyhow::Result<()> {
        let manifest = br#"{"schemaVersion":2,"mediaType":"application/vnd.oci.image.manifest.v1+json","config":{"mediaType":"application/vnd.oci.image.config.v1+json","size":7023,"digest":"sha256:0000000000000000000000000000000000000000000000000000000000000000"},"layers":[]}"#;
        let blob = b"blob-data";
        let (base, server) = start_upstream_server(manifest, blob).await?;

        let storage = test_storage().await;
        let repository_id = Uuid::new_v4();
        let upstream = ProxyUpstream::new(&DockerProxyConfig {
            upstream_url: base,
            upstream_auth: None,
            cache_enabled: true,
            revalidation_ttl_seconds: default_revalidation_ttl(),
            skip_tag_revalidation: false,
        })?;

        let cached = fetch_and_cache_manifest(
            &upstream,
            &storage,
            repository_id,
            "library/alpine",
            "latest",
            None,
        )
        .await?;

        let CachedManifest {
            reader,
            digest,
            content_type,
            length,
        } = cached;
        let cached_bytes = reader_bytes(reader, manifest.len() as u64).await?;
        assert_eq!(cached_bytes, manifest);
        assert_eq!(length, manifest.len() as u64);
        assert_eq!(
            content_type,
            "application/vnd.docker.distribution.manifest.v2+json"
        );

        // Verify digest path saved
        let digest_path =
            StoragePath::from(format!("v2/{}/manifests/{}", "library/alpine", digest));
        let stored = storage
            .open_file(repository_id, &digest_path)
            .await?
            .expect("digest entry");
        let stored_bytes = match stored {
            nr_storage::StorageFile::File { mut content, .. } => {
                let mut buf = Vec::new();
                use tokio::io::AsyncReadExt;
                content.read_to_end(&mut buf).await?;
                buf
            }
            _ => vec![],
        };
        assert_eq!(stored_bytes, manifest);

        server.abort();
        Ok(())
    }

    #[tokio::test]
    async fn fetch_blob_uses_cache_on_second_request() -> anyhow::Result<()> {
        let manifest = br#"{}"#;
        let blob = b"blob-body-for-cache";
        let (base, server) = start_upstream_server(manifest, blob).await?;

        let storage = test_storage().await;
        let repository_id = Uuid::new_v4();
        let digest = format!("sha256:{:x}", sha2::Sha256::digest(blob));
        let upstream = ProxyUpstream::new(&DockerProxyConfig {
            upstream_url: base,
            upstream_auth: None,
            cache_enabled: true,
            revalidation_ttl_seconds: default_revalidation_ttl(),
            skip_tag_revalidation: false,
        })?;

        // First fetch - hit upstream
        let first = fetch_and_cache_blob(
            &upstream,
            &storage,
            repository_id,
            "library/alpine",
            &digest,
        )
        .await?;
        let CachedBlob {
            reader,
            digest: first_digest,
            length,
        } = first;
        let first_body = reader_bytes(reader, blob.len() as u64).await?;
        assert_eq!(first_body, blob);
        assert_eq!(length, blob.len() as u64);
        assert_eq!(first_digest, digest);

        // Stop upstream to ensure second call reads cache
        server.abort();

        let second = fetch_and_cache_blob(
            &upstream,
            &storage,
            repository_id,
            "library/alpine",
            &digest,
        )
        .await?;
        let CachedBlob { reader, .. } = second;
        let second_body = reader_bytes(reader, blob.len() as u64).await?;
        assert_eq!(second_body, blob);

        Ok(())
    }

    #[test]
    fn accept_allows_media_type_handles_oci_aliases() {
        assert!(
            accept_allows_media_type(
                Some("application/vnd.oci.image.manifest.v1+json"),
                "application/vnd.docker.distribution.manifest.v2+json"
            ),
            "OCI image manifest should satisfy Docker schema2 requests"
        );
        assert!(
            accept_allows_media_type(
                Some("application/vnd.oci.image.index.v1+json"),
                "application/vnd.docker.distribution.manifest.list.v2+json"
            ),
            "OCI index should satisfy Docker manifest list requests"
        );
    }

    #[tokio::test]
    async fn manifest_accept_mismatch_falls_back_to_cache() -> anyhow::Result<()> {
        let manifest_list = br#"{"schemaVersion":2,"mediaType":"application/vnd.docker.distribution.manifest.list.v2+json","manifests":[]}"#;
        let (base, server) = start_accept_filtered_upstream_server(manifest_list).await?;

        let storage = test_storage().await;
        let repository_id = Uuid::new_v4();
        let upstream = ProxyUpstream::new(&DockerProxyConfig {
            upstream_url: base,
            upstream_auth: None,
            cache_enabled: true,
            revalidation_ttl_seconds: default_revalidation_ttl(),
            skip_tag_revalidation: false,
        })?;

        // Prime cache with Accept that the upstream honors.
        fetch_and_cache_manifest(
            &upstream,
            &storage,
            repository_id,
            "library/alpine",
            "latest",
            Some("application/vnd.docker.distribution.manifest.list.v2+json"),
        )
        .await?;

        // Request a manifest with a media type the upstream refuses; should fall back to cache.
        let cached = fetch_and_cache_manifest(
            &upstream,
            &storage,
            repository_id,
            "library/alpine",
            "latest",
            Some("application/vnd.docker.distribution.manifest.v2+json"),
        )
        .await?;

        assert_eq!(
            cached.content_type,
            "application/vnd.docker.distribution.manifest.list.v2+json"
        );
        let bytes = reader_bytes(cached.reader, manifest_list.len() as u64).await?;
        assert_eq!(bytes, manifest_list);

        server.abort();
        Ok(())
    }

    #[tokio::test]
    async fn cached_manifest_uses_manifest_media_type() -> anyhow::Result<()> {
        let manifest = br#"{"schemaVersion":2,"mediaType":"application/vnd.docker.distribution.manifest.v2+json"}"#;
        let blob = b"blob-data";
        let (base, server) = start_upstream_server(manifest, blob).await?;

        let storage = test_storage().await;
        let repository_id = Uuid::new_v4();
        let upstream = ProxyUpstream::new(&DockerProxyConfig {
            upstream_url: base,
            upstream_auth: None,
            cache_enabled: true,
            revalidation_ttl_seconds: default_revalidation_ttl(),
            skip_tag_revalidation: false,
        })?;

        // Prime the cache using the upstream manifest
        fetch_and_cache_manifest(
            &upstream,
            &storage,
            repository_id,
            "library/alpine",
            "latest",
            None,
        )
        .await?;

        // Kill upstream to ensure the second call reads from cache only
        server.abort();

        let cached = fetch_and_cache_manifest(
            &upstream,
            &storage,
            repository_id,
            "library/alpine",
            "latest",
            None,
        )
        .await?;

        assert_eq!(
            cached.content_type,
            "application/vnd.docker.distribution.manifest.v2+json"
        );

        Ok(())
    }

    #[tokio::test]
    async fn tag_revalidation_fetches_moved_digest() -> anyhow::Result<()> {
        let repo_id = Uuid::new_v4();
        let storage = test_storage().await;

        let manifest_v1 = br#"{"schemaVersion":2,"mediaType":"application/vnd.docker.distribution.manifest.v2+json","config":{"mediaType":"application/vnd.docker.container.image.v1+json","size":2,"digest":"sha256:aaaa"},"layers":[]}"#;
        let manifest_v2 = br#"{"schemaVersion":2,"mediaType":"application/vnd.docker.distribution.manifest.v2+json","config":{"mediaType":"application/vnd.docker.container.image.v1+json","size":2,"digest":"sha256:bbbb"},"layers":[]}"#;

        let (upstream_url, state, server) =
            start_revalidating_upstream_server(manifest_v1.to_vec()).await?;

        let upstream = ProxyUpstream::new(&DockerProxyConfig {
            upstream_url,
            upstream_auth: None,
            cache_enabled: true,
            revalidation_ttl_seconds: 0,
            skip_tag_revalidation: false,
        })?;

        let first = fetch_and_cache_manifest(
            &upstream,
            &storage,
            repo_id,
            "library/test",
            "latest",
            Some(MODERN_UPSTREAM_ACCEPT),
        )
        .await?;

        let first_digest = first.digest.clone();
        assert!(first_digest.starts_with("sha256:"));

        // Change upstream manifest and trigger revalidation
        *state.write() = manifest_v2.to_vec();

        let refreshed = fetch_and_cache_manifest(
            &upstream,
            &storage,
            repo_id,
            "library/test",
            "latest",
            Some(MODERN_UPSTREAM_ACCEPT),
        )
        .await?;

        assert_ne!(refreshed.digest, first_digest);

        server.abort();
        Ok(())
    }

    #[tokio::test]
    async fn accept_mismatch_triggers_refetch_with_preferred_media_type() -> anyhow::Result<()> {
        // Upstream returns schema1 by default, schema2 when explicitly requested via Accept
        let v1_manifest = br#"{"schemaVersion":1,"name":"library/alpine","fsLayers":[]}"#;
        let v2_manifest = br#"{"schemaVersion":2,"mediaType":"application/vnd.docker.distribution.manifest.v2+json"}"#;
        let (base, server) = start_negotiating_upstream_server(v1_manifest, v2_manifest).await?;

        let storage = test_storage().await;
        let repository_id = Uuid::new_v4();
        let upstream = ProxyUpstream::new(&DockerProxyConfig {
            upstream_url: base,
            upstream_auth: None,
            cache_enabled: true,
            revalidation_ttl_seconds: default_revalidation_ttl(),
            skip_tag_revalidation: false,
        })?;

        // Cache initial manifest; proxy now prefers modern media types so it should store schema2
        let cached_v1 = fetch_and_cache_manifest(
            &upstream,
            &storage,
            repository_id,
            "library/alpine",
            "latest",
            None,
        )
        .await?;
        assert_eq!(
            cached_v1.content_type,
            "application/vnd.docker.distribution.manifest.v2+json"
        );

        // Request again, this time with an Accept that prefers schema2
        let accept_header = "application/vnd.docker.distribution.manifest.v2+json";
        let refreshed = fetch_and_cache_manifest(
            &upstream,
            &storage,
            repository_id,
            "library/alpine",
            "latest",
            Some(accept_header),
        )
        .await?;

        assert_eq!(
            refreshed.content_type,
            "application/vnd.docker.distribution.manifest.v2+json"
        );

        server.abort();
        Ok(())
    }

    #[tokio::test]
    async fn deleted_manifest_is_downloaded_again() -> anyhow::Result<()> {
        use crate::app::api::repository::packages::delete_docker_package;

        let manifest = br#"{"schemaVersion":2}"#;
        let blob = b"bin";
        let (base, server) = start_upstream_server(manifest, blob).await?;

        let storage = test_storage().await;
        let repository_id = Uuid::new_v4();
        let upstream = ProxyUpstream::new(&DockerProxyConfig {
            upstream_url: base,
            upstream_auth: None,
            cache_enabled: true,
            revalidation_ttl_seconds: default_revalidation_ttl(),
            skip_tag_revalidation: false,
        })?;

        // Cache manifest once
        fetch_and_cache_manifest(
            &upstream,
            &storage,
            repository_id,
            "library/alpine",
            "latest",
            None,
        )
        .await?;

        let manifest_path = StoragePath::from("v2/library/alpine/manifests/latest");
        assert!(
            storage.file_exists(repository_id, &manifest_path).await?,
            "manifest should be cached before deletion"
        );

        // Simulate admin deletion via API helper
        delete_docker_package(&storage, repository_id, manifest_path.to_string().as_str())
            .await
            .expect("docker deletion should succeed");

        assert!(
            !storage.file_exists(repository_id, &manifest_path).await?,
            "manifest cache file should be removed"
        );

        // Next fetch should re-download from upstream and cache again
        let refreshed = fetch_and_cache_manifest(
            &upstream,
            &storage,
            repository_id,
            "library/alpine",
            "latest",
            None,
        )
        .await?;
        let CachedManifest { reader, .. } = refreshed;
        let refreshed_bytes = reader_bytes(reader, manifest.len() as u64).await?;
        assert_eq!(refreshed_bytes, manifest);
        assert!(
            storage.file_exists(repository_id, &manifest_path).await?,
            "manifest should be cached again after re-download"
        );

        server.abort();
        Ok(())
    }

    #[tokio::test]
    async fn blob_digest_mismatch_is_reported() -> anyhow::Result<()> {
        let blob = b"blob-body-for-cache";
        let correct_digest = format!("sha256:{:x}", sha2::Sha256::digest(blob));
        let wrong_header =
            "sha256:0000000000000000000000000000000000000000000000000000000000000000";

        let app = Router::new().route(
            &format!("/v2/library/alpine/blobs/{correct_digest}"),
            get(move || {
                let chunk = Bytes::from_static(blob);
                let stream = stream::iter(vec![Ok::<_, Infallible>(chunk)]);
                async move {
                    (
                        StatusCode::OK,
                        [("Docker-Content-Digest", wrong_header)],
                        AxumBody::from_stream(stream),
                    )
                }
            }),
        );

        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        let storage = test_storage().await;
        let repository_id = Uuid::new_v4();
        let upstream = ProxyUpstream::new(&DockerProxyConfig {
            upstream_url: format!("http://{addr}"),
            upstream_auth: None,
            cache_enabled: true,
            revalidation_ttl_seconds: default_revalidation_ttl(),
            skip_tag_revalidation: false,
        })?;

        let err = fetch_and_cache_blob(
            &upstream,
            &storage,
            repository_id,
            "library/alpine",
            &correct_digest,
        )
        .await
        .expect_err("digest mismatch expected");

        match err {
            DockerError::DigestMismatch { expected, .. } => {
                assert_eq!(expected, wrong_header);
            }
            other => panic!("expected digest mismatch, got {other:?}"),
        }

        server.abort();
        Ok(())
    }

    #[tokio::test]
    async fn large_blob_is_streamed_without_buffering() -> anyhow::Result<()> {
        let chunk = Bytes::from(vec![b'x'; 1_024 * 1_024]); // 1MiB chunk
        let chunks = 6;
        let mut full = Vec::with_capacity(chunk.len() * chunks);
        for _ in 0..chunks {
            full.extend_from_slice(&chunk);
        }
        let digest = format!("sha256:{:x}", sha2::Sha256::digest(&full));

        let app = Router::new().route(
            &format!("/v2/library/alpine/blobs/{digest}"),
            get({
                let chunk = chunk.clone();
                let digest_header =
                    HeaderValue::from_str(&digest).expect("valid digest header value");
                let content_type = HeaderValue::from_static("application/octet-stream");
                move || {
                    let digest_header = digest_header.clone();
                    let content_type = content_type.clone();
                    let stream = stream::iter((0..chunks).map({
                        let chunk = chunk.clone();
                        move |_| Ok::<_, Infallible>(chunk.clone())
                    }));
                    async move {
                        (
                            StatusCode::OK,
                            [
                                ("Docker-Content-Digest", digest_header),
                                ("Content-Type", content_type),
                            ],
                            AxumBody::from_stream(stream),
                        )
                    }
                }
            }),
        );

        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        let storage = test_storage().await;
        let repository_id = Uuid::new_v4();
        let upstream = ProxyUpstream::new(&DockerProxyConfig {
            upstream_url: format!("http://{addr}"),
            upstream_auth: None,
            cache_enabled: true,
            revalidation_ttl_seconds: default_revalidation_ttl(),
            skip_tag_revalidation: false,
        })?;

        let blob = fetch_and_cache_blob(
            &upstream,
            &storage,
            repository_id,
            "library/alpine",
            &digest,
        )
        .await?;

        assert!(
            !matches!(&blob.reader, nr_storage::StorageFileReader::Bytes(_)),
            "streaming should not return in-memory reader"
        );
        let CachedBlob {
            reader,
            digest: found_digest,
            length,
        } = blob;

        assert_eq!(found_digest, digest);
        assert_eq!(length, full.len() as u64);

        let content = reader_bytes(reader, full.len() as u64).await?;
        assert_eq!(content.len(), full.len());
        assert_eq!(content, full);

        let blob_path = StoragePath::from(format!("v2/library/alpine/blobs/{digest}"));
        assert!(
            storage.file_exists(repository_id, &blob_path).await?,
            "blob should be cached on disk"
        );

        server.abort();
        Ok(())
    }

    #[tokio::test]
    async fn fetch_lock_map_is_bounded() {
        // Create many unique keys to force evictions
        for i in 0..(LOCK_CACHE_CAPACITY + 500) {
            let key = format!("manifest:test:{i}");
            with_fetch_lock(&key, async {}).await;
        }

        assert!(
            fetch_lock_len() <= LOCK_CACHE_CAPACITY,
            "lock cache should be bounded"
        );
    }

    fn streamed_from_bytes(bytes: &[u8]) -> anyhow::Result<StreamedDownload> {
        use std::io::Write;
        let mut file = tempfile::Builder::new()
            .prefix("manifest-test-")
            .tempfile()?;
        file.write_all(bytes)?;
        let path = file.into_temp_path();
        let digest = format!("sha256:{:x}", sha2::Sha256::digest(bytes));
        Ok(StreamedDownload {
            path,
            size: bytes.len() as u64,
            digest,
        })
    }

    #[tokio::test]
    async fn manifest_collision_with_different_digest_errors() -> anyhow::Result<()> {
        let storage = test_storage().await;
        let repository_id = Uuid::new_v4();
        let tag_path = StoragePath::from("v2/library/test/manifests/latest");

        let manifest1 = br#"{"schemaVersion":2,"config":{"digest":"sha256:aaaa"},"layers":[]}"#;
        let streamed1 = streamed_from_bytes(manifest1)?;
        let digest_path1 = StoragePath::from(format!(
            "v2/{}/manifests/{}",
            "library/test", streamed1.digest
        ));

        save_manifest_atomically(
            &storage,
            repository_id,
            &tag_path,
            Some(&digest_path1),
            &streamed1,
            "latest",
            &streamed1.digest,
        )
        .await?;

        let manifest2 = br#"{"schemaVersion":2,"config":{"digest":"sha256:bbbb"},"layers":[]}"#;
        let streamed2 = streamed_from_bytes(manifest2)?;
        let digest_path2 = StoragePath::from(format!(
            "v2/{}/manifests/{}",
            "library/test", streamed2.digest
        ));

        save_manifest_atomically(
            &storage,
            repository_id,
            &tag_path,
            Some(&digest_path2),
            &streamed2,
            "latest",
            &streamed2.digest,
        )
        .await?;

        let cached = load_cached_manifest(&storage, repository_id, &tag_path, "latest")
            .await?
            .expect("tag should be cached");
        assert_eq!(cached.digest, streamed2.digest);

        Ok(())
    }

    #[tokio::test]
    async fn bearer_challenge_is_followed_for_public_token() -> anyhow::Result<()> {
        // Token service
        let token_app = Router::new().route(
            "/token",
            get(|| async { axum::Json(serde_json::json!({ "token": "abc123" })) }),
        );
        let token_listener = TcpListener::bind("127.0.0.1:0").await?;
        let token_addr = token_listener.local_addr()?;
        let token_server = tokio::spawn(async move {
            let _ = axum::serve(token_listener, token_app).await;
        });

        // Upstream that challenges then succeeds
        let manifest_body = br#"{"schemaVersion":2}"#;
        let manifest_digest = format!("sha256:{:x}", sha2::Sha256::digest(manifest_body));
        let manifest_digest_expected = manifest_digest.clone();
        let guarded_path = "/v2/library/alpine/manifests/latest";
        let app = Router::new().route(
            guarded_path,
            get({
                let challenge = format!(
                    "Bearer realm=\"http://{}/token\",service=\"registry-1.docker.io\",scope=\"repository:library/alpine:pull\"",
                    token_addr
                );
                move |headers: HeaderMap| {
                    let challenge = challenge.clone();
                    async move {
                        let auth_ok = headers
                            .get(http::header::AUTHORIZATION)
                            .map(|v| v == "Bearer abc123")
                            .unwrap_or(false);
                        if auth_ok {
                            let mut builder = ResponseBuilder::ok()
                                .header(
                                    "Docker-Content-Digest",
                                    manifest_digest.clone(),
                                )
                                .header(
                                    CONTENT_TYPE,
                                    "application/vnd.docker.distribution.manifest.v2+json",
                                );
                            builder = builder.header(
                                CONTENT_LENGTH,
                                manifest_body.len().to_string(),
                            );
                            builder.body(manifest_body as &[u8])
                        } else {
                            ResponseBuilder::unauthorized()
                                .header("WWW-Authenticate", challenge.clone())
                                .header(CONTENT_TYPE, "application/json")
                                .body(b"{}" as &[u8])
                        }
                    }
                }
            }),
        );

        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let upstream_server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });

        let storage = test_storage().await;
        let repository_id = Uuid::new_v4();
        let upstream = ProxyUpstream::new(&DockerProxyConfig {
            upstream_url: format!("http://{addr}"),
            upstream_auth: None,
            cache_enabled: true,
            revalidation_ttl_seconds: default_revalidation_ttl(),
            skip_tag_revalidation: false,
        })?;

        let manifest = fetch_and_cache_manifest(
            &upstream,
            &storage,
            repository_id,
            "library/alpine",
            "latest",
            None,
        )
        .await?;

        let CachedManifest {
            reader,
            digest,
            length,
            ..
        } = manifest;

        assert_eq!(digest, manifest_digest_expected);
        let manifest_bytes = reader_bytes(reader, length).await?;
        assert_eq!(manifest_bytes, manifest_body);

        upstream_server.abort();
        token_server.abort();
        Ok(())
    }
}
