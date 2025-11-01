use std::cmp::min;

use axum::{
    Json,
    extract::{Path, Query, State},
    response::{IntoResponse, Response},
    routing::get,
};
use chrono::{DateTime, FixedOffset};
use nr_storage::{FileType, Storage, StorageFile};
use serde::{Deserialize, Serialize};
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
    repository::{Repository, utils::can_read_repository_with_auth},
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
    let storage = repository.get_storage();
    let packages_root = nr_core::storage::StoragePath::from("packages/");
    let Some(root_dir) = storage.open_file(repository.id(), &packages_root).await? else {
        let empty = PackageListResponse {
            page: query.page,
            per_page: query.per_page,
            total_packages: 0,
            items: Vec::new(),
        };
        return Ok(ResponseBuilder::ok().json(&empty));
    };
    let StorageFile::Directory { .. } = root_dir else {
        let empty = PackageListResponse {
            page: query.page,
            per_page: query.per_page,
            total_packages: 0,
            items: Vec::new(),
        };
        return Ok(ResponseBuilder::ok().json(&empty));
    };

    let mut package_dirs = gather_package_dirs(&storage, repository.id()).await?;

    package_dirs.sort();

    let total_packages = package_dirs.len();
    if total_packages == 0 {
        let empty = PackageListResponse {
            page: query.page,
            per_page: query.per_page,
            total_packages,
            items: Vec::new(),
        };
        return Ok(ResponseBuilder::ok().json(&empty));
    }
    let per_page = query.per_page.clamp(1, 200);
    let page = query.page.max(1);
    let start = (page - 1) * per_page;
    if start >= total_packages {
        let empty = PackageListResponse {
            page,
            per_page,
            total_packages,
            items: Vec::new(),
        };
        return Ok(ResponseBuilder::ok().json(&empty));
    }
    let end = min(start + per_page, total_packages);
    let mut items = Vec::new();
    for package in &package_dirs[start..end] {
        let path = if package.is_empty() {
            "packages/".to_string()
        } else {
            format!("packages/{}/", package)
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
                    let cache_path = if package.is_empty() {
                        format!("packages/{}", meta.name())
                    } else {
                        format!("packages/{}/{}", package, meta.name())
                    };
                    items.push(PackageFileEntry {
                        package: package.clone(),
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
        page,
        per_page,
        total_packages,
        items,
    };
    Ok(ResponseBuilder::ok().json(&response))
}

fn should_ignore(name: &str) -> bool {
    name.starts_with('.') || name.ends_with(".nr-meta")
}

fn is_valid_cache_path(path: &str) -> bool {
    path.starts_with("packages/")
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

    let storage = repository.get_storage();
    let mut deleted = 0usize;
    let mut missing = Vec::new();
    let mut rejected = Vec::new();

    for path in request.paths.iter() {
        if !is_valid_cache_path(path) {
            rejected.push(path.clone());
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
) -> Result<Vec<String>, nr_storage::StorageError> {
    use std::collections::VecDeque;

    let mut queue: VecDeque<(String, String)> = VecDeque::new();
    queue.push_back(("packages/".to_string(), String::new()));
    let mut packages = Vec::new();

    while let Some((path, relative)) = queue.pop_front() {
        let storage_path = nr_core::storage::StoragePath::from(path.clone());
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
                    let child_path = format!("{}{}/", path, name);
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
            let package_name = if relative.is_empty() {
                path.trim_end_matches('/')
                    .split('/')
                    .skip(1)
                    .filter(|segment| !segment.is_empty())
                    .collect::<Vec<_>>()
                    .join("/")
            } else {
                relative.clone()
            };
            if !package_name.is_empty() {
                packages.push(package_name);
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

        let mut packages = gather_package_dirs(&storage, repository).await?;
        packages.sort();
        assert_eq!(
            packages,
            vec!["@scope/pkg".to_string(), "example".to_string()]
        );
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
        assert!(is_valid_cache_path("packages/example/pkg-1.0.whl"));
        assert!(!is_valid_cache_path("/etc/passwd"));
        assert!(!is_valid_cache_path("../packages/pkg.whl"));
        assert!(!is_valid_cache_path("package.zip"));
    }
}
