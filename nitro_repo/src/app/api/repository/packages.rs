use std::{cmp::min, collections::HashSet};

use axum::{
    Json,
    extract::{Path, Query, State},
    response::{IntoResponse, Response},
    routing::get,
};
use chrono::{DateTime, FixedOffset};
use nr_storage::{FileType, Storage, StorageFile};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::Row;
use tokio::io::AsyncReadExt;
use tracing::{instrument, warn};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use crate::{
    app::{
        NitroRepo,
        authentication::Authentication,
        responses::{MissingPermission, RepositoryNotFound},
    },
    error::InternalError,
    repository::{
        DynRepository, Repository,
        docker::{
            metadata::collect_manifest_entries,
            types::{Manifest as DockerManifest, MediaType},
        },
        utils::can_read_repository_with_auth,
    },
    utils::ResponseBuilder,
};
use nr_core::user::permissions::{HasPermissions, RepositoryActions};

#[derive(Debug, Clone, Copy, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct PackageListQuery {
    #[serde(default = "default_page")]
    #[param(default = 1)]
    pub page: usize,
    #[serde(default = "default_per_page")]
    #[param(default = 50)]
    pub per_page: usize,
}

const fn default_page() -> usize {
    1
}
const fn default_per_page() -> usize {
    50
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PackageFileEntry {
    pub package: String,
    pub name: String,
    pub cache_path: String,
    pub size: u64,
    pub modified: DateTime<FixedOffset>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PackageListResponse {
    pub page: usize,
    pub per_page: usize,
    pub total_packages: usize,
    pub items: Vec<PackageFileEntry>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PackageStrategy {
    PackagesDirectory { base: Option<&'static str> },
    Maven,
    PythonHosted,
    Docker,
}

fn package_strategy(repository: &DynRepository) -> PackageStrategy {
    match repository {
        DynRepository::Maven(_) => PackageStrategy::Maven,
        DynRepository::Python(python_repo) => match python_repo {
            crate::repository::python::PythonRepository::Hosted(_) => PackageStrategy::PythonHosted,
            _ => PackageStrategy::PackagesDirectory {
                base: Some("packages/"),
            },
        },
        DynRepository::Helm(_) => PackageStrategy::PackagesDirectory {
            base: Some("charts/"),
        },
        DynRepository::Docker(_) => PackageStrategy::Docker,
        DynRepository::Go(go_repo) => match go_repo {
            crate::repository::go::GoRepository::Hosted(_) => PackageStrategy::PackagesDirectory {
                base: Some("packages/"),
            },
            crate::repository::go::GoRepository::Proxy(_) => PackageStrategy::PackagesDirectory {
                base: Some("go-proxy-cache/"),
            },
        },
        _ => PackageStrategy::PackagesDirectory {
            base: Some("packages/"),
        },
    }
}

pub fn package_routes() -> axum::Router<NitroRepo> {
    axum::Router::new().route(
        "/{repository_id}/packages",
        get(list_cached_packages).delete(delete_cached_packages),
    )
}

#[utoipa::path(
    get,
    path = "/{repository_id}/packages",
    params(
        PackageListQuery,
        ("repository_id" = Uuid, Path, description = "The Repository ID"),
    ),
    responses(
        (status = 200, description = "Cached package listing", body = PackageListResponse),
        (status = 404, description = "Repository or packages not found"),
        (status = 403, description = "Missing permission")
    )
)]
#[instrument]
pub async fn list_cached_packages(
    State(site): State<NitroRepo>,
    auth: Option<Authentication>,
    Path(repository_id): Path<Uuid>,
    Query(query): Query<PackageListQuery>,
) -> Result<Response, InternalError> {
    let Some(repository) = site.get_repository(repository_id) else {
        return Ok(RepositoryNotFound::Uuid(repository_id).into_response());
    };
    let auth_config = site.get_repository_auth_config(repository.id()).await?;
    if !can_read_repository_with_auth(
        &auth,
        repository.visibility(),
        repository.id(),
        site.as_ref(),
        &auth_config,
    )
    .await?
    {
        return Ok(MissingPermission::ReadRepository(repository.id()).into_response());
    }
    match package_strategy(&repository) {
        PackageStrategy::PackagesDirectory { base } => {
            list_directory_packages(repository, query.page, query.per_page, base).await
        }
        PackageStrategy::Maven => {
            list_maven_packages(site, repository, query.page, query.per_page).await
        }
        PackageStrategy::PythonHosted => {
            list_directory_packages(repository, query.page, query.per_page, None).await
        }
        PackageStrategy::Docker => {
            list_docker_packages(repository, query.page, query.per_page).await
        }
    }
}

fn should_ignore(name: &str) -> bool {
    name.starts_with('.') || name.ends_with(".nr-meta")
}

async fn list_directory_packages(
    repository: DynRepository,
    page: usize,
    per_page_raw: usize,
    base: Option<&str>,
) -> Result<Response, InternalError> {
    let storage = repository.get_storage();
    let mut package_dirs = gather_package_dirs(&storage, repository.id(), base).await?;
    package_dirs.sort_by(|a, b| a.0.cmp(&b.0));

    let total_packages = package_dirs.len();
    if total_packages == 0 {
        let empty = PackageListResponse {
            page,
            per_page: per_page_raw,
            total_packages,
            items: Vec::new(),
        };
        return Ok(ResponseBuilder::ok().json(&empty));
    }

    let per_page = per_page_raw.clamp(1, 200);
    let current_page = page.max(1);
    let start = (current_page - 1) * per_page;
    if start >= total_packages {
        let empty = PackageListResponse {
            page: current_page,
            per_page,
            total_packages,
            items: Vec::new(),
        };
        return Ok(ResponseBuilder::ok().json(&empty));
    }
    let end = min(start + per_page, total_packages);
    let mut items = Vec::new();
    for (display_name, storage_relative) in &package_dirs[start..end] {
        let path = match base {
            Some(prefix) => {
                let mut combined = String::from(prefix);
                if !storage_relative.is_empty() {
                    combined.push_str(storage_relative);
                    if !combined.ends_with('/') {
                        combined.push('/');
                    }
                }
                combined
            }
            None => {
                let mut path = storage_relative.clone();
                if !path.is_empty() && !path.ends_with('/') {
                    path.push('/');
                }
                path
            }
        };
        let storage_path = nr_core::storage::StoragePath::from(path.clone());
        if let Some(StorageFile::Directory { files, .. }) =
            storage.open_file(repository.id(), &storage_path).await?
        {
            for meta in files.iter() {
                if should_ignore(meta.name()) {
                    continue;
                }
                if let FileType::File(file_meta) = meta.file_type() {
                    let directory_prefix = path.trim_end_matches('/');
                    let cache_path = if directory_prefix.is_empty() {
                        meta.name().to_string()
                    } else {
                        format!("{}/{}", directory_prefix, meta.name())
                    };
                    items.push(PackageFileEntry {
                        package: display_name.clone(),
                        name: meta.name().to_string(),
                        cache_path,
                        size: file_meta.file_size,
                        modified: meta.modified().clone(),
                    });
                }
            }
        }
    }

    let response = PackageListResponse {
        page: current_page,
        per_page,
        total_packages,
        items,
    };
    Ok(ResponseBuilder::ok().json(&response))
}

async fn list_maven_packages(
    site: NitroRepo,
    repository: DynRepository,
    page: usize,
    per_page_raw: usize,
) -> Result<Response, InternalError> {
    let per_page = per_page_raw.clamp(1, 200);
    let current_page = page.max(1);
    let offset = ((current_page - 1) * per_page) as i64;

    let total_versions: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*)
        FROM project_versions pv
        INNER JOIN projects p ON pv.project_id = p.id
        WHERE p.repository_id = $1
        "#,
    )
    .bind(repository.id())
    .fetch_one(&site.database)
    .await?;

    if total_versions == 0 {
        let empty = PackageListResponse {
            page: current_page,
            per_page,
            total_packages: 0,
            items: Vec::new(),
        };
        return Ok(ResponseBuilder::ok().json(&empty));
    }

    if offset >= total_versions {
        let empty = PackageListResponse {
            page: current_page,
            per_page,
            total_packages: total_versions as usize,
            items: Vec::new(),
        };
        return Ok(ResponseBuilder::ok().json(&empty));
    }

    let rows = sqlx::query(
        r#"
        SELECT
            p.key AS project_key,
            pv.version AS version,
            pv.path AS version_path
        FROM project_versions pv
        INNER JOIN projects p ON pv.project_id = p.id
        WHERE p.repository_id = $1
        ORDER BY p.key ASC, pv.version ASC
        LIMIT $2 OFFSET $3
        "#,
    )
    .bind(repository.id())
    .bind(per_page as i64)
    .bind(offset)
    .fetch_all(&site.database)
    .await?;

    let storage = repository.get_storage();
    let mut items = Vec::new();

    for row in rows {
        let project_key: String = row.try_get("project_key")?;
        let version: String = row.try_get("version")?;
        let version_path: String = row.try_get("version_path")?;

        let normalized_path = if version_path.ends_with('/') {
            version_path.clone()
        } else {
            format!("{}/", version_path)
        };

        let storage_path = nr_core::storage::StoragePath::from(normalized_path.as_str());
        let Some(StorageFile::Directory { files, .. }) =
            storage.open_file(repository.id(), &storage_path).await?
        else {
            continue;
        };

        let cache_prefix = version_path.trim_end_matches('/');
        let package_label = format!("{}:{}", project_key, version);

        for meta in files.iter() {
            if should_ignore(meta.name()) {
                continue;
            }
            if let FileType::File(file_meta) = meta.file_type() {
                let cache_path = if cache_prefix.is_empty() {
                    meta.name().to_string()
                } else {
                    format!("{}/{}", cache_prefix, meta.name())
                };
                items.push(PackageFileEntry {
                    package: package_label.clone(),
                    name: meta.name().to_string(),
                    cache_path,
                    size: file_meta.file_size,
                    modified: meta.modified().clone(),
                });
            }
        }
    }

    let response = PackageListResponse {
        page: current_page,
        per_page,
        total_packages: total_versions as usize,
        items,
    };
    Ok(ResponseBuilder::ok().json(&response))
}

async fn list_docker_packages(
    repository: DynRepository,
    page: usize,
    per_page_raw: usize,
) -> Result<Response, InternalError> {
    let storage = repository.get_storage();
    let mut manifests = collect_manifest_entries(&storage, repository.id())
        .await
        .map_err(InternalError::from)?;

    manifests.sort_by(|a, b| {
        a.repository
            .cmp(&b.repository)
            .then(a.reference.cmp(&b.reference))
    });

    let per_page = per_page_raw.clamp(1, 200);
    let current_page = page.max(1);
    let total_packages = manifests.len();

    if total_packages == 0 {
        let empty = PackageListResponse {
            page: current_page,
            per_page,
            total_packages,
            items: Vec::new(),
        };
        return Ok(ResponseBuilder::ok().json(&empty));
    }

    let start = (current_page - 1) * per_page;
    if start >= total_packages {
        let empty = PackageListResponse {
            page: current_page,
            per_page,
            total_packages,
            items: Vec::new(),
        };
        return Ok(ResponseBuilder::ok().json(&empty));
    }

    let end = min(start + per_page, total_packages);
    let mut items = Vec::with_capacity(end - start);

    for entry in manifests[start..end].iter() {
        items.push(PackageFileEntry {
            package: entry.repository.clone(),
            name: entry.reference.clone(),
            cache_path: entry.cache_path.clone(),
            size: entry.size,
            modified: entry.modified,
        });
    }

    let response = PackageListResponse {
        page: current_page,
        per_page,
        total_packages,
        items,
    };
    Ok(ResponseBuilder::ok().json(&response))
}

fn is_valid_cache_path(path: &str, strategy: PackageStrategy) -> bool {
    match strategy {
        PackageStrategy::PackagesDirectory { base } => {
            if let Some(prefix) = base {
                path.starts_with(prefix) && is_valid_repository_path(path)
            } else {
                is_valid_repository_path(path)
            }
        }
        PackageStrategy::Maven | PackageStrategy::PythonHosted => is_valid_repository_path(path),
        PackageStrategy::Docker => is_valid_docker_manifest_path(path),
    }
}

fn is_valid_repository_path(path: &str) -> bool {
    if path.is_empty() {
        return false;
    }
    if path.starts_with('/') || path.contains("..") {
        return false;
    }
    true
}

fn is_valid_docker_manifest_path(path: &str) -> bool {
    if path.is_empty() || path.starts_with('/') || path.contains("..") {
        return false;
    }
    path.starts_with("v2/") && path.contains("/manifests/")
}

#[derive(Debug, Default, Clone, Copy)]
pub struct DockerDeletionResult {
    pub removed_manifests: usize,
    pub removed_blobs: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum DockerDeletionError {
    #[error("manifest not found")]
    ManifestMissing,
    #[error("invalid manifest path")]
    InvalidManifestPath,
    #[error("storage error: {0}")]
    Storage(#[from] nr_storage::StorageError),
    #[error("invalid manifest: {0}")]
    InvalidManifest(String),
}

pub async fn delete_docker_package(
    storage: &nr_storage::DynStorage,
    repository_id: Uuid,
    cache_path: &str,
) -> Result<DockerDeletionResult, DockerDeletionError> {
    let (repository_name, _) =
        split_manifest_cache_path(cache_path).ok_or(DockerDeletionError::InvalidManifestPath)?;

    let mut visited_manifests = HashSet::new();
    let mut deleted_blobs = HashSet::new();
    let mut total = DockerDeletionResult::default();
    let mut stack = Vec::new();
    stack.push(cache_path.to_string());

    while let Some(current_path) = stack.pop() {
        match process_manifest(
            storage,
            repository_id,
            &repository_name,
            &current_path,
            &mut visited_manifests,
            &mut deleted_blobs,
        )
        .await
        {
            Ok(process) => {
                total.removed_manifests += process.delta.removed_manifests;
                total.removed_blobs += process.delta.removed_blobs;
                stack.extend(process.nested);
            }
            Err(DockerDeletionError::ManifestMissing) if current_path != cache_path => {
                // Nested manifest already removed; skip silently.
            }
            Err(err) => return Err(err),
        }
    }

    Ok(total)
}

fn split_manifest_cache_path(path: &str) -> Option<(String, String)> {
    if !path.starts_with("v2/") {
        return None;
    }
    let without_prefix = &path[3..];
    let marker = "/manifests/";
    let split_index = without_prefix.find(marker)?;
    let repository = &without_prefix[..split_index];
    let reference = &without_prefix[split_index + marker.len()..];
    if repository.is_empty() || reference.is_empty() {
        return None;
    }
    Some((repository.to_string(), reference.to_string()))
}

async fn delete_blob_digest(
    storage: &nr_storage::DynStorage,
    repository_id: Uuid,
    repository_name: &str,
    digest: &str,
    deleted_blobs: &mut HashSet<String>,
    result: &mut DockerDeletionResult,
) -> Result<(), DockerDeletionError> {
    if !deleted_blobs.insert(digest.to_string()) {
        return Ok(());
    }
    let blob_path =
        nr_core::storage::StoragePath::from(format!("v2/{}/blobs/{}", repository_name, digest));
    match storage.delete_file(repository_id, &blob_path).await {
        Ok(true) => result.removed_blobs += 1,
        Ok(false) => {}
        Err(err) => return Err(DockerDeletionError::Storage(err)),
    }
    Ok(())
}

struct ManifestProcess {
    delta: DockerDeletionResult,
    nested: Vec<String>,
}

async fn process_manifest(
    storage: &nr_storage::DynStorage,
    repository_id: Uuid,
    repository_name: &str,
    cache_path: &str,
    visited_manifests: &mut HashSet<String>,
    deleted_blobs: &mut HashSet<String>,
) -> Result<ManifestProcess, DockerDeletionError> {
    let storage_path = nr_core::storage::StoragePath::from(cache_path);
    let Some(file) = storage.open_file(repository_id, &storage_path).await? else {
        return Err(DockerDeletionError::ManifestMissing);
    };
    let nr_storage::StorageFile::File { meta, mut content } = file else {
        return Err(DockerDeletionError::InvalidManifest(
            "expected manifest file".to_string(),
        ));
    };

    let size_hint = usize::try_from(meta.file_type.file_size).unwrap_or(0);
    let mut bytes = Vec::with_capacity(size_hint);
    content
        .read_to_end(&mut bytes)
        .await
        .map_err(|err| DockerDeletionError::InvalidManifest(err.to_string()))?;

    let manifest_digest = format!("sha256:{:x}", Sha256::digest(&bytes));
    let manifest = DockerManifest::from_bytes(&bytes, MediaType::OCI_IMAGE_MANIFEST)
        .map_err(|err| DockerDeletionError::InvalidManifest(err.to_string()))?;

    let mut result = DockerDeletionResult::default();
    if storage.delete_file(repository_id, &storage_path).await? {
        result.removed_manifests += 1;
    }

    let digest_path_str = format!("v2/{}/manifests/{}", repository_name, manifest_digest);
    let digest_path = nr_core::storage::StoragePath::from(digest_path_str.as_str());
    if digest_path != storage_path {
        if storage.delete_file(repository_id, &digest_path).await? {
            result.removed_manifests += 1;
        }
    }

    let first_visit = visited_manifests.insert(manifest_digest.clone());
    if !first_visit {
        return Ok(ManifestProcess {
            delta: result,
            nested: Vec::new(),
        });
    }

    let mut nested = Vec::new();
    match manifest {
        DockerManifest::DockerV2(manifest) => {
            delete_blob_digest(
                storage,
                repository_id,
                repository_name,
                &manifest.config.digest,
                deleted_blobs,
                &mut result,
            )
            .await?;
            for layer in manifest.layers {
                delete_blob_digest(
                    storage,
                    repository_id,
                    repository_name,
                    &layer.digest,
                    deleted_blobs,
                    &mut result,
                )
                .await?;
            }
        }
        DockerManifest::OciImage(manifest) => {
            if let Some(config) = manifest.config {
                delete_blob_digest(
                    storage,
                    repository_id,
                    repository_name,
                    &config.digest,
                    deleted_blobs,
                    &mut result,
                )
                .await?;
            }
            for layer in manifest.layers {
                delete_blob_digest(
                    storage,
                    repository_id,
                    repository_name,
                    &layer.digest,
                    deleted_blobs,
                    &mut result,
                )
                .await?;
            }
        }
        DockerManifest::OciIndex(index) => {
            for descriptor in index.manifests {
                if !visited_manifests.contains(&descriptor.digest) {
                    nested.push(format!(
                        "v2/{}/manifests/{}",
                        repository_name, descriptor.digest
                    ));
                }
            }
        }
    }

    Ok(ManifestProcess {
        delta: result,
        nested,
    })
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct PackageDeleteRequest {
    pub paths: Vec<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PackageDeleteResponse {
    pub deleted: usize,
    pub missing: Vec<String>,
    pub rejected: Vec<String>,
}

#[utoipa::path(
    delete,
    path = "/{repository_id}/packages",
    request_body = PackageDeleteRequest,
    params(
        ("repository_id" = Uuid, Path, description = "The Repository ID"),
    ),
    responses(
        (status = 200, description = "Deleted cached packages", body = PackageDeleteResponse),
        (status = 400, description = "Invalid request"),
        (status = 403, description = "Missing permission"),
        (status = 404, description = "Repository not found"),
    )
)]
#[instrument]
pub async fn delete_cached_packages(
    State(site): State<NitroRepo>,
    auth: Authentication,
    Path(repository_id): Path<Uuid>,
    Json(request): Json<PackageDeleteRequest>,
) -> Result<Response, InternalError> {
    if request.paths.is_empty() {
        return Ok(ResponseBuilder::bad_request().body("paths cannot be empty".to_string()));
    }

    let Some(repository) = site.get_repository(repository_id) else {
        return Ok(RepositoryNotFound::Uuid(repository_id).into_response());
    };

    if !auth
        .has_action(RepositoryActions::Edit, repository.id(), site.as_ref())
        .await?
    {
        return Ok(MissingPermission::EditRepository(repository.id()).into_response());
    }

    let strategy = package_strategy(&repository);
    let storage = repository.get_storage();
    let mut deleted = 0usize;
    let mut missing = Vec::new();
    let mut rejected = Vec::new();

    for path in request.paths.iter() {
        if !is_valid_cache_path(path, strategy) {
            rejected.push(path.clone());
            continue;
        }
        if let PackageStrategy::Docker = strategy {
            match delete_docker_package(&storage, repository.id(), path).await {
                Ok(result) => {
                    if result.removed_manifests > 0 {
                        deleted += 1;
                    } else {
                        missing.push(path.clone());
                    }
                }
                Err(DockerDeletionError::ManifestMissing) => {
                    missing.push(path.clone());
                }
                Err(DockerDeletionError::InvalidManifestPath) => {
                    rejected.push(path.clone());
                }
                Err(DockerDeletionError::InvalidManifest(err)) => {
                    warn!(
                        ?err,
                        path, "Failed to parse Docker manifest during deletion"
                    );
                    missing.push(path.clone());
                }
                Err(DockerDeletionError::Storage(err)) => {
                    warn!(?err, path, "Storage error while deleting Docker manifest");
                    missing.push(path.clone());
                }
            }
            continue;
        }
        let storage_path = nr_core::storage::StoragePath::from(path.as_str());
        match storage.delete_file(repository.id(), &storage_path).await {
            Ok(true) => deleted += 1,
            Ok(false) => missing.push(path.clone()),
            Err(err) => {
                warn!(?err, path, "Failed to delete cached package");
                missing.push(path.clone());
            }
        }
    }

    let response = PackageDeleteResponse {
        deleted,
        missing,
        rejected,
    };
    Ok(ResponseBuilder::ok().json(&response))
}

async fn gather_package_dirs(
    storage: &nr_storage::DynStorage,
    repository_id: Uuid,
    base: Option<&str>,
) -> Result<Vec<(String, String)>, nr_storage::StorageError> {
    use std::collections::VecDeque;

    let mut queue: VecDeque<(String, String)> = VecDeque::new();
    let initial_path = base.unwrap_or("");
    queue.push_back((initial_path.to_string(), String::new()));
    let mut packages = Vec::new();

    while let Some((path, relative)) = queue.pop_front() {
        let storage_path = if path.is_empty() {
            nr_core::storage::StoragePath::default()
        } else {
            nr_core::storage::StoragePath::from(path.clone())
        };
        let Some(StorageFile::Directory { files, .. }) =
            storage.open_file(repository_id, &storage_path).await?
        else {
            continue;
        };

        let mut has_files = false;
        for entry in files.iter() {
            if should_ignore(entry.name()) {
                continue;
            }
            match entry.file_type() {
                FileType::File(_) => {
                    has_files = true;
                }
                FileType::Directory(_) => {
                    let name = entry.name();
                    let child_path = if path.is_empty() {
                        format!("{}/", name)
                    } else {
                        format!("{}{}/", path, name)
                    };
                    let child_relative = if relative.is_empty() {
                        name.to_string()
                    } else {
                        format!("{}/{}", relative, name)
                    };
                    queue.push_back((child_path, child_relative));
                }
            }
        }

        if has_files {
            let storage_relative = if !relative.is_empty() {
                relative.trim_matches('/').to_string()
            } else if let Some(prefix) = base {
                path.trim_start_matches(prefix)
                    .trim_matches('/')
                    .to_string()
            } else {
                path.trim_matches('/').to_string()
            };

            if storage_relative.is_empty() {
                continue;
            }

            let mut display_name = storage_relative.clone();
            if let Some(prefix) = base {
                if prefix == "go-proxy-cache/" {
                    if let Some(stripped) = display_name.strip_suffix("/@v") {
                        display_name = stripped.to_string();
                    }
                }
            }
            if !display_name.is_empty() {
                packages.push((display_name, storage_relative));
            }
        }
    }

    Ok(packages)
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;
    use chrono::Utc;
    use nr_core::ConfigTimeStamp;
    use nr_storage::{
        DynStorage, FileContent, StaticStorageFactory,
        local::{LocalConfig, LocalStorageFactory},
    };
    use serde_json::json;
    use sha2::{Digest, Sha256};
    use tempfile::TempDir;

    async fn local_storage() -> Result<(DynStorage, TempDir)> {
        let tempdir = tempfile::tempdir()?;
        let storage_config = nr_storage::StorageConfig {
            storage_config: nr_storage::StorageConfigInner {
                storage_name: "test-storage".into(),
                storage_id: Uuid::new_v4(),
                storage_type: "Local".into(),
                created_at: ConfigTimeStamp::from(Utc::now()),
            },
            type_config: nr_storage::StorageTypeConfig::Local(LocalConfig {
                path: tempdir.path().to_path_buf(),
            }),
        };
        let local = <LocalStorageFactory as StaticStorageFactory>::create_storage_from_config(
            storage_config,
        )
        .await?;
        Ok((DynStorage::Local(local), tempdir))
    }

    #[tokio::test]
    async fn gather_package_dirs_lists_nested_packages() -> Result<()> {
        let (storage, _tempdir) = local_storage().await?;
        let repository = Uuid::new_v4();
        storage
            .save_file(
                repository,
                FileContent::from(b"wheel"),
                &nr_core::storage::StoragePath::from("packages/example/example-1.0.0.whl"),
            )
            .await?;
        storage
            .save_file(
                repository,
                FileContent::from(b"tarball"),
                &nr_core::storage::StoragePath::from("packages/@scope/pkg/pkg-2.3.4.tgz"),
            )
            .await?;

        let mut packages = gather_package_dirs(&storage, repository, Some("packages/")).await?;
        packages.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(
            packages,
            vec![
                ("@scope/pkg".to_string(), "@scope/pkg".to_string()),
                ("example".to_string(), "example".to_string())
            ]
        );
        Ok(())
    }

    #[tokio::test]
    async fn gather_package_dirs_root_lists_python_packages() -> Result<()> {
        let (storage, _tempdir) = local_storage().await?;
        let repository = Uuid::new_v4();
        storage
            .save_file(
                repository,
                FileContent::from(b"wheel"),
                &nr_core::storage::StoragePath::from("example_pkg/1.0.0/example_pkg-1.0.0.whl"),
            )
            .await?;

        let mut packages = gather_package_dirs(&storage, repository, None).await?;
        packages.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(
            packages,
            vec![(
                "example_pkg/1.0.0".to_string(),
                "example_pkg/1.0.0".to_string()
            )]
        );
        Ok(())
    }

    #[tokio::test]
    async fn gather_package_dirs_handles_go_proxy_layout() -> Result<()> {
        let (storage, _tempdir) = local_storage().await?;
        let repository = Uuid::new_v4();
        storage
            .save_file(
                repository,
                FileContent::from(b"info-json"),
                &nr_core::storage::StoragePath::from(
                    "go-proxy-cache/github.com/example/module/@v/v1.0.0.info",
                ),
            )
            .await?;

        let mut packages =
            gather_package_dirs(&storage, repository, Some("go-proxy-cache/")).await?;
        packages.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(
            packages,
            vec![(
                "github.com/example/module".to_string(),
                "github.com/example/module/@v".to_string()
            )]
        );
        Ok(())
    }

    #[tokio::test]
    async fn delete_docker_manifest_removes_all_payloads() -> Result<()> {
        let (storage, _tempdir) = local_storage().await?;
        let repository_id = Uuid::new_v4();
        let repository_name = "library/alpine";

        let config_bytes = b"config-json";
        let layer_a = b"layer-a";
        let layer_b = b"layer-b";
        let config_digest = format!("sha256:{:x}", Sha256::digest(config_bytes));
        let layer_a_digest = format!("sha256:{:x}", Sha256::digest(layer_a));
        let layer_b_digest = format!("sha256:{:x}", Sha256::digest(layer_b));

        let manifest_json = json!({
            "schemaVersion": 2,
            "mediaType": "application/vnd.docker.distribution.manifest.v2+json",
            "config": {
                "mediaType": "application/vnd.docker.container.image.v1+json",
                "size": config_bytes.len(),
                "digest": config_digest,
            },
            "layers": [
                {
                    "mediaType": "application/vnd.docker.image.rootfs.diff.tar",
                    "size": layer_a.len(),
                    "digest": layer_a_digest,
                },
                {
                    "mediaType": "application/vnd.docker.image.rootfs.diff.tar",
                    "size": layer_b.len(),
                    "digest": layer_b_digest,
                }
            ]
        });
        let manifest_bytes = serde_json::to_vec(&manifest_json)?;
        let manifest_digest = format!("sha256:{:x}", Sha256::digest(&manifest_bytes));

        let tag_path =
            nr_core::storage::StoragePath::from(format!("v2/{}/manifests/latest", repository_name));
        storage
            .save_file(
                repository_id,
                FileContent::from(manifest_bytes.clone()),
                &tag_path,
            )
            .await?;

        let digest_path = nr_core::storage::StoragePath::from(format!(
            "v2/{}/manifests/{}",
            repository_name, manifest_digest
        ));
        storage
            .save_file(
                repository_id,
                FileContent::from(manifest_bytes.clone()),
                &digest_path,
            )
            .await?;

        let blobs = [
            (&config_digest, config_bytes.as_slice()),
            (&layer_a_digest, layer_a.as_slice()),
            (&layer_b_digest, layer_b.as_slice()),
        ];

        for (digest, content) in blobs {
            let blob_path = nr_core::storage::StoragePath::from(format!(
                "v2/{}/blobs/{}",
                repository_name, digest
            ));
            storage
                .save_file(
                    repository_id,
                    FileContent::from(content.to_vec()),
                    &blob_path,
                )
                .await?;
        }

        let tag_cache_path = tag_path.to_string();
        let result =
            delete_docker_package(&storage, repository_id, tag_cache_path.as_str()).await?;
        assert_eq!(result.removed_manifests, 2);
        assert_eq!(result.removed_blobs, 3);

        assert!(!storage.file_exists(repository_id, &tag_path).await?);
        assert!(!storage.file_exists(repository_id, &digest_path).await?);

        for digest in [config_digest, layer_a_digest, layer_b_digest] {
            let blob_path = nr_core::storage::StoragePath::from(format!(
                "v2/{}/blobs/{}",
                repository_name, digest
            ));
            assert!(!storage.file_exists(repository_id, &blob_path).await?);
        }

        Ok(())
    }

    #[tokio::test]
    async fn delete_docker_manifest_handles_digest_path() -> Result<()> {
        let (storage, _tempdir) = local_storage().await?;
        let repository_id = Uuid::new_v4();
        let repository_name = "library/busybox";

        let config_bytes = b"config-blob";
        let layer_bytes = b"layer-blob";
        let config_digest = format!("sha256:{:x}", Sha256::digest(config_bytes));
        let layer_digest = format!("sha256:{:x}", Sha256::digest(layer_bytes));

        let manifest_json = json!({
            "schemaVersion": 2,
            "mediaType": "application/vnd.docker.distribution.manifest.v2+json",
            "config": {
                "mediaType": "application/vnd.docker.container.image.v1+json",
                "size": config_bytes.len(),
                "digest": config_digest,
            },
            "layers": [
                {
                    "mediaType": "application/vnd.docker.image.rootfs.diff.tar",
                    "size": layer_bytes.len(),
                    "digest": layer_digest,
                }
            ]
        });
        let manifest_bytes = serde_json::to_vec(&manifest_json)?;
        let manifest_digest = format!("sha256:{:x}", Sha256::digest(&manifest_bytes));

        let digest_path = nr_core::storage::StoragePath::from(format!(
            "v2/{}/manifests/{}",
            repository_name, manifest_digest
        ));
        storage
            .save_file(
                repository_id,
                FileContent::from(manifest_bytes.clone()),
                &digest_path,
            )
            .await?;

        for (digest, content) in [
            (&config_digest, config_bytes.as_slice()),
            (&layer_digest, layer_bytes.as_slice()),
        ] {
            let blob_path = nr_core::storage::StoragePath::from(format!(
                "v2/{}/blobs/{}",
                repository_name, digest
            ));
            storage
                .save_file(
                    repository_id,
                    FileContent::from(content.to_vec()),
                    &blob_path,
                )
                .await?;
        }

        let digest_cache_path = digest_path.to_string();
        let result =
            delete_docker_package(&storage, repository_id, digest_cache_path.as_str()).await?;
        assert_eq!(result.removed_manifests, 1);
        assert_eq!(result.removed_blobs, 2);

        assert!(!storage.file_exists(repository_id, &digest_path).await?);
        for digest in [config_digest, layer_digest] {
            let blob_path = nr_core::storage::StoragePath::from(format!(
                "v2/{}/blobs/{}",
                repository_name, digest
            ));
            assert!(!storage.file_exists(repository_id, &blob_path).await?);
        }

        Ok(())
    }

    #[test]
    fn ignore_hidden_and_meta() {
        assert!(should_ignore(".DS_Store"));
        assert!(should_ignore("package.nr-meta"));
        assert!(!should_ignore("package.tar.gz"));
    }

    #[test]
    fn validate_cache_path_rules() {
        assert!(is_valid_cache_path(
            "packages/example/pkg-1.0.whl",
            PackageStrategy::PackagesDirectory {
                base: Some("packages/"),
            },
        ));
        assert!(!is_valid_cache_path(
            "/etc/passwd",
            PackageStrategy::PackagesDirectory {
                base: Some("packages/"),
            },
        ));
        assert!(!is_valid_cache_path(
            "../packages/pkg.whl",
            PackageStrategy::PackagesDirectory {
                base: Some("packages/"),
            },
        ));
        assert!(!is_valid_cache_path(
            "package.zip",
            PackageStrategy::PackagesDirectory {
                base: Some("packages/"),
            },
        ));
    }

    #[test]
    fn validate_maven_cache_paths() {
        assert!(is_valid_repository_path(
            "com/example/app/1.0.0/app-1.0.0.jar"
        ));
        assert!(!is_valid_repository_path("../com/example/app.jar"));
        assert!(!is_valid_repository_path("/absolute/path"));
        assert!(!is_valid_repository_path(""));
    }

    #[test]
    fn validate_docker_manifest_paths() {
        assert!(is_valid_cache_path(
            "v2/library/nginx/manifests/latest",
            PackageStrategy::Docker,
        ));
        assert!(!is_valid_cache_path(
            "/v2/library/nginx/manifests/latest",
            PackageStrategy::Docker,
        ));
        assert!(!is_valid_cache_path(
            "v2/library/nginx/blobs/sha256:abc",
            PackageStrategy::Docker,
        ));
        assert!(!is_valid_cache_path(
            "v2/library/../../etc/passwd",
            PackageStrategy::Docker,
        ));
    }
}
