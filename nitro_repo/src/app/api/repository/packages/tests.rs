#![allow(clippy::expect_used, clippy::panic, clippy::todo, clippy::unwrap_used)]
use super::*;
use anyhow::Result;
use chrono::{FixedOffset, Utc};
use nr_core::ConfigTimeStamp;
use nr_storage::{
    DynStorage, FileContent, StaticStorageFactory,
    local::{LocalConfig, LocalStorageFactory},
};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::sync::Arc;
use tempfile::TempDir;
use tokio::{
    sync::Barrier,
    time::{Duration, sleep, timeout},
};

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
    let local =
        <LocalStorageFactory as StaticStorageFactory>::create_storage_from_config(storage_config)
            .await?;
    Ok((DynStorage::Local(local), tempdir))
}

#[tokio::test]
async fn map_ordered_concurrent_executes_tasks_in_parallel() -> Result<()> {
    let barrier = Arc::new(Barrier::new(2));
    let inputs = vec![1, 2];
    let fut = super::map_ordered_concurrent(inputs.clone(), 2, move |value| {
        let barrier = barrier.clone();
        async move {
            barrier.wait().await;
            Ok::<_, ()>(value)
        }
    });

    let values = timeout(Duration::from_millis(250), fut)
        .await
        .expect("tasks should complete in parallel")
        .expect("task execution should succeed");
    assert_eq!(values, inputs);
    Ok(())
}

#[tokio::test]
async fn map_ordered_concurrent_preserves_input_order() -> Result<()> {
    let inputs = vec![1, 2, 3, 4];
    let results = super::map_ordered_concurrent(inputs.clone(), 4, move |value| async move {
        let delay = Duration::from_millis((5 - value) as u64 * 5);
        sleep(delay).await;
        Ok::<_, ()>(value * 2)
    })
    .await
    .expect("task execution should succeed");

    assert_eq!(results, inputs.iter().map(|v| v * 2).collect::<Vec<_>>());
    Ok(())
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

    let mut packages = gather_package_dirs(&storage, repository, Some("go-proxy-cache/")).await?;
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
async fn build_maven_proxy_package_list_exposes_cached_files() -> Result<()> {
    let (storage, _tempdir) = local_storage().await?;
    let repository = Uuid::new_v4();

    let base_dir = "com/example/demo/1.0.0";
    storage
        .save_file(
            repository,
            FileContent::from(b"jar-bytes"),
            &nr_core::storage::StoragePath::from(format!("{base_dir}/demo-1.0.0.jar")),
        )
        .await?;
    storage
        .save_file(
            repository,
            FileContent::from(b"pom-bytes"),
            &nr_core::storage::StoragePath::from(format!("{base_dir}/demo-1.0.0.pom")),
        )
        .await?;
    storage
        .save_file(
            repository,
            FileContent::from(b"metadata"),
            &nr_core::storage::StoragePath::from("com/example/demo/maven-metadata.xml"),
        )
        .await?;

    let response = super::build_maven_proxy_package_list(&storage, repository, 1, 50).await?;
    assert_eq!(response.total_packages, 1);
    assert_eq!(response.items.len(), 2);

    let mut names: Vec<&str> = response
        .items
        .iter()
        .map(|item| item.name.as_str())
        .collect();
    names.sort_unstable();
    assert_eq!(names, vec!["demo-1.0.0.jar", "demo-1.0.0.pom"]);
    assert!(
        response
            .items
            .iter()
            .all(|item| item.package == "com.example:demo:1.0.0")
    );
    assert!(
        response
            .items
            .iter()
            .all(|item| item.cache_path.starts_with("com/example/demo/1.0.0/"))
    );
    Ok(())
}

#[tokio::test]
async fn build_maven_proxy_package_list_paginates_versions() -> Result<()> {
    let (storage, _tempdir) = local_storage().await?;
    let repository = Uuid::new_v4();

    let versions = [("1.0.0", b"v1"), ("1.1.0", b"v2")];
    for (version, data) in versions.iter() {
        let base_dir = format!("com/example/demo/{version}");
        storage
            .save_file(
                repository,
                FileContent::from(*data),
                &nr_core::storage::StoragePath::from(format!("{base_dir}/demo-{version}.jar")),
            )
            .await?;
        storage
            .save_file(
                repository,
                FileContent::from(*data),
                &nr_core::storage::StoragePath::from(format!("{base_dir}/demo-{version}.pom")),
            )
            .await?;
    }

    let first_page = super::build_maven_proxy_package_list(&storage, repository, 1, 1).await?;
    assert_eq!(first_page.total_packages, 2);
    assert!(
        first_page
            .items
            .iter()
            .all(|item| item.package.ends_with(":1.0.0"))
    );

    let second_page = super::build_maven_proxy_package_list(&storage, repository, 2, 1).await?;
    assert_eq!(second_page.total_packages, 2);
    assert!(
        second_page
            .items
            .iter()
            .all(|item| item.package.ends_with(":1.1.0"))
    );
    Ok(())
}

#[tokio::test]
async fn collect_go_package_entries_deduplicates_versions() -> Result<()> {
    let (storage, _tempdir) = local_storage().await?;
    let repository = Uuid::new_v4();
    storage
        .save_file(
            repository,
            FileContent::from(b"info"),
            &nr_core::storage::StoragePath::from(
                "go-proxy-cache/github.com/example/module/@v/v1.0.0.info",
            ),
        )
        .await?;
    storage
        .save_file(
            repository,
            FileContent::from(b"zip"),
            &nr_core::storage::StoragePath::from(
                "go-proxy-cache/github.com/example/module/@v/v1.0.0.zip",
            ),
        )
        .await?;
    storage
        .save_file(
            repository,
            FileContent::from(b"mod"),
            &nr_core::storage::StoragePath::from(
                "go-proxy-cache/github.com/example/module/@v/v1.1.0.mod",
            ),
        )
        .await?;

    let entries =
        super::collect_go_package_entries(&storage, repository, "go-proxy-cache/").await?;
    assert_eq!(entries.len(), 2);
    let mut versions: Vec<_> = entries.iter().map(|entry| entry.name.clone()).collect();
    versions.sort();
    assert_eq!(versions, vec!["v1.0.0".to_string(), "v1.1.0".to_string()]);
    let zip_entry = entries
        .iter()
        .find(|entry| entry.name == "v1.0.0")
        .expect("zip entry present");
    assert!(
        zip_entry.cache_path.ends_with(".zip"),
        "expected zip cache path but found {}",
        zip_entry.cache_path
    );
    Ok(())
}

#[tokio::test]
async fn collect_go_package_entries_handles_hosted_storage() -> Result<()> {
    let (storage, _tempdir) = local_storage().await?;
    let repository = Uuid::new_v4();
    storage
        .save_file(
            repository,
            FileContent::from(b"zip"),
            &nr_core::storage::StoragePath::from(
                "packages/github.com/example/module/@v/v2.3.4.zip",
            ),
        )
        .await?;

    let entries = super::collect_go_package_entries(&storage, repository, "packages/").await?;
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].package, "github.com/example/module");
    assert_eq!(entries[0].name, "v2.3.4");
    assert!(
        entries[0].cache_path.ends_with(".zip"),
        "expected zip cache path but found {}",
        entries[0].cache_path
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

    let digest_path_str = format!("v2/{}/manifests/{}", repository_name, manifest_digest);
    let digest_path = nr_core::storage::StoragePath::from(digest_path_str.clone());
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
        let blob_path =
            nr_core::storage::StoragePath::from(format!("v2/{}/blobs/{}", repository_name, digest));
        storage
            .save_file(
                repository_id,
                FileContent::from(content.to_vec()),
                &blob_path,
            )
            .await?;
    }

    let tag_cache_path = tag_path.to_string();
    let result = delete_docker_package(&storage, repository_id, tag_cache_path.as_str()).await?;
    assert_eq!(result.removed_manifests, 2);
    assert_eq!(result.removed_blobs, 3);

    assert!(!storage.file_exists(repository_id, &tag_path).await?);
    assert!(!storage.file_exists(repository_id, &digest_path).await?);

    for digest in [config_digest, layer_a_digest, layer_b_digest] {
        let blob_path =
            nr_core::storage::StoragePath::from(format!("v2/{}/blobs/{}", repository_name, digest));
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
        let blob_path =
            nr_core::storage::StoragePath::from(format!("v2/{}/blobs/{}", repository_name, digest));
        storage
            .save_file(
                repository_id,
                FileContent::from(content.to_vec()),
                &blob_path,
            )
            .await?;
    }

    let digest_cache_path = digest_path.to_string();
    let result = delete_docker_package(&storage, repository_id, digest_cache_path.as_str()).await?;
    assert_eq!(result.removed_manifests, 1);
    assert_eq!(result.removed_blobs, 2);

    assert!(!storage.file_exists(repository_id, &digest_path).await?);
    for digest in [config_digest, layer_digest] {
        let blob_path =
            nr_core::storage::StoragePath::from(format!("v2/{}/blobs/{}", repository_name, digest));
        assert!(!storage.file_exists(repository_id, &blob_path).await?);
    }

    Ok(())
}

#[tokio::test]
async fn collect_docker_deletions_batch_deduplicates_shared_layers() -> Result<()> {
    let (storage, _tempdir) = local_storage().await?;
    let repository_id = Uuid::new_v4();
    let repository_name = "library/shared";

    let config_bytes = b"config-json";
    let layer_bytes = b"layer-bytes";
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

    // Two tags pointing to the same manifest
    let tag_paths = [
        format!("v2/{}/manifests/latest", repository_name),
        format!("v2/{}/manifests/v1", repository_name),
    ];

    for tag in tag_paths.iter() {
        storage
            .save_file(
                repository_id,
                FileContent::from(manifest_bytes.clone()),
                &nr_core::storage::StoragePath::from(tag.as_str()),
            )
            .await?;
    }

    // Store the digest manifest and blobs
    let digest_path_str = format!("v2/{}/manifests/{}", repository_name, manifest_digest);
    let digest_path = nr_core::storage::StoragePath::from(digest_path_str.clone());
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
        let blob_path =
            nr_core::storage::StoragePath::from(format!("v2/{}/blobs/{}", repository_name, digest));
        storage
            .save_file(
                repository_id,
                FileContent::from(content.to_vec()),
                &blob_path,
            )
            .await?;
    }

    let batch = super::collect_docker_deletions_batch(
        &storage,
        repository_id,
        &tag_paths.iter().cloned().collect::<Vec<_>>(),
    )
    .await?;

    assert!(batch.deleted_objects > 0);
    assert_eq!(batch.deleted_packages, 2);
    assert!(batch.missing.is_empty());
    assert!(batch.rejected.is_empty());

    for tag in tag_paths.iter() {
        let tag_storage_path = nr_core::storage::StoragePath::from(tag.as_str());
        assert!(
            !storage
                .file_exists(repository_id, &tag_storage_path)
                .await?
        );

        let sidecar = nr_core::storage::StoragePath::from(format!("{tag}.nr-docker-tagmeta"));
        assert!(!storage.file_exists(repository_id, &sidecar).await?);
    }

    assert!(!storage.file_exists(repository_id, &digest_path).await?);
    let digest_sidecar =
        nr_core::storage::StoragePath::from(format!("{digest_path_str}.nr-docker-tagmeta"));
    assert!(!storage.file_exists(repository_id, &digest_sidecar).await?);

    for digest in [&config_digest, &layer_digest] {
        let blob_path =
            nr_core::storage::StoragePath::from(format!("v2/{}/blobs/{}", repository_name, digest));
        assert!(!storage.file_exists(repository_id, &blob_path).await?);
    }

    Ok(())
}

#[tokio::test]
async fn collect_docker_deletions_batch_streams_large_batches() -> Result<()> {
    const LARGE_DELETE_COUNT: usize = 1_200;

    let (storage, _tempdir) = local_storage().await?;
    let repository_id = Uuid::new_v4();
    let repository_name = "library/huge";

    let config_bytes = b"config-json";
    let layer_bytes = b"layer-bytes";
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

    let digest_path_str = format!("v2/{}/manifests/{}", repository_name, manifest_digest);
    let digest_path = nr_core::storage::StoragePath::from(digest_path_str.clone());
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
        let blob_path =
            nr_core::storage::StoragePath::from(format!("v2/{}/blobs/{}", repository_name, digest));
        storage
            .save_file(
                repository_id,
                FileContent::from(content.to_vec()),
                &blob_path,
            )
            .await?;
    }

    let manifest_paths: Vec<String> = (0..LARGE_DELETE_COUNT)
        .map(|index| format!("v2/{}/manifests/tag-{index}", repository_name))
        .collect();

    for path in manifest_paths.iter() {
        let storage_path = nr_core::storage::StoragePath::from(path.as_str());
        storage
            .save_file(
                repository_id,
                FileContent::from(manifest_bytes.clone()),
                &storage_path,
            )
            .await?;
    }

    let batch =
        super::collect_docker_deletions_batch(&storage, repository_id, &manifest_paths).await?;

    assert!(batch.deleted_objects > 0);
    assert_eq!(batch.deleted_packages, LARGE_DELETE_COUNT);
    assert!(batch.missing.is_empty());
    assert!(batch.rejected.is_empty());

    assert!(!storage.file_exists(repository_id, &digest_path).await?);
    let digest_sidecar =
        nr_core::storage::StoragePath::from(format!("{digest_path_str}.nr-docker-tagmeta"));
    assert!(!storage.file_exists(repository_id, &digest_sidecar).await?);

    for path in manifest_paths.iter() {
        let storage_path = nr_core::storage::StoragePath::from(path.as_str());
        assert!(!storage.file_exists(repository_id, &storage_path).await?);

        let sidecar = nr_core::storage::StoragePath::from(format!("{path}.nr-docker-tagmeta"));
        assert!(!storage.file_exists(repository_id, &sidecar).await?);
    }

    for digest in [&config_digest, &layer_digest] {
        let blob_path =
            nr_core::storage::StoragePath::from(format!("v2/{}/blobs/{}", repository_name, digest));
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

#[test]
fn cargo_cache_path_matches_crate_layout() {
    let path = super::cargo_cache_path("serde", "1.0.0");
    assert_eq!(path, "crates/serde/1.0.0/serde-1.0.0.crate");
}

#[test]
fn cargo_package_entry_uses_metadata() {
    let mut metadata = CargoPackageMetadata::default();
    metadata.crate_size = 1_337;
    let updated_at = chrono::DateTime::from_timestamp(1_700_000_000, 0)
        .unwrap()
        .with_timezone(&FixedOffset::east_opt(0).unwrap());
    let entry = super::build_cargo_package_entry("Serde", "serde", "1.0.0", updated_at, &metadata);
    assert_eq!(entry.package, "Serde");
    assert_eq!(entry.name, "1.0.0");
    assert_eq!(entry.size, 1_337);
    assert_eq!(entry.cache_path, "crates/serde/1.0.0/serde-1.0.0.crate");
    assert_eq!(entry.modified, updated_at);
}

#[test]
fn validate_cargo_cache_paths() {
    assert!(is_valid_cache_path(
        "crates/serde/1.0.0/serde-1.0.0.crate",
        PackageStrategy::Cargo
    ));
    assert!(!is_valid_cache_path(
        "/crates/serde/1.0.0/serde-1.0.0.crate",
        PackageStrategy::Cargo
    ));
    assert!(!is_valid_cache_path(
        "../crates/serde/1.0.0/serde-1.0.0.crate",
        PackageStrategy::Cargo
    ));
}

fn pkg_obj(key: &str, size: u64) -> PackageObject {
    PackageObject {
        key: key.to_string(),
        size,
        modified: chrono::Local::now().fixed_offset(),
    }
}

#[test]
fn build_package_page_groups_by_directory_and_ignores_meta() {
    let objects = vec![
        pkg_obj("packages/@scope/pkg/pkg-2.3.4.tgz", 42),
        // Hidden metadata object should be ignored
        pkg_obj("packages/@scope/pkg/pkg-2.3.4.tgz.nr-meta", 1),
        pkg_obj("packages/example/example-1.0.0.whl", 10),
    ];

    let response = super::build_package_page_from_objects(objects, Some("packages/"), 1, 50);

    assert_eq!(response.total_packages, 2);
    // Package ordering follows lexicographic directory order
    assert_eq!(response.items.len(), 2);
    assert_eq!(response.items[0].package, "@scope/pkg");
    assert_eq!(
        response.items[0].cache_path,
        "packages/@scope/pkg/pkg-2.3.4.tgz"
    );
    assert_eq!(response.items[1].package, "example");
}

#[test]
fn build_package_page_respects_pagination() {
    let objects = vec![
        pkg_obj("packages/alpha/a-1.tgz", 1),
        pkg_obj("packages/bravo/b-1.tgz", 1),
        pkg_obj("packages/charlie/c-1.tgz", 1),
    ];

    let response = super::build_package_page_from_objects(objects, Some("packages/"), 2, 1);

    assert_eq!(response.total_packages, 3);
    assert_eq!(response.items.len(), 1);
    assert_eq!(response.items[0].package, "bravo");
    assert_eq!(response.items[0].name, "b-1.tgz");
}

#[test]
fn build_package_page_trims_go_proxy_suffix() {
    let objects = vec![pkg_obj(
        "go-proxy-cache/github.com/example/module/@v/v1.0.0.zip",
        123,
    )];

    let response = super::build_package_page_from_objects(objects, Some("go-proxy-cache/"), 1, 10);

    assert_eq!(response.total_packages, 1);
    assert_eq!(response.items.len(), 1);
    assert_eq!(response.items[0].package, "github.com/example/module");
    assert_eq!(
        response.items[0].cache_path,
        "go-proxy-cache/github.com/example/module/@v/v1.0.0.zip"
    );
}
