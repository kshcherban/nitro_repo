#![allow(
    clippy::expect_used,
    clippy::field_reassign_with_default,
    clippy::panic,
    clippy::todo,
    clippy::unwrap_used
)]
use uuid::Uuid;

use super::LocationMeta;
use crate::meta::RepositoryMeta;
fn random_repo_meta() -> RepositoryMeta {
    let mut meta = RepositoryMeta::default();
    meta.project_id = Some(Uuid::new_v4());
    meta.project_version_id = Some(Uuid::new_v4());

    meta.insert("test", "map");

    meta
}
#[test]
pub fn post_card_compatible_meta_directory() {
    let meta = LocationMeta {
        created: chrono::Local::now().fixed_offset(),
        modified: chrono::Local::now().fixed_offset(),
        location_typed_meta: super::LocationTypedMeta::Directory(super::DirectoryMeta {
            number_of_files: 0,
        }),
        repository_meta: random_repo_meta(),
    };

    let bytes = postcard::to_allocvec(&meta).unwrap();

    let from_bytes: LocationMeta = postcard::from_bytes(&bytes).unwrap();

    assert_eq!(meta, from_bytes);
}

#[test]
pub fn post_card_compatible_meta_file() {
    let meta = LocationMeta {
        created: chrono::Local::now().fixed_offset(),
        modified: chrono::Local::now().fixed_offset(),
        location_typed_meta: super::LocationTypedMeta::File(super::FileMeta {
            hashes: super::FileHashes {
                md5: Some("md5".to_string()),
                sha1: Some("sha1".to_string()),
                sha2_256: Some("sha2_256".to_string()),
                sha3_256: Some("sha3_256".to_string()),
            },
        }),
        repository_meta: random_repo_meta(),
    };

    let bytes = postcard::to_allocvec(&meta).unwrap();

    let from_bytes: LocationMeta = postcard::from_bytes(&bytes).unwrap();

    assert_eq!(meta, from_bytes);
}
