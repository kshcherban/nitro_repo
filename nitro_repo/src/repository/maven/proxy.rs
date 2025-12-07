use std::{
    fmt,
    ops::Deref,
    sync::{Arc, atomic::AtomicBool},
};

use axum::{body::Body, response::Response};
use bytes::Bytes;
use chrono::Utc;
use futures::StreamExt;
use http::{
    StatusCode,
    header::{CONTENT_LENGTH, CONTENT_TYPE, ETAG, LAST_MODIFIED},
};
use maven_rs::pom::Pom;
use nr_core::{
    database::entities::repository::{DBRepository, DBRepositoryConfig},
    repository::{
        Visibility,
        config::{RepositoryConfigType as _, repository_page::RepositoryPageType},
        project::{ProxyArtifactKey, ProxyArtifactMeta},
        proxy_url::ProxyURL,
    },
    storage::StoragePath,
};
use nr_storage::{DynStorage, FileContent, Storage, StorageFile};
use parking_lot::RwLock;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tempfile::Builder;
use tokio::io::AsyncWriteExt;
use tracing::{debug, error, instrument, warn};
use uuid::Uuid;

use super::{
    MavenError, MavenRepositoryConfig, MavenRepositoryConfigType, REPOSITORY_TYPE_ID, RepoResponse,
    RepositoryRequest, repo_type::RepositoryFactoryError, utils::MavenRepositoryExt,
};
use crate::{
    app::NitroRepo,
    repository::{
        Repository, RepositoryAuthConfigType,
        proxy::base_proxy::{evict_proxy_cache_entry, record_proxy_cache_hit},
        proxy_indexing::{DatabaseProxyIndexer, ProxyIndexing, ProxyIndexingError},
    },
};
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct MavenProxyConfig {
    pub routes: Vec<MavenProxyRepositoryRoute>,
}
impl MavenProxyConfig {
    pub fn sort(&mut self) {
        self.routes.sort_by(|a, b| match (a.priority, b.priority) {
            (Some(a), Some(b)) => a.cmp(&b),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        });
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct MavenProxyRepositoryRoute {
    pub url: ProxyURL,
    pub name: Option<String>,
    /// If Null then it will be the lowest priority
    pub priority: Option<i32>,
    // TODO: Credentials
}

#[derive(Debug)]
struct StreamedDownload {
    path: tempfile::TempPath,
    size: u64,
}

fn snapshot_routes(config: &RwLock<MavenProxyConfig>) -> Vec<MavenProxyRepositoryRoute> {
    config.read().routes.clone()
}

async fn stream_response_to_tempfile(
    response: reqwest::Response,
) -> Result<StreamedDownload, MavenError> {
    let named = Builder::new().prefix("maven-proxy-").tempfile()?;
    let (std_file, path) = named.into_parts();
    let mut file = tokio::fs::File::from_std(std_file);

    let mut total = 0u64;
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        total += chunk.len() as u64;
        file.write_all(&chunk).await?;
    }
    file.flush().await?;
    file.sync_all().await?;

    Ok(StreamedDownload { path, size: total })
}

async fn persist_streamed_download(
    storage: &DynStorage,
    repository_id: Uuid,
    indexer: Option<&dyn ProxyIndexing>,
    streamed: &StreamedDownload,
    to: &StoragePath,
) -> Result<(), MavenError> {
    storage
        .save_file(
            repository_id,
            FileContent::Path(streamed.path.to_path_buf()),
            to,
        )
        .await?;
    record_maven_proxy_cache_hit(indexer, to, streamed.size).await?;
    Ok(())
}
fn project_download_files(pom: &Pom) -> Result<Vec<String>, MavenError> {
    let version = pom
        .get_version()
        .ok_or(MavenError::MissingFromPom("version"))?;
    Ok(vec![
        format!("{}-{}.jar", pom.artifact_id, version),
        format!("{}-{}-sources.jar", pom.artifact_id, version),
        format!("{}-{}-javadoc.jar", pom.artifact_id, version),
    ])
}
pub struct MavenProxyInner {
    pub storage: DynStorage,
    pub site: NitroRepo,
    pub id: Uuid,
    pub name: String,
    pub visibility: RwLock<Visibility>,
    pub active: AtomicBool,
    pub config: RwLock<MavenProxyConfig>,
    pub indexer: Arc<dyn ProxyIndexing>,
}

impl fmt::Debug for MavenProxyInner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MavenProxyInner")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("active", &self.active)
            .field("visibility", &self.visibility.read())
            .finish()
    }
}
#[derive(Debug, Clone)]
pub struct MavenProxy(Arc<MavenProxyInner>);
impl Deref for MavenProxy {
    type Target = MavenProxyInner;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl MavenProxy {
    pub async fn load(
        repository: DBRepository,
        storage: DynStorage,
        site: NitroRepo,
        proxy_config: MavenProxyConfig,
    ) -> Result<Self, RepositoryFactoryError> {
        let indexer: Arc<dyn ProxyIndexing> =
            Arc::new(DatabaseProxyIndexer::new(site.clone(), repository.id));
        let inner = MavenProxyInner {
            id: repository.id,
            name: repository.name.into(),
            active: AtomicBool::new(repository.active),
            visibility: RwLock::new(repository.visibility),
            config: RwLock::new(proxy_config),
            storage,
            site,
            indexer,
        };
        Ok(Self(Arc::new(inner)))
    }
    #[instrument(skip(self), fields(nr.repository.id = %self.id, nr.repository.name = %self.name))]
    pub async fn proxy_project_download(
        &self,
        path: StoragePath,
        proxy_config: MavenProxyRepositoryRoute,
        pom: Bytes,
    ) -> Result<(), MavenError> {
        let pom = self.parse_pom(pom.to_vec())?;
        let version_dir = path.clone().parent();
        let http_client = reqwest::Client::builder()
            .user_agent("Nitro Repo")
            .build()?;

        for file in project_download_files(&pom)? {
            debug!(?file, "Downloading file");
            let mut path = version_dir.clone();
            path.push_mut(&file);
            let url = format!("{}/{}", proxy_config.url, path);
            match http_client.get(&url).send().await {
                Ok(ok) => {
                    if ok.status().is_success() {
                        let streamed = stream_response_to_tempfile(ok).await?;
                        persist_streamed_download(
                            &self.storage,
                            self.id,
                            Some(self.indexer().as_ref()),
                            &streamed,
                            &path,
                        )
                        .await?;
                    } else {
                        warn!(?url, ?file, ?ok, "Failed to download file");
                    }
                }
                Err(err) => {
                    warn!(?url, ?file, ?err, "Failed to download file");
                }
            }
        }
        self.post_pom_upload(path.clone(), None, pom).await;
        // TODO: Trigger project indexing
        Ok(())
    }
    #[instrument(skip(self), fields(nr.repository.id = %self.id, nr.repository.name = %self.name))]
    pub async fn get_from_proxy(
        &self,
        path: StoragePath,
    ) -> Result<Option<StorageFile>, MavenError> {
        // TODO: Setup internal cache to check the following
        //  If a recent previous request was made with a similar path use that proxy config.
        //  Similar path being both starting with /dev/kingtux/tms/... They should be in the same proxy
        // TODO: Handle projects. When requesting a path such as /dev/kingtux/tms/1.0.0/tms-1.0.0.pom. Go ahead and download all files in that directory.
        let routes = snapshot_routes(&self.config);
        let http_client = reqwest::Client::builder()
            .user_agent("Nitro Repo")
            .build()?;
        for route in routes {
            let mut path_as_string = path.to_string();
            if path_as_string.starts_with("/") {
                path_as_string = path_as_string[1..].into();
            }
            let url_string = format!("{}/{}", route.url, path_as_string);
            debug!(?url_string, "Proxying request");
            let url = match url::Url::parse(&url_string) {
                Ok(ok) => ok,
                Err(err) => {
                    error!(?err, ?url_string, "Failed to parse URL");
                    continue;
                }
            };
            let response = match http_client.get(url).send().await {
                Ok(ok) => ok,
                Err(err) => {
                    error!(?err, ?url_string, "Failed to send request");
                    continue;
                }
            };
            if response.status().is_success() {
                let is_pom = path_as_string.ends_with(".pom");
                let streamed = stream_response_to_tempfile(response).await?;
                if is_pom {
                    let pom_bytes = tokio::fs::read(&streamed.path).await?;
                    self.proxy_project_download(
                        path.clone(),
                        route.clone(),
                        Bytes::from(pom_bytes),
                    )
                    .await?;
                }
                persist_streamed_download(
                    &self.storage,
                    self.id,
                    Some(self.indexer().as_ref()),
                    &streamed,
                    &path,
                )
                .await?;
                if path_as_string.ends_with(".pom") {
                    // POM was already parsed and project files downloaded above
                }
                return Ok(self.storage.open_file(self.id, &path).await?);
            } else {
                warn!(?response, ?url_string, "Failed to proxy request");
            }
        }
        Ok(None)
    }

    fn indexer(&self) -> &Arc<dyn ProxyIndexing> {
        &self.0.indexer
    }

    pub async fn handle_external_eviction(&self, path: &StoragePath) -> Result<(), MavenError> {
        evict_maven_proxy_cache_entry(Some(self.indexer().as_ref()), path).await?;
        Ok(())
    }

    #[instrument(skip(self), fields(nr.repository.id = %self.id, nr.repository.name = %self.name))]
    pub async fn head_from_proxy(
        &self,
        path: StoragePath,
    ) -> Result<Option<RepoResponse>, MavenError> {
        let proxy_config = self.config.read().clone();
        let http_client = reqwest::Client::builder()
            .user_agent("Nitro Repo")
            .build()?;

        for route in proxy_config.routes {
            let mut path_as_string = path.to_string();
            if path_as_string.starts_with('/') {
                path_as_string = path_as_string[1..].into();
            }
            let url_string = format!("{}/{}", route.url, path_as_string);
            debug!(?url_string, "HEAD proxying request");
            let url = match url::Url::parse(&url_string) {
                Ok(ok) => ok,
                Err(err) => {
                    error!(?err, ?url_string, "Failed to parse URL");
                    continue;
                }
            };

            let response = match http_client.head(url).send().await {
                Ok(ok) => ok,
                Err(err) => {
                    warn!(?err, ?url_string, "Failed to send HEAD request");
                    continue;
                }
            };

            if response.status().is_success() {
                let mut builder = Response::builder().status(response.status());
                if let Some(headers) = builder.headers_mut() {
                    for header in [CONTENT_LENGTH, CONTENT_TYPE, LAST_MODIFIED, ETAG] {
                        if let Some(value) = response.headers().get(&header) {
                            headers.insert(header.clone(), value.clone());
                        }
                    }
                }

                let response = builder.body(Body::empty()).unwrap_or_else(|err| {
                    warn!(?err, "Failed to build HEAD proxy response");
                    Response::builder()
                        .status(StatusCode::INTERNAL_SERVER_ERROR)
                        .body(Body::from("Failed to proxy HEAD request"))
                        .unwrap_or_default()
                });

                return Ok(Some(RepoResponse::Other(response)));
            }
        }

        Ok(None)
    }
}

fn parse_maven_coordinates(path: &StoragePath) -> Option<(String, String, String)> {
    let components: Vec<String> = path.clone().into_iter().map(|p| p.to_string()).collect();
    if components.len() < 4 {
        return None;
    }
    let file_name = components.last()?.to_string();
    let lowered = file_name.to_ascii_lowercase();
    if lowered == "maven-metadata.xml"
        || lowered.ends_with(".sha1")
        || lowered.ends_with(".sha256")
        || lowered.ends_with(".md5")
        || lowered.ends_with(".asc")
    {
        return None;
    }
    let version = components.get(components.len() - 2)?.to_string();
    if version.is_empty() {
        return None;
    }
    let artifact = components.get(components.len() - 3)?.to_string();
    if artifact.is_empty() {
        return None;
    }
    let group_segments = &components[..components.len() - 3];
    if group_segments.is_empty() {
        return None;
    }
    let group = group_segments.join(".");
    if group.is_empty() {
        return None;
    }
    Some((group, artifact, version))
}

pub(super) fn maven_proxy_meta_from_cache_path(
    path: &StoragePath,
    size: u64,
) -> Option<ProxyArtifactMeta> {
    let (group, artifact, version) = parse_maven_coordinates(path)?;
    let package_key = format!("{}:{}", group, artifact);
    Some(
        ProxyArtifactMeta::builder(artifact, package_key, path.to_string())
            .version(version)
            .size(size)
            .fetched_at(Utc::now())
            .build(),
    )
}

pub(super) fn maven_proxy_key_from_cache_path(path: &StoragePath) -> Option<ProxyArtifactKey> {
    let (group, artifact, version) = parse_maven_coordinates(path)?;
    Some(ProxyArtifactKey {
        package_key: format!("{}:{}", group, artifact),
        version: Some(version),
        cache_path: Some(path.to_string()),
    })
}

pub(super) async fn record_maven_proxy_cache_hit(
    indexer: Option<&dyn ProxyIndexing>,
    path: &StoragePath,
    size: u64,
) -> Result<(), ProxyIndexingError> {
    let Some(indexer) = indexer else {
        return Ok(());
    };
    let meta = maven_proxy_meta_from_cache_path(path, size);
    record_proxy_cache_hit(indexer, meta).await
}

pub(super) async fn evict_maven_proxy_cache_entry(
    indexer: Option<&dyn ProxyIndexing>,
    path: &StoragePath,
) -> Result<(), ProxyIndexingError> {
    let Some(indexer) = indexer else {
        return Ok(());
    };
    let key = maven_proxy_key_from_cache_path(path);
    evict_proxy_cache_entry(indexer, key).await
}

#[cfg(test)]
mod tests;

impl Repository for MavenProxy {
    type Error = MavenError;
    fn get_storage(&self) -> nr_storage::DynStorage {
        self.0.storage.clone()
    }
    fn visibility(&self) -> Visibility {
        Visibility::Public
    }

    fn get_type(&self) -> &'static str {
        REPOSITORY_TYPE_ID
    }
    fn full_type(&self) -> &'static str {
        "maven/proxy"
    }
    fn config_types(&self) -> Vec<&str> {
        vec![
            RepositoryPageType::get_type_static(),
            MavenRepositoryConfigType::get_type_static(),
            RepositoryAuthConfigType::get_type_static(),
        ]
    }

    fn name(&self) -> String {
        self.0.name.clone()
    }

    fn id(&self) -> Uuid {
        self.0.id
    }

    fn is_active(&self) -> bool {
        self.0.active.load(std::sync::atomic::Ordering::Relaxed)
    }
    #[instrument(fields(repository_type = "maven/proxy"))]
    async fn reload(&self) -> Result<(), RepositoryFactoryError> {
        let Some(maven_config_db) = DBRepositoryConfig::<MavenRepositoryConfig>::get_config(
            self.id,
            MavenRepositoryConfigType::get_type_static(),
            self.site.as_ref(),
        )
        .await?
        else {
            return Err(RepositoryFactoryError::MissingConfig(
                MavenRepositoryConfigType::get_type_static(),
            ));
        };
        {
            match maven_config_db.value.0 {
                MavenRepositoryConfig::Proxy(proxy_config) => {
                    let mut maven_config = self.config.write();
                    *maven_config = proxy_config;
                }
                _ => {
                    return Err(RepositoryFactoryError::InvalidConfig(
                        MavenRepositoryConfigType::get_type_static(),
                        "Expected Proxy Config".into(),
                    ));
                }
            }
        }
        Ok(())
    }
    async fn handle_get(
        &self,
        RepositoryRequest {
            parts: _,
            path,
            authentication,
            ..
        }: RepositoryRequest,
    ) -> Result<RepoResponse, MavenError> {
        if let Some(err) = self.check_read(&authentication).await? {
            return Ok(err);
        }
        let _visibility = self.visibility();
        let Some(file) = self.0.storage.open_file(self.id, &path).await? else {
            debug!(?path, "File not found in storage. Proxying request");
            return match self.get_from_proxy(path).await {
                Ok(ok) => Ok(RepoResponse::from(ok)),
                Err(err) => {
                    warn!(?err, "Failed to proxy request");
                    Ok(Response::builder()
                        .status(StatusCode::SERVICE_UNAVAILABLE)
                        .body(format!("Failed to proxy request: {}", err).into())
                        .into())
                }
            };
        };
        // TODO: Check file age. If it is older than the configured time then re-download the file.
        return self.indexing_check(file, &authentication).await;
    }
    async fn handle_head(
        &self,
        RepositoryRequest {
            parts: _,
            path,
            authentication,
            ..
        }: RepositoryRequest,
    ) -> Result<RepoResponse, MavenError> {
        let _visibility = self.visibility();
        // TODO: Proxy HEAD request
        if let Some(err) = self.check_read(&authentication).await? {
            return Ok(err);
        }
        let file = self.storage.get_file_information(self.id, &path).await?;
        if file.is_none() {
            if let Some(response) = self.head_from_proxy(path.clone()).await? {
                return Ok(response);
            }
        }
        self.indexing_check_option(file, &authentication).await
    }
    fn site(&self) -> NitroRepo {
        self.0.site.clone()
    }
}
impl MavenRepositoryExt for MavenProxy {}
