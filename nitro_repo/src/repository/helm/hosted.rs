use std::{
    collections::BTreeMap,
    future::Future,
    sync::Arc,
    time::{Duration, Instant},
};

use axum::http::{
    StatusCode,
    header::{CONTENT_TYPE, HOST},
};
use bytes::Bytes;
use chrono::{DateTime, Utc};
use nr_core::{
    database::entities::{
        project::{
            DBProject, NewProject, ProjectDBType,
            versions::{DBProjectVersion, NewVersion, UpdateProjectVersion},
        },
        repository::DBRepository,
    },
    repository::{
        Visibility,
        config::RepositoryConfigType,
        project::{Author, ReleaseType, VersionData},
    },
    storage::StoragePath,
    user::permissions::RepositoryActions,
};
use nr_storage::{DynStorage, FileContent, Storage};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use serde_json::{Map as JsonMap, Value as JsonValue};
use sha2::{Digest, Sha256};
use sqlx::Row;
use uuid::Uuid;

use super::{
    HelmRepositoryError,
    chart::{
        ChartMaintainer, ChartProvenanceState, ChartValidationOptions, ParsedChartArchive,
        parse_chart_archive,
    },
    configs::{HelmRepositoryConfig, HelmRepositoryConfigType, HelmRepositoryMode},
    index::{IndexEntry, IndexRenderConfig, IndexUrlMode, render_index_yaml},
    oci::{HelmOciManifestInput, build_helm_manifest},
};
use crate::{
    app::NitroRepo,
    repository::docker::{DockerError, handlers as docker_handlers, hosted::DockerHosted},
    repository::{
        RepoResponse, Repository, RepositoryAuthConfig, RepositoryAuthConfigType,
        RepositoryAuthentication, RepositoryFactoryError, RepositoryRequest,
    },
    utils::ResponseBuilder,
};

#[derive(Debug)]
pub struct HelmRepositoryInner {
    pub id: Uuid,
    pub name: String,
    pub visibility: RwLock<Visibility>,
    pub repository: DBRepository,
    pub config: HelmRepositoryConfig,
    pub auth_config: RepositoryAuthConfig,
    pub storage: DynStorage,
    pub storage_name: String,
    pub site: NitroRepo,
    index_cache: RwLock<Option<CachedIndex>>,
}

#[derive(Debug, Clone)]
pub struct HelmHosted(pub Arc<HelmRepositoryInner>);

#[derive(Debug)]
struct CachedIndex {
    content: String,
    generated_at: Instant,
}

impl CachedIndex {
    fn is_expired(&self, ttl: Option<Duration>) -> bool {
        match ttl {
            Some(ttl) => self.generated_at.elapsed() >= ttl,
            None => true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ChartArtifactPath {
    name: String,
    version: String,
    is_provenance: bool,
    alias: bool,
}

impl ChartArtifactPath {
    fn canonical_storage_path(&self) -> StoragePath {
        let mut path = StoragePath::from("charts/");
        path.push_mut(&self.name);
        path.push_mut(&self.canonical_file_name());
        path
    }

    fn canonical_file_name(&self) -> String {
        if self.is_provenance {
            format!("{}-{}.tgz.prov", self.name, self.version)
        } else {
            format!("{}-{}.tgz", self.name, self.version)
        }
    }
}

fn parse_chart_artifact(path: &StoragePath) -> Option<ChartArtifactPath> {
    if path.is_directory() {
        return None;
    }
    let path_str = path.to_string();
    if path_str.is_empty() {
        return None;
    }

    let mut segments = path_str.split('/').collect::<Vec<_>>();
    if segments.is_empty() {
        return None;
    }

    let file = segments.pop().unwrap();
    let alias = if segments.is_empty() {
        false
    } else if segments.len() == 1 && segments[0] == "charts" {
        true
    } else {
        return None;
    };

    let (is_provenance, trimmed) = if let Some(rest) = file.strip_suffix(".tgz.prov") {
        (true, rest)
    } else if let Some(rest) = file.strip_suffix(".tgz") {
        (false, rest)
    } else {
        return None;
    };

    let (name, version) = trimmed.rsplit_once('-')?;
    if name.is_empty() || version.is_empty() {
        return None;
    }

    Some(ChartArtifactPath {
        name: name.to_string(),
        version: version.to_string(),
        is_provenance,
        alias,
    })
}

#[derive(Debug, Clone)]
struct HybridOciArtifacts {
    repository: String,
    config_bytes: Vec<u8>,
    config_digest: String,
    manifest_bytes: Vec<u8>,
    manifest_digest: String,
}

fn build_oci_config(metadata: &super::chart::HelmChartMetadata) -> JsonValue {
    let mut map = JsonMap::new();
    map.insert("name".into(), JsonValue::String(metadata.name.clone()));
    map.insert(
        "version".into(),
        JsonValue::String(metadata.version.to_string()),
    );
    if let Some(description) = &metadata.description {
        map.insert("description".into(), JsonValue::String(description.clone()));
    }
    if let Some(home) = &metadata.home {
        map.insert("home".into(), JsonValue::String(home.clone()));
    }
    if let Some(icon) = &metadata.icon {
        map.insert("icon".into(), JsonValue::String(icon.clone()));
    }
    if let Some(app_version) = &metadata.app_version {
        map.insert("appVersion".into(), JsonValue::String(app_version.clone()));
    }
    if let Some(kube_version) = &metadata.kube_version {
        map.insert(
            "kubeVersion".into(),
            JsonValue::String(kube_version.clone()),
        );
    }
    if let Some(engine) = &metadata.engine {
        map.insert("engine".into(), JsonValue::String(engine.clone()));
    }
    if let Some(tiller_version) = &metadata.tiller_version {
        map.insert(
            "tillerVersion".into(),
            JsonValue::String(tiller_version.clone()),
        );
    }
    let keywords: Vec<JsonValue> = metadata
        .keywords
        .iter()
        .cloned()
        .map(JsonValue::String)
        .collect();
    if !keywords.is_empty() {
        map.insert("keywords".into(), JsonValue::Array(keywords));
    }
    let sources: Vec<JsonValue> = metadata
        .sources
        .iter()
        .cloned()
        .map(JsonValue::String)
        .collect();
    if !sources.is_empty() {
        map.insert("sources".into(), JsonValue::Array(sources));
    }
    if let Ok(dependencies) = serde_json::to_value(&metadata.dependencies) {
        if dependencies.is_array() && !dependencies.as_array().unwrap().is_empty() {
            map.insert("dependencies".into(), dependencies);
        }
    }
    if !metadata.maintainers.is_empty() {
        let maintainers = metadata
            .maintainers
            .iter()
            .map(|maintainer| {
                let mut maintainer_map = JsonMap::new();
                maintainer_map.insert("name".into(), JsonValue::String(maintainer.name.clone()));
                if let Some(email) = &maintainer.email {
                    maintainer_map.insert("email".into(), JsonValue::String(email.clone()));
                }
                if let Some(url) = &maintainer.url {
                    maintainer_map.insert("url".into(), JsonValue::String(url.clone()));
                }
                JsonValue::Object(maintainer_map)
            })
            .collect();
        map.insert("maintainers".into(), JsonValue::Array(maintainers));
    }
    if !metadata.annotations.is_empty() {
        let mut annotations = JsonMap::new();
        for (key, value) in &metadata.annotations {
            annotations.insert(key.clone(), JsonValue::String(value.clone()));
        }
        map.insert("annotations".into(), JsonValue::Object(annotations));
    }

    map.insert(
        "apiVersion".into(),
        JsonValue::String(metadata.api_version.as_str().to_string()),
    );
    map.insert(
        "type".into(),
        JsonValue::String(metadata.chart_type.as_str().to_string()),
    );

    JsonValue::Object(map)
}

fn sha256_digest(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    format!("sha256:{:x}", digest)
}

impl HelmHosted {
    pub async fn load(
        site: NitroRepo,
        storage: DynStorage,
        repository: DBRepository,
        config: HelmRepositoryConfig,
        auth_config: RepositoryAuthConfig,
    ) -> Result<Self, RepositoryFactoryError> {
        let storage_name = storage.storage_config().storage_config.storage_name.clone();

        Ok(Self(Arc::new(HelmRepositoryInner {
            id: repository.id,
            name: repository.name.to_string(),
            visibility: RwLock::new(repository.visibility),
            repository,
            config,
            auth_config,
            storage,
            storage_name,
            site,
            index_cache: RwLock::new(None),
        })))
    }

    fn storage(&self) -> DynStorage {
        self.0.storage.clone()
    }

    fn config(&self) -> &HelmRepositoryConfig {
        &self.0.config
    }

    fn index_cache_ttl(&self) -> Option<Duration> {
        self.config()
            .index_cache_ttl
            .map(|ttl| Duration::from_secs(ttl as u64))
    }

    fn compute_repository_base(&self, request: &RepositoryRequest) -> String {
        if let Some(url) = &self.config().public_base_url {
            return url.trim_end_matches('/').to_string();
        }

        let headers = &request.parts.headers;
        let host = headers
            .get(HOST)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("localhost");
        let proto = headers
            .get("x-forwarded-proto")
            .and_then(|value| value.to_str().ok())
            .or_else(|| request.parts.uri.scheme_str())
            .unwrap_or("http");

        format!(
            "{}://{}/repositories/{}/{}",
            proto.trim_matches(','),
            host.trim_matches('/'),
            self.0.storage_name,
            self.name()
        )
    }

    fn index_url_mode(&self) -> IndexUrlMode {
        match self.config().mode {
            HelmRepositoryMode::Http => IndexUrlMode::Http,
            HelmRepositoryMode::Oci => IndexUrlMode::Oci,
            HelmRepositoryMode::Hybrid => IndexUrlMode::Hybrid,
        }
    }

    async fn fetch_version_metadata(
        &self,
    ) -> Result<Vec<(String, HelmChartVersionExtra)>, HelmRepositoryError> {
        let rows = sqlx::query(
            r#"
                SELECT
                    p.name AS chart_name,
                    pv.extra
                FROM project_versions pv
                INNER JOIN projects p ON pv.project_id = p.id
                WHERE p.repository_id = $1
            "#,
        )
        .bind(self.id())
        .fetch_all(&self.site().database)
        .await?;

        let mut records = Vec::new();
        for row in rows {
            let json: sqlx::types::Json<VersionData> = row.try_get("extra")?;
            let version_data = json.0;
            let Some(extra_value) = version_data.extra.clone() else {
                continue;
            };
            let stored: HelmChartVersionExtra = serde_json::from_value(extra_value)?;
            let chart_name: String = row.try_get("chart_name")?;
            records.push((chart_name, stored));
        }

        Ok(records)
    }

    async fn build_index_entries(
        &self,
        base_url: &str,
    ) -> Result<Vec<IndexEntry>, HelmRepositoryError> {
        let records = self.fetch_version_metadata().await?;
        let mut entries = Vec::new();

        for (_name, stored) in records {
            let HelmChartVersionExtra {
                metadata,
                digest,
                canonical_path,
                size_bytes,
                provenance,
                ..
            } = stored;
            let url = format!(
                "{}/{}",
                base_url.trim_end_matches('/'),
                canonical_path.trim_start_matches('/')
            );
            let provenance_state = if provenance {
                ChartProvenanceState::Present
            } else {
                ChartProvenanceState::Missing
            };
            entries.push(IndexEntry::new(
                metadata,
                digest,
                size_bytes,
                vec![url],
                provenance_state,
            ));
        }

        Ok(entries)
    }

    fn chart_validation_options(&self) -> ChartValidationOptions {
        let mut options = ChartValidationOptions::default();
        if let Some(limit) = self.config().max_chart_size {
            options.max_chart_size = limit;
        }
        if let Some(limit) = self.config().max_file_count {
            options.max_file_count = limit;
        }
        options
    }

    fn prepare_hybrid_oci_artifacts(
        &self,
        parsed: &ParsedChartArchive,
    ) -> Result<HybridOciArtifacts, HelmRepositoryError> {
        let repository = parsed.metadata.name.clone();
        let config_value = build_oci_config(&parsed.metadata);
        let config_bytes = serde_json::to_vec(&config_value)?;
        let config_digest = sha256_digest(&config_bytes);

        let manifest = build_helm_manifest(HelmOciManifestInput {
            chart_digest: parsed.digest.clone(),
            chart_size: parsed.size_bytes,
            chart_name: parsed.metadata.name.clone(),
            chart_version: parsed.metadata.version.to_string(),
            config_digest: config_digest.clone(),
            config_size: config_bytes.len() as u64,
        })?;

        let manifest_bytes = serde_json::to_vec(&manifest)?;
        let manifest_digest = sha256_digest(&manifest_bytes);

        Ok(HybridOciArtifacts {
            repository,
            config_bytes,
            config_digest,
            manifest_bytes,
            manifest_digest,
        })
    }

    async fn persist_hybrid_oci_artifacts(
        &self,
        artifacts: &HybridOciArtifacts,
        parsed: &ParsedChartArchive,
    ) -> Result<(), HelmRepositoryError> {
        let storage = self.storage();
        let repository_id = self.id();
        let chart_blob_path = StoragePath::from(format!(
            "v2/{}/blobs/{}",
            artifacts.repository, parsed.digest
        ));
        storage
            .save_file(
                repository_id,
                FileContent::Bytes(Bytes::from(parsed.archive_bytes.clone())),
                &chart_blob_path,
            )
            .await?;

        let config_blob_path = StoragePath::from(format!(
            "v2/{}/blobs/{}",
            artifacts.repository, artifacts.config_digest
        ));
        storage
            .save_file(
                repository_id,
                FileContent::Bytes(Bytes::from(artifacts.config_bytes.clone())),
                &config_blob_path,
            )
            .await?;

        let version_tag = parsed.metadata.version.to_string();
        let manifest_tag_path = StoragePath::from(format!(
            "v2/{}/manifests/{}",
            artifacts.repository, version_tag
        ));
        storage
            .save_file(
                repository_id,
                FileContent::Bytes(Bytes::from(artifacts.manifest_bytes.clone())),
                &manifest_tag_path,
            )
            .await?;

        let manifest_digest_path = StoragePath::from(format!(
            "v2/{}/manifests/{}",
            artifacts.repository, artifacts.manifest_digest
        ));
        storage
            .save_file(
                repository_id,
                FileContent::Bytes(Bytes::from(artifacts.manifest_bytes.clone())),
                &manifest_digest_path,
            )
            .await?;

        Ok(())
    }

    async fn update_version_oci_metadata(
        &self,
        chart_name: &str,
        chart_version: &str,
        artifacts: &HybridOciArtifacts,
    ) -> Result<(), HelmRepositoryError> {
        let db = &self.site().database;
        let project = DBProject::find_by_project_key(chart_name, self.id(), db)
            .await?
            .ok_or_else(|| HelmRepositoryError::ChartNotFound(chart_name.to_string()))?;

        let version = DBProjectVersion::find_by_version_and_project(chart_version, project.id, db)
            .await?
            .ok_or_else(|| {
                HelmRepositoryError::ChartNotFound(format!("{chart_name}@{chart_version}"))
            })?;

        let mut version_data = version.extra.0;
        let extra_value = version_data
            .extra
            .clone()
            .ok_or_else(|| HelmRepositoryError::InvalidRequest("missing chart metadata".into()))?;
        let mut stored: HelmChartVersionExtra = serde_json::from_value(extra_value)?;
        stored.oci_manifest_digest = Some(artifacts.manifest_digest.clone());
        stored.oci_config_digest = Some(artifacts.config_digest.clone());
        stored.oci_repository = Some(artifacts.repository.clone());
        version_data.extra = Some(serde_json::to_value(stored)?);

        let update = UpdateProjectVersion {
            release_type: None,
            publisher: None,
            version_page: None,
            extra: Some(version_data),
        };
        update.update(version.id, db).await?;
        Ok(())
    }

    fn invalidate_index_cache(&self) {
        let mut cache = self.0.index_cache.write();
        cache.take();
    }

    fn http_enabled(&self) -> bool {
        matches!(
            self.config().mode,
            HelmRepositoryMode::Http | HelmRepositoryMode::Hybrid
        )
    }

    fn oci_enabled(&self) -> bool {
        matches!(
            self.config().mode,
            HelmRepositoryMode::Oci | HelmRepositoryMode::Hybrid
        )
    }

    async fn with_docker_repo<F, Fut>(
        &self,
        request: RepositoryRequest,
        op: F,
    ) -> Result<RepoResponse, HelmRepositoryError>
    where
        F: FnOnce(DockerHosted, RepositoryRequest) -> Fut,
        Fut: Future<Output = Result<RepoResponse, DockerError>> + Send,
    {
        let docker_repo =
            DockerHosted::load(self.0.repository.clone(), self.storage(), self.site()).await?;

        let response = op(docker_repo, request).await?;
        Ok(response)
    }

    async fn has_read_access_auth(
        &self,
        auth: &RepositoryAuthentication,
    ) -> Result<bool, HelmRepositoryError> {
        let allowed = crate::repository::utils::can_read_repository_with_auth(
            auth,
            self.visibility(),
            self.id(),
            &self.site().database,
            &self.0.auth_config,
        )
        .await?;
        Ok(allowed)
    }

    async fn get_write_user_id(
        &self,
        auth: &RepositoryAuthentication,
    ) -> Result<Option<i32>, HelmRepositoryError> {
        let user = auth
            .get_user_if_has_action(RepositoryActions::Write, self.id(), self.site().as_ref())
            .await?;
        Ok(user.map(|u| u.id))
    }

    fn build_version_extra(
        &self,
        parsed: &ParsedChartArchive,
        canonical_path: &StoragePath,
        provenance: bool,
    ) -> HelmChartVersionExtra {
        HelmChartVersionExtra {
            metadata: parsed.metadata.clone(),
            digest: parsed.digest.clone(),
            canonical_path: canonical_path.to_string(),
            size_bytes: parsed.archive_bytes.len() as u64,
            provenance,
            provenance_path: None,
            oci_manifest_digest: None,
            oci_config_digest: None,
            oci_repository: None,
        }
    }

    fn build_version_data(
        &self,
        parsed: &ParsedChartArchive,
        extra: &HelmChartVersionExtra,
    ) -> Result<VersionData, HelmRepositoryError> {
        let authors = parsed
            .metadata
            .maintainers
            .iter()
            .map(|maintainer| Author {
                name: Some(maintainer.name.clone()),
                email: maintainer.email.clone(),
                website: maintainer.url.clone(),
            })
            .collect::<Vec<_>>();

        Ok(VersionData {
            documentation_url: parsed.metadata.home.clone(),
            website: parsed.metadata.home.clone(),
            authors,
            description: parsed.metadata.description.clone(),
            extra: Some(serde_json::to_value(extra)?),
            ..Default::default()
        })
    }

    async fn persist_chart_archive(
        &self,
        parsed: &ParsedChartArchive,
        artifact: &ChartArtifactPath,
        canonical_path: &StoragePath,
        user_id: i32,
    ) -> Result<bool, HelmRepositoryError> {
        let db = &self.site().database;

        let project =
            match DBProject::find_by_project_key(&parsed.metadata.name, self.id(), db).await? {
                Some(project) => project,
                None => {
                    let new_project = NewProject {
                        scope: None,
                        project_key: parsed.metadata.name.clone(),
                        name: parsed.metadata.name.clone(),
                        description: parsed.metadata.description.clone(),
                        repository: self.id(),
                        storage_path: format!("charts/{}/", parsed.metadata.name),
                    };
                    new_project.insert(db).await?
                }
            };

        let existing_version =
            DBProjectVersion::find_by_version_and_project(&artifact.version, project.id, db)
                .await?;

        let existing_extra = existing_version
            .as_ref()
            .and_then(|version| version.extra.0.extra.clone())
            .map(|value| serde_json::from_value::<HelmChartVersionExtra>(value))
            .transpose()?;
        let existing_provenance = existing_extra
            .as_ref()
            .map(|extra| extra.provenance)
            .unwrap_or(false);

        let mut version_extra =
            self.build_version_extra(parsed, canonical_path, existing_provenance);

        if let Some(extra) = existing_extra {
            version_extra.provenance_path = extra.provenance_path;
            version_extra.oci_manifest_digest = extra.oci_manifest_digest;
            version_extra.oci_config_digest = extra.oci_config_digest;
            version_extra.oci_repository = extra.oci_repository;
        }
        let version_data = self.build_version_data(parsed, &version_extra)?;
        let release_type = ReleaseType::release_type_from_version(&artifact.version);

        if let Some(existing) = existing_version {
            if !self.config().overwrite {
                return Err(HelmRepositoryError::ChartAlreadyExists {
                    name: parsed.metadata.name.clone(),
                    version: artifact.version.clone(),
                });
            }

            let update = UpdateProjectVersion {
                release_type: Some(release_type),
                publisher: Some(Some(user_id)),
                version_page: None,
                extra: Some(version_data),
            };
            update.update(existing.id, db).await?;
            Ok(false)
        } else {
            let new_version = NewVersion {
                project_id: project.id,
                version: artifact.version.clone(),
                release_type,
                version_path: canonical_path.to_string(),
                publisher: Some(user_id),
                version_page: None,
                extra: version_data,
            };
            new_version.insert(db).await?;
            Ok(true)
        }
    }

    async fn persist_provenance(
        &self,
        artifact: &ChartArtifactPath,
        canonical_path: &StoragePath,
        user_id: i32,
    ) -> Result<(), HelmRepositoryError> {
        let db = &self.site().database;
        let project = DBProject::find_by_project_key(&artifact.name, self.id(), db)
            .await?
            .ok_or_else(|| HelmRepositoryError::ChartNotFound(artifact.name.clone()))?;

        let version =
            DBProjectVersion::find_by_version_and_project(&artifact.version, project.id, db)
                .await?
                .ok_or_else(|| {
                    HelmRepositoryError::ChartNotFound(format!(
                        "{}@{}",
                        artifact.name, artifact.version
                    ))
                })?;

        let mut version_data = version.extra.0;
        let extra_value = version_data
            .extra
            .clone()
            .ok_or_else(|| HelmRepositoryError::InvalidRequest("missing chart metadata".into()))?;
        let mut stored: HelmChartVersionExtra = serde_json::from_value(extra_value)?;
        update_provenance_extra(&mut stored, canonical_path);
        version_data.extra = Some(serde_json::to_value(stored)?);

        let update = UpdateProjectVersion {
            release_type: None,
            publisher: Some(Some(user_id)),
            version_page: None,
            extra: Some(version_data),
        };
        update.update(version.id, db).await?;
        Ok(())
    }

    async fn handle_get_index(
        &self,
        request: RepositoryRequest,
    ) -> Result<RepoResponse, HelmRepositoryError> {
        if !self.http_enabled() {
            return Ok(ResponseBuilder::not_found()
                .body("HTTP chart access disabled for this repository")
                .into());
        }
        if !self.has_read_access_auth(&request.authentication).await? {
            return Ok(RepoResponse::unauthorized());
        }

        let force_refresh = request
            .parts
            .uri
            .query()
            .map(|query| {
                query.split('&').any(|pair| {
                    if let Some((key, value)) = pair.split_once('=') {
                        key == "force" && value == "true"
                    } else {
                        pair == "force"
                    }
                })
            })
            .unwrap_or(false);

        let base_url = self.compute_repository_base(&request);
        let ttl = self.index_cache_ttl();

        if !force_refresh {
            if let Some(cached) = self.0.index_cache.read().as_ref() {
                if !cached.is_expired(ttl) {
                    let response = ResponseBuilder::ok()
                        .header(CONTENT_TYPE, "application/x-yaml")
                        .body(cached.content.clone());
                    return Ok(response.into());
                }
            }
        }

        let entries = self.build_index_entries(&base_url).await?;
        let render_config = IndexRenderConfig {
            http_base_url: &base_url,
            include_charts_prefix: true,
            mode: self.index_url_mode(),
        };

        let yaml = render_index_yaml(&entries, &render_config)?;

        {
            let mut cache = self.0.index_cache.write();
            *cache = Some(CachedIndex {
                content: yaml.clone(),
                generated_at: Instant::now(),
            });
        }

        let response = ResponseBuilder::ok()
            .header(CONTENT_TYPE, "application/x-yaml")
            .body(yaml);

        Ok(response.into())
    }

    async fn handle_get_chart(
        &self,
        request: RepositoryRequest,
        artifact: ChartArtifactPath,
        head_only: bool,
    ) -> Result<RepoResponse, HelmRepositoryError> {
        if !self.http_enabled() {
            return Ok(ResponseBuilder::not_found()
                .body("HTTP chart access disabled for this repository")
                .into());
        }
        if !self.has_read_access_auth(&request.authentication).await? {
            return Ok(RepoResponse::unauthorized());
        }

        let canonical_path = artifact.canonical_storage_path();
        let storage = self.storage();

        if head_only {
            let Some(meta) = storage
                .get_file_information(self.id(), &canonical_path)
                .await?
            else {
                return Ok(ResponseBuilder::not_found()
                    .body("Chart artifact not found")
                    .into());
            };
            Ok(RepoResponse::FileMetaResponse(Box::new(meta)))
        } else {
            let Some(file) = storage.open_file(self.id(), &canonical_path).await? else {
                return Ok(ResponseBuilder::not_found()
                    .body("Chart artifact not found")
                    .into());
            };
            Ok(RepoResponse::FileResponse(Box::new(file)))
        }
    }

    async fn handle_get_packages(
        &self,
        request: RepositoryRequest,
    ) -> Result<RepoResponse, HelmRepositoryError> {
        if !self.http_enabled() {
            return Ok(ResponseBuilder::not_found()
                .body("HTTP chart access disabled for this repository")
                .into());
        }
        if !self.has_read_access_auth(&request.authentication).await? {
            return Ok(RepoResponse::unauthorized());
        }

        let records = self.fetch_version_metadata().await?;
        let mut items = Vec::new();

        for (name, stored) in records {
            let HelmChartVersionExtra {
                metadata,
                digest,
                canonical_path,
                size_bytes,
                provenance,
                ..
            } = stored;
            items.push(PackageOverview {
                name,
                version: metadata.version.to_string(),
                canonical_path,
                digest,
                size_bytes,
                provenance,
            });
        }

        let response = ResponseBuilder::ok().json(&items);
        Ok(response.into())
    }

    async fn handle_chartmuseum_get(
        &self,
        request: RepositoryRequest,
    ) -> Result<RepoResponse, HelmRepositoryError> {
        if !self.http_enabled() {
            return Ok(ResponseBuilder::not_found()
                .body("HTTP chart access disabled for this repository")
                .into());
        }
        if !self.has_read_access_auth(&request.authentication).await? {
            return Ok(RepoResponse::unauthorized());
        }

        let base_url = self.compute_repository_base(&request);
        let records = self.fetch_version_metadata().await?;

        let mut grouped: BTreeMap<String, Vec<ChartMuseumVersion>> = BTreeMap::new();

        for (chart_name, stored) in records {
            let HelmChartVersionExtra {
                metadata,
                digest,
                canonical_path,
                size_bytes: _,
                provenance,
                ..
            } = stored;
            let version_entry = ChartMuseumVersion {
                name: metadata.name.clone(),
                version: metadata.version.to_string(),
                app_version: metadata.app_version.clone(),
                description: metadata.description.clone(),
                digest,
                urls: vec![format!(
                    "{}/{}",
                    base_url.trim_end_matches('/'),
                    canonical_path.trim_start_matches('/')
                )],
                created: metadata.created,
                api_version: metadata.api_version.as_str().to_string(),
                chart_type: metadata.chart_type.as_str().to_string(),
                home: metadata.home.clone(),
                sources: metadata.sources.clone(),
                keywords: metadata.keywords.clone(),
                maintainers: metadata.maintainers.clone(),
                kube_version: metadata.kube_version.clone(),
                annotations: metadata.annotations.clone(),
            };
            grouped.entry(chart_name).or_default().push(version_entry);
        }

        for versions in grouped.values_mut() {
            versions.sort_by(|a, b| b.version.cmp(&a.version));
        }

        let path = request.path.to_string();
        let segments = path.split('/').collect::<Vec<_>>();

        let response = match segments.as_slice() {
            ["api", "charts"] => ResponseBuilder::ok().json(&grouped),
            ["api", "charts", chart] => {
                if let Some(versions) = grouped.get(*chart) {
                    ResponseBuilder::ok().json(versions)
                } else {
                    return Ok(ResponseBuilder::not_found().body("chart not found").into());
                }
            }
            _ => {
                return Ok(ResponseBuilder::not_found()
                    .body("unsupported ChartMuseum route")
                    .into());
            }
        };

        Ok(response.into())
    }

    async fn handle_get_oci(
        &self,
        request: RepositoryRequest,
    ) -> Result<RepoResponse, HelmRepositoryError> {
        if !self.oci_enabled() {
            return Ok(ResponseBuilder::not_found()
                .body("OCI registry disabled for this repository")
                .into());
        }
        self.with_docker_repo(request, docker_handlers::handle_get)
            .await
    }

    async fn handle_put_chart(
        &self,
        request: RepositoryRequest,
        artifact: ChartArtifactPath,
    ) -> Result<RepoResponse, HelmRepositoryError> {
        if !self.http_enabled() {
            return Ok(ResponseBuilder::default()
                .status(StatusCode::METHOD_NOT_ALLOWED)
                .body("HTTP uploads disabled for this repository")
                .into());
        }
        let user_id = match self.get_write_user_id(&request.authentication).await? {
            Some(id) => id,
            None => return Ok(RepoResponse::unauthorized()),
        };

        let RepositoryRequest { body, .. } = request;
        let bytes = body.body_as_bytes().await?;
        let canonical_path = artifact.canonical_storage_path();

        if artifact.is_provenance {
            self.storage()
                .save_file(
                    self.id(),
                    FileContent::Bytes(bytes.clone()),
                    &canonical_path,
                )
                .await?;

            self.persist_provenance(&artifact, &canonical_path, user_id)
                .await?;
            self.invalidate_index_cache();

            return Ok(RepoResponse::put_response(
                false,
                canonical_path.to_string(),
            ));
        }

        let validation_options = self.chart_validation_options();
        if bytes.len() > validation_options.max_chart_size {
            return Err(HelmRepositoryError::InvalidRequest(format!(
                "chart archive exceeds configured size limit ({} bytes)",
                validation_options.max_chart_size
            )));
        }

        let parsed = parse_chart_archive(bytes.as_ref(), &validation_options)?;

        let hybrid_plan =
            if self.oci_enabled() && matches!(self.config().mode, HelmRepositoryMode::Hybrid) {
                Some(self.prepare_hybrid_oci_artifacts(&parsed)?)
            } else {
                None
            };

        if parsed.metadata.name != artifact.name {
            return Err(HelmRepositoryError::InvalidRequest(format!(
                "chart name mismatch: archive declares '{}' but path expects '{}'",
                parsed.metadata.name, artifact.name
            )));
        }

        if parsed.metadata.version.to_string() != artifact.version {
            return Err(HelmRepositoryError::InvalidRequest(format!(
                "chart version mismatch: archive declares '{}' but path expects '{}'",
                parsed.metadata.version, artifact.version
            )));
        }

        let storage = self.storage();
        storage
            .save_file(
                self.id(),
                FileContent::Bytes(bytes.clone()),
                &canonical_path,
            )
            .await?;

        let persist_result = self
            .persist_chart_archive(&parsed, &artifact, &canonical_path, user_id)
            .await;

        if let Err(error) = persist_result {
            let _ = storage.delete_file(self.id(), &canonical_path).await;
            return Err(error);
        }

        let created = persist_result?;

        if let Some(artifacts) = &hybrid_plan {
            self.persist_hybrid_oci_artifacts(artifacts, &parsed)
                .await?;
            self.update_version_oci_metadata(&parsed.metadata.name, &artifact.version, artifacts)
                .await?;
        }

        self.invalidate_index_cache();

        Ok(RepoResponse::put_response(
            created,
            canonical_path.to_string(),
        ))
    }

    async fn handle_put_oci(
        &self,
        request: RepositoryRequest,
    ) -> Result<RepoResponse, HelmRepositoryError> {
        if !self.oci_enabled() {
            return Ok(ResponseBuilder::not_found()
                .body("OCI registry disabled for this repository")
                .into());
        }
        self.with_docker_repo(request, docker_handlers::handle_put)
            .await
    }

    async fn handle_chartmuseum_upload(
        &self,
        request: RepositoryRequest,
    ) -> Result<RepoResponse, HelmRepositoryError> {
        if !self.http_enabled() {
            return Ok(ResponseBuilder::default()
                .status(StatusCode::METHOD_NOT_ALLOWED)
                .body("HTTP uploads disabled for this repository")
                .into());
        }
        let user_id = match self.get_write_user_id(&request.authentication).await? {
            Some(id) => id,
            None => return Ok(RepoResponse::unauthorized()),
        };

        let content_type_header = request
            .parts
            .headers
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("");

        if !content_type_header.starts_with("multipart/form-data") {
            return Ok(ResponseBuilder::unsupported_media_type()
                .body("ChartMuseum uploads must use multipart/form-data")
                .into());
        }

        let boundary = multer::parse_boundary(content_type_header)
            .map_err(|err| HelmRepositoryError::InvalidRequest(err.to_string()))?;

        let mut multipart = multer::Multipart::new(request.body.into_byte_stream(), boundary);

        let mut chart_payload: Option<(ChartArtifactPath, Bytes)> = None;
        let mut prov_payload: Option<(ChartArtifactPath, Bytes)> = None;

        while let Some(field) = multipart
            .next_field()
            .await
            .map_err(|err| HelmRepositoryError::InvalidRequest(err.to_string()))?
        {
            let name = field.name().unwrap_or_default();
            match name {
                "chart" => {
                    let filename = field
                        .file_name()
                        .ok_or_else(|| {
                            HelmRepositoryError::InvalidRequest(
                                "chart upload missing filename".to_string(),
                            )
                        })?
                        .to_string();
                    let path = StoragePath::from(filename.as_str());
                    let artifact = parse_chart_artifact(&path).ok_or_else(|| {
                        HelmRepositoryError::InvalidRequest(
                            "unable to parse chart filename".to_string(),
                        )
                    })?;
                    let bytes = field
                        .bytes()
                        .await
                        .map_err(|err| HelmRepositoryError::InvalidRequest(err.to_string()))?;
                    chart_payload = Some((artifact, bytes));
                }
                "prov" => {
                    let filename = field
                        .file_name()
                        .ok_or_else(|| {
                            HelmRepositoryError::InvalidRequest(
                                "provenance upload missing filename".to_string(),
                            )
                        })?
                        .to_string();
                    let path = StoragePath::from(filename.as_str());
                    let artifact = parse_chart_artifact(&path).ok_or_else(|| {
                        HelmRepositoryError::InvalidRequest(
                            "unable to parse provenance filename".to_string(),
                        )
                    })?;
                    if !artifact.is_provenance {
                        return Err(HelmRepositoryError::InvalidRequest(
                            "provenance filename must end with .tgz.prov".to_string(),
                        ));
                    }
                    let bytes = field
                        .bytes()
                        .await
                        .map_err(|err| HelmRepositoryError::InvalidRequest(err.to_string()))?;
                    prov_payload = Some((artifact, bytes));
                }
                _ => {
                    // ignore other fields
                }
            }
        }

        let Some((chart_artifact, chart_bytes)) = chart_payload else {
            return Err(HelmRepositoryError::InvalidRequest(
                "multipart upload missing chart field".to_string(),
            ));
        };

        let validation_options = self.chart_validation_options();
        if chart_bytes.len() > validation_options.max_chart_size {
            return Err(HelmRepositoryError::InvalidRequest(format!(
                "chart archive exceeds configured size limit ({} bytes)",
                validation_options.max_chart_size
            )));
        }

        let parsed = parse_chart_archive(chart_bytes.as_ref(), &validation_options)?;
        if parsed.metadata.name != chart_artifact.name {
            return Err(HelmRepositoryError::InvalidRequest(format!(
                "chart name mismatch: archive declares '{}' but filename is '{}'",
                parsed.metadata.name, chart_artifact.name
            )));
        }
        if parsed.metadata.version.to_string() != chart_artifact.version {
            return Err(HelmRepositoryError::InvalidRequest(format!(
                "chart version mismatch: archive declares '{}' but filename is '{}'",
                parsed.metadata.version, chart_artifact.version
            )));
        }

        let canonical_path = chart_artifact.canonical_storage_path();
        self.storage()
            .save_file(
                self.id(),
                FileContent::Bytes(chart_bytes.clone()),
                &canonical_path,
            )
            .await?;

        let created = self
            .persist_chart_archive(&parsed, &chart_artifact, &canonical_path, user_id)
            .await?;

        if let Some((prov_artifact, prov_bytes)) = prov_payload {
            self.storage()
                .save_file(
                    self.id(),
                    FileContent::Bytes(prov_bytes.clone()),
                    &prov_artifact.canonical_storage_path(),
                )
                .await?;
            self.persist_provenance(
                &prov_artifact,
                &prov_artifact.canonical_storage_path(),
                user_id,
            )
            .await?;
        }

        self.invalidate_index_cache();

        Ok(RepoResponse::put_response(
            created,
            canonical_path.to_string(),
        ))
    }

    async fn handle_post_oci(
        &self,
        request: RepositoryRequest,
    ) -> Result<RepoResponse, HelmRepositoryError> {
        if !self.oci_enabled() {
            return Ok(ResponseBuilder::not_found()
                .body("OCI registry disabled for this repository")
                .into());
        }
        self.with_docker_repo(request, docker_handlers::handle_post)
            .await
    }

    async fn handle_delete_packages(
        &self,
        request: RepositoryRequest,
    ) -> Result<RepoResponse, HelmRepositoryError> {
        if !self.http_enabled() {
            return Ok(ResponseBuilder::default()
                .status(StatusCode::METHOD_NOT_ALLOWED)
                .body("HTTP chart access disabled for this repository")
                .into());
        }

        let user_id = match self.get_write_user_id(&request.authentication).await? {
            Some(id) => id,
            None => return Ok(RepoResponse::unauthorized()),
        };

        let body = request.body.body_as_json::<DeletePackagesRequest>().await?;
        let db = &self.site().database;

        for entry in body.charts {
            let chart_name = entry.name;
            let chart_version = entry.version;

            let project = DBProject::find_by_project_key(&chart_name, self.id(), db)
                .await?
                .ok_or_else(|| HelmRepositoryError::ChartNotFound(chart_name.clone()))?;

            let version =
                DBProjectVersion::find_by_version_and_project(&chart_version, project.id, db)
                    .await?
                    .ok_or_else(|| {
                        HelmRepositoryError::ChartNotFound(format!(
                            "{}@{}",
                            chart_name, chart_version
                        ))
                    })?;

            let mut chart_path = StoragePath::from(format!(
                "charts/{}/{}-{}.tgz",
                chart_name, chart_name, chart_version
            ));
            let mut prov_path: Option<StoragePath> = Some(StoragePath::from(format!(
                "charts/{}/{}-{}.tgz.prov",
                chart_name, chart_name, chart_version
            )));

            let extra = version
                .extra
                .0
                .extra
                .clone()
                .map(|value| serde_json::from_value::<HelmChartVersionExtra>(value))
                .transpose()?;

            if let Some(extra_ref) = extra.as_ref() {
                chart_path = StoragePath::from(extra_ref.canonical_path.as_str());
                prov_path = extra_ref.provenance_path.as_deref().map(StoragePath::from);
            }

            let storage = self.storage();
            let _ = storage.delete_file(self.id(), &chart_path).await?;
            if let Some(path) = prov_path {
                let _ = storage.delete_file(self.id(), &path).await?;
            }
            if let Some(extra_ref) = extra.as_ref() {
                if let Some(repository) = &extra_ref.oci_repository {
                    let chart_blob_path =
                        StoragePath::from(format!("v2/{}/blobs/{}", repository, extra_ref.digest));
                    let _ = storage.delete_file(self.id(), &chart_blob_path).await?;
                    if let Some(config_digest) = &extra_ref.oci_config_digest {
                        let config_blob_path =
                            StoragePath::from(format!("v2/{}/blobs/{}", repository, config_digest));
                        let _ = storage.delete_file(self.id(), &config_blob_path).await?;
                    }
                    let manifest_tag_path =
                        StoragePath::from(format!("v2/{}/manifests/{}", repository, chart_version));
                    let _ = storage.delete_file(self.id(), &manifest_tag_path).await?;
                    if let Some(manifest_digest) = &extra_ref.oci_manifest_digest {
                        let manifest_digest_path = StoragePath::from(format!(
                            "v2/{}/manifests/{}",
                            repository, manifest_digest
                        ));
                        let _ = storage
                            .delete_file(self.id(), &manifest_digest_path)
                            .await?;
                    }
                }
            }

            sqlx::query("DELETE FROM project_versions WHERE project_id = $1 AND version = $2")
                .bind(project.id)
                .bind(&chart_version)
                .execute(db)
                .await?;

            let remaining: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM project_versions WHERE project_id = $1")
                    .bind(project.id)
                    .fetch_one(db)
                    .await?;

            if remaining == 0 {
                sqlx::query("DELETE FROM projects WHERE id = $1")
                    .bind(project.id)
                    .execute(db)
                    .await?;
            }
        }

        self.invalidate_index_cache();

        Ok(ResponseBuilder::no_content().empty().into())
    }

    async fn handle_delete_oci(
        &self,
        request: RepositoryRequest,
    ) -> Result<RepoResponse, HelmRepositoryError> {
        if !self.oci_enabled() {
            return Ok(ResponseBuilder::not_found()
                .body("OCI registry disabled for this repository")
                .into());
        }
        self.with_docker_repo(request, docker_handlers::handle_delete)
            .await
    }
}

impl Repository for HelmHosted {
    type Error = HelmRepositoryError;

    fn get_storage(&self) -> DynStorage {
        self.storage()
    }

    fn site(&self) -> NitroRepo {
        self.0.site.clone()
    }

    fn get_type(&self) -> &'static str {
        "helm"
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
        self.0.repository.active
    }

    fn config_types(&self) -> Vec<&str> {
        vec![
            HelmRepositoryConfigType::get_type_static(),
            RepositoryAuthConfigType::get_type_static(),
        ]
    }

    async fn handle_get(&self, request: RepositoryRequest) -> Result<RepoResponse, Self::Error> {
        let path = request.path.clone();
        let path_str = path.to_string();

        if path_str == "index.yaml" {
            return self.handle_get_index(request).await;
        }

        if let Some(artifact) = parse_chart_artifact(&path) {
            return self.handle_get_chart(request, artifact, false).await;
        }

        if path_str == "packages" {
            return self.handle_get_packages(request).await;
        }

        if path_str.starts_with("api/charts") {
            return self.handle_chartmuseum_get(request).await;
        }

        if path_str.starts_with("v2/") {
            return self.handle_get_oci(request).await;
        }

        Ok(ResponseBuilder::not_found()
            .body("Resource not found")
            .into())
    }

    async fn handle_put(&self, request: RepositoryRequest) -> Result<RepoResponse, Self::Error> {
        let path = request.path.clone();

        if let Some(artifact) = parse_chart_artifact(&path) {
            return self.handle_put_chart(request, artifact).await;
        }

        if path.to_string().starts_with("v2/") {
            return self.handle_put_oci(request).await;
        }

        Ok(ResponseBuilder::bad_request()
            .body("Unsupported PUT operation for Helm repository")
            .into())
    }

    async fn handle_post(&self, request: RepositoryRequest) -> Result<RepoResponse, Self::Error> {
        let path = request.path.clone();
        let path_str = path.to_string();

        if path_str == "api/charts" {
            return self.handle_chartmuseum_upload(request).await;
        }

        if path_str.starts_with("v2/") {
            return self.handle_post_oci(request).await;
        }

        Ok(ResponseBuilder::bad_request()
            .body("Unsupported POST operation for Helm repository")
            .into())
    }

    async fn handle_delete(&self, request: RepositoryRequest) -> Result<RepoResponse, Self::Error> {
        let path = request.path.to_string();
        if path == "packages" {
            return self.handle_delete_packages(request).await;
        }

        if path.starts_with("v2/") {
            return self.handle_delete_oci(request).await;
        }

        Ok(ResponseBuilder::bad_request()
            .body("Unsupported DELETE operation for Helm repository")
            .into())
    }

    async fn handle_head(&self, request: RepositoryRequest) -> Result<RepoResponse, Self::Error> {
        if let Some(artifact) = parse_chart_artifact(&request.path) {
            return self.handle_get_chart(request, artifact, true).await;
        }

        Ok(ResponseBuilder::default()
            .status(StatusCode::METHOD_NOT_ALLOWED)
            .body("HEAD not supported for this resource")
            .into())
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct HelmChartVersionExtra {
    metadata: super::chart::HelmChartMetadata,
    digest: String,
    canonical_path: String,
    size_bytes: u64,
    provenance: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    provenance_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    oci_manifest_digest: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    oci_config_digest: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    oci_repository: Option<String>,
}

#[derive(Debug, Serialize)]
struct ChartMuseumVersion {
    name: String,
    version: String,
    #[serde(rename = "appVersion")]
    app_version: Option<String>,
    description: Option<String>,
    digest: String,
    urls: Vec<String>,
    created: DateTime<Utc>,
    #[serde(rename = "apiVersion")]
    api_version: String,
    #[serde(rename = "type")]
    chart_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    home: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    sources: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    keywords: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    maintainers: Vec<ChartMaintainer>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "kubeVersion")]
    kube_version: Option<String>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    annotations: BTreeMap<String, String>,
}

#[derive(Debug, Serialize)]
struct PackageOverview {
    name: String,
    version: String,
    canonical_path: String,
    digest: String,
    size_bytes: u64,
    provenance: bool,
}

#[derive(Debug, Deserialize)]
struct DeletePackagesRequest {
    charts: Vec<DeletePackageEntry>,
}

#[derive(Debug, Deserialize)]
struct DeletePackageEntry {
    name: String,
    version: String,
}

fn update_provenance_extra(extra: &mut HelmChartVersionExtra, canonical_path: &StoragePath) {
    extra.provenance = true;
    extra.provenance_path = Some(canonical_path.to_string());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repository::helm::chart::{ChartApiVersion, ChartType, HelmChartMetadata};
    use chrono::Utc;
    use semver::Version;
    use std::collections::BTreeMap;

    fn sample_metadata() -> HelmChartMetadata {
        HelmChartMetadata {
            name: "webapp".to_string(),
            version: Version::parse("1.0.0").unwrap(),
            description: None,
            app_version: None,
            kube_version: None,
            home: None,
            sources: Vec::new(),
            keywords: Vec::new(),
            maintainers: Vec::new(),
            engine: None,
            icon: None,
            annotations: BTreeMap::new(),
            dependencies: Vec::new(),
            created: Utc::now(),
            api_version: ChartApiVersion::V2,
            chart_type: ChartType::Application,
            tiller_version: None,
        }
    }

    #[test]
    fn parses_root_chart_artifact() {
        let path = StoragePath::from("webapp-1.0.0.tgz");
        let artifact = parse_chart_artifact(&path).expect("should parse chart");
        assert_eq!(artifact.name, "webapp");
        assert_eq!(artifact.version, "1.0.0");
        assert!(!artifact.is_provenance);
        assert!(!artifact.alias);
    }

    #[test]
    fn parses_alias_provenance_artifact() {
        let path = StoragePath::from("charts/webapp-1.0.0.tgz.prov");
        let artifact = parse_chart_artifact(&path).expect("should parse provenance");
        assert_eq!(artifact.name, "webapp");
        assert_eq!(artifact.version, "1.0.0");
        assert!(artifact.is_provenance);
        assert!(artifact.alias);
    }

    #[test]
    fn oci_config_contains_chart_metadata() {
        let metadata = sample_metadata();
        let config = build_oci_config(&metadata);
        assert_eq!(config["name"].as_str(), Some("webapp"));
        assert_eq!(config["version"].as_str(), Some("1.0.0"));
        assert_eq!(config["apiVersion"].as_str(), Some("v2"));
        assert_eq!(config["type"].as_str(), Some("application"));
    }

    #[test]
    fn sha256_digest_includes_prefix() {
        let digest = sha256_digest(b"example-bytes");
        assert!(digest.starts_with("sha256:"));
        assert_eq!(
            digest,
            "sha256:ff00188058a5a5549cc34752ee3fee897c4303e8cb44996eab8caa134c5253fa"
        );
    }

    #[test]
    fn update_provenance_keeps_canonical_path() {
        let mut extra = HelmChartVersionExtra {
            metadata: sample_metadata(),
            digest: "sha256:abc".to_string(),
            canonical_path: "charts/webapp/webapp-1.0.0.tgz".to_string(),
            size_bytes: 512,
            provenance: false,
            provenance_path: None,
            oci_manifest_digest: None,
            oci_config_digest: None,
            oci_repository: None,
        };
        let prov_path = StoragePath::from("charts/webapp/webapp-1.0.0.tgz.prov");
        update_provenance_extra(&mut extra, &prov_path);
        assert!(extra.provenance);
        assert_eq!(extra.canonical_path, "charts/webapp/webapp-1.0.0.tgz");
        assert_eq!(
            extra.provenance_path.as_deref(),
            Some("charts/webapp/webapp-1.0.0.tgz.prov")
        );
    }
}
