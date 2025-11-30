#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use super::{delete_repository_sequence, format_missing_storage_error};
use crate::error::{InternalError, OtherInternalError};
use std::{
    io,
    sync::{Arc, Mutex},
};
use uuid::Uuid;

fn sample_internal_error(message: &'static str) -> InternalError {
    InternalError::from(OtherInternalError::new(io::Error::new(
        io::ErrorKind::Other,
        message,
    )))
}

#[tokio::test]
async fn delete_sequence_runs_db_before_storage() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let calls_db = Arc::clone(&calls);
    let calls_storage = Arc::clone(&calls);
    let calls_cache = Arc::clone(&calls);

    delete_repository_sequence(
        || async {
            calls_db.lock().unwrap().push("db");
            Ok::<(), InternalError>(())
        },
        || async {
            calls_storage.lock().unwrap().push("storage");
            Ok::<(), InternalError>(())
        },
        || {
            calls_cache.lock().unwrap().push("cache");
        },
    )
    .await
    .expect("sequence should succeed");

    let calls = calls.lock().unwrap();
    assert_eq!(
        calls.as_slice(),
        ["db", "storage", "cache"],
        "db must run before storage and cache removal last"
    );
}

#[tokio::test]
async fn delete_sequence_stops_when_db_delete_fails() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let calls_db = Arc::clone(&calls);
    let calls_storage = Arc::clone(&calls);

    let result = delete_repository_sequence(
        || async {
            calls_db.lock().unwrap().push("db");
            Err(sample_internal_error("db failed"))
        },
        || async {
            calls_storage.lock().unwrap().push("storage");
            Ok::<(), InternalError>(())
        },
        || {},
    )
    .await;

    assert!(result.is_err(), "sequence should propagate db failure");
    let calls = calls.lock().unwrap();
    assert_eq!(
        calls.as_slice(),
        ["db"],
        "storage/cache must not run after db error"
    );
}

#[test]
fn missing_storage_error_includes_identifiers() {
    let storage_id = Uuid::parse_str("aaaaaaaa-aaaa-4aaa-aaaa-aaaaaaaaaaaa").unwrap();
    let repository = Uuid::parse_str("bbbbbbbb-bbbb-4bbb-bbbb-bbbbbbbbbbbb").unwrap();

    let message = format_missing_storage_error(storage_id, repository);

    assert!(
        message.contains(&storage_id.to_string()),
        "storage id should be present"
    );
    assert!(
        message.contains(&repository.to_string()),
        "repository id should be present"
    );
    assert!(
        message.contains("Storage backend"),
        "message should explain missing backend"
    );
}
