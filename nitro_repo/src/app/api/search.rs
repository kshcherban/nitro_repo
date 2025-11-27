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
    repository::{DynRepository, Repository, docker::metadata::collect_manifest_entries},
    utils::ResponseBuilder,
};

mod database;
mod go;
mod query_parser;
mod version_constraint;

use self::query_parser::{SearchQuery, parse_search_query};

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
    PackagesDirectory { base: Option<&'static str> },
    Docker,
    GoModules,
    Database,
}

const fn default_limit() -> usize {
    25
}

fn determine_search_strategy(repository: &DynRepository) -> Option<SearchStrategy> {
    match repository {
        DynRepository::Python(inner) => match inner {
            crate::repository::python::PythonRepository::Hosted(_) => {
                Some(SearchStrategy::PackagesDirectory { base: None })
            }
            _ => Some(SearchStrategy::PackagesDirectory {
                base: Some("packages"),
            }),
        },
        DynRepository::NPM(inner) => match inner {
            crate::repository::npm::NPMRegistry::Hosted(_) => {
                Some(SearchStrategy::PackagesDirectory { base: None })
            }
            crate::repository::npm::NPMRegistry::Proxy(_) => {
                Some(SearchStrategy::PackagesDirectory {
                    base: Some("packages"),
                })
            }
        },
        DynRepository::Php(_) => Some(SearchStrategy::PackagesDirectory {
            base: Some("packages"),
        }),
        DynRepository::Docker(_) => Some(SearchStrategy::Docker),
        DynRepository::Go(inner) => match inner {
            crate::repository::go::GoRepository::Hosted(_) => Some(SearchStrategy::GoModules),
            crate::repository::go::GoRepository::Proxy(_) => {
                Some(SearchStrategy::PackagesDirectory {
                    base: Some("packages"),
                })
            }
        },
        DynRepository::Cargo(_) => Some(SearchStrategy::Database),
        DynRepository::Helm(_) | DynRepository::Maven(_) | DynRepository::Deb(_) => {
            Some(SearchStrategy::Database)
        }
    }
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

pub(crate) struct RepositorySummary {
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
    let raw_query = params.q.trim();
    if raw_query.is_empty() {
        let empty: [PackageSearchResult; 0] = [];
        return Ok(ResponseBuilder::ok().json(&empty));
    }
    let parsed_query = match parse_search_query(raw_query) {
        Ok(query) => query,
        Err(err) => {
            return Ok(ResponseBuilder::bad_request().body(err.to_string()));
        }
    };
    if !parsed_query.has_filters() && !parsed_query.terms.iter().any(|term| term.len() >= 2) {
        let empty: [PackageSearchResult; 0] = [];
        return Ok(ResponseBuilder::ok().json(&empty));
    }
    let limit = params.limit.clamp(1, 200);
    let mut results = Vec::new();

    for (repository_id, repository) in site.loaded_repositories() {
        if results.len() >= limit {
            break;
        }

        let Some(strategy) = determine_search_strategy(&repository) else {
            continue;
        };

        let Some(info) =
            DBRepositoryWithStorageName::get_by_id(repository_id, site.as_ref()).await?
        else {
            continue;
        };

        if matches!(info.visibility, Visibility::Hidden) {
            continue;
        }

        let repo_name = repository.name();
        let summary = RepositorySummary {
            repository_id,
            repository_name: repo_name,
            storage_name: info.storage_name.to_string(),
            repository_type: info.repository_type,
        };

        if !parsed_query.matches_repository(
            &summary.repository_name,
            &summary.storage_name,
            &summary.repository_type,
        ) {
            continue;
        }

        let remaining = min(limit.saturating_sub(results.len()), limit);
        let repo_results = match strategy {
            SearchStrategy::Database => {
                database::search_database_packages(&site, &summary, &parsed_query, remaining)
                    .await?
            }
            SearchStrategy::GoModules => {
                let storage = repository.get_storage();
                go::search_go_modules(&storage, &summary, &parsed_query, remaining).await?
            }
            _ => {
                let storage = repository.get_storage();
                search_repository_storage(&storage, &summary, strategy, &parsed_query, remaining)
                    .await?
            }
        };
        results.extend(repo_results);
    }

    Ok(ResponseBuilder::ok().json(&results))
}

async fn search_repository_storage(
    storage: &DynStorage,
    summary: &RepositorySummary,
    strategy: SearchStrategy,
    query: &SearchQuery,
    limit: usize,
) -> Result<Vec<PackageSearchResult>, InternalError> {
    match strategy {
        SearchStrategy::PackagesDirectory { base } => {
            search_packages_directory(storage, summary, base, query, limit).await
        }
        SearchStrategy::Docker => search_docker_manifests(storage, summary, query, limit).await,
        SearchStrategy::GoModules | SearchStrategy::Database => unreachable!(
            "Go module and database searches handled before delegating to storage search"
        ),
    }
}

async fn search_packages_directory(
    storage: &DynStorage,
    summary: &RepositorySummary,
    base: Option<&str>,
    query: &SearchQuery,
    limit: usize,
) -> Result<Vec<PackageSearchResult>, InternalError> {
    if limit == 0 {
        return Ok(Vec::new());
    }
    let mut matches = Vec::new();
    let mut stack = Vec::new();
    match base {
        Some(dir) if !dir.is_empty() => stack.push(dir.trim_matches('/').to_string()),
        _ => stack.push(String::new()),
    }

    while let Some(directory) = stack.pop() {
        if matches.len() >= limit {
            break;
        }
        let storage_path = if directory.is_empty() {
            StoragePath::from("/")
        } else {
            StoragePath::from(format!("{directory}/"))
        };
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
                    let path = join_file_path(&directory, &entry_name);
                    if !matches_directory_entry(query, &entry_name, &path) {
                        continue;
                    }
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
    query: &SearchQuery,
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

        let repo_refs: Vec<&str> = if entry.repository.is_empty() {
            Vec::new()
        } else {
            vec![entry.repository.as_str()]
        };
        if query.package_filter.is_some()
            && (repo_refs.is_empty() || !query.matches_package_names(&repo_refs))
        {
            continue;
        }

        let file_name = if entry.repository.is_empty() {
            entry.reference.clone()
        } else {
            format!("{}:{}", entry.repository, entry.reference)
        };

        if !query.terms.is_empty() {
            let repo_lower = entry.repository.to_lowercase();
            let reference_lower = entry.reference.to_lowercase();
            let file_lower = file_name.to_lowercase();
            if !query.terms.iter().all(|term| {
                repo_lower.contains(term)
                    || reference_lower.contains(term)
                    || file_lower.contains(term)
            }) {
                continue;
            }
        }

        if !query.matches_version(entry.reference.as_str()) {
            continue;
        }

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

fn matches_directory_entry(query: &SearchQuery, name: &str, path: &str) -> bool {
    if query.package_filter.is_some() && !query.matches_package_names(&[name]) {
        return false;
    }

    if !query.terms.is_empty() {
        let lowered_name = name.to_lowercase();
        let lowered_path = path.to_lowercase();
        if !query
            .terms
            .iter()
            .all(|term| lowered_name.contains(term) || lowered_path.contains(term))
        {
            return false;
        }
    }

    if let Some(_) = query.version_constraint {
        let mut candidates = extract_version_candidates(name);
        candidates.extend(extract_version_candidates(path));
        if candidates.is_empty() {
            candidates.push(name.to_string());
        }
        if !candidates
            .iter()
            .any(|candidate| query.matches_version(candidate))
        {
            return false;
        }
    }

    true
}

fn extract_version_candidates(value: &str) -> Vec<String> {
    let mut candidates = Vec::new();
    for segment in value.split(|c: char| matches!(c, '/' | '\\' | '-' | '_' | '@' | ' ')) {
        if !segment.chars().any(|ch| ch.is_ascii_digit()) {
            continue;
        }
        let cleaned = segment
            .trim_matches(|ch: char| {
                (!ch.is_ascii_alphanumeric() && ch != '.' && ch != '+' && ch != '-') || ch == '"'
            })
            .trim_matches('.');
        if cleaned.is_empty() {
            continue;
        }
        candidates.push(cleaned.to_string());
        if let Some(stripped) = cleaned.strip_prefix('v') {
            if !stripped.is_empty() {
                candidates.push(stripped.to_string());
            }
        }
    }
    candidates
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
mod tests;
