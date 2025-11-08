//! Docker Registry API V2 HTTP handlers
//!
//! Implements the Docker Registry HTTP API V2 specification.
//! Reference: https://docs.docker.com/registry/spec/api/

use axum::{body::Body, response::Response};
use bytes::Bytes;
use futures::StreamExt;
use http::StatusCode;
use nr_core::{
    storage::{FileHashes, StoragePath},
    utils::base64_utils,
};
use nr_storage::{Storage, StorageError, StorageFile, local::LocalStorage};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWrite, AsyncWriteExt, BufWriter};
use tokio_util::io::ReaderStream;
use tracing::{debug, info, instrument, warn};

use super::{
    DockerError, DockerHosted, RepoResponse, Repository, RepositoryHandlerError, RepositoryRequest,
    types::{Manifest, MediaType},
};
use crate::app::{BlobUploadStateHandle, FinalizedUpload, NitroRepo};
use uuid::Uuid;

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

const LOCAL_UPLOAD_BUFFER_SIZE: usize = 4 * 1024 * 1024;

#[tracing::instrument(
    name = "docker_stream_to_writer",
    skip(writer, on_chunk_written, stream),
    fields(
        chunk_count = tracing::field::Empty,
        total_bytes = tracing::field::Empty
    )
)]
pub(super) async fn stream_to_writer<S, W, F>(
    mut stream: S,
    writer: &mut BufWriter<W>,
    mut on_chunk_written: F,
) -> Result<(), DockerError>
where
    S: futures::Stream<Item = Result<Bytes, RepositoryHandlerError>> + Unpin,
    W: AsyncWrite + Unpin,
    F: FnMut(&[u8]) -> Result<(), DockerError>,
{
    let mut chunk_count = 0u64;
    let mut total_bytes = 0u64;

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(DockerError::from)?;
        if chunk.is_empty() {
            continue;
        }

        chunk_count += 1;
        total_bytes += chunk.len() as u64;

        writer.write_all(&chunk).await.map_err(DockerError::from)?;
        on_chunk_written(&chunk)?;

        // Record progress every 10 chunks
        if chunk_count % 10 == 0 {
            tracing::Span::current().record("chunk_count", chunk_count);
            tracing::Span::current().record("total_bytes", total_bytes);
        }
    }

    // Final update
    tracing::Span::current().record("chunk_count", chunk_count);
    tracing::Span::current().record("total_bytes", total_bytes);

    writer.flush().await.map_err(DockerError::from)?;

    Ok(())
}

async fn collect_stream_bytes<S>(mut stream: S) -> Result<Vec<u8>, DockerError>
where
    S: futures::Stream<Item = Result<Bytes, RepositoryHandlerError>> + Unpin,
{
    let mut data = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(DockerError::from)?;
        if !chunk.is_empty() {
            data.extend_from_slice(&chunk);
        }
    }
    Ok(data)
}

async fn write_local_stream<S>(
    storage: LocalStorage,
    repository_id: Uuid,
    upload_path: &StoragePath,
    stream: S,
    site: &NitroRepo,
    upload_id: &str,
    state_handle: BlobUploadStateHandle,
) -> Result<u64, DockerError>
where
    S: futures::Stream<Item = Result<Bytes, RepositoryHandlerError>> + Unpin,
{
    let (file, _path) = storage
        .open_append_handle(repository_id, upload_path)
        .await
        .map_err(StorageError::from)
        .map_err(DockerError::from)?;

    let mut writer = BufWriter::with_capacity(LOCAL_UPLOAD_BUFFER_SIZE, file);

    let handle_for_stream = state_handle.clone();

    stream_to_writer(stream, &mut writer, |chunk| {
        site.update_upload_state_handle(&handle_for_stream, chunk);
        Ok(())
    })
    .await?;

    Ok(site.blob_upload_state_length(&state_handle))
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

    // Determine content type from the stored manifest itself
    let content_type = if let Ok(value) = serde_json::from_slice::<serde_json::Value>(&content) {
        if let Some(media_type) = value.get("mediaType").and_then(|v| v.as_str()) {
            media_type.to_string()
        } else {
            // No mediaType field - likely OCI manifest (mediaType is optional in OCI spec)
            MediaType::OCI_IMAGE_MANIFEST.to_string()
        }
    } else {
        MediaType::OCI_IMAGE_MANIFEST.to_string()
    };

    // If client sent Accept header, verify we can serve what they want
    if let Some(ref accept) = accept_header {
        // Client may send multiple types separated by comma
        let acceptable_types: Vec<&str> = accept.split(',').map(|s| s.trim()).collect();
        if !acceptable_types.is_empty()
            && !acceptable_types.contains(&"*/*")
            && !acceptable_types
                .iter()
                .any(|&t| t == content_type || t.starts_with("application/*"))
        {
            debug!(
                "Client requested {} but manifest is {}",
                accept, content_type
            );
        }
    }

    // Calculate digest
    let digest = format!("sha256:{:x}", Sha256::digest(&content));

    Ok(custom_response(
        StatusCode::OK,
        vec![
            ("Content-Type", &content_type),
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

    // Determine content type from the stored manifest itself
    let content_type = if let Ok(value) = serde_json::from_slice::<serde_json::Value>(&content) {
        if let Some(media_type) = value.get("mediaType").and_then(|v| v.as_str()) {
            media_type.to_string()
        } else {
            MediaType::OCI_IMAGE_MANIFEST.to_string()
        }
    } else {
        MediaType::OCI_IMAGE_MANIFEST.to_string()
    };

    Ok(custom_response(
        StatusCode::OK,
        vec![
            ("Docker-Content-Digest", &digest),
            ("Content-Length", &content.len().to_string()),
            ("Content-Type", &content_type),
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

    let (reader, meta) = file
        .file()
        .ok_or_else(|| DockerError::BlobNotFound(digest.to_string()))?;

    let size = meta.file_type.file_size;
    let stream = ReaderStream::new(reader);

    let mut builder = Response::builder().status(StatusCode::OK);
    builder = builder.header("Docker-Content-Digest", digest);
    builder = builder.header("Content-Length", size.to_string());
    builder = builder.header("Content-Type", "application/octet-stream");

    Ok(RepoResponse::Other(
        builder
            .body(Body::from_stream(stream))
            .expect("failed to build blob response"),
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

    let (_, meta) = file
        .file()
        .ok_or_else(|| DockerError::BlobNotFound(digest.to_string()))?;

    let size = meta.file_type.file_size;
    let stored_digest = meta
        .file_type
        .file_hash
        .sha2_256
        .as_ref()
        .map(|hash| format!("sha256:{hash}"))
        .unwrap_or_else(|| digest.to_string());

    Ok(custom_response(
        StatusCode::OK,
        vec![
            ("Docker-Content-Digest", stored_digest.as_str()),
            ("Content-Length", &size.to_string()),
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
#[tracing::instrument(
    name = "docker_initiate_blob_upload",
    skip(repo),
    fields(
        repository_name,
        user_id = tracing::field::Empty,
        upload_id = tracing::field::Empty
    )
)]
async fn initiate_blob_upload(
    repo: &DockerHosted,
    repository_name: &str,
    request: RepositoryRequest,
) -> Result<RepoResponse, DockerError> {
    let user_id = request.authentication.get_user().map(|u| u.id.to_string());
    tracing::Span::current().record(
        "user_id",
        &user_id.unwrap_or_else(|| "anonymous".to_string()),
    );

    info!("Initiating blob upload for: {}", repository_name);

    // Check authentication
    if request.authentication.get_user().is_none() {
        return Ok(RepoResponse::unauthorized());
    }

    // Generate upload ID
    let upload_id = uuid::Uuid::new_v4().to_string();
    tracing::Span::current().record("upload_id", &upload_id);

    // Prepare upload state tracking (SHA256 only for Docker)
    let site = repo.site();
    site.begin_docker_blob_upload_state(repo.id(), &upload_id);

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
#[tracing::instrument(
    name = "docker_upload_blob_chunk",
    skip(repo),
    fields(
        repository_name,
        upload_id,
        chunk_size = tracing::field::Empty
    )
)]
async fn upload_blob_chunk(
    repo: &DockerHosted,
    repository_name: &str,
    upload_id: &str,
    request: RepositoryRequest,
) -> Result<RepoResponse, DockerError> {
    debug!("Uploading blob chunk: {}/{}", repository_name, upload_id);

    let RepositoryRequest {
        body,
        authentication,
        ..
    } = request;

    // Check authentication
    if authentication.get_user().is_none() {
        return Ok(RepoResponse::unauthorized());
    }

    let upload_path = StoragePath::from(format!("v2/{}/uploads/{}", repository_name, upload_id));

    let site = repo.site();
    let state_handle = if let Some(handle) = site.get_upload_state_handle(repo.id(), upload_id) {
        handle
    } else {
        return Err(DockerError::BlobUploadNotFound(upload_id.to_string()));
    };
    let mut total_size = site.blob_upload_state_length(&state_handle);

    let stream = body.into_byte_stream();

    match repo.get_storage() {
        nr_storage::DynStorage::Local(local) => {
            total_size = write_local_stream(
                local,
                repo.id(),
                &upload_path,
                stream,
                &site,
                upload_id,
                state_handle.clone(),
            )
            .await?;
        }
        storage => {
            let bytes = collect_stream_bytes(stream).await?;
            if !bytes.is_empty() {
                storage
                    .append_file(repo.id(), bytes.clone().into(), &upload_path)
                    .await?;

                // Update blob upload state
                total_size = site.update_upload_state_handle(&state_handle, &bytes);
            }
        }
    }
    drop(state_handle);

    let range_end = if total_size == 0 {
        0
    } else {
        total_size.saturating_sub(1)
    };
    let range = format!("0-{}", range_end);
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
#[tracing::instrument(
    name = "docker_complete_blob_upload",
    skip(repo),
    fields(
        repository_name,
        upload_id,
        final_size = tracing::field::Empty,
        upload_duration_ms = tracing::field::Empty
    )
)]
async fn complete_blob_upload(
    repo: &DockerHosted,
    repository_name: &str,
    upload_id: &str,
    request: RepositoryRequest,
) -> Result<RepoResponse, DockerError> {
    info!("Completing blob upload: {}/{}", repository_name, upload_id);

    let RepositoryRequest {
        parts,
        body,
        authentication,
        ..
    } = request;

    // Check authentication
    if authentication.get_user().is_none() {
        return Ok(RepoResponse::unauthorized());
    }

    // Extract digest from query parameters
    let raw_digest = parts
        .uri
        .query()
        .and_then(|q| {
            q.split('&')
                .find(|param| param.starts_with("digest="))
                .and_then(|param| param.strip_prefix("digest="))
        })
        .ok_or_else(|| DockerError::InvalidManifest("Missing digest parameter".to_string()))?;

    let digest = percent_decode(raw_digest)
        .map_err(|err| DockerError::InvalidManifest(format!("Invalid digest encoding: {err}")))?;

    let upload_path = StoragePath::from(format!("v2/{}/uploads/{}", repository_name, upload_id));
    let blob_path = StoragePath::from(format!("v2/{}/blobs/{}", repository_name, digest));

    let site = repo.site();
    let state_handle = if let Some(handle) = site.get_upload_state_handle(repo.id(), upload_id) {
        handle
    } else {
        return Err(DockerError::BlobUploadNotFound(upload_id.to_string()));
    };
    let mut _current_size = site.blob_upload_state_length(&state_handle);

    let stream = body.into_byte_stream();

    match repo.get_storage() {
        nr_storage::DynStorage::Local(local) => {
            _current_size = write_local_stream(
                local,
                repo.id(),
                &upload_path,
                stream,
                &site,
                upload_id,
                state_handle.clone(),
            )
            .await?;
        }
        storage => {
            let bytes = collect_stream_bytes(stream).await?;
            if !bytes.is_empty() {
                storage
                    .append_file(repo.id(), bytes.clone().into(), &upload_path)
                    .await?;

                // Update blob upload state
                _current_size = site.update_upload_state_handle(&state_handle, &bytes);
            }
        }
    }

    drop(state_handle);

    let finalized = if let Some(result) = site.finalize_blob_upload_state(repo.id(), upload_id) {
        result
    } else {
        // Fallback: compute digest by reading the file (legacy behaviour)
        // For Docker, only SHA256 is needed
        let upload_file = repo
            .get_storage()
            .open_file(repo.id(), &upload_path)
            .await?
            .ok_or_else(|| DockerError::BlobUploadNotFound(upload_id.to_string()))?;
        let data_bytes = get_file_bytes(upload_file).await?;
        let sha2_bytes = Sha256::digest(&data_bytes);
        let digest_value = format!("sha256:{:x}", sha2_bytes);
        let hashes = FileHashes {
            md5: None,
            sha1: None,
            sha2_256: Some(base64_utils::encode(&sha2_bytes)),
            sha3_256: None,
        };
        FinalizedUpload {
            digest: digest_value,
            hashes,
            length: data_bytes.len() as u64,
        }
    };

    if finalized.digest != digest {
        site.abandon_blob_upload_state(repo.id(), upload_id);
        repo.get_storage()
            .delete_file(repo.id(), &upload_path)
            .await?;
        return Err(DockerError::DigestMismatch {
            expected: digest.to_string(),
            actual: finalized.digest,
        });
    }

    if let nr_storage::DynStorage::Local(local) = repo.get_storage() {
        local.register_precomputed_hash(repo.id(), &blob_path, finalized.hashes.clone());
    }

    // Move upload to final blob location
    let moved = repo
        .get_storage()
        .move_file(repo.id(), &upload_path, &blob_path)
        .await?;
    if !moved {
        site.abandon_blob_upload_state(repo.id(), upload_id);
        return Err(DockerError::BlobUploadNotFound(upload_id.to_string()));
    }

    let location = format!("/v2/{}/blobs/{}", repository_name, digest);

    Ok(custom_response(
        StatusCode::CREATED,
        vec![
            ("Location", &location),
            ("Docker-Content-Digest", finalized.digest.as_str()),
            ("Content-Length", "0"),
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
    let manifest_path_str = manifest_path.to_string();

    // Use the same comprehensive deletion logic as the packages API
    // This ensures proper garbage collection of associated blobs and layers
    match crate::app::api::repository::packages::delete_docker_package(
        &repo.get_storage(),
        repo.id(),
        &manifest_path_str,
    )
    .await
    {
        Ok(result) => {
            info!(
                "Successfully deleted Docker manifest: {}/{} (removed {} manifests, {} blobs)",
                repository_name, reference, result.removed_manifests, result.removed_blobs
            );
            Ok(custom_response(StatusCode::ACCEPTED, vec![], vec![]))
        }
        Err(crate::app::api::repository::packages::DockerDeletionError::ManifestMissing) => {
            info!("Manifest not found: {}/{}", repository_name, reference);
            // Still return ACCEPTED as the spec requires idempotent deletion
            Ok(custom_response(StatusCode::ACCEPTED, vec![], vec![]))
        }
        Err(err) => {
            warn!(
                "Failed to delete Docker manifest: {}/{} - {}",
                repository_name, reference, err
            );
            Err(DockerError::InvalidManifest(format!(
                "Failed to delete manifest: {}",
                err
            )))
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use futures::stream;
    use tempfile::tempdir;
    use tokio::{
        fs::{self, OpenOptions},
        io::{AsyncReadExt, BufWriter},
    };

    fn test_stream_from_bytes(
        data: &[u8],
        chunk_size: usize,
    ) -> impl futures::Stream<Item = Result<Bytes, RepositoryHandlerError>> {
        let chunks = data
            .chunks(chunk_size)
            .map(|chunk| Bytes::copy_from_slice(chunk))
            .collect::<Vec<_>>();
        stream::iter(chunks.into_iter().map(|bytes| Ok(bytes)))
    }

    #[tokio::test]
    async fn stream_writer_persists_full_payload() -> anyhow::Result<()> {
        let payload = (0u32..(512 * 1024))
            .map(|value| (value % 251) as u8)
            .collect::<Vec<u8>>();
        let dir = tempdir()?;
        let file_path = dir.path().join("payload.bin");
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .open(&file_path)
            .await?;
        let mut writer = BufWriter::with_capacity(16 * 1024, file);
        let mut observed = Vec::new();
        let mut total_written = 0usize;

        stream_to_writer(
            test_stream_from_bytes(&payload, 1024),
            &mut writer,
            |chunk| {
                total_written += chunk.len();
                observed.extend_from_slice(chunk);
                Ok(())
            },
        )
        .await?;

        drop(writer);

        let mut saved = Vec::new();
        fs::File::open(&file_path)
            .await?
            .read_to_end(&mut saved)
            .await?;

        assert_eq!(total_written, payload.len());
        assert_eq!(observed, payload);
        assert_eq!(saved, payload);

        Ok(())
    }
}
