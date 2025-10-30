use std::sync::{Arc, LazyLock};

use http::StatusCode;
use nr_core::{
    database::entities::repository::DBRepository,
    repository::{Visibility, config::RepositoryConfigType, proxy_url::ProxyURL},
    storage::StoragePath,
};
use nr_storage::{DynStorage, FileContent, Storage};
use parking_lot::RwLock;
use regex::Regex;
use serde_json::Value;
use tracing::{debug, warn};
use url::Url;
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
    pub storage_name: String,
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
        let storage_name = storage.storage_config().storage_config.storage_name.clone();
        Ok(Self(Arc::new(PythonProxyInner {
            id: repository.id,
            name: repository.name.to_string(),
            visibility: RwLock::new(repository.visibility),
            storage,
            site,
            routes: config.routes,
            client,
            active: repository.active,
            storage_name,
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
    fn storage_name(&self) -> &str {
        &self.0.storage_name
    }
    fn repository_slug(&self) -> &str {
        &self.0.name
    }
    fn base_repository_path(&self) -> String {
        format!(
            "/repositories/{}/{}",
            self.storage_name(),
            self.repository_slug()
        )
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
        base_path: &str,
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
                            let mut body_vec = body.to_vec();
                            if let Some(content_type) = headers.get(CONTENT_TYPE) {
                                if let Ok(content_type) = content_type.to_str() {
                                    if content_type.starts_with("text/html") {
                                        if let Some(rewritten) =
                                            rewrite_simple_html(&body_vec, base_path, &url)
                                        {
                                            body_vec = rewritten;
                                        }
                                    } else if content_type.contains("application/vnd.pypi.simple")
                                        || content_type.contains("application/json")
                                    {
                                        if let Some(rewritten) =
                                            rewrite_simple_json(&body_vec, base_path, &url)
                                        {
                                            body_vec = rewritten;
                                        }
                                    }
                                }
                            }
                            builder = builder.header(CONTENT_LENGTH, body_vec.len().to_string());
                            return Ok(Some(RepoResponse::Other(builder.body(body_vec))));
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
            let base_path = this.base_repository_path();

            if path.is_directory() {
                if let Some(response) = this
                    .proxy_passthrough(&path, &base_path, query.as_deref(), true)
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
                .proxy_passthrough(&path, &base_path, query.as_deref(), true)
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
            let base_path = this.base_repository_path();

            if path.is_directory() {
                if let Some(response) = this
                    .proxy_passthrough(&path, &base_path, query.as_deref(), false)
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
                .proxy_passthrough(&path, &base_path, query.as_deref(), false)
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

fn rewrite_simple_html(body: &[u8], base_path: &str, upstream_base: &Url) -> Option<Vec<u8>> {
    let html = std::str::from_utf8(body).ok()?;
    let normalized_base = normalize_base_path(base_path);
    let rewritten = HREF_REGEX.replace_all(html, |caps: &regex::Captures<'_>| {
        let original = &caps[1];
        match resolve_upstream_link(original, upstream_base) {
            Some(resolved) => format!("href=\"{}\"", build_local_url(&resolved, &normalized_base)),
            None => caps[0].to_string(),
        }
    });
    let rewritten = replace_absolute_urls(&rewritten, &normalized_base, upstream_base);
    Some(rewritten.into_bytes())
}

fn rewrite_simple_json(body: &[u8], base_path: &str, upstream_base: &Url) -> Option<Vec<u8>> {
    let mut value: Value = serde_json::from_slice(body).ok()?;
    let normalized_base = normalize_base_path(base_path);
    rewrite_json_value(&mut value, &normalized_base, upstream_base);
    serde_json::to_vec(&value).ok()
}

fn rewrite_json_value(value: &mut Value, normalized_base: &str, upstream_base: &Url) {
    match value {
        Value::String(s) => {
            if let Some(resolved) = resolve_upstream_link(s, upstream_base) {
                *s = build_local_url(&resolved, normalized_base);
            } else if let Some(rewritten) =
                rewrite_known_host_url(s, normalized_base, upstream_base)
            {
                *s = rewritten;
            }
        }
        Value::Array(items) => {
            for item in items {
                rewrite_json_value(item, normalized_base, upstream_base);
            }
        }
        Value::Object(map) => {
            for value in map.values_mut() {
                rewrite_json_value(value, normalized_base, upstream_base);
            }
        }
        _ => {}
    }
}

fn normalize_base_path(base_path: &str) -> String {
    if base_path.ends_with('/') {
        base_path.to_string()
    } else {
        format!("{}/", base_path)
    }
}

fn resolve_upstream_link(original: &str, upstream_base: &Url) -> Option<Url> {
    if let Ok(parsed) = Url::parse(original) {
        let host = parsed.host_str()?;
        if is_allowed_host(host, upstream_base) {
            return Some(parsed);
        }
        return None;
    }
    upstream_base.join(original).ok()
}

fn rewrite_known_host_url(
    original: &str,
    normalized_base: &str,
    upstream_base: &Url,
) -> Option<String> {
    let parsed = Url::parse(original).ok()?;
    let host = parsed.host_str()?;
    if !is_allowed_host(host, upstream_base) {
        return None;
    }
    Some(build_local_url(&parsed, normalized_base))
}

fn build_local_url(resolved: &Url, normalized_base: &str) -> String {
    let mut path = normalized_base.to_string();
    path.push_str(resolved.path().trim_start_matches('/'));
    if let Some(query) = resolved.query() {
        path.push('?');
        path.push_str(query);
    }
    if let Some(fragment) = resolved.fragment() {
        path.push('#');
        path.push_str(fragment);
    }
    path
}

fn replace_absolute_urls(input: &str, normalized_base: &str, upstream_base: &Url) -> String {
    let mut output = input.to_string();
    output = output
        .replace("https://files.pythonhosted.org/", normalized_base)
        .replace("http://files.pythonhosted.org/", normalized_base);

    if let Some(host) = upstream_base.host_str() {
        let https = format!("https://{host}/");
        let http = format!("http://{host}/");
        output = output.replace(&https, normalized_base);
        output = output.replace(&http, normalized_base);
    }
    output
}

fn is_allowed_host(host: &str, upstream_base: &Url) -> bool {
    let upstream_host = upstream_base.host_str().unwrap_or_default();
    host == upstream_host || host.ends_with("pythonhosted.org")
}

fn build_url(base: &ProxyURL, path: StoragePath, query: Option<&str>) -> Option<Url> {
    let mut upstream = Url::parse(base.as_ref()).ok()?;
    let path_string = path.to_string();
    let mut segments: Vec<&str> = path_string
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect();
    if segments.first().map(|s| *s) == Some("simple")
        && segments.get(1).map(|s| *s) == Some("packages")
    {
        segments.remove(0);
    }
    if segments.first().map(|s| *s) == Some("packages") {
        if upstream.host_str() == Some("pypi.org") {
            if let Err(err) = upstream.set_host(Some("files.pythonhosted.org")) {
                warn!(?err, "Failed to set upstream host for python proxy");
                return None;
            }
            upstream.set_path("");
        }
    }
    {
        let mut path_segments = match upstream.path_segments_mut() {
            Ok(segments_mut) => segments_mut,
            Err(_) => {
                warn!("Upstream URL cannot be a base");
                return None;
            }
        };
        path_segments.clear();
        for segment in &segments {
            path_segments.push(segment);
        }
    }
    if path_string.ends_with('/') {
        let mut current = upstream.path().to_string();
        if !current.ends_with('/') {
            current.push('/');
            upstream.set_path(&current);
        }
    }
    upstream.set_query(query);
    Some(upstream)
}
static HREF_REGEX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"href="([^"]+)""#).unwrap());
