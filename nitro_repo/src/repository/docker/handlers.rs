//! Docker Registry API V2 HTTP handlers
//!
//! Implements the Docker Registry HTTP API V2 specification.
//! Reference: https://docs.docker.com/registry/spec/api/

use axum::{body::Body, response::Response};
use http::StatusCode;
use nr_core::storage::StoragePath;
use nr_storage::{Storage, StorageFile};
use sha2::{Digest, Sha256};
use tokio::io::AsyncReadExt;
use tracing::{debug, info, instrument};

use super::{
    DockerError, DockerHosted, RepoResponse, Repository, RepositoryRequest,
    types::{Manifest, MediaType},
};

/// Helper to extract bytes from StorageFile
async fn get_file_bytes(storage_file: StorageFile) -> Result<Vec<u8>, DockerError> {
    match storage_file {
        StorageFile::File { mut content, .. } => {
            let mut buffer = Vec::new();
            content.read_to_end(&mut buffer).await?;
            Ok(buffer)
        }
        StorageFile::Directory { .. } => Err(DockerError::InvalidManifest(
            "Expected file, got directory".to_string(),
        )),
    }
}

/// Helper to create custom response with headers
fn custom_response(status: StatusCode, headers: Vec<(&str, &str)>, body: Vec<u8>) -> RepoResponse {
    let mut builder = Response::builder().status(status);
    for (key, value) in headers {
        builder = builder.header(key, value);
    }
    RepoResponse::Other(builder.body(Body::from(body)).unwrap())
}

/// Helper to create JSON response
fn json_response(value: serde_json::Value) -> RepoResponse {
    let json_str = serde_json::to_string(&value).unwrap();
    custom_response(
        StatusCode::OK,
        vec![("Content-Type", "application/json")],
        json_str.into_bytes(),
    )
}

/// Main routing handler for GET requests
#[instrument(skip(repo, request))]
pub async fn handle_get(
    repo: DockerHosted,
    request: RepositoryRequest,
) -> Result<RepoResponse, DockerError> {
    let path_str = request.path.to_string();
    let parts: Vec<&str> = path_str.trim_start_matches('/').split('/').collect();

    debug!("Docker GET request path: {:?}", parts);

    match parts.as_slice() {
        // GET /v2/ - Base API check
        ["v2"] => handle_api_version_check(),

        // GET /v2/<name>/tags/list - List all tags for a repository
        ["v2", name @ .., "tags", "list"] if !name.is_empty() => {
            let repository_name = name.join("/");
            list_tags(&repo, &repository_name).await
        }

        // GET /v2/<name>/manifests/<reference> - Get manifest by tag or digest
        ["v2", name @ .., "manifests", reference] if !name.is_empty() => {
            let repository_name = name.join("/");
            let accept_header = request
                .parts
                .headers
                .get("Accept")
                .and_then(|h| h.to_str().ok())
                .map(|s| s.to_string());
            get_manifest(&repo, &repository_name, reference, accept_header).await
        }

        // GET /v2/<name>/blobs/<digest> - Download blob
        ["v2", name @ .., "blobs", digest] if !name.is_empty() => {
            let repository_name = name.join("/");
            get_blob(&repo, &repository_name, digest).await
        }

        _ => Ok(RepoResponse::basic_text_response(
            StatusCode::NOT_FOUND,
            "Not Found",
        )),
    }
}

/// Handle HEAD requests (same as GET but without body)
#[instrument(skip(repo, request))]
pub async fn handle_head(
    repo: DockerHosted,
    request: RepositoryRequest,
) -> Result<RepoResponse, DockerError> {
    let path_str = request.path.to_string();
    let parts: Vec<&str> = path_str.trim_start_matches('/').split('/').collect();

    debug!("Docker HEAD request path: {:?}", parts);

    match parts.as_slice() {
        // HEAD /v2/<name>/manifests/<reference> - Check manifest exists
        ["v2", name @ .., "manifests", reference] if !name.is_empty() => {
            let repository_name = name.join("/");
            head_manifest(&repo, &repository_name, reference).await
        }

        // HEAD /v2/<name>/blobs/<digest> - Check blob exists
        ["v2", name @ .., "blobs", digest] if !name.is_empty() => {
            let repository_name = name.join("/");
            head_blob(&repo, &repository_name, digest).await
        }

        _ => Ok(RepoResponse::basic_text_response(
            StatusCode::NOT_FOUND,
            "Not Found",
        )),
    }
}

/// Handle PUT requests (upload manifests and complete blob uploads)
#[instrument(skip(repo, request))]
pub async fn handle_put(
    repo: DockerHosted,
    request: RepositoryRequest,
) -> Result<RepoResponse, DockerError> {
    let path_str = request.path.to_string();
    let parts: Vec<&str> = path_str.trim_start_matches('/').split('/').collect();

    debug!("Docker PUT request path: {:?}", parts);

    match parts.as_slice() {
        // PUT /v2/<name>/manifests/<reference> - Upload manifest
        ["v2", name @ .., "manifests", reference] if !name.is_empty() => {
            let repository_name = name.join("/");
            put_manifest(&repo, &repository_name, reference, request).await
        }

        // PUT /v2/<name>/blobs/uploads/<uuid>?digest=<digest> - Complete blob upload
        ["v2", name @ .., "blobs", "uploads", upload_id] if !name.is_empty() => {
            let repository_name = name.join("/");
            complete_blob_upload(&repo, &repository_name, upload_id, request).await
        }

        _ => Ok(RepoResponse::basic_text_response(
            StatusCode::NOT_FOUND,
            "Not Found",
        )),
    }
}

/// Handle POST requests (initiate blob uploads)
#[instrument(skip(repo, request))]
pub async fn handle_post(
    repo: DockerHosted,
    request: RepositoryRequest,
) -> Result<RepoResponse, DockerError> {
    let path_str = request.path.to_string();
    let parts: Vec<&str> = path_str.trim_start_matches('/').split('/').collect();

    debug!("Docker POST request path: {:?}", parts);

    match parts.as_slice() {
        // POST /v2/<name>/blobs/uploads/ - Initiate blob upload
        ["v2", name @ .., "blobs", "uploads", ""] | ["v2", name @ .., "blobs", "uploads"]
            if !name.is_empty() =>
        {
            let repository_name = name.join("/");
            initiate_blob_upload(&repo, &repository_name, request).await
        }

        _ => Ok(RepoResponse::basic_text_response(
            StatusCode::NOT_FOUND,
            "Not Found",
        )),
    }
}

/// Handle PATCH requests (chunked blob uploads)
#[instrument(skip(repo, request))]
pub async fn handle_patch(
    repo: DockerHosted,
    request: RepositoryRequest,
) -> Result<RepoResponse, DockerError> {
    let path_str = request.path.to_string();
    let parts: Vec<&str> = path_str.trim_start_matches('/').split('/').collect();

    debug!("Docker PATCH request path: {:?}", parts);

    match parts.as_slice() {
        // PATCH /v2/<name>/blobs/uploads/<uuid> - Upload blob chunk
        ["v2", name @ .., "blobs", "uploads", upload_id] if !name.is_empty() => {
            let repository_name = name.join("/");
            upload_blob_chunk(&repo, &repository_name, upload_id, request).await
        }

        _ => Ok(RepoResponse::basic_text_response(
            StatusCode::NOT_FOUND,
            "Not Found",
        )),
    }
}

/// Handle DELETE requests (delete manifests and blobs)
#[instrument(skip(repo, request))]
pub async fn handle_delete(
    repo: DockerHosted,
    request: RepositoryRequest,
) -> Result<RepoResponse, DockerError> {
    let path_str = request.path.to_string();
    let parts: Vec<&str> = path_str.trim_start_matches('/').split('/').collect();

    debug!("Docker DELETE request path: {:?}", parts);

    match parts.as_slice() {
        // DELETE /v2/<name>/manifests/<reference> - Delete manifest
        ["v2", name @ .., "manifests", reference] if !name.is_empty() => {
            let repository_name = name.join("/");
            delete_manifest(&repo, &repository_name, reference).await
        }

        // DELETE /v2/<name>/blobs/<digest> - Delete blob
        ["v2", name @ .., "blobs", digest] if !name.is_empty() => {
            let repository_name = name.join("/");
            delete_blob(&repo, &repository_name, digest).await
        }

        _ => Ok(RepoResponse::basic_text_response(
            StatusCode::NOT_FOUND,
            "Not Found",
        )),
    }
}

/// GET /v2/ - API version check
fn handle_api_version_check() -> Result<RepoResponse, DockerError> {
    Ok(custom_response(
        StatusCode::OK,
        vec![("Docker-Distribution-API-Version", "registry/2.0")],
        vec![],
    ))
}

/// GET /v2/<name>/tags/list - List all tags
async fn list_tags(
    repo: &DockerHosted,
    repository_name: &str,
) -> Result<RepoResponse, DockerError> {
    debug!("Listing tags for repository: {}", repository_name);

    // TODO: Implement proper tag listing by querying storage directory contents
    // For now, return empty tags list
    let tags: Vec<String> = Vec::new();

    let response = serde_json::json!({
        "name": repository_name,
        "tags": tags
    });

    Ok(json_response(response))
}

/// GET /v2/<name>/manifests/<reference> - Get manifest
async fn get_manifest(
    repo: &DockerHosted,
    repository_name: &str,
    reference: &str,
    accept_header: Option<String>,
) -> Result<RepoResponse, DockerError> {
    debug!("Getting manifest: {}/{}", repository_name, reference);

    let manifest_path = if reference.starts_with("sha256:") {
        // Direct digest reference
        StoragePath::from(format!("v2/{}/manifests/{}", repository_name, reference))
    } else {
        // Tag reference - resolve to digest
        StoragePath::from(format!("v2/{}/manifests/{}", repository_name, reference))
    };

    let file = repo
        .get_storage()
        .open_file(repo.id(), &manifest_path)
        .await?
        .ok_or_else(|| DockerError::ManifestNotFound(reference.to_string()))?;

    let content = get_file_bytes(file).await?;

    // Determine content type from manifest content or request
    let content_type = accept_header
        .as_deref()
        .unwrap_or(MediaType::OCI_IMAGE_MANIFEST);

    // Calculate digest
    let digest = format!("sha256:{:x}", Sha256::digest(&content));

    Ok(custom_response(
        StatusCode::OK,
        vec![
            ("Content-Type", content_type),
            ("Docker-Content-Digest", &digest),
            ("Content-Length", &content.len().to_string()),
        ],
        content.to_vec(),
    ))
}

/// HEAD /v2/<name>/manifests/<reference> - Check manifest exists
async fn head_manifest(
    repo: &DockerHosted,
    repository_name: &str,
    reference: &str,
) -> Result<RepoResponse, DockerError> {
    debug!("Checking manifest: {}/{}", repository_name, reference);

    let manifest_path =
        StoragePath::from(format!("v2/{}/manifests/{}", repository_name, reference));

    let file = repo
        .get_storage()
        .open_file(repo.id(), &manifest_path)
        .await?
        .ok_or_else(|| DockerError::ManifestNotFound(reference.to_string()))?;

    let content = get_file_bytes(file).await?;
    let digest = format!("sha256:{:x}", Sha256::digest(&content));

    Ok(custom_response(
        StatusCode::OK,
        vec![
            ("Docker-Content-Digest", &digest),
            ("Content-Length", &content.len().to_string()),
            ("Content-Type", MediaType::OCI_IMAGE_MANIFEST),
        ],
        vec![],
    ))
}

/// GET /v2/<name>/blobs/<digest> - Download blob
async fn get_blob(
    repo: &DockerHosted,
    repository_name: &str,
    digest: &str,
) -> Result<RepoResponse, DockerError> {
    debug!("Getting blob: {}/{}", repository_name, digest);

    let blob_path = StoragePath::from(format!("v2/{}/blobs/{}", repository_name, digest));

    let file = repo
        .get_storage()
        .open_file(repo.id(), &blob_path)
        .await?
        .ok_or_else(|| DockerError::BlobNotFound(digest.to_string()))?;

    let content = get_file_bytes(file).await?;

    Ok(custom_response(
        StatusCode::OK,
        vec![
            ("Docker-Content-Digest", digest),
            ("Content-Length", &content.len().to_string()),
            ("Content-Type", "application/octet-stream"),
        ],
        content.to_vec(),
    ))
}

/// HEAD /v2/<name>/blobs/<digest> - Check blob exists
async fn head_blob(
    repo: &DockerHosted,
    repository_name: &str,
    digest: &str,
) -> Result<RepoResponse, DockerError> {
    debug!("Checking blob: {}/{}", repository_name, digest);

    let blob_path = StoragePath::from(format!("v2/{}/blobs/{}", repository_name, digest));

    let file = repo
        .get_storage()
        .open_file(repo.id(), &blob_path)
        .await?
        .ok_or_else(|| DockerError::BlobNotFound(digest.to_string()))?;

    let content = get_file_bytes(file).await?;

    Ok(custom_response(
        StatusCode::OK,
        vec![
            ("Docker-Content-Digest", digest),
            ("Content-Length", &content.len().to_string()),
            ("Content-Type", "application/octet-stream"),
        ],
        vec![],
    ))
}

/// PUT /v2/<name>/manifests/<reference> - Upload manifest
async fn put_manifest(
    repo: &DockerHosted,
    repository_name: &str,
    reference: &str,
    request: RepositoryRequest,
) -> Result<RepoResponse, DockerError> {
    info!("Uploading manifest: {}/{}", repository_name, reference);

    // Check authentication
    if request.authentication.get_user().is_none() {
        return Ok(RepoResponse::unauthorized());
    }

    let content_type = request
        .parts
        .headers
        .get("Content-Type")
        .and_then(|h| h.to_str().ok())
        .unwrap_or(MediaType::OCI_IMAGE_MANIFEST);

    let body = request.body.body_as_bytes().await?;

    // Parse and validate manifest
    let _manifest = Manifest::from_bytes(&body, content_type)
        .map_err(|e| DockerError::InvalidManifest(e.to_string()))?;

    // Calculate digest
    let digest = format!("sha256:{:x}", Sha256::digest(&body));

    // Check if tag already exists and overwrite is not allowed
    if !reference.starts_with("sha256:") {
        let allow_overwrite = {
            let push_rules = repo.push_rules.read();
            push_rules.allow_tag_overwrite
        }; // Drop lock before await

        if !allow_overwrite {
            let tag_path =
                StoragePath::from(format!("v2/{}/manifests/{}", repository_name, reference));
            if repo
                .get_storage()
                .open_file(repo.id(), &tag_path)
                .await?
                .is_some()
            {
                return Err(DockerError::TagOverwriteNotAllowed(reference.to_string()));
            }
        }
    }

    // Save manifest by tag/reference
    let manifest_path =
        StoragePath::from(format!("v2/{}/manifests/{}", repository_name, reference));
    repo.get_storage()
        .save_file(repo.id(), body.clone().into(), &manifest_path)
        .await?;

    // Also save by digest if this is a tag reference
    if !reference.starts_with("sha256:") {
        let digest_path = StoragePath::from(format!("v2/{}/manifests/{}", repository_name, digest));
        repo.get_storage()
            .save_file(repo.id(), body.into(), &digest_path)
            .await?;
    }

    Ok(custom_response(
        StatusCode::CREATED,
        vec![
            ("Docker-Content-Digest", &digest),
            (
                "Location",
                &format!("/v2/{}/manifests/{}", repository_name, digest),
            ),
        ],
        vec![],
    ))
}

/// POST /v2/<name>/blobs/uploads/ - Initiate blob upload
async fn initiate_blob_upload(
    repo: &DockerHosted,
    repository_name: &str,
    request: RepositoryRequest,
) -> Result<RepoResponse, DockerError> {
    info!("Initiating blob upload for: {}", repository_name);

    // Check authentication
    if request.authentication.get_user().is_none() {
        return Ok(RepoResponse::unauthorized());
    }

    // Generate upload ID
    let upload_id = uuid::Uuid::new_v4().to_string();

    // Create upload session directory
    let upload_path = StoragePath::from(format!("v2/{}/uploads/{}", repository_name, upload_id));
    repo.get_storage()
        .save_file(repo.id(), vec![].into(), &upload_path)
        .await?;

    let location = format!("/v2/{}/blobs/uploads/{}", repository_name, upload_id);

    Ok(custom_response(
        StatusCode::ACCEPTED,
        vec![
            ("Location", &location),
            ("Range", "0-0"),
            ("Docker-Upload-UUID", &upload_id),
        ],
        vec![],
    ))
}

/// PATCH /v2/<name>/blobs/uploads/<uuid> - Upload blob chunk
async fn upload_blob_chunk(
    repo: &DockerHosted,
    repository_name: &str,
    upload_id: &str,
    request: RepositoryRequest,
) -> Result<RepoResponse, DockerError> {
    debug!("Uploading blob chunk: {}/{}", repository_name, upload_id);

    // Check authentication
    if request.authentication.get_user().is_none() {
        return Ok(RepoResponse::unauthorized());
    }

    let upload_path = StoragePath::from(format!("v2/{}/uploads/{}", repository_name, upload_id));

    // Get existing data
    let existing_file = repo
        .get_storage()
        .open_file(repo.id(), &upload_path)
        .await?;
    let mut existing_data = if let Some(upload_file) = existing_file {
        get_file_bytes(upload_file).await?
    } else {
        return Err(DockerError::BlobUploadNotFound(upload_id.to_string()));
    };

    // Append new data
    let new_data = request.body.body_as_bytes().await?;
    existing_data.extend_from_slice(&new_data);

    // Save updated data
    repo.get_storage()
        .save_file(repo.id(), existing_data.clone().into(), &upload_path)
        .await?;

    let range = format!("0-{}", existing_data.len());
    let location = format!("/v2/{}/blobs/uploads/{}", repository_name, upload_id);

    Ok(custom_response(
        StatusCode::ACCEPTED,
        vec![
            ("Location", &location),
            ("Range", &range),
            ("Docker-Upload-UUID", upload_id),
        ],
        vec![],
    ))
}

/// PUT /v2/<name>/blobs/uploads/<uuid>?digest=<digest> - Complete blob upload
async fn complete_blob_upload(
    repo: &DockerHosted,
    repository_name: &str,
    upload_id: &str,
    request: RepositoryRequest,
) -> Result<RepoResponse, DockerError> {
    info!("Completing blob upload: {}/{}", repository_name, upload_id);

    // Check authentication
    if request.authentication.get_user().is_none() {
        return Ok(RepoResponse::unauthorized());
    }

    // Extract digest from query parameters
    let raw_digest = request
        .parts
        .uri
        .query()
        .and_then(|q| {
            q.split('&')
                .find(|param| param.starts_with("digest="))
                .and_then(|param| param.strip_prefix("digest="))
        })
        .ok_or_else(|| DockerError::InvalidManifest("Missing digest parameter".to_string()))?;

    let digest = percent_decode(raw_digest).map_err(|err| {
        DockerError::InvalidManifest(format!("Invalid digest encoding: {err}"))
    })?;

    let upload_path = StoragePath::from(format!("v2/{}/uploads/{}", repository_name, upload_id));

    // Get uploaded data
    let upload_file = repo
        .get_storage()
        .open_file(repo.id(), &upload_path)
        .await?
        .ok_or_else(|| DockerError::BlobUploadNotFound(upload_id.to_string()))?;

    let mut data = get_file_bytes(upload_file).await?;

    // Append any final data from request body
    let final_data = request.body.body_as_bytes().await?;
    if !final_data.is_empty() {
        data.extend_from_slice(&final_data);
    }

    // Verify digest
    let calculated_digest = format!("sha256:{:x}", Sha256::digest(&data));
    if calculated_digest != digest {
        return Err(DockerError::DigestMismatch {
            expected: digest.to_string(),
            actual: calculated_digest,
        });
    }

    // Save blob
    let blob_path = StoragePath::from(format!("v2/{}/blobs/{}", repository_name, digest));
    repo.get_storage()
        .save_file(repo.id(), data.into(), &blob_path)
        .await?;

    // Clean up upload session
    repo.get_storage()
        .delete_file(repo.id(), &upload_path)
        .await?;

    let location = format!("/v2/{}/blobs/{}", repository_name, digest);

    Ok(custom_response(
        StatusCode::CREATED,
        vec![
            ("Location", &location),
            ("Docker-Content-Digest", digest.as_str()),
        ],
        vec![],
    ))
}

fn percent_decode(value: &str) -> Result<String, &'static str> {
    let mut output = String::with_capacity(value.len());
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'%' => {
                if index + 2 >= bytes.len() {
                    return Err("truncated percent encoding");
                }
                let hi = bytes[index + 1];
                let lo = bytes[index + 2];
                let decoded = hex_pair_to_byte(hi, lo).ok_or("invalid percent encoding")?;
                output.push(decoded as char);
                index += 3;
            }
            b'+' => {
                output.push(' ');
                index += 1;
            }
            other => {
                output.push(other as char);
                index += 1;
            }
        }
    }
    Ok(output)
}

fn hex_pair_to_byte(high: u8, low: u8) -> Option<u8> {
    fn value(byte: u8) -> Option<u8> {
        match byte {
            b'0'..=b'9' => Some(byte - b'0'),
            b'a'..=b'f' => Some(byte - b'a' + 10),
            b'A'..=b'F' => Some(byte - b'A' + 10),
            _ => None,
        }
    }

    Some(value(high)? << 4 | value(low)?)
}

/// DELETE /v2/<name>/manifests/<reference> - Delete manifest
async fn delete_manifest(
    repo: &DockerHosted,
    repository_name: &str,
    reference: &str,
) -> Result<RepoResponse, DockerError> {
    info!("Deleting manifest: {}/{}", repository_name, reference);

    // Check authentication
    // if request.authentication.get_user().is_none() {
    //     return Ok(RepoResponse::unauthorized());
    // }

    let manifest_path =
        StoragePath::from(format!("v2/{}/manifests/{}", repository_name, reference));

    repo.get_storage()
        .delete_file(repo.id(), &manifest_path)
        .await?;

    Ok(custom_response(StatusCode::ACCEPTED, vec![], vec![]))
}

/// DELETE /v2/<name>/blobs/<digest> - Delete blob
async fn delete_blob(
    repo: &DockerHosted,
    repository_name: &str,
    digest: &str,
) -> Result<RepoResponse, DockerError> {
    info!("Deleting blob: {}/{}", repository_name, digest);

    let blob_path = StoragePath::from(format!("v2/{}/blobs/{}", repository_name, digest));

    repo.get_storage()
        .delete_file(repo.id(), &blob_path)
        .await?;

    Ok(custom_response(StatusCode::ACCEPTED, vec![], vec![]))
}
