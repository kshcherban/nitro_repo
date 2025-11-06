use std::cmp::min;

use axum::{
    extract::{Query, State},
    response::Response,
    routing::get,
};
use chrono::{DateTime, FixedOffset};
use nr_core::{
    database::entities::repository::DBRepositoryWithStorageName, repository::Visibility,
    storage::StoragePath,
};
use nr_storage::{DynStorage, FileType, Storage, StorageFile};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, OpenApi, ToSchema};
use uuid::Uuid;

use crate::{
    app::NitroRepo,
    error::InternalError,
    repository::{Repository, docker::metadata::collect_manifest_entries},
    utils::ResponseBuilder,
};

#[derive(OpenApi)]
#[openapi(paths(search_packages), components(schemas(PackageSearchResult)))]
pub struct SearchApi;

pub fn search_routes() -> axum::Router<NitroRepo> {
    axum::Router::new().route("/packages", get(search_packages))
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
struct PackageSearchQuery {
    #[serde(alias = "query")]
    pub q: String,
    #[serde(default = "default_limit")]
    pub limit: usize,
}

#[derive(Debug, Clone, Copy)]
enum SearchStrategy {
    PackagesDirectory,
    Docker,
}

const fn default_limit() -> usize {
    25
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PackageSearchResult {
    pub repository_id: Uuid,
    pub repository_name: String,
    pub storage_name: String,
    pub repository_type: String,
    pub file_name: String,
    pub cache_path: String,
    pub size: u64,
    pub modified: DateTime<FixedOffset>,
}

struct RepositorySummary {
    repository_id: Uuid,
    repository_name: String,
    storage_name: String,
    repository_type: String,
}

#[utoipa::path(
    get,
    path = "/packages",
    params(PackageSearchQuery),
    responses((status = 200, description = "Package search results", body = [PackageSearchResult])),
    tag = "search"
)]
async fn search_packages(
    State(site): State<NitroRepo>,
    Query(params): Query<PackageSearchQuery>,
) -> Result<Response, InternalError> {
    let query = params.q.trim().to_lowercase();
    if query.len() < 2 {
        let empty: [PackageSearchResult; 0] = [];
        return Ok(ResponseBuilder::ok().json(&empty));
    }
    let limit = params.limit.clamp(1, 200);
    let mut results = Vec::new();

    for (repository_id, repository) in site.loaded_repositories() {
        if results.len() >= limit {
            break;
        }

        let repo_type = repository.get_type();
        let strategy = match repo_type {
            "python" | "npm" => SearchStrategy::PackagesDirectory,
            "docker" => SearchStrategy::Docker,
            _ => continue,
        };

        let Some(info) =
            DBRepositoryWithStorageName::get_by_id(repository_id, site.as_ref()).await?
        else {
            continue;
        };

        if matches!(info.visibility, Visibility::Hidden) {
            continue;
        }

        let summary = RepositorySummary {
            repository_id,
            repository_name: repository.name(),
            storage_name: info.storage_name.to_string(),
            repository_type: info.repository_type,
        };

        let storage = repository.get_storage();
        let repo_results = search_repository_storage(
            &storage,
            &summary,
            strategy,
            &query,
            min(limit.saturating_sub(results.len()), limit),
        )
        .await?;
        results.extend(repo_results);
    }

    Ok(ResponseBuilder::ok().json(&results))
}

async fn search_repository_storage(
    storage: &DynStorage,
    summary: &RepositorySummary,
    strategy: SearchStrategy,
    query: &str,
    limit: usize,
) -> Result<Vec<PackageSearchResult>, InternalError> {
    match strategy {
        SearchStrategy::PackagesDirectory => {
            search_packages_directory(storage, summary, query, limit).await
        }
        SearchStrategy::Docker => search_docker_manifests(storage, summary, query, limit).await,
    }
}

async fn search_packages_directory(
    storage: &DynStorage,
    summary: &RepositorySummary,
    query: &str,
    limit: usize,
) -> Result<Vec<PackageSearchResult>, InternalError> {
    if limit == 0 {
        return Ok(Vec::new());
    }
    let mut matches = Vec::new();
    let mut stack = vec!["packages".to_string()];

    while let Some(directory) = stack.pop() {
        if matches.len() >= limit {
            break;
        }
        let storage_path = StoragePath::from(format!("{directory}/"));
        let Some(entry) = storage
            .open_file(summary.repository_id, &storage_path)
            .await?
        else {
            continue;
        };

        let StorageFile::Directory { files, .. } = entry else {
            continue;
        };

        for meta in files {
            if matches.len() >= limit {
                break;
            }
            let entry_name = meta.name().to_string();
            let modified = meta.modified().clone();
            match meta.file_type {
                FileType::Directory(_) => {
                    stack.push(join_dir_path(&directory, &entry_name));
                }
                FileType::File(file_meta) => {
                    if should_ignore(&entry_name) {
                        continue;
                    }
                    if !entry_name.to_lowercase().contains(query) {
                        continue;
                    }
                    let path = join_file_path(&directory, &entry_name);
                    matches.push(PackageSearchResult {
                        repository_id: summary.repository_id,
                        repository_name: summary.repository_name.clone(),
                        storage_name: summary.storage_name.clone(),
                        repository_type: summary.repository_type.clone(),
                        file_name: entry_name,
                        cache_path: path,
                        size: file_meta.file_size,
                        modified,
                    });
                }
            }
        }
    }

    Ok(matches)
}

async fn search_docker_manifests(
    storage: &DynStorage,
    summary: &RepositorySummary,
    query: &str,
    limit: usize,
) -> Result<Vec<PackageSearchResult>, InternalError> {
    if limit == 0 {
        return Ok(Vec::new());
    }

    let manifests = collect_manifest_entries(storage, summary.repository_id)
        .await
        .map_err(InternalError::from)?;

    let mut matches = Vec::new();

    for entry in manifests {
        if matches.len() >= limit {
            break;
        }

        let repo_lower = entry.repository.to_lowercase();
        let reference_lower = entry.reference.to_lowercase();
        if !repo_lower.contains(query) && !reference_lower.contains(query) {
            continue;
        }

        let file_name = if entry.repository.is_empty() {
            entry.reference.clone()
        } else {
            format!("{}:{}", entry.repository, entry.reference)
        };

        matches.push(PackageSearchResult {
            repository_id: summary.repository_id,
            repository_name: summary.repository_name.clone(),
            storage_name: summary.storage_name.clone(),
            repository_type: summary.repository_type.clone(),
            file_name,
            cache_path: entry.cache_path,
            size: entry.size,
            modified: entry.modified,
        });
    }

    Ok(matches)
}

fn join_dir_path(parent: &str, child: &str) -> String {
    let base = parent.trim_end_matches('/');
    if base.is_empty() {
        format!("{child}")
    } else {
        format!("{base}/{child}")
    }
}

fn join_file_path(parent: &str, file: &str) -> String {
    let base = parent.trim_end_matches('/');
    if base.is_empty() {
        file.to_string()
    } else {
        format!("{base}/{file}")
    }
}

fn should_ignore(name: &str) -> bool {
    name.starts_with('.') || name.ends_with(".nr-meta") || name.ends_with(".metadata")
}

#[cfg(test)]
mod tests {
    use bytes::Bytes;
    use nr_storage::{FileContent, Storage};

    use super::*;
    use crate::repository::test_helpers::test_storage;

    #[tokio::test]
    async fn search_repository_storage_finds_packages() {
        let storage = test_storage().await;
        let repo_id = Uuid::new_v4();
        let path = StoragePath::from(
            "packages/bc/66/875d449b23194f45debb8a2b70c704217f0aa2700d967098b2e1b812dd44/parallel_ssh-2.12.0-py3-none-any.whl",
        );
        storage
            .save_file(
                repo_id,
                FileContent::Bytes(Bytes::from_static(b"data")),
                &path,
            )
            .await
            .unwrap();
        let summary = RepositorySummary {
            repository_id: repo_id,
            repository_name: "py-proxy".into(),
            storage_name: "test".into(),
            repository_type: "python".into(),
        };

        let results = super::search_repository_storage(
            &storage,
            &summary,
            SearchStrategy::PackagesDirectory,
            "parallel",
            10,
        )
        .await
        .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].file_name, "parallel_ssh-2.12.0-py3-none-any.whl");
    }

    #[tokio::test]
    async fn search_repository_storage_ignores_metadata() {
        let storage = test_storage().await;
        let repo_id = Uuid::new_v4();
        let base = "packages/a1/b2/";
        let file_path = StoragePath::from(format!("{base}artifact-1.0.0.whl"));
        let metadata_path = StoragePath::from(format!("{base}artifact-1.0.0.whl.metadata"));
        storage
            .save_file(
                repo_id,
                FileContent::Bytes(Bytes::from_static(b"data")),
                &file_path,
            )
            .await
            .unwrap();
        storage
            .save_file(
                repo_id,
                FileContent::Bytes(Bytes::from_static(b"meta")),
                &metadata_path,
            )
            .await
            .unwrap();
        let summary = RepositorySummary {
            repository_id: repo_id,
            repository_name: "py-proxy".into(),
            storage_name: "test".into(),
            repository_type: "python".into(),
        };

        let results = super::search_repository_storage(
            &storage,
            &summary,
            SearchStrategy::PackagesDirectory,
            "artifact",
            10,
        )
        .await
        .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].file_name, "artifact-1.0.0.whl");
    }

    #[tokio::test]
    async fn search_repository_storage_respects_limit() {
        let storage = test_storage().await;
        let repo_id = Uuid::new_v4();
        for i in 0..3 {
            let path = StoragePath::from(format!("packages/{:02}/pkg/package-{i}.whl", i));
            storage
                .save_file(
                    repo_id,
                    FileContent::Bytes(Bytes::from_static(b"data")),
                    &path,
                )
                .await
                .unwrap();
        }
        let summary = RepositorySummary {
            repository_id: repo_id,
            repository_name: "py-proxy".into(),
            storage_name: "test".into(),
            repository_type: "python".into(),
        };

        let results = super::search_repository_storage(
            &storage,
            &summary,
            SearchStrategy::PackagesDirectory,
            "package",
            2,
        )
        .await
        .unwrap();
        assert_eq!(results.len(), 2);
    }

    #[tokio::test]
    async fn search_repository_storage_finds_docker_images() {
        let storage = test_storage().await;
        let repo_id = Uuid::new_v4();
        let manifest_path = StoragePath::from("v2/library/nginx/manifests/latest");
        let manifest = r#"
        {
            "schemaVersion": 2,
            "mediaType": "application/vnd.docker.distribution.manifest.v2+json",
            "config": {
                "mediaType": "application/vnd.docker.container.image.v1+json",
                "size": 7023,
                "digest": "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
            },
            "layers": [
                {
                    "mediaType": "application/vnd.docker.image.rootfs.diff.tar.gzip",
                    "size": 32654,
                    "digest": "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                }
            ]
        }
        "#;
        storage
            .save_file(
                repo_id,
                FileContent::Bytes(Bytes::from(manifest)),
                &manifest_path,
            )
            .await
            .unwrap();

        let summary = RepositorySummary {
            repository_id: repo_id,
            repository_name: "docker-hosted".into(),
            storage_name: "test".into(),
            repository_type: "docker".into(),
        };

        let results = super::search_repository_storage(
            &storage,
            &summary,
            SearchStrategy::Docker,
            "nginx",
            10,
        )
        .await
        .unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].file_name, "library/nginx:latest");
        assert_eq!(results[0].cache_path, "v2/library/nginx/manifests/latest");
        assert_eq!(results[0].size, 7023 + 32654);
    }
}
