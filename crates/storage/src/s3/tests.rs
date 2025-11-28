#![allow(clippy::expect_used, clippy::panic, clippy::todo, clippy::unwrap_used)]
use super::{
    AdaptiveBufferConfig, BodyRetrievalStrategy, CustomRegion, DEFAULT_MAX_BUFFERED_OBJECT_BYTES,
    S3CacheConfig, S3Config, S3Credentials, S3DiskCache, S3StorageRegion,
};
use bytes::Bytes;
use tempfile::tempdir;
use tokio::{
    fs,
    time::{Duration, sleep},
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
        adaptive_buffer: AdaptiveBufferConfig::default(),
    };

    let resolved = config
        .resolved_region()
        .expect("custom region should resolve");
    assert_eq!(resolved.as_ref(), "minio");
    assert!(config.custom_endpoint().is_some());
}

#[test]
fn body_strategy_caches_small_objects() {
    let limit = DEFAULT_MAX_BUFFERED_OBJECT_BYTES;
    let result = BodyRetrievalStrategy::from_content_length(Some(limit - 1), true, limit);
    assert_eq!(result, BodyRetrievalStrategy::BufferAndCache);
}

#[test]
fn body_strategy_streams_large_objects() {
    let limit = DEFAULT_MAX_BUFFERED_OBJECT_BYTES;
    let result = BodyRetrievalStrategy::from_content_length(Some(limit + 1), true, limit);
    assert_eq!(result, BodyRetrievalStrategy::StreamWithoutCache);
}

#[test]
fn body_strategy_streams_when_cache_disabled() {
    let result = BodyRetrievalStrategy::from_content_length(Some(1), false, 1);
    assert_eq!(result, BodyRetrievalStrategy::StreamWithoutCache);
}

#[test]
fn body_strategy_streams_when_size_unknown() {
    let limit = DEFAULT_MAX_BUFFERED_OBJECT_BYTES;
    let result = BodyRetrievalStrategy::from_content_length(None, true, limit);
    assert_eq!(result, BodyRetrievalStrategy::StreamWithoutCache);
}

fn cache_config_with_dir(dir: &std::path::Path) -> S3CacheConfig {
    S3CacheConfig {
        enabled: true,
        path: Some(dir.to_path_buf()),
        max_bytes: 8,
        max_entries: 4,
    }
}

#[tokio::test]
async fn disk_cache_retries_failed_deletions_on_next_put() {
    let temp_dir = tempdir().expect("tempdir");
    let cache = S3DiskCache::new(&cache_config_with_dir(temp_dir.path()), "test-cache")
        .await
        .expect("cache");

    cache
        .put("first", Bytes::from_static(b"abcdefgh"), None)
        .await
        .expect("initial write");

    let relative = S3DiskCache::hashed_filename("first");
    let disk_path = cache.dir.join(&relative);
    fs::remove_file(&disk_path)
        .await
        .expect("remove original file");
    fs::create_dir_all(&disk_path)
        .await
        .expect("replace file with dir");

    cache
        .put("second", Bytes::from_static(b"ijklmnop"), None)
        .await
        .expect("evict first entry");

    let metadata = fs::metadata(&disk_path).await.expect("metadata");
    assert!(metadata.is_dir(), "corrupted entry stays on disk");

    fs::remove_dir_all(&disk_path)
        .await
        .expect("cleanup dir before retry");
    fs::File::create(&disk_path)
        .await
        .expect("recreate file so deletion can succeed");

    sleep(Duration::from_millis(150)).await;

    cache
        .put("third", Bytes::from_static(b"qrstuvwx"), None)
        .await
        .expect("trigger retry");

    let exists = fs::try_exists(&disk_path).await.expect("exists check");
    assert!(!exists, "failed deletions get retried before new puts");
}

#[test]
fn adaptive_buffer_respects_pressure_threshold() {
    let config = AdaptiveBufferConfig {
        min_buffer_bytes: 1024 * 1024,
        max_buffer_bytes: 16 * 1024 * 1024,
        memory_pressure_threshold: 0.5,
    };

    assert_eq!(config.limit_for_pressure(0.0), 16 * 1024 * 1024);
    let mid = config.limit_for_pressure(0.25);
    assert!(mid < 16 * 1024 * 1024 && mid > 1024 * 1024);
    assert_eq!(config.limit_for_pressure(0.5), 1024 * 1024);
    assert_eq!(config.limit_for_pressure(0.9), 1024 * 1024);
}
