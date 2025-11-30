use std::sync::{Arc, LazyLock};

use chrono::Utc;
use http::StatusCode;
use nr_core::{
    database::entities::repository::{DBRepository, DBRepositoryConfig},
    repository::{
        Visibility,
        config::RepositoryConfigType,
        project::{ProxyArtifactKey, ProxyArtifactMeta},
        proxy_url::ProxyURL,
    },
    storage::StoragePath,
};
use nr_storage::{DynStorage, FileContent, Storage};
use parking_lot::{RwLock, RwLockReadGuard};
use tracing::{debug, warn};
use url::Url;
use uuid::Uuid;

use super::{
    NPMRegistryError,
    configs::{NPMRegistryConfigType, NpmProxyConfig, NpmProxyRoute},
};
use crate::{
    app::NitroRepo,
    repository::{
        RepoResponse, Repository, RepositoryAuthConfigType, RepositoryFactoryError,
        RepositoryRequest,
        proxy_indexing::{DatabaseProxyIndexer, ProxyIndexing, ProxyIndexingError},
        utils::can_read_repository_with_auth,
    },
    utils::ResponseBuilder,
};

pub struct NpmProxyInner {
    pub id: Uuid,
    pub name: String,
    pub visibility: RwLock<Visibility>,
    pub storage: DynStorage,
    pub site: NitroRepo,
    pub routes: RwLock<Vec<NpmProxyRoute>>,
    pub client: reqwest::Client,
    pub active: bool,
    pub indexer: Arc<dyn ProxyIndexing>,
}

#[derive(Debug, Clone)]
pub struct NpmProxyRegistry(pub Arc<NpmProxyInner>);

impl std::fmt::Debug for NpmProxyInner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NpmProxyInner")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("visibility", &self.visibility.read())
            .field("active", &self.active)
            .finish()
    }
}

static DEFAULT_ROUTE: LazyLock<NpmProxyRoute> = LazyLock::new(|| NpmProxyRoute {
    url: ProxyURL::try_from(String::from("https://registry.npmjs.org"))
        .unwrap_or_else(|_| panic!("valid npm default route")),
    name: Some("npmjs".to_string()),
});

fn normalize_routes(routes: Vec<NpmProxyRoute>) -> Vec<NpmProxyRoute> {
    if routes.is_empty() {
        vec![DEFAULT_ROUTE.clone()]
    } else {
        routes
    }
}

impl NpmProxyRegistry {
    pub async fn load(
        site: NitroRepo,
        storage: DynStorage,
        repository: DBRepository,
        config: NpmProxyConfig,
    ) -> Result<Self, RepositoryFactoryError> {
        let client = reqwest::Client::builder()
            .user_agent("Nitro Repo NPM Proxy")
            .build()
            .map_err(|err| {
                RepositoryFactoryError::InvalidConfig(
                    NPMRegistryConfigType::get_type_static(),
                    err.to_string(),
                )
            })?;
        let indexer: Arc<dyn ProxyIndexing> =
            Arc::new(DatabaseProxyIndexer::new(site.clone(), repository.id));
        Ok(Self(Arc::new(NpmProxyInner {
            id: repository.id,
            name: repository.name.to_string(),
            visibility: RwLock::new(repository.visibility),
            storage,
            site: site.clone(),
            routes: RwLock::new(normalize_routes(config.routes)),
            client,
            active: repository.active,
            indexer,
        })))
    }

    fn storage(&self) -> DynStorage {
        self.0.storage.clone()
    }
    fn site(&self) -> NitroRepo {
        self.0.site.clone()
    }
    fn visibility(&self) -> Visibility {
        *self.0.visibility.read()
    }
    fn routes(&self) -> RwLockReadGuard<'_, Vec<NpmProxyRoute>> {
        self.0.routes.read()
    }

    fn indexer(&self) -> &Arc<dyn ProxyIndexing> {
        &self.0.indexer
    }

    pub async fn handle_external_eviction(
        &self,
        path: &StoragePath,
    ) -> Result<(), NPMRegistryError> {
        let canonical = cache_path_for_npm_proxy(path).unwrap_or_else(|| path.clone());
        evict_npm_proxy_cache_entry(self.indexer().as_ref(), &canonical).await?;
        Ok(())
    }

    async fn download_and_cache(
        &self,
        path: &StoragePath,
        query: Option<&str>,
    ) -> Result<bool, NPMRegistryError> {
        if path.is_directory() {
            return Ok(false);
        }
        let routes = {
            let guard = self.routes();
            guard.clone()
        };
        for route in routes.iter() {
            let Some(url) = build_url(&route.url, path.clone(), query) else {
                continue;
            };
            match self.0.client.get(url.clone()).send().await {
                Ok(response) => {
                    if response.status().is_success() {
                        let bytes =
                            response
                                .bytes()
                                .await
                                .map_err(|err| NPMRegistryError::ProxyFetch {
                                    url: url.to_string(),
                                    error: err.to_string(),
                                })?;
                        match self
                            .storage()
                            .save_file(self.0.id, FileContent::Bytes(bytes.clone()), path)
                            .await
                        {
                            Ok(_) => {}
                            Err(nr_storage::StorageError::PathCollision(_)) => {
                                debug!(%url, "Skipping cache write for existing npm metadata file");
                            }
                            Err(other) => return Err(other.into()),
                        }

                        let canonical_path =
                            if let Some(cache_path) = cache_path_for_npm_proxy(path) {
                                if cache_path != *path {
                                    if let Err(err) = self
                                        .storage()
                                        .save_file(
                                            self.0.id,
                                            FileContent::Bytes(bytes.clone()),
                                            &cache_path,
                                        )
                                        .await
                                    {
                                        match err {
                                            nr_storage::StorageError::PathCollision(_) => {
                                                debug!(
                                                    ?cache_path,
                                                    "Cache file already exists, skipping overwrite"
                                                );
                                            }
                                            other => {
                                                warn!(
                                                    ?other,
                                                    ?cache_path,
                                                    "Failed to persist npm proxy cache entry"
                                                );
                                                return Err(other.into());
                                            }
                                        }
                                    }
                                }
                                cache_path
                            } else {
                                path.clone()
                            };

                        record_npm_proxy_cache_hit(
                            self.indexer().as_ref(),
                            &canonical_path,
                            bytes.len() as u64,
                            Some(&url),
                        )
                        .await?;
                        debug!(%url, "Cached npm proxy resource");
                        return Ok(true);
                    }
                    if response.status().is_client_error() {
                        continue;
                    }
                    warn!(
                        status = ?response.status(),
                        %url,
                        "Upstream proxy error for npm"
                    );
                }
                Err(err) => {
                    warn!(%url, error = %err, "Failed to reach npm proxy upstream");
                }
            }
        }
        Ok(false)
    }

    async fn proxy_passthrough(
        &self,
        path: &StoragePath,
        query: Option<&str>,
        include_body: bool,
    ) -> Result<Option<RepoResponse>, NPMRegistryError> {
        use http::header::{CONTENT_LENGTH, CONTENT_TYPE};

        let routes = {
            let guard = self.routes();
            guard.clone()
        };
        for route in routes.iter() {
            let Some(url) = build_url(&route.url, path.clone(), query) else {
                continue;
            };
            if !include_body {
                match self.0.client.head(url.clone()).send().await {
                    Ok(response) if response.status().is_success() => {
                        return Ok(Some(build_head_response(response)));
                    }
                    Ok(response) if response.status() == StatusCode::METHOD_NOT_ALLOWED => {
                        // fall through to GET request below
                    }
                    Ok(response) => {
                        if response.status().is_client_error() {
                            continue;
                        }
                        warn!(
                            status = ?response.status(),
                            %url,
                            "Upstream head error for npm proxy"
                        );
                        continue;
                    }
                    Err(err) => {
                        warn!(%url, error = %err, "Failed head request for npm proxy");
                        continue;
                    }
                }
            }

            match self.0.client.get(url.clone()).send().await {
                Ok(response) => {
                    if response.status().is_success() {
                        if include_body {
                            let status = response.status();
                            let headers = response.headers().clone();
                            let body = response.bytes().await.map_err(|err| {
                                NPMRegistryError::ProxyFetch {
                                    url: url.to_string(),
                                    error: err.to_string(),
                                }
                            })?;
                            let mut builder = ResponseBuilder::default().status(status);
                            if let Some(content_type) = headers.get(CONTENT_TYPE) {
                                builder = builder.header(CONTENT_TYPE, content_type.clone());
                            }
                            builder = builder.header(CONTENT_LENGTH, body.len().to_string());
                            return Ok(Some(RepoResponse::Other(builder.body(body.to_vec()))));
                        } else {
                            return Ok(Some(build_head_response(response)));
                        }
                    }
                    if response.status().is_client_error() {
                        continue;
                    }
                    warn!(
                        status = ?response.status(),
                        %url,
                        "Upstream error for npm proxy"
                    );
                }
                Err(err) => {
                    warn!(%url, error = %err, "Failed to reach npm proxy upstream");
                }
            }
        }
        Ok(None)
    }
}

impl Repository for NpmProxyRegistry {
    type Error = NPMRegistryError;

    fn get_storage(&self) -> DynStorage {
        self.storage()
    }

    fn get_type(&self) -> &'static str {
        "npm"
    }

    fn full_type(&self) -> &'static str {
        "npm/proxy"
    }

    fn config_types(&self) -> Vec<&str> {
        vec![
            NPMRegistryConfigType::get_type_static(),
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
        self.visibility()
    }

    fn is_active(&self) -> bool {
        self.0.active
    }

    fn site(&self) -> NitroRepo {
        self.site()
    }

    async fn reload(&self) -> Result<(), RepositoryFactoryError> {
        let config = DBRepositoryConfig::<NpmProxyConfig>::get_config(
            self.0.id,
            NPMRegistryConfigType::get_type_static(),
            self.site().as_ref(),
        )
        .await?
        .map(|cfg| cfg.value.0)
        .unwrap_or_default();
        let mut routes = self.0.routes.write();
        *routes = normalize_routes(config.routes);
        Ok(())
    }

    fn handle_get(
        &self,
        request: RepositoryRequest,
    ) -> impl std::future::Future<Output = Result<RepoResponse, Self::Error>> + Send {
        let this = self.clone();
        async move {
            let query = request.parts.uri.query().map(|q| q.to_string());
            if !can_read_repository_with_auth(
                &request.authentication,
                this.visibility(),
                this.id(),
                this.site().as_ref(),
                &request.auth_config,
            )
            .await?
            {
                return Ok(RepoResponse::unauthorized());
            }

            let path = request.path;

            let cache_path = cache_path_for_npm_proxy(&path);

            if path.is_directory() {
                if let Some(response) = this
                    .proxy_passthrough(&path, query.as_deref(), true)
                    .await?
                {
                    return Ok(response);
                }
                return Ok(RepoResponse::basic_text_response(
                    StatusCode::NOT_FOUND,
                    "Directory not found",
                ));
            }

            if let Some(file) = this.storage().open_file(this.id(), &path).await? {
                return Ok(file.into());
            }

            if let Some(cache_path) = &cache_path {
                if let Some(file) = this.storage().open_file(this.id(), cache_path).await? {
                    return Ok(file.into());
                }
            }

            if this.download_and_cache(&path, query.as_deref()).await? {
                if let Some(file) = this.storage().open_file(this.id(), &path).await? {
                    return Ok(file.into());
                }
                if let Some(cache_path) = &cache_path {
                    if let Some(file) = this.storage().open_file(this.id(), cache_path).await? {
                        return Ok(file.into());
                    }
                }
            }

            if let Some(response) = this
                .proxy_passthrough(&path, query.as_deref(), true)
                .await?
            {
                return Ok(response);
            }

            Ok(RepoResponse::basic_text_response(
                StatusCode::NOT_FOUND,
                "File not found",
            ))
        }
    }

    fn handle_head(
        &self,
        request: RepositoryRequest,
    ) -> impl std::future::Future<Output = Result<RepoResponse, Self::Error>> + Send {
        let this = self.clone();
        async move {
            let query = request.parts.uri.query().map(|q| q.to_string());
            if !can_read_repository_with_auth(
                &request.authentication,
                this.visibility(),
                this.id(),
                this.site().as_ref(),
                &request.auth_config,
            )
            .await?
            {
                return Ok(RepoResponse::unauthorized());
            }

            let path = request.path;

            let cache_path = cache_path_for_npm_proxy(&path);

            if path.is_directory() {
                if let Some(response) = this
                    .proxy_passthrough(&path, query.as_deref(), false)
                    .await?
                {
                    return Ok(response);
                }
                return Ok(RepoResponse::basic_text_response(
                    StatusCode::NOT_FOUND,
                    "Directory not found",
                ));
            }

            if let Some(meta) = this
                .storage()
                .get_file_information(this.id(), &path)
                .await?
            {
                return Ok(meta.into());
            }

            if let Some(cache_path) = &cache_path {
                if let Some(meta) = this
                    .storage()
                    .get_file_information(this.id(), cache_path)
                    .await?
                {
                    return Ok(meta.into());
                }
            }

            if this.download_and_cache(&path, query.as_deref()).await? {
                if let Some(meta) = this
                    .storage()
                    .get_file_information(this.id(), &path)
                    .await?
                {
                    return Ok(meta.into());
                }
                if let Some(cache_path) = &cache_path {
                    if let Some(meta) = this
                        .storage()
                        .get_file_information(this.id(), cache_path)
                        .await?
                    {
                        return Ok(meta.into());
                    }
                }
            }

            if let Some(response) = this
                .proxy_passthrough(&path, query.as_deref(), false)
                .await?
            {
                return Ok(response);
            }

            Ok(RepoResponse::basic_text_response(
                StatusCode::NOT_FOUND,
                "File not found",
            ))
        }
    }
}

fn cache_path_for_npm_proxy(path: &StoragePath) -> Option<StoragePath> {
    if path.is_directory() {
        return None;
    }
    let components: Vec<String> = path
        .clone()
        .into_iter()
        .map(|component| component.to_string())
        .collect();
    if components.is_empty() {
        return None;
    }
    if matches!(components.first().map(String::as_str), Some("packages")) {
        return Some(path.clone());
    }
    if components.len() < 3 {
        return None;
    }
    if components.get(components.len() - 2).map(String::as_str) != Some("-") {
        return None;
    }
    let file_name = components.last()?.clone();
    let package_components = &components[..components.len() - 2];
    if package_components.is_empty() {
        return None;
    }
    let package_path = package_components.join("/");
    Some(StoragePath::from(format!(
        "packages/{}/{}",
        package_path, file_name
    )))
}

#[cfg(test)]
mod tests;

fn npm_package_components(path: &StoragePath) -> Option<(String, String)> {
    let components: Vec<String> = path.clone().into_iter().map(String::from).collect();
    if components.len() < 3 || components.first().map(String::as_str) != Some("packages") {
        return None;
    }
    let file_name = components.last()?.clone();
    let package_components = &components[1..components.len() - 1];
    if package_components.is_empty() {
        return None;
    }
    let package_name = package_components.join("/");
    Some((package_name, file_name))
}

fn npm_version_from_filename(file_name: &str) -> Option<String> {
    let stem = file_name.strip_suffix(".tgz")?;
    let (_name_part, version_part) = stem.rsplit_once('-')?;
    if version_part.is_empty() {
        return None;
    }
    if !version_part
        .chars()
        .next()
        .map(|c| c.is_ascii_digit())
        .unwrap_or(false)
    {
        return None;
    }
    Some(version_part.to_string())
}

pub(super) fn npm_proxy_meta_from_cache_path(
    path: &StoragePath,
    size: u64,
    upstream_url: Option<&Url>,
) -> Option<ProxyArtifactMeta> {
    let (package_name, file_name) = npm_package_components(path)?;
    let version = npm_version_from_filename(&file_name)?;
    let mut builder =
        ProxyArtifactMeta::builder(package_name.clone(), package_name.clone(), path.to_string())
            .version(version)
            .size(size)
            .fetched_at(Utc::now());
    if let Some(url) = upstream_url {
        builder = builder.upstream_url(url.to_string());
    }
    Some(builder.build())
}

pub(super) fn npm_proxy_key_from_cache_path(path: &StoragePath) -> Option<ProxyArtifactKey> {
    let (package_name, file_name) = npm_package_components(path)?;
    let version = npm_version_from_filename(&file_name)?;
    Some(ProxyArtifactKey {
        package_key: package_name,
        version: Some(version),
        cache_path: Some(path.to_string()),
    })
}

pub(super) async fn record_npm_proxy_cache_hit(
    indexer: &dyn ProxyIndexing,
    path: &StoragePath,
    size: u64,
    upstream_url: Option<&Url>,
) -> Result<(), ProxyIndexingError> {
    if let Some(meta) = npm_proxy_meta_from_cache_path(path, size, upstream_url) {
        indexer.record_cached_artifact(meta).await?;
    }
    Ok(())
}

pub(super) async fn evict_npm_proxy_cache_entry(
    indexer: &dyn ProxyIndexing,
    path: &StoragePath,
) -> Result<(), ProxyIndexingError> {
    if let Some(key) = npm_proxy_key_from_cache_path(path) {
        indexer.evict_cached_artifact(key).await?;
    }
    Ok(())
}

fn build_head_response(response: reqwest::Response) -> RepoResponse {
    use http::header::{CONTENT_LENGTH, CONTENT_TYPE};

    let status = response.status();
    let headers = response.headers().clone();
    let mut builder = ResponseBuilder::default().status(status);
    if let Some(content_type) = headers.get(CONTENT_TYPE) {
        builder = builder.header(CONTENT_TYPE, content_type.clone());
    }
    if let Some(content_length) = headers.get(CONTENT_LENGTH) {
        builder = builder.header(CONTENT_LENGTH, content_length.clone());
    }
    RepoResponse::Other(builder.empty())
}

fn build_url(base: &ProxyURL, path: StoragePath, query: Option<&str>) -> Option<url::Url> {
    match base.add_storage_path(path) {
        Ok(mut url) => {
            url.set_query(query);
            Some(url)
        }
        Err(err) => {
            warn!(error = %err, "Invalid proxy URL");
            None
        }
    }
}
