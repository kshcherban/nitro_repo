#![allow(clippy::expect_used, clippy::panic, clippy::todo, clippy::unwrap_used)]
use super::*;
use crate::{
    repository::{RepositoryType, test_helpers::test_storage},
    utils::response::IntoErrorResponse,
};
use ahash::HashMap;
use axum::body::to_bytes;
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

#[tokio::test]
async fn create_new_php_proxy_accepts_empty_routes() {
    let storage = test_storage().await;
    let mut configs = HashMap::default();
    configs.insert(
        PhpRepositoryConfigType::get_type_static().to_string(),
        json!({ "type": "Proxy", "config": { "routes": [] } }),
    );
    let result = PhpRepositoryType::default()
        .create_new("php-proxy".into(), Uuid::new_v4(), configs, storage)
        .await;
    let repository = result.expect("php proxy repository created");
    assert_eq!(repository.repository_type, "php");
}

#[tokio::test]
async fn invalid_composer_error_returns_json() {
    let err = PhpRepositoryError::InvalidComposer("bad composer".into());
    let response = Box::new(err).into_response_boxed();
    assert_eq!(response.status(), axum::http::StatusCode::BAD_REQUEST);
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read body");
    let value: serde_json::Value = serde_json::from_slice(&body).expect("json body");
    assert_eq!(value["error"], serde_json::json!("bad composer"));
}
