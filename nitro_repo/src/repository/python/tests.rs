#![allow(clippy::expect_used, clippy::panic, clippy::todo, clippy::unwrap_used)]
use super::*;
use crate::repository::{RepositoryType, test_helpers::test_storage};
use ahash::HashMap;
use serde_json::json;
use uuid::Uuid;

#[tokio::test]
async fn create_new_python_hosted_accepts_default_config() {
    let storage = test_storage().await;
    let mut configs = HashMap::default();
    configs.insert(
        PythonRepositoryConfigType::get_type_static().to_string(),
        json!({ "type": "Hosted" }),
    );
    let result = PythonRepositoryType::default()
        .create_new("python-hosted".into(), Uuid::new_v4(), configs, storage)
        .await;
    let repository = result.expect("hosted repository to be created");
    assert_eq!(repository.repository_type, "python");
    assert!(
        repository
            .configs
            .contains_key(PythonRepositoryConfigType::get_type_static()),
        "expected python config to be persisted",
    );
}

#[tokio::test]
async fn create_new_python_rejects_config_missing_type() {
    let storage = test_storage().await;
    let mut configs = HashMap::default();
    configs.insert(
        PythonRepositoryConfigType::get_type_static().to_string(),
        json!({}),
    );
    let result = PythonRepositoryType::default()
        .create_new("python-invalid".into(), Uuid::new_v4(), configs, storage)
        .await;
    match result {
        Err(RepositoryFactoryError::InvalidConfig(repository, message)) => {
            assert_eq!(repository, "python");
            assert!(
                message.contains("type"),
                "expected error message to mention missing type, got: {message}"
            );
        }
        other => panic!("expected invalid config error, got: {other:?}"),
    }
}
