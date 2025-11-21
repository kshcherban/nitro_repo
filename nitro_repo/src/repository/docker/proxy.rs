//! Docker proxy (pull-through cache) support.
//!
//! The proxy repository is read-only and forwards GET/HEAD requests to an
//! upstream Docker registry, caching responses locally.

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use axum::body::Body;
use http::{
    HeaderMap, StatusCode,
    header::{ACCEPT, CONTENT_LENGTH, CONTENT_TYPE},
};
use nr_core::{
    repository::Visibility, repository::config::RepositoryConfigType, storage::StoragePath,
    utils::base64_utils,
};
use nr_storage::{DynStorage, FileType, Storage, StorageFile};
use parking_lot::RwLock;
use reqwest::{Client, Response};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::Digest;
use tracing::instrument;
use url::Url;
use uuid::Uuid;

use super::{DockerError, types::MediaType};
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
}

fn default_cache_enabled() -> bool {
    true
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
}

impl ProxyUpstream {
    pub(crate) fn new(config: &DockerProxyConfig) -> Result<Self, DockerError> {
        let base = Url::parse(&config.upstream_url)?;
        let client = Client::new();
        Ok(Self {
            base,
            client,
            cache_enabled: config.cache_enabled,
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
        let mut current_bearer = bearer;
        for _ in 0..2 {
            let mut request = self.client.get(url.clone());
            if let Some(value) = accept {
                request = request.header(ACCEPT, value);
            }
            if let Some(token) = current_bearer.as_deref() {
                request = request.header(http::header::AUTHORIZATION, format!("Bearer {token}"));
            }
            let response = request.send().await?;

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
        let mut request = self.client.get(url);
        if let Some(value) = accept {
            request = request.header(ACCEPT, value);
        }
        if let Some(token) = current_bearer.as_deref() {
            request = request.header(http::header::AUTHORIZATION, format!("Bearer {token}"));
        }
        Ok(request.send().await?)
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

async fn read_storage_file_bytes(file: StorageFile) -> Result<Vec<u8>, DockerError> {
    match file {
        StorageFile::File { mut content, .. } => {
            let mut buffer = Vec::new();
            tokio::io::AsyncReadExt::read_to_end(&mut content, &mut buffer).await?;
            Ok(buffer)
        }
        StorageFile::Directory { .. } => Err(DockerError::InvalidManifest(
            "Expected file, got directory".to_string(),
        )),
    }
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
            blob_head_response(&blob.digest, blob.bytes.len() as u64)
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

#[derive(Debug, Clone)]
pub(crate) struct CachedManifest {
    pub bytes: Vec<u8>,
    pub digest: String,
    pub content_type: String,
}

#[derive(Debug, Clone)]
pub(crate) struct CachedBlob {
    pub bytes: Vec<u8>,
    pub digest: String,
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

    if let Some(file) = storage.open_file(repository_id, &manifest_path).await? {
        let bytes = read_storage_file_bytes(file).await?;
        let digest = compute_sha256_hex(&bytes);
        let content_type = manifest_media_type(&bytes, None);
        return Ok(CachedManifest {
            bytes,
            digest,
            content_type,
        });
    }

    let upstream_repo = upstream_image_name(repository_name, upstream);
    let path = format!("/v2/{}/manifests/{}", upstream_repo, reference);
    let response = upstream.fetch(&path, accept).await?;
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
    let body = response.bytes().await?.to_vec();
    let content_type = manifest_media_type(&body, Some(&headers));
    let computed_digest = compute_sha256_hex(&body);

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
        storage
            .save_file(repository_id, body.clone().into(), &manifest_path)
            .await?;

        if !reference.starts_with("sha256:") {
            let digest_path = StoragePath::from(format!(
                "v2/{}/manifests/{}",
                repository_name, &computed_digest
            ));
            let _ = storage
                .save_file(repository_id, body.clone().into(), &digest_path)
                .await?;
        }
    }

    Ok(CachedManifest {
        bytes: body,
        digest: computed_digest,
        content_type,
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
    if let Some(existing) = storage.open_file(repository_id, &blob_path).await? {
        let bytes = read_storage_file_bytes(existing).await?;
        let computed_digest = compute_sha256_hex(&bytes);
        if digest.starts_with("sha256:") && digest != computed_digest {
            return Err(DockerError::DigestMismatch {
                expected: digest.to_string(),
                actual: computed_digest,
            });
        }
        return Ok(CachedBlob {
            bytes,
            digest: computed_digest,
        });
    }

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
    let body = response.bytes().await?.to_vec();
    let computed_digest = compute_sha256_hex(&body);

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
        storage
            .save_file(repository_id, body.clone().into(), &blob_path)
            .await?;
    }

    Ok(CachedBlob {
        bytes: body,
        digest: computed_digest,
    })
}

fn manifest_get_response(manifest: CachedManifest) -> RepoResponse {
    let length = manifest.bytes.len().to_string();
    RepoResponse::Other(
        ResponseBuilder::ok()
            .header("Docker-Distribution-API-Version", "registry/2.0")
            .header("Docker-Content-Digest", manifest.digest.clone())
            .header(CONTENT_TYPE, manifest.content_type)
            .header(CONTENT_LENGTH, length)
            .body(Body::from(manifest.bytes)),
    )
}

fn manifest_head_response(manifest: &CachedManifest) -> RepoResponse {
    RepoResponse::Other(
        ResponseBuilder::ok()
            .header("Docker-Distribution-API-Version", "registry/2.0")
            .header("Docker-Content-Digest", manifest.digest.clone())
            .header(CONTENT_TYPE, manifest.content_type.clone())
            .header(CONTENT_LENGTH, manifest.bytes.len().to_string())
            .body(Body::empty()),
    )
}

fn blob_get_response(blob: CachedBlob) -> RepoResponse {
    RepoResponse::Other(
        ResponseBuilder::ok()
            .header("Docker-Distribution-API-Version", "registry/2.0")
            .header("Docker-Content-Digest", blob.digest.clone())
            .header(CONTENT_TYPE, "application/octet-stream")
            .header(CONTENT_LENGTH, blob.bytes.len().to_string())
            .body(Body::from(blob.bytes)),
    )
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
        vec![RepositoryAuthConfigType::get_type_static()]
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
    use axum::{Router, routing::get};
    use http::{HeaderValue, StatusCode};
    use nr_core::storage::StoragePath;
    use sha2::Digest;
    use tokio::net::TcpListener;
    use tokio::task::JoinHandle;

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

        assert_eq!(cached.bytes, manifest);
        assert_eq!(
            cached.content_type,
            "application/vnd.docker.distribution.manifest.v2+json"
        );

        // Verify digest path saved
        let digest_path = StoragePath::from(format!(
            "v2/{}/manifests/{}",
            "library/alpine", cached.digest
        ));
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
        assert_eq!(first.bytes, blob);

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
        assert_eq!(second.bytes, blob);

        Ok(())
    }

    #[tokio::test]
    async fn blob_digest_mismatch_is_reported() -> anyhow::Result<()> {
        use axum::routing::get;

        let blob = b"blob-body-for-cache";
        let correct_digest = format!("sha256:{:x}", sha2::Sha256::digest(blob));
        let wrong_header =
            "sha256:0000000000000000000000000000000000000000000000000000000000000000";

        let app = Router::new().route(
            &format!("/v2/library/alpine/blobs/{correct_digest}"),
            get(move || async move {
                (
                    StatusCode::OK,
                    [("Docker-Content-Digest", wrong_header)],
                    blob,
                )
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
        let manifest = br#"{"schemaVersion":2}"#;
        let manifest_digest = format!("sha256:{:x}", sha2::Sha256::digest(manifest));
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
                                manifest.len().to_string(),
                            );
                            builder.body(manifest as &[u8])
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

        assert_eq!(manifest.digest, manifest_digest_expected);

        upstream_server.abort();
        token_server.abort();
        Ok(())
    }
}
