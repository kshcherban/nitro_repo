#![allow(clippy::expect_used, clippy::panic, clippy::todo, clippy::unwrap_used)]
use super::*;
use crate::repository::{RepositoryType, test_helpers::test_storage};
use ahash::HashMap;
use serde_json::{Value, json};
use uuid::Uuid;

#[tokio::test]
async fn create_new_php_hosted_accepts_default_config() {
    let storage = test_storage().await;
    let mut configs = HashMap::default();
    configs.insert(
        PhpRepositoryConfigType::get_type_static().to_string(),
        json!({ "type": "Hosted" }),
    );
    let result = PhpRepositoryType::default()
        .create_new("php-hosted".into(), Uuid::new_v4(), configs, storage)
        .await;
    let repository = result.expect("php repository to be created");
    assert_eq!(repository.repository_type, "php");
}

#[tokio::test]
async fn create_new_php_missing_config_returns_error() {
    let storage = test_storage().await;
    let configs: HashMap<String, Value> = HashMap::default();
    let result = PhpRepositoryType::default()
        .create_new("php-missing".into(), Uuid::new_v4(), configs, storage)
        .await;
    match result {
        Err(RepositoryFactoryError::MissingConfig(config)) => {
            assert_eq!(config, PhpRepositoryConfigType::get_type_static());
        }
        other => panic!("expected missing config error, got: {other:?}"),
    }
}
