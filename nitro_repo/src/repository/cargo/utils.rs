use axum::http::Uri;
use nr_core::storage::StoragePath;
use serde::{Deserialize, Serialize};
use serde_json::json;

/// Errors that can occur while handling Cargo helper utilities.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum CargoUtilError {
    #[error("Invalid crate name: {0}")]
    InvalidCrateName(String),
    #[error("Publish payload is truncated")]
    TruncatedPayload,
    #[error("Publish metadata length mismatch")]
    MetadataLengthMismatch,
    #[error("Crate archive length mismatch")]
    ArchiveLengthMismatch,
    #[error("Invalid publish metadata: {0}")]
    InvalidPublishMetadata(String),
}

/// Metadata extracted from a publish request.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PublishMetadata {
    pub name: String,
    pub vers: semver::Version,
    #[serde(default)]
    pub deps: Vec<PublishDependency>,
    #[serde(default)]
    pub features: ahash::HashMap<String, Vec<String>>,
    #[serde(default)]
    pub authors: Vec<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub documentation: Option<String>,
    #[serde(default)]
    pub homepage: Option<String>,
    #[serde(default)]
    pub repository: Option<String>,
    #[serde(default)]
    pub keywords: Vec<String>,
    #[serde(default)]
    pub categories: Vec<String>,
    #[serde(default)]
    pub license: Option<String>,
    #[serde(default)]
    pub license_file: Option<String>,
    #[serde(default)]
    pub readme: Option<String>,
    #[serde(default)]
    pub readme_file: Option<String>,
    #[serde(default)]
    pub badges: ahash::HashMap<String, serde_json::Value>,
    #[serde(default)]
    pub links: Option<String>,
    #[serde(default)]
    pub v: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PublishDependency {
    pub name: String,
    pub vers: Option<String>,
    #[serde(default)]
    pub optional: bool,
    #[serde(default)]
    pub default_features: bool,
    #[serde(default)]
    pub features: Vec<String>,
    #[serde(default)]
    pub target: Option<String>,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub registry: Option<String>,
    #[serde(default)]
    pub package: Option<String>,
}

/// Parsed representation of a publish payload.
#[derive(Debug, Clone, PartialEq)]
pub struct PublishPayload {
    pub metadata: PublishMetadata,
    pub crate_archive: Vec<u8>,
}

pub fn normalize_crate_name(name: &str) -> String {
    name.to_ascii_lowercase()
}

pub fn crate_index_relative_path(crate_name: &str) -> Result<String, CargoUtilError> {
    let normalized = normalize_crate_name(crate_name);
    if normalized.is_empty()
        || !normalized
            .as_bytes()
            .iter()
            .all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'_' | b'-'))
    {
        return Err(CargoUtilError::InvalidCrateName(crate_name.to_string()));
    }

    let bytes = normalized.as_bytes();
    let path = match bytes.len() {
        1 => format!("1/{}", normalized),
        2 => format!("2/{}", normalized),
        3 => {
            let first = &normalized[0..1];
            format!("3/{first}/{normalized}")
        }
        _ => {
            let first_two = &normalized[0..2];
            let next_two = &normalized[2..std::cmp::min(4, bytes.len())];
            format!("{first_two}/{next_two}/{normalized}")
        }
    };
    Ok(path)
}

pub fn crate_archive_storage_path(crate_name: &str, version: &semver::Version) -> StoragePath {
    let normalized = normalize_crate_name(crate_name);
    let file_name = format!("{normalized}-{}.crate", version);
    StoragePath::from(format!(
        "crates/{normalized}/{version}/{file_name}",
        version = version
    ))
}

pub fn sparse_index_storage_path(crate_name: &str) -> Result<StoragePath, CargoUtilError> {
    let relative = crate_index_relative_path(crate_name)?;
    Ok(StoragePath::from(format!("index/{relative}")))
}

pub fn parse_publish_payload(body: &[u8]) -> Result<PublishPayload, CargoUtilError> {
    if body.len() < 8 {
        return Err(CargoUtilError::TruncatedPayload);
    }

    let mut offset = 0usize;
    let metadata_len = u32::from_le_bytes(
        body[offset..offset + 4]
            .try_into()
            .expect("slice has length 4"),
    ) as usize;
    offset += 4;

    if body.len() < offset + metadata_len {
        return Err(CargoUtilError::MetadataLengthMismatch);
    }
    let metadata_bytes = &body[offset..offset + metadata_len];
    offset += metadata_len;

    if body.len() < offset + 4 {
        return Err(CargoUtilError::TruncatedPayload);
    }
    let crate_len = u32::from_le_bytes(
        body[offset..offset + 4]
            .try_into()
            .expect("slice has length 4"),
    ) as usize;
    offset += 4;

    if body.len() < offset + crate_len {
        return Err(CargoUtilError::ArchiveLengthMismatch);
    }

    let crate_archive = body[offset..offset + crate_len].to_vec();

    let metadata: PublishMetadata = serde_json::from_slice(metadata_bytes)
        .map_err(|err| CargoUtilError::InvalidPublishMetadata(err.to_string()))?;

    Ok(PublishPayload {
        metadata,
        crate_archive,
    })
}

pub fn build_index_entry(metadata: &PublishMetadata, checksum: &str) -> serde_json::Value {
    let deps: Vec<serde_json::Value> = metadata
        .deps
        .iter()
        .map(|dep| {
            json!({
                "name": dep.name,
                "req": dep.vers.clone().unwrap_or_else(|| "*".into()),
                "features": dep.features.clone(),
                "optional": dep.optional,
                "default_features": dep.default_features,
                "target": dep.target.clone(),
                "kind": dep.kind.clone(),
                "registry": dep.registry.clone(),
                "package": dep.package.clone(),
                "explicit_name_in_toml": dep.package.is_some(),
            })
        })
        .collect();
    json!({
        "name": metadata.name.clone(),
        "vers": metadata.vers.to_string(),
        "deps": deps,
        "cksum": checksum,
        "features": metadata.features.clone(),
        "yanked": false,
        "links": metadata.links.clone(),
        "v": metadata.v.unwrap_or(2),
    })
}

pub fn build_config_json(
    base_url: &Uri,
    storage_name: &str,
    repository_name: &str,
    auth_required: bool,
) -> serde_json::Value {
    let mut base = base_url.to_string();
    if base.ends_with('/') {
        base.truncate(base.trim_end_matches('/').len());
    }
    let repository_base = format!("{base}/repositories/{storage_name}/{repository_name}");
    let api = repository_base.clone();
    let dl = format!("{repository_base}/api/v1/crates");
    let index = format!("sparse+{repository_base}/index");
    json!({
        "dl": dl,
        "api": api,
        "index": index,
        "auth-required": auth_required,
    })
}

pub fn build_login_response(base_url: &Uri) -> serde_json::Value {
    let mut base = base_url.to_string();
    if base.ends_with('/') {
        base.truncate(base.trim_end_matches('/').len());
    }
    json!({
        "message": "Use Nitro Repo UI to generate an API token.",
        "token_help_url": format!("{base}/app/settings/tokens"),
        "documentation": "https://doc.rust-lang.org/cargo/reference/registries.html#logging-in",
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn normalize_crate_name_lowercases() {
        assert_eq!(normalize_crate_name("Serde"), "serde");
        assert_eq!(normalize_crate_name("my-crate"), "my-crate");
    }

    #[test]
    fn crate_index_path_follows_cargo_rules() {
        let cases = [
            ("a", "1/a"),
            ("ab", "2/ab"),
            ("abc", "3/a/abc"),
            ("serde", "se/rd/serde"),
            ("serde_json", "se/rd/serde_json"),
            ("MyCrate", "my/cr/mycrate"),
        ];

        for (name, expected) in cases {
            let path = crate_index_relative_path(name).expect("path");
            assert_eq!(path, expected, "crate {name}");
        }
    }

    #[test]
    fn crate_index_path_rejects_invalid_name() {
        let err = crate_index_relative_path("bad space").expect_err("invalid name");
        assert!(matches!(err, CargoUtilError::InvalidCrateName(_)));
    }

    #[test]
    fn archive_storage_path_places_crates_under_version_directory() {
        let path = crate_archive_storage_path(
            "serde",
            &semver::Version::parse("1.2.3").expect("parse version"),
        );
        assert_eq!(path.to_string(), "crates/serde/1.2.3/serde-1.2.3.crate");
    }

    #[test]
    fn sparse_index_storage_path_matches_index_relative_path() {
        let path = sparse_index_storage_path("serde").expect("path");
        assert_eq!(path.to_string(), "index/se/rd/serde");
    }

    #[test]
    fn parse_publish_payload_splits_metadata_and_archive() {
        let metadata = json!({
            "name": "serde",
            "vers": "1.0.0",
            "deps": [],
            "features": {},
            "authors": ["Serde Developers"],
            "description": "Serde description",
            "documentation": null,
            "homepage": null,
            "repository": null,
            "keywords": [],
            "categories": [],
            "license": "MIT",
            "readme": null,
            "readme_file": null,
            "badges": {},
            "links": null
        });
        let metadata_bytes = serde_json::to_vec(&metadata).unwrap();
        let crate_bytes = vec![1, 2, 3, 4, 5];

        let mut body = Vec::new();
        body.extend(&(metadata_bytes.len() as u32).to_le_bytes());
        body.extend(&metadata_bytes);
        body.extend(&(crate_bytes.len() as u32).to_le_bytes());
        body.extend(&crate_bytes);

        let parsed = parse_publish_payload(&body).expect("parse payload");

        assert_eq!(parsed.metadata.name, "serde");
        assert_eq!(
            parsed.metadata.vers,
            semver::Version::parse("1.0.0").unwrap()
        );
        assert_eq!(parsed.crate_archive, crate_bytes);
    }

    #[test]
    fn parse_publish_payload_rejects_truncated_body() {
        let body = vec![0, 1, 2];
        let err = parse_publish_payload(&body).expect_err("truncated payload");
        assert_eq!(err, CargoUtilError::TruncatedPayload);
    }

    #[test]
    fn parse_publish_payload_rejects_bad_lengths() {
        let metadata_bytes = br#"{"name":"serde","vers":"1.0.0","deps":[],"features":{},"authors":[],"keywords":[],"categories":[],"badges":{}}"#;
        let mut body = Vec::new();
        body.extend(&(metadata_bytes.len() as u32 + 10).to_le_bytes());
        body.extend(metadata_bytes);
        body.extend(&0u32.to_le_bytes());

        let err = parse_publish_payload(&body).expect_err("length mismatch");
        assert_eq!(err, CargoUtilError::MetadataLengthMismatch);
    }

    #[test]
    fn build_index_entry_sets_checksum_and_metadata() {
        let metadata = PublishMetadata {
            name: "serde".into(),
            vers: semver::Version::parse("1.0.0").unwrap(),
            deps: vec![PublishDependency {
                name: "serde_derive".into(),
                vers: Some("^1".into()),
                optional: false,
                default_features: true,
                features: vec![],
                target: None,
                kind: None,
                registry: None,
                package: None,
            }],
            features: Default::default(),
            authors: vec!["Serde Developers".into()],
            description: Some("Serde description".into()),
            documentation: Some("https://docs.rs/serde".into()),
            homepage: None,
            repository: Some("https://github.com/serde-rs/serde".into()),
            keywords: vec!["serde".into()],
            categories: vec!["parsing".into()],
            license: Some("MIT OR Apache-2.0".into()),
            license_file: None,
            readme: None,
            readme_file: None,
            badges: Default::default(),
            links: None,
            v: Some(2),
        };
        let entry = build_index_entry(
            &metadata,
            "deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef",
        );

        assert_eq!(entry["name"], "serde");
        assert_eq!(entry["vers"], "1.0.0");
        assert_eq!(
            entry["cksum"],
            "deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef"
        );
        assert!(entry["deps"].is_array());
    }

    #[test]
    fn build_config_json_points_to_download_and_api() {
        let base = "https://example.com".parse::<Uri>().unwrap();
        let json = build_config_json(&base, "main", "crates", false);

        assert_eq!(
            json["dl"],
            "https://example.com/repositories/main/crates/api/v1/crates"
        );
        assert_eq!(json["api"], "https://example.com/repositories/main/crates");
        assert_eq!(
            json["index"],
            "sparse+https://example.com/repositories/main/crates/index"
        );
        assert_eq!(json["auth-required"], false);
    }

    #[test]
    fn build_config_json_marks_auth_required() {
        let base = "https://example.com".parse::<Uri>().unwrap();
        let json = build_config_json(&base, "secure", "cargo", true);
        assert_eq!(json["auth-required"], true);
    }

    #[test]
    fn build_login_response_points_to_web_ui() {
        let base = "https://example.com".parse::<Uri>().unwrap();
        let json = build_login_response(&base);
        assert_eq!(
            json["message"],
            "Use Nitro Repo UI to generate an API token."
        );
        assert_eq!(
            json["token_help_url"],
            "https://example.com/app/settings/tokens"
        );
    }
}
