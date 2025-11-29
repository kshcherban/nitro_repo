#![allow(clippy::expect_used, clippy::panic, clippy::todo, clippy::unwrap_used)]
use bytes::Bytes;
use nr_storage::{FileContent, Storage};

use super::query_parser::SearchQuery;
use super::*;
use crate::repository::test_helpers::test_storage;

fn simple_query(term: &str) -> SearchQuery {
    SearchQuery {
        terms: vec![term.to_lowercase()],
        ..SearchQuery::default()
    }
}

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
        SearchStrategy::PackagesDirectory {
            base: Some("packages"),
        },
        &simple_query("parallel"),
        10,
    )
    .await
    .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].file_name, "parallel_ssh-2.12.0-py3-none-any.whl");
}

#[tokio::test]
async fn search_repository_storage_handles_root_packages() {
    let storage = test_storage().await;
    let repo_id = Uuid::new_v4();
    let path = StoragePath::from("left-pad/1.0.0/package.tgz");
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
        repository_name: "npm-hosted".into(),
        storage_name: "test".into(),
        repository_type: "npm".into(),
    };

    let results = super::search_repository_storage(
        &storage,
        &summary,
        SearchStrategy::PackagesDirectory { base: None },
        &simple_query("left-pad"),
        10,
    )
    .await
    .unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].cache_path, "left-pad/1.0.0/package.tgz");
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
        SearchStrategy::PackagesDirectory {
            base: Some("packages"),
        },
        &simple_query("artifact"),
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
        SearchStrategy::PackagesDirectory {
            base: Some("packages"),
        },
        &simple_query("package"),
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
        &simple_query("nginx"),
        10,
    )
    .await
    .unwrap();

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].file_name, "library/nginx:latest");
    assert_eq!(results[0].cache_path, "v2/library/nginx/manifests/latest");
    assert_eq!(results[0].size, 7023 + 32654);
}

#[tokio::test]
async fn search_repository_storage_skips_docker_tag_metadata() {
    let storage = test_storage().await;
    let repo_id = Uuid::new_v4();
    let manifest_path = StoragePath::from("v2/local/docker-proxy/manifests/nightly");
    let manifest = r#"{"schemaVersion": 2, "config": {"size": 1}, "layers": []}"#;
    storage
        .save_file(
            repo_id,
            FileContent::Bytes(Bytes::from(manifest)),
            &manifest_path,
        )
        .await
        .unwrap();

    let tag_meta_path =
        StoragePath::from("v2/local/docker-proxy/manifests/nightly.nr-docker-tagmeta");
    storage
        .save_file(
            repo_id,
            FileContent::Bytes(Bytes::from_static(b"{}")),
            &tag_meta_path,
        )
        .await
        .unwrap();

    let summary = RepositorySummary {
        repository_id: repo_id,
        repository_name: "docker-proxy".into(),
        storage_name: "test".into(),
        repository_type: "docker".into(),
    };

    let results = super::search_repository_storage(
        &storage,
        &summary,
        SearchStrategy::Docker,
        &simple_query("docker"),
        10,
    )
    .await
    .unwrap();

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].file_name, "local/docker-proxy:nightly");
    assert!(
        results
            .iter()
            .all(|result| !result.file_name.ends_with(".nr-docker-tagmeta"))
    );
}
