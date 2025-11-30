use std::collections::VecDeque;

use anyhow::{anyhow, bail};
use nr_core::storage::StoragePath;
use nr_storage::{DynStorage, FileType, Storage, StorageFile};
use tracing::{info, warn};
use uuid::Uuid;

use crate::{
    app::NitroRepo,
    repository::{
        DynRepository, Repository,
        python::{PythonRepository, hosted::PythonHosted},
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReindexKind {
    PythonHosted,
}

pub async fn reindex_repository(
    site: NitroRepo,
    repository_id: Uuid,
    kind: ReindexKind,
) -> anyhow::Result<usize> {
    let repository = site
        .get_repository(repository_id)
        .ok_or_else(|| anyhow!("Repository {repository_id} is not loaded"))?;

    match (kind, repository) {
        (ReindexKind::PythonHosted, DynRepository::Python(PythonRepository::Hosted(repo))) => {
            reindex_python_repo(&repo).await
        }
        (ReindexKind::PythonHosted, other) => bail!(
            "Repository {repository_id} is of type {} but python-hosted reindexing was requested",
            other.full_type()
        ),
    }
}

async fn reindex_python_repo(repo: &PythonHosted) -> anyhow::Result<usize> {
    let storage = repo.get_storage();
    let repository_id = repo.id();
    let paths = collect_repository_files(&storage, repository_id).await?;

    info!(
        repo_id = %repository_id,
        count = paths.len(),
        "Discovered potential python artifacts for reindex"
    );

    let mut processed = 0usize;
    for path in paths {
        match crate::repository::python::utils::PythonPackagePathInfo::try_from(&path) {
            Ok(info) => {
                if let Err(err) = repo.upsert_metadata(None, &info).await {
                    warn!(
                        ?err,
                        repository = %repository_id,
                        package = %info.package,
                        version = %info.version,
                        "Failed to upsert python metadata during reindex"
                    );
                    continue;
                }
                processed += 1;
            }
            Err(_) => {
                // Ignore non-package files (metadata, README, etc.)
                continue;
            }
        }
    }

    Ok(processed)
}

async fn collect_repository_files(
    storage: &DynStorage,
    repository_id: Uuid,
) -> Result<Vec<StoragePath>, nr_storage::StorageError> {
    let mut files = Vec::new();
    let mut stack = VecDeque::new();
    stack.push_back(StoragePath::default());

    while let Some(current) = stack.pop_front() {
        let Some(node) = storage.open_file(repository_id, &current).await? else {
            continue;
        };

        match node {
            StorageFile::Directory { files: entries, .. } => {
                for entry in entries {
                    let name = entry.name();
                    if should_skip(name) {
                        continue;
                    }
                    match entry.file_type() {
                        FileType::File(_) => {
                            let mut path = current.clone();
                            path.push_mut(name);
                            files.push(path);
                        }
                        FileType::Directory(_) => {
                            let mut path = current.clone();
                            path.push_mut(name);
                            stack.push_back(path);
                        }
                    }
                }
            }
            StorageFile::File { .. } => {
                files.push(current);
            }
        }
    }

    Ok(files)
}

fn should_skip(name: &str) -> bool {
    name.is_empty() || name.starts_with('.') || name.ends_with(".nr-meta")
}

#[cfg(test)]
mod tests {
    use super::{collect_repository_files, should_skip};
    use chrono::Utc;
    use nr_core::{ConfigTimeStamp, storage::StoragePath};
    use nr_storage::{
        DynStorage, FileContent, StaticStorageFactory, Storage,
        local::{LocalConfig, LocalStorageFactory},
    };
    use tempfile::TempDir;
    use uuid::Uuid;

    async fn local_storage() -> (DynStorage, TempDir) {
        let tempdir = tempfile::tempdir().expect("tempdir");
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
        let storage = <LocalStorageFactory as StaticStorageFactory>::create_storage_from_config(
            storage_config,
        )
        .await
        .expect("storage");
        (DynStorage::Local(storage), tempdir)
    }

    #[tokio::test]
    async fn collect_repository_files_discovers_artifacts() {
        let (storage, _tempdir) = local_storage().await;
        let repo = Uuid::new_v4();
        storage
            .save_file(
                repo,
                FileContent::from(b"wheel".as_slice()),
                &StoragePath::from("example_pkg/1.0.0/example_pkg-1.0.0.whl"),
            )
            .await
            .expect("write wheel");
        storage
            .save_file(
                repo,
                FileContent::from(b"readme".as_slice()),
                &StoragePath::from("example_pkg/README"),
            )
            .await
            .expect("write readme");

        let files = collect_repository_files(&storage, repo)
            .await
            .expect("collect");
        let file_list: Vec<String> = files.into_iter().map(|p| p.to_string()).collect();
        assert!(file_list.contains(&"example_pkg/1.0.0/example_pkg-1.0.0.whl".to_string()));
        assert!(file_list.contains(&"example_pkg/README".to_string()));
    }

    #[test]
    fn should_skip_filters_hidden_entries() {
        assert!(should_skip(".DS_Store"));
        assert!(should_skip("package.nr-meta"));
        assert!(!should_skip("package.whl"));
    }
}
