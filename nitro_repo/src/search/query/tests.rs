#![allow(clippy::expect_used, clippy::panic, clippy::todo, clippy::unwrap_used)]

use std::{iter::FromIterator, time::Duration};

use ahash::{HashMap, HashSet};
use nr_core::{
    database::{
        entities::{
            project::{NewProject, versions::NewVersion},
            storage::NewDBStorage,
        },
        migration::run_migrations,
    },
    repository::project::{ReleaseType, VersionData},
    storage::StorageName,
};
use once_cell::sync::Lazy;
use serde_json::json;
use sqlx::PgPool;
use testcontainers::{
    ContainerAsync, GenericImage, ImageExt,
    core::{IntoContainerPort, WaitFor},
    runners::AsyncRunner,
};
use uuid::Uuid;

use crate::{
    app::api::search::{Operator, SearchQuery},
    repository::NewRepository,
    search::PackageSearchRepository,
};

const POSTGRES_IMAGE: &str = "postgres";
const POSTGRES_TAG: &str = "14-alpine";
const POSTGRES_PORT: u16 = 5432;

static DB_LOCK: Lazy<tokio::sync::Mutex<()>> = Lazy::new(|| tokio::sync::Mutex::new(()));
static DATABASE_CONTAINER: tokio::sync::OnceCell<PostgresFixture> =
    tokio::sync::OnceCell::const_new();
static DB_MIGRATIONS: tokio::sync::OnceCell<()> = tokio::sync::OnceCell::const_new();

struct PostgresFixture {
    connection_string: String,
    _container: ContainerAsync<GenericImage>,
}

impl PostgresFixture {
    fn url(&self) -> &str {
        &self.connection_string
    }
}

async fn postgres_fixture() -> &'static PostgresFixture {
    DATABASE_CONTAINER
        .get_or_init(|| async {
            let base_image = GenericImage::new(POSTGRES_IMAGE, POSTGRES_TAG)
                .with_wait_for(WaitFor::seconds(5))
                .with_exposed_port(POSTGRES_PORT.tcp());

            let image = base_image
                .with_env_var("POSTGRES_USER", "test")
                .with_env_var("POSTGRES_PASSWORD", "test")
                .with_env_var(
                    "POSTGRES_INITDB_ARGS",
                    "--locale=C --encoding=UTF8 --lc-collate=C --lc-ctype=C",
                )
                .with_startup_timeout(Duration::from_secs(60));

            let container = image.start().await.expect("start postgres test container");
            let host = container
                .get_host()
                .await
                .expect("postgres host")
                .to_string();
            let port = container
                .get_host_port_ipv4(POSTGRES_PORT)
                .await
                .expect("postgres mapped port");
            let admin_url = format!("postgres://test:test@{host}:{port}/postgres");
            let admin_pool = PgPool::connect(&admin_url)
                .await
                .expect("connect to admin database");
            sqlx::query("DROP DATABASE IF EXISTS nitro_repo_test")
                .execute(&admin_pool)
                .await
                .expect("drop test database");
            sqlx::query(
                "CREATE DATABASE nitro_repo_test TEMPLATE template0 LC_COLLATE 'C' LC_CTYPE 'C'",
            )
            .execute(&admin_pool)
            .await
            .expect("create test database");
            let collation_pool = PgPool::connect(&format!(
                "postgres://test:test@{host}:{port}/nitro_repo_test"
            ))
            .await
            .expect("connect to target database for collation setup");
            sqlx::query(
                "CREATE COLLATION IF NOT EXISTS ignoreCase (\n  provider = 'icu',\n  locale = 'und-u-ks-level2',\n  deterministic = false\n)",
            )
            .execute(&collation_pool)
            .await
            .expect("ensure deterministic ignoreCase collation");
            let connection_string = format!("postgres://test:test@{host}:{port}/nitro_repo_test");

            PostgresFixture {
                connection_string,
                _container: container,
            }
        })
        .await
}

async fn fresh_pool() -> PgPool {
    let fixture = postgres_fixture().await;
    let pool = PgPool::connect(fixture.url())
        .await
        .expect("connect to test database");

    let migrate_pool = pool.clone();
    DB_MIGRATIONS
        .get_or_init(|| async move {
            run_migrations(&migrate_pool)
                .await
                .expect("run database migrations");
        })
        .await;

    pool
}

async fn reset_database(pool: &PgPool) {
    sqlx::query(
        "TRUNCATE TABLE project_versions, projects, repositories, storages RESTART IDENTITY CASCADE",
    )
    .execute(pool)
    .await
    .expect("truncate test tables");
}

fn unique_name(prefix: &str) -> String {
    let raw = Uuid::new_v4().simple().to_string();
    format!("{prefix}-{}", &raw[..12])
}

async fn insert_storage(pool: &PgPool) -> Uuid {
    let storage_name = StorageName::new(unique_name("storage")).expect("valid storage name");
    let storage = NewDBStorage::new("Local".into(), storage_name, json!({ "path": "/tmp" }));
    storage
        .insert(pool)
        .await
        .expect("insert storage")
        .expect("storage row")
        .id
}

async fn insert_repository(pool: &PgPool, storage_id: Uuid) -> Uuid {
    let repo_name = unique_name("repo");
    let repo = NewRepository {
        name: repo_name,
        uuid: Uuid::new_v4(),
        repository_type: "npm".into(),
        configs: HashMap::with_hasher(Default::default()),
    };
    repo.insert(storage_id, pool)
        .await
        .expect("insert repository")
        .id
}

async fn insert_package(pool: &PgPool, repository_id: Uuid, package: &str, version: &str) {
    let project = NewProject {
        scope: None,
        project_key: package.to_string(),
        name: package.to_string(),
        description: None,
        repository: repository_id,
        storage_path: format!("packages/{package}"),
    }
    .insert(pool)
    .await
    .expect("insert project");

    let mut extra = VersionData::default();
    extra.extra = Some(json!({
        "size": 1024,
        "filename": format!("packages/{package}/{package}-{version}.tgz"),
    }));

    NewVersion {
        project_id: project.id,
        repository_id,
        version: version.to_string(),
        release_type: ReleaseType::Stable,
        version_path: format!("packages/{package}/{version}/{package}-{version}.tgz"),
        publisher: None,
        version_page: None,
        extra,
    }
    .insert(pool)
    .await
    .expect("insert version");
}

#[tokio::test]
async fn fetch_repository_rows_filters_by_package() {
    let _guard = DB_LOCK.lock().await;
    let pool = fresh_pool().await;
    reset_database(&pool).await;

    let storage_id = insert_storage(&pool).await;
    let repository_id = insert_repository(&pool, storage_id).await;
    insert_package(&pool, repository_id, "alpha", "1.0.0").await;
    insert_package(&pool, repository_id, "beta", "1.0.0").await;

    let repository = PackageSearchRepository::new(&pool);
    let query = SearchQuery {
        package_filter: Some((Operator::Equals, "alpha".into())),
        ..SearchQuery::default()
    };
    let rows = repository
        .fetch_repository_rows(repository_id, &query, 10)
        .await
        .expect("query rows");

    let names: HashSet<_> = rows.iter().map(|row| row.package_name.as_str()).collect();
    assert_eq!(names, HashSet::from_iter(["alpha"]));
}

#[tokio::test]
async fn fetch_repository_rows_filters_by_terms() {
    let _guard = DB_LOCK.lock().await;
    let pool = fresh_pool().await;
    reset_database(&pool).await;

    let storage_id = insert_storage(&pool).await;
    let repository_id = insert_repository(&pool, storage_id).await;
    insert_package(&pool, repository_id, "core-lib", "2.0.0").await;
    insert_package(&pool, repository_id, "support-lib", "1.1.0").await;

    let repository = PackageSearchRepository::new(&pool);
    let mut query = SearchQuery::default();
    query.terms = vec!["support".into()];

    let rows = repository
        .fetch_repository_rows(repository_id, &query, 5)
        .await
        .expect("query rows");

    let names: Vec<_> = rows.iter().map(|row| row.package_name.as_str()).collect();
    assert_eq!(names, vec!["support-lib"]);
}

#[tokio::test]
async fn repository_has_index_rows_detects_catalog_state() {
    let _guard = DB_LOCK.lock().await;
    let pool = fresh_pool().await;
    reset_database(&pool).await;

    let storage_id = insert_storage(&pool).await;
    let repository_id = insert_repository(&pool, storage_id).await;
    let repository = PackageSearchRepository::new(&pool);

    assert!(
        !repository
            .repository_has_index_rows(repository_id)
            .await
            .expect("query flag")
    );

    insert_package(&pool, repository_id, "delta", "0.1.0").await;

    assert!(
        repository
            .repository_has_index_rows(repository_id)
            .await
            .expect("query flag")
    );
}
