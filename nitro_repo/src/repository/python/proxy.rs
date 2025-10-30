use std::sync::Arc;

use http::StatusCode;
use nr_core::{
    database::entities::repository::DBRepository,
    repository::{Visibility, config::RepositoryConfigType, proxy_url::ProxyURL},
    storage::StoragePath,
};
use nr_storage::{DynStorage, FileContent, Storage};
use parking_lot::RwLock;
use tracing::{debug, warn};
use uuid::Uuid;

use super::{
    PythonRepositoryError,
    configs::{PythonProxyConfig, PythonProxyRoute, PythonRepositoryConfigType},
};
use crate::{
    app::NitroRepo,
    repository::{
        RepoResponse, Repository, RepositoryFactoryError, RepositoryRequest,
        utils::can_read_repository,
    },
    utils::ResponseBuilder,
};

#[derive(Debug)]
pub struct PythonProxyInner {
    pub id: Uuid,
    pub name: String,
    pub visibility: RwLock<Visibility>,
    pub storage: DynStorage,
    pub site: NitroRepo,
    pub routes: Vec<PythonProxyRoute>,
    pub client: reqwest::Client,
    pub active: bool,
}

#[derive(Debug, Clone)]
pub struct PythonProxy(pub Arc<PythonProxyInner>);

impl PythonProxy {
    pub async fn load(
        site: NitroRepo,
        storage: DynStorage,
        repository: DBRepository,
        config: PythonProxyConfig,
    ) -> Result<Self, RepositoryFactoryError> {
        let client = reqwest::Client::builder()
            .user_agent("Nitro Repo Python Proxy")
            .build()
            .map_err(|err| {
                RepositoryFactoryError::InvalidConfig(
                    PythonRepositoryConfigType::get_type_static(),
                    err.to_string(),
                )
            })?;
        Ok(Self(Arc::new(PythonProxyInner {
            id: repository.id,
            name: repository.name.to_string(),
            visibility: RwLock::new(repository.visibility),
            storage,
            site,
            routes: config.routes,
            client,
            active: repository.active,
        })))
    }

    fn id(&self) -> Uuid {
        self.0.id
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
    fn routes(&self) -> &[PythonProxyRoute] {
        &self.0.routes
    }

    async fn download_and_cache(
        &self,
        path: &StoragePath,
        query: Option<&str>,
    ) -> Result<bool, PythonRepositoryError> {
        if path.is_directory() {
            return Ok(false);
        }
        for route in self.routes() {
            let Some(url) = build_url(&route.url, path.clone(), query) else {
                continue;
            };
            match self.0.client.get(url.clone()).send().await {
                Ok(response) => {
                    if response.status().is_success() {
                        let bytes = response.bytes().await.map_err(|err| {
                            PythonRepositoryError::Other(Box::new(
                                crate::error::OtherInternalError::new(err),
                            ))
                        })?;
                        self.storage()
                            .save_file(self.id(), FileContent::Bytes(bytes.clone()), path)
                            .await?;
                        debug!(%url, "Cached python proxy resource");
                        return Ok(true);
                    }
                    if response.status().is_client_error() {
                        continue;
                    }
                    warn!(
                        status = ?response.status(),
                        %url,
                        "Upstream returned error while proxying python resource"
                    );
                }
                Err(err) => {
                    warn!(%url, error = %err, "Failed to reach python proxy upstream");
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
    ) -> Result<Option<RepoResponse>, PythonRepositoryError> {
        use http::header::{CONTENT_LENGTH, CONTENT_TYPE};

        for route in self.routes() {
            let Some(url) = build_url(&route.url, path.clone(), query) else {
                continue;
            };
            if !include_body {
                match self.0.client.head(url.clone()).send().await {
                    Ok(response) if response.status().is_success() => {
                        return Ok(Some(build_head_response(response)));
                    }
                    Ok(response) if response.status() == StatusCode::METHOD_NOT_ALLOWED => {
                        // fall through to GET below
                    }
                    Ok(response) => {
                        if response.status().is_client_error() {
                            continue;
                        }
                        warn!(
                            status = ?response.status(),
                            %url,
                            "Upstream head error for python proxy"
                        );
                        continue;
                    }
                    Err(err) => {
                        warn!(%url, error = %err, "Failed head request for python proxy");
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
                                PythonRepositoryError::Other(Box::new(
                                    crate::error::OtherInternalError::new(err),
                                ))
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
                        "Upstream error for python proxy"
                    );
                }
                Err(err) => {
                    warn!(%url, error = %err, "Failed to reach python proxy upstream");
                }
            }
        }
        Ok(None)
    }
}

impl Repository for PythonProxy {
    type Error = PythonRepositoryError;

    fn get_storage(&self) -> DynStorage {
        self.storage()
    }

    fn get_type(&self) -> &'static str {
        "python"
    }

    fn full_type(&self) -> &'static str {
        "python/proxy"
    }

    fn config_types(&self) -> Vec<&str> {
        vec![PythonRepositoryConfigType::get_type_static()]
    }

    fn name(&self) -> String {
        self.0.name.clone()
    }

    fn id(&self) -> Uuid {
        self.id()
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

    fn handle_get(
        &self,
        request: RepositoryRequest,
    ) -> impl std::future::Future<Output = Result<RepoResponse, Self::Error>> + Send {
        let this = self.clone();
        async move {
            let query = request.parts.uri.query().map(|q| q.to_string());
            if !can_read_repository(
                &request.authentication,
                this.visibility(),
                this.id(),
                this.site().as_ref(),
            )
            .await?
            {
                return Ok(RepoResponse::unauthorized());
            }

            let path = request.path;

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

            if this.download_and_cache(&path, query.as_deref()).await? {
                if let Some(file) = this.storage().open_file(this.id(), &path).await? {
                    return Ok(file.into());
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
            if !can_read_repository(
                &request.authentication,
                this.visibility(),
                this.id(),
                this.site().as_ref(),
            )
            .await?
            {
                return Ok(RepoResponse::unauthorized());
            }

            let path = request.path;

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

            if this.download_and_cache(&path, query.as_deref()).await? {
                if let Some(meta) = this
                    .storage()
                    .get_file_information(this.id(), &path)
                    .await?
                {
                    return Ok(meta.into());
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
