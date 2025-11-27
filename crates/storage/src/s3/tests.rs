#![allow(clippy::expect_used, clippy::panic, clippy::todo, clippy::unwrap_used)]
use super::{
    BodyRetrievalStrategy, CustomRegion, MAX_BUFFERED_OBJECT_BYTES, S3CacheConfig, S3Config,
    S3Credentials, S3StorageRegion,
};

#[test]
fn static_credentials_detected() {
    let creds = S3Credentials::new_access_key("AKIA", "secret");
    let static_keys = creds.static_keys();
    assert!(static_keys.is_some());
    let keys = static_keys.unwrap();
    assert_eq!(keys.access_key, "AKIA");
    assert_eq!(keys.secret_key, "secret");
    assert!(keys.session_token.is_none());
}

#[test]
fn missing_keys_use_default_chain() {
    let creds = S3Credentials::default();
    assert!(creds.static_keys().is_none());
}

#[test]
fn role_detection_prefers_non_empty_strings() {
    let creds = S3Credentials {
        role_arn: Some("arn:aws:iam::123:role/demo".into()),
        role_session_name: Some("nitro".into()),
        ..Default::default()
    };
    let role = creds.role_to_assume().expect("role should be detected");
    assert_eq!(role.role_arn, "arn:aws:iam::123:role/demo");
    assert_eq!(role.session_name.as_deref(), Some("nitro"));

    let empty_role = S3Credentials {
        role_arn: Some("   ".into()),
        ..Default::default()
    };
    assert!(empty_role.role_to_assume().is_none());
}

#[test]
fn custom_region_returns_endpoint_and_name() {
    let config = S3Config {
        bucket_name: "nitro".into(),
        region: Some(S3StorageRegion::UsEast1),
        custom_region: Some(CustomRegion {
            custom_region: Some("minio".into()),
            endpoint: "https://minio.local".parse().unwrap(),
        }),
        credentials: S3Credentials::default(),
        path_style: true,
        cache: S3CacheConfig::default(),
    };

    let resolved = config
        .resolved_region()
        .expect("custom region should resolve");
    assert_eq!(resolved.as_ref(), "minio");
    assert!(config.custom_endpoint().is_some());
}

#[test]
fn body_strategy_caches_small_objects() {
    let result =
        BodyRetrievalStrategy::from_content_length(Some(MAX_BUFFERED_OBJECT_BYTES - 1), true);
    assert_eq!(result, BodyRetrievalStrategy::BufferAndCache);
}

#[test]
fn body_strategy_streams_large_objects() {
    let result =
        BodyRetrievalStrategy::from_content_length(Some(MAX_BUFFERED_OBJECT_BYTES + 1), true);
    assert_eq!(result, BodyRetrievalStrategy::StreamWithoutCache);
}

#[test]
fn body_strategy_streams_when_cache_disabled() {
    let result = BodyRetrievalStrategy::from_content_length(Some(1), false);
    assert_eq!(result, BodyRetrievalStrategy::StreamWithoutCache);
}

#[test]
fn body_strategy_streams_when_size_unknown() {
    let result = BodyRetrievalStrategy::from_content_length(None, true);
    assert_eq!(result, BodyRetrievalStrategy::StreamWithoutCache);
}
