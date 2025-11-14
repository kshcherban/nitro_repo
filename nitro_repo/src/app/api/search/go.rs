use std::collections::VecDeque;

use nr_core::storage::StoragePath;
use nr_storage::{DynStorage, FileType, Storage, StorageFile};

use super::{InternalError, PackageSearchResult, RepositorySummary, query_parser::SearchQuery};

const HOSTED_BASE: &str = "packages";
const PROXY_BASE: &str = "go-proxy-cache";

pub async fn search_go_modules(
    storage: &DynStorage,
    summary: &RepositorySummary,
    query: &SearchQuery,
    limit: usize,
) -> Result<Vec<PackageSearchResult>, InternalError> {
    if limit == 0 {
        return Ok(Vec::new());
    }

    let mut matches = Vec::new();
    collect_from_base(storage, summary, query, HOSTED_BASE, limit, &mut matches).await?;
    if matches.len() < limit {
        collect_from_base(storage, summary, query, PROXY_BASE, limit, &mut matches).await?;
    }
    Ok(matches)
}

async fn collect_from_base(
    storage: &DynStorage,
    summary: &RepositorySummary,
    query: &SearchQuery,
    base: &str,
    limit: usize,
    results: &mut Vec<PackageSearchResult>,
) -> Result<(), InternalError> {
    let mut queue: VecDeque<String> = VecDeque::new();
    queue.push_back(base.to_string());

    while let Some(current) = queue.pop_front() {
        if results.len() >= limit {
            break;
        }

        let storage_path = if current.is_empty() {
            StoragePath::default()
        } else {
            StoragePath::from(format!("{current}/"))
        };

        let Some(StorageFile::Directory { files, .. }) = storage
            .open_file(summary.repository_id, &storage_path)
            .await?
        else {
            continue;
        };

        for entry in files {
            if results.len() >= limit {
                break;
            }
            let name = entry.name();
            match entry.file_type() {
                FileType::Directory(_) => {
                    let child_path = join_dir_path(&current, name);
                    queue.push_back(child_path);
                }
                FileType::File(file_meta) => {
                    if !name.ends_with(".info") {
                        continue;
                    }
                    let relative_path = join_file_path(&current, name);
                    if let Some((module, version)) = parse_go_entry(&relative_path, base) {
                        let name_refs = [module.as_str()];
                        if !query.matches_package_names(&name_refs) {
                            continue;
                        }
                        if !query.matches_version(&version) {
                            continue;
                        }

                        results.push(PackageSearchResult {
                            repository_id: summary.repository_id,
                            repository_name: summary.repository_name.clone(),
                            storage_name: summary.storage_name.clone(),
                            repository_type: summary.repository_type.clone(),
                            file_name: format!("{module}@{version}"),
                            cache_path: relative_path,
                            size: file_meta.file_size,
                            modified: entry.modified().clone(),
                        });
                    }
                }
            }
        }
    }

    Ok(())
}

fn join_dir_path(parent: &str, child: &str) -> String {
    let base = parent.trim_end_matches('/');
    if base.is_empty() {
        child.to_string()
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

fn parse_go_entry(path: &str, base: &str) -> Option<(String, String)> {
    let trimmed = path.trim_start_matches(base).trim_start_matches('/');
    let (module_part, file_part) = trimmed.split_once("/@v/")?;
    if file_part == "list" {
        return None;
    }
    let version = file_part.strip_suffix(".info")?.to_string();
    Some((module_part.to_string(), version))
}

#[cfg(test)]
mod tests {
    use bytes::Bytes;
    use nr_storage::{FileContent, Storage};
    use uuid::Uuid;

    use super::*;
    use crate::app::api::search::query_parser::SearchQuery;
    use crate::repository::test_helpers::test_storage;

    #[tokio::test]
    async fn search_go_modules_includes_hosted_packages() {
        let storage = test_storage().await;
        let repository_id = Uuid::new_v4();
        storage
            .save_file(
                repository_id,
                FileContent::Bytes(Bytes::from_static(b"mod-zip")),
                &StoragePath::from("packages/github.com/example/project/@v/v1.2.3.info"),
            )
            .await
            .unwrap();

        let summary = RepositorySummary {
            repository_id,
            repository_name: "go-hosted".into(),
            storage_name: "local".into(),
            repository_type: "go".into(),
        };
        let query = SearchQuery {
            terms: vec!["project".to_string()],
            ..SearchQuery::default()
        };

        let result = search_go_modules(&storage, &summary, &query, 10).await;
        assert!(result.is_ok());
        let packages = result.unwrap();
        assert_eq!(packages.len(), 1);
        assert_eq!(packages[0].file_name, "github.com/example/project@v1.2.3");
    }

    #[tokio::test]
    async fn search_go_modules_limits_proxy_results() {
        let storage = test_storage().await;
        let repository_id = Uuid::new_v4();
        storage
            .save_file(
                repository_id,
                FileContent::Bytes(Bytes::from_static(b"info-json")),
                &StoragePath::from("go-proxy-cache/github.com/example/project/@v/v1.0.0.info"),
            )
            .await
            .unwrap();
        storage
            .save_file(
                repository_id,
                FileContent::Bytes(Bytes::from_static(b"info-json")),
                &StoragePath::from("go-proxy-cache/github.com/example/other/@v/v1.0.0.info"),
            )
            .await
            .unwrap();

        let summary = RepositorySummary {
            repository_id,
            repository_name: "go-proxy".into(),
            storage_name: "local".into(),
            repository_type: "go".into(),
        };
        let query = SearchQuery {
            terms: vec!["github.com/example".to_string()],
            ..SearchQuery::default()
        };

        let result = search_go_modules(&storage, &summary, &query, 1).await;
        assert!(result.is_ok());
        let packages = result.unwrap();
        assert_eq!(packages.len(), 1);
    }
}
