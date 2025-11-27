#![allow(clippy::expect_used, clippy::panic, clippy::todo, clippy::unwrap_used)]
use super::*;

#[test]
fn default_config_has_reasonable_values() {
    let config = DebRepositoryConfig::default();
    assert_eq!(config.distributions, vec!["stable".to_string()]);
    assert_eq!(config.components, vec!["main".to_string()]);
    assert!(config.architectures.contains(&"amd64".to_string()));
}

#[test]
fn validation_rejects_empty_distribution() {
    let config = DebRepositoryConfig {
        distributions: vec![],
        components: vec!["main".into()],
        architectures: vec!["amd64".into()],
    };
    let serialized = serde_json::to_value(&config).expect("serde");
    let err = DebRepositoryConfigType
        .validate_config(serialized)
        .expect_err("should reject empty distributions");
    let message = err.to_string();
    assert!(
        message.contains("distribution"),
        "unexpected message: {message}"
    );
}

#[test]
fn validation_rejects_invalid_identifier() {
    let config = DebRepositoryConfig {
        distributions: vec!["stable".into()],
        components: vec!["main".into()],
        architectures: vec!["amd64!".into()],
    };
    let serialized = serde_json::to_value(&config).expect("serde");
    let err = DebRepositoryConfigType
        .validate_config(serialized)
        .expect_err("should reject invalid characters");
    assert!(
        err.to_string().contains("alphanumeric characters"),
        "unexpected message: {}",
        err
    );
}
