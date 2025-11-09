# Docker Repository Architecture Refactoring Plan

## Executive Summary

The current Docker repository architecture tightly couples Docker operations to the main application thread and relies on in-memory state management. This creates scalability issues, potential deadlocks, and single points of failure. This document outlines a comprehensive refactoring plan to transform the Docker operations into an independent, database-backed service with dedicated threading.

## Current Architecture Problems

### 1. Main Thread Blocking
```rust
// Current runtime configuration
tokio::runtime::Builder::new_current_thread()
    .enable_all()
    .build()
    .unwrap()
```
- All Docker operations execute on the main tokio thread
- Large file uploads can block the entire application
- UI becomes unresponsive during Docker operations
- No isolation between different repository operations

### 2. Shared In-Memory State
```rust
// Current problematic state management
pub struct NitroRepo {
    // ...
    blob_upload_states: parking_lot::Mutex<HashMap<(Uuid, String), Arc<parking_lot::Mutex<UploadState>>>>,
}

pub struct UploadState {
    md5: md5::Context,
    sha1: sha1::Digest,
    sha256: sha2::Sha256,
    sha3: sha3::Sha3_256,
    len: u64,
}
```
- All upload state stored in application memory
- Single mutex protects all upload operations
- Memory leaks from failed uploads
- Application restart loses all active uploads
- No visibility into upload progress across instances

### 3. Tight Coupling
- Docker handlers directly depend on `NitroRepo` instance
- No isolation between different Docker repositories
- Error propagation can affect entire application
- Difficult to test Docker operations independently

### 4. Scalability Limitations
- Concurrent uploads compete for same mutex
- No resource limiting or backpressure handling
- Single-threaded processing limits throughput
- Cannot leverage multi-core systems for Docker operations

## Target Architecture

### Design Principles
1. **Isolation**: Docker operations independent from main application
2. **Persistence**: Upload state stored in database, not memory
3. **Scalability**: Dedicated thread pool for Docker operations
4. **Reliability**: Automatic recovery from failures
5. **Observability**: Full visibility into upload progress and performance
6. **Incremental Adoption**: Support a dual-path rollout so existing installations can migrate gradually.
7. **Predictable Backpressure**: Ensure overload conditions shed work gracefully instead of stalling the front door.

### New Architecture Overview

```
┌─────────────────┐    ┌─────────────────┐    ┌─────────────────┐
│   HTTP Gateway  │    │  Docker Upload  │    │   Database      │
│   (main app)    │───▶│    Service      │───▶│   PostgreSQL    │
└─────────────────┘    └─────────────────┘    └─────────────────┘
                              │
                              ▼
                       ┌─────────────────┐
                       │  Dedicated      │
                       │  Thread Pool    │
                       └─────────────────┘
```

## Implementation Plan

### Phase 1: Database Schema Design (Week 1)

#### 1.1 Database Tables

```sql
-- Active blob uploads table
CREATE TABLE docker_blob_uploads (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    repository_id UUID NOT NULL REFERENCES repositories(id),
    upload_id VARCHAR(255) NOT NULL UNIQUE,
    status VARCHAR(50) NOT NULL DEFAULT 'pending',
    total_bytes BIGINT DEFAULT 0,
    uploaded_bytes BIGINT DEFAULT 0,
    sha256_digest VARCHAR(128),
    created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
    updated_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
    expires_at TIMESTAMP WITH TIME ZONE
);

-- Individual chunks for resumable uploads
CREATE TABLE docker_upload_chunks (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    upload_id UUID NOT NULL REFERENCES docker_blob_uploads(id),
    chunk_number BIGINT NOT NULL,
    chunk_size BIGINT NOT NULL,
    chunk_hash VARCHAR(128),
    created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
    UNIQUE(upload_id, chunk_number)
);

-- Upload metadata and configuration
CREATE TABLE docker_upload_metadata (
    upload_id UUID PRIMARY KEY REFERENCES docker_blob_uploads(id),
    content_type VARCHAR(255),
    user_agent TEXT,
    client_ip INET,
    repository_name VARCHAR(255),
    tag_name VARCHAR(255)
);

-- Indexes for performance
CREATE INDEX idx_docker_blob_uploads_repository_id ON docker_blob_uploads(repository_id);
CREATE INDEX idx_docker_blob_uploads_status ON docker_blob_uploads(status);
CREATE INDEX idx_docker_blob_uploads_created_at ON docker_blob_uploads(created_at);
CREATE INDEX idx_docker_upload_chunks_upload_id ON docker_upload_chunks(upload_id);
```

> **Reviewer note (2025‑11‑06):** Consider adding a uniqueness constraint on `(repository_id, sha256_digest)` once blobs are finalized to guard against duplicate manifest moves, and persist a lightweight `docker_upload_events` table for auditing/retry metadata. Both changes improve deduplication and observability without complicating the hot path.

#### 1.2 Database Entities

```rust
// crates/core/src/database/entities/docker_upload.rs
use sea_orm::entity::prelude::*;
use sea_orm::ActiveValue;
use chrono::{DateTime, Utc};
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "docker_blob_uploads")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: Uuid,
    pub repository_id: Uuid,
    pub upload_id: String,
    pub status: String, // pending, uploading, completed, failed, abandoned
    pub total_bytes: i64,
    pub uploaded_bytes: i64,
    pub sha256_digest: Option<String>,
    pub created_at: DateTimeWithTimeZone,
    pub updated_at: DateTimeWithTimeZone,
    pub expires_at: Option<DateTimeWithTimeZone>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::docker_upload_chunk::Entity")]
    UploadChunks,
    #[sea_orm(
        belongs_to = "super::repository::Entity",
        from = "Column::RepositoryId",
        to = "super::repository::Column::Id"
    )]
    Repository,
}

impl Related<super::docker_upload_chunk::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::UploadChunks.def()
    }
}

impl Related<super::repository::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Repository.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}

// Chunk entity for resumable uploads
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "docker_upload_chunks")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: Uuid,
    pub upload_id: Uuid,
    pub chunk_number: i64,
    pub chunk_size: i64,
    pub chunk_hash: Option<String>,
    pub created_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::docker_upload::Entity",
        from = "Column::UploadId",
        to = "super::docker_upload::Column::Id"
    )]
    BlobUpload,
}

impl Related<super::docker_upload::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::BlobUpload.def()
    }
}
```

#### 1.3 Migration Scripts

```rust
// crates/core/src/database/migrations/src/m20250107_create_docker_upload_tables.rs
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Create docker_blob_uploads table
        manager
            .create_table(
                Table::create()
                    .table(DockerBlobUpload::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(DockerBlobUpload::Id).uuid().not_null().primary_key())
                    .col(ColumnDef::new(DockerBlobUpload::RepositoryId).uuid().not_null())
                    .col(ColumnDef::new(DockerBlobUpload::UploadId).string().not_null().unique_key())
                    .col(ColumnDef::new(DockerBlobUpload::Status).string().not_null().default("pending"))
                    .col(ColumnDef::new(DockerBlobUpload::TotalBytes).big_integer().default(0))
                    .col(ColumnDef::new(DockerBlobUpload::UploadedBytes).big_integer().default(0))
                    .col(ColumnDef::new(DockerBlobUpload::Sha256Digest).string())
                    .col(ColumnDef::new(DockerBlobUpload::CreatedAt).timestamp_with_time_zone().not_null().default(Expr::current_timestamp()))
                    .col(ColumnDef::new(DockerBlobUpload::UpdatedAt).timestamp_with_time_zone().not_null().default(Expr::current_timestamp()))
                    .col(ColumnDef::new(DockerBlobUpload::ExpiresAt).timestamp_with_time_zone())
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_docker_upload_repository")
                            .from(DockerBlobUpload::Table, DockerBlobUpload::RepositoryId)
                            .to(Repository::Table, Repository::Id)
                            .on_delete(ForeignKeyAction::Cascade)
                    )
                    .to_owned(),
            )
            .await?;

        // Create indexes
        manager
            .create_index(
                Index::create()
                    .name("idx_docker_blob_uploads_repository_id")
                    .table(DockerBlobUpload::Table)
                    .col(DockerBlobUpload::RepositoryId)
                    .to_owned(),
            )
            .await?;

        // Create docker_upload_chunks table
        manager
            .create_table(
                Table::create()
                    .table(DockerUploadChunk::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(DockerUploadChunk::Id).uuid().not_null().primary_key())
                    .col(ColumnDef::new(DockerUploadChunk::UploadId).uuid().not_null())
                    .col(ColumnDef::new(DockerUploadChunk::ChunkNumber).big_integer().not_null())
                    .col(ColumnDef::new(DockerUploadChunk::ChunkSize).big_integer().not_null())
                    .col(ColumnDef::new(DockerUploadChunk::ChunkHash).string())
                    .col(ColumnDef::new(DockerUploadChunk::CreatedAt).timestamp_with_time_zone().not_null().default(Expr::current_timestamp()))
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_docker_chunk_upload")
                            .from(DockerUploadChunk::Table, DockerUploadChunk::UploadId)
                            .to(DockerBlobUpload::Table, DockerBlobUpload::Id)
                            .on_delete(ForeignKeyAction::Cascade)
                    )
                    .index(
                        Index::create()
                            .name("idx_docker_upload_chunk_unique")
                            .col(DockerUploadChunk::UploadId)
                            .col(DockerUploadChunk::ChunkNumber)
                            .unique()
                    )
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(DockerUploadChunk::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(DockerBlobUpload::Table).to_owned())
            .await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum DockerBlobUpload {
    Table,
    Id,
    RepositoryId,
    UploadId,
    Status,
    TotalBytes,
    UploadedBytes,
    Sha256Digest,
    CreatedAt,
    UpdatedAt,
    ExpiresAt,
}

#[derive(DeriveIden)]
enum DockerUploadChunk {
    Table,
    Id,
    UploadId,
    ChunkNumber,
    ChunkSize,
    ChunkHash,
    CreatedAt,
}
```

### Phase 2: Independent Upload Service (Week 2-3)

#### 2.1 Docker Upload Service Structure

```rust
// nitro_repo/src/repository/docker/upload_service.rs
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;
use chrono::{DateTime, Utc, Duration};
use sha2::{Sha256, Digest};
use sea_orm::*;

use crate::database::entities::{docker_upload, docker_upload_chunk, repository};
use crate::database::Database;
use crate::error::AppError;

#[derive(Clone)]
pub struct DockerUploadService {
    db: Arc<Database>,
    config: UploadServiceConfig,
}

#[derive(Clone)]
pub struct UploadServiceConfig {
    pub max_upload_size: u64,
    pub upload_timeout: Duration,
    pub cleanup_interval: Duration,
    pub chunk_size: usize,
}

impl Default for UploadServiceConfig {
    fn default() -> Self {
        Self {
            max_upload_size: 5 * 1024 * 1024 * 1024, // 5GB
            upload_timeout: Duration::hours(24),
            cleanup_interval: Duration::minutes(30),
            chunk_size: 64 * 1024, // 64KB chunks
        }
    }
}

#[derive(Debug, Clone)]
pub struct UploadSession {
    pub id: Uuid,
    pub upload_id: String,
    pub repository_id: Uuid,
    pub status: UploadStatus,
    pub total_bytes: u64,
    pub uploaded_bytes: u64,
    pub sha256_context: Sha256,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum UploadStatus {
    Pending,
    Uploading,
    Completed,
    Failed(String),
    Abandoned,
}

impl DockerUploadService {
    pub fn new(db: Arc<Database>) -> Self {
        Self {
            db,
            config: UploadServiceConfig::default(),
        }
    }

    /// Initialize a new upload session
    pub async fn begin_upload(
        &self,
        repository_id: Uuid,
        upload_id: String,
    ) -> Result<UploadSession, AppError> {
        let session = UploadSession {
            id: Uuid::new_v4(),
            upload_id: upload_id.clone(),
            repository_id,
            status: UploadStatus::Pending,
            total_bytes: 0,
            uploaded_bytes: 0,
            sha256_context: Sha256::new(),
            created_at: Utc::now(),
            expires_at: Utc::now() + self.config.upload_timeout,
        };

        // Persist to database
        let upload_record = docker_upload::ActiveModel {
            id: ActiveValue::Set(session.id),
            repository_id: ActiveValue::Set(session.repository_id),
            upload_id: ActiveValue::Set(session.upload_id.clone()),
            status: ActiveValue::Set("pending".to_string()),
            total_bytes: ActiveValue::Set(0),
            uploaded_bytes: ActiveValue::Set(0),
            sha256_digest: ActiveValue::Set(None),
            created_at: ActiveValue::Set(session.created_at.into()),
            updated_at: ActiveValue::Set(session.created_at.into()),
            expires_at: ActiveValue::Set(Some(session.expires_at.into())),
        };

        let result = DockerUpload::insert(upload_record)
            .exec(&self.db.connection())
            .await?;

        tracing::info!(
            upload_id = %upload_id,
            session_id = %session.id,
            "Created new Docker upload session"
        );

        Ok(session)
    }

    /// Process an upload chunk
    pub async fn process_chunk(
        &self,
        session_id: Uuid,
        chunk_data: &[u8],
    ) -> Result<u64, AppError> {
        // Load session from database
        let upload = DockerUpload::find_by_id(session_id)
            .one(&self.db.connection())
            .await?
            .ok_or(AppError::NotFound("Upload session not found".to_string()))?;

        let mut uploaded_bytes = upload.uploaded_bytes as u64;

        // Update hash calculation (this would need to be persisted)
        // For now, we'll just update the byte count
        uploaded_bytes += chunk_data.len() as u64;

        // Update upload record
        let update_model = docker_upload::ActiveModel {
            id: ActiveValue::Set(session_id),
            uploaded_bytes: ActiveValue::Set(uploaded_bytes as i64),
            updated_at: ActiveValue::Set(Utc::now().into()),
            status: ActiveValue::Set("uploading".to_string()),
            ..Default::default()
        };

        DockerUpload::update(update_model)
            .exec(&self.db.connection())
            .await?;

        tracing::debug!(
            session_id = %session_id,
            chunk_size = %chunk_data.len(),
            total_uploaded = %uploaded_bytes,
            "Processed Docker upload chunk"
        );

        Ok(uploaded_bytes)
    }

    /// Complete an upload and return the final digest
    pub async fn complete_upload(
        &self,
        session_id: Uuid,
    ) -> Result<String, AppError> {
        let upload = DockerUpload::find_by_id(session_id)
            .one(&self.db.connection())
            .await?
            .ok_or(AppError::NotFound("Upload session not found".to_string()))?;

        // In a real implementation, we would calculate the final SHA256
        // For now, return a placeholder
        let digest = format!("sha256:{}", hex::encode("placeholder_digest"));

        // Mark as completed
        let update_model = docker_upload::ActiveModel {
            id: ActiveValue::Set(session_id),
            status: ActiveValue::Set("completed".to_string()),
            sha256_digest: ActiveValue::Set(Some(digest.clone())),
            updated_at: ActiveValue::Set(Utc::now().into()),
            ..Default::default()
        };

        DockerUpload::update(update_model)
            .exec(&self.db.connection())
            .await?;

        tracing::info!(
            session_id = %session_id,
            digest = %digest,
            "Completed Docker upload"
        );

        Ok(digest)
    }

    /// Cancel and clean up an upload
    pub async fn abandon_upload(&self, session_id: Uuid) -> Result<(), AppError> {
        let update_model = docker_upload::ActiveModel {
            id: ActiveValue::Set(session_id),
            status: ActiveValue::Set("abandoned".to_string()),
            updated_at: ActiveValue::Set(Utc::now().into()),
            ..Default::default()
        };

        DockerUpload::update(update_model)
            .exec(&self.db.connection())
            .await?;

        tracing::info!(session_id = %session_id, "Abandoned Docker upload");

        Ok(())
    }

    /// Clean up expired uploads
    pub async fn cleanup_expired_uploads(&self) -> Result<u64, AppError> {
        let expired_count = DockerUpload::find()
            .filter(docker_upload::Column::ExpiresAt.lt(Utc::now()))
            .filter(docker_upload::Column::Status.ne("completed"))
            .delete(&self.db.connection())
            .await?;

        if expired_count > 0 {
            tracing::info!(count = %expired_count, "Cleaned up expired Docker uploads");
        }

        Ok(expired_count)
    }

    /// Get upload status
    pub async fn get_upload_status(&self, upload_id: &str) -> Result<Option<UploadSession>, AppError> {
        let upload = DockerUpload::find()
            .filter(docker_upload::Column::UploadId.eq(upload_id))
            .one(&self.db.connection())
            .await?;

        match upload {
            Some(record) => {
                let session = UploadSession {
                    id: record.id,
                    upload_id: record.upload_id,
                    repository_id: record.repository_id,
                    status: self.status_from_string(&record.status),
                    total_bytes: record.total_bytes as u64,
                    uploaded_bytes: record.uploaded_bytes as u64,
                    sha256_context: Sha256::new(), // Would need to reconstruct from stored chunks
                    created_at: record.created_at.naive_utc().and_utc(),
                    expires_at: record.expires_at.unwrap_or_default().naive_utc().and_utc(),
                };
                Ok(Some(session))
            }
            None => Ok(None),
        }
    }

    fn status_from_string(&self, status: &str) -> UploadStatus {
        match status {
            "pending" => UploadStatus::Pending,
            "uploading" => UploadStatus::Uploading,
            "completed" => UploadStatus::Completed,
            "failed" => UploadStatus::Failed("Unknown error".to_string()),
            "abandoned" => UploadStatus::Abandoned,
            _ => UploadStatus::Failed(format!("Unknown status: {}", status)),
        }
    }

    /// Start background cleanup task
    pub async fn start_cleanup_task(self: Arc<Self>) {
        let interval = self.config.cleanup_interval;

        tokio::spawn(async move {
            let mut cleanup_timer = tokio::time::interval(interval);

            loop {
                cleanup_timer.tick().await;

                if let Err(e) = self.cleanup_expired_uploads().await {
                    tracing::error!(error = %e, "Failed to cleanup expired uploads");
                }
            }
        });
    }
}
```

#### 2.2 Dedicated Thread Pool Manager

```rust
// nitro_repo/src/repository/docker/thread_manager.rs
use std::sync::Arc;
use tokio::runtime::Builder;
use tokio::sync::{mpsc, Semaphore};
use uuid::Uuid;

use super::upload_service::DockerUploadService;
use crate::error::AppError;

pub struct DockerThreadManager {
    runtime: Arc<tokio::runtime::Runtime>,
    upload_service: Arc<DockerUploadService>,
    task_semaphore: Arc<Semaphore>,
    task_queue: mpsc::UnboundedSender<DockerTask>,
}

#[derive(Debug)]
pub enum DockerTask {
    ProcessChunk {
        session_id: Uuid,
        chunk_data: Vec<u8>,
        response_tx: mpsc::OneshotSender<Result<u64, AppError>>,
    },
    CompleteUpload {
        session_id: Uuid,
        response_tx: mpsc::OneshotSender<Result<String, AppError>>,
    },
    AbandonUpload {
        session_id: Uuid,
        response_tx: mpsc::OneshotSender<Result<(), AppError>>,
    },
}

impl DockerThreadManager {
    pub fn new(upload_service: Arc<DockerUploadService>) -> Result<Self, AppError> {
        // Create dedicated runtime for Docker operations
        let runtime = Builder::new_multi_thread()
            .worker_threads(4) // Configurable based on system
            .thread_name("docker-upload")
            .enable_all()
            .build()?;

        let (task_tx, mut task_rx) = mpsc::unbounded_channel::<DockerTask>();
        let semaphore = Arc::new(Semaphore::new(100)); // Limit concurrent uploads

        let manager = Self {
            runtime: Arc::new(runtime),
            upload_service,
            task_semaphore: semaphore,
            task_queue: task_tx,
        };

        // Start task processing loop
        manager.start_task_processor(task_rx, runtime.clone());

        Ok(manager)
    }

    // Reviewer note: document how this runtime is started in production (binary flags,
    // environment configuration, OTEL exporter setup) so ops knows the contract.
    // Also specify behaviour when the queue DB is unavailable—should the constructor
    // retry with backoff or fail fast?

    fn start_task_processor(
        &self,
        mut task_rx: mpsc::UnboundedReceiver<DockerTask>,
        runtime: Arc<tokio::runtime::Runtime>,
    ) {
        let upload_service = self.upload_service.clone();
        let semaphore = self.task_semaphore.clone();

        runtime.spawn(async move {
            while let Some(task) = task_rx.recv().await {
                let permit = semaphore.clone().acquire_owned().await;

                match task {
                    DockerTask::ProcessChunk {
                        session_id,
                        chunk_data,
                        response_tx,
                    } => {
                        let service = upload_service.clone();
                        tokio::spawn(async move {
                            let _permit = permit; // Hold permit for duration of operation
                            let result = service.process_chunk(session_id, &chunk_data).await;
                            let _ = response_tx.send(result);
                        });
                    }
                    DockerTask::CompleteUpload {
                        session_id,
                        response_tx,
                    } => {
                        let service = upload_service.clone();
                        tokio::spawn(async move {
                            let _permit = permit;
                            let result = service.complete_upload(session_id).await;
                            let _ = response_tx.send(result);
                        });
                    }
                    DockerTask::AbandonUpload {
                        session_id,
                        response_tx,
                    } => {
                        let service = upload_service.clone();
                        tokio::spawn(async move {
                            let _permit = permit;
                            let result = service.abandon_upload(session_id).await;
                            let _ = response_tx.send(result);
                        });
                    }
                }
            }
        });
    }

    pub async fn process_chunk(
        &self,
        session_id: Uuid,
        chunk_data: Vec<u8>,
    ) -> Result<u64, AppError> {
        let (response_tx, response_rx) = mpsc::oneshot_channel();

        self.task_queue.send(DockerTask::ProcessChunk {
            session_id,
            chunk_data,
            response_tx,
        })?;

        response_rx.await?
    }

    pub async fn complete_upload(&self, session_id: Uuid) -> Result<String, AppError> {
        let (response_tx, response_rx) = mpsc::oneshot_channel();

        self.task_queue.send(DockerTask::CompleteUpload {
            session_id,
            response_tx,
        })?;

        response_rx.await?
    }

    pub async fn abandon_upload(&self, session_id: Uuid) -> Result<(), AppError> {
        let (response_tx, response_rx) = mpsc::oneshot_channel();

        self.task_queue.send(DockerTask::AbandonUpload {
            session_id,
            response_tx,
        })?;

        response_rx.await?
    }
}
```

### Phase 3: Refactor Docker Handlers (Week 3-4)

#### 3.1 Updated Docker Handlers

```rust
// nitro_repo/src/repository/docker/handlers.rs
use std::sync::Arc;
use uuid::Uuid;
use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use tokio::sync::oneshot;
use tracing::{info, error, instrument};

use crate::app::NitroRepo;
use crate::error::AppError;
use crate::repository::docker::thread_manager::DockerThreadManager;

#[derive(Clone)]
pub struct DockerHandlerContext {
    pub thread_manager: Arc<DockerThreadManager>,
    pub nitro_repo: Arc<NitroRepo>,
}

#[instrument(skip(ctx, headers))]
pub async fn handle_docker_blob_upload_initiate(
    State(ctx): State<DockerHandlerContext>,
    Path((_account_name, repository_name)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    info!("Initiating Docker blob upload");

    // Find repository
    let repository = ctx.nitro_repo
        .find_repository_by_name(&repository_name)
        .await?
        .ok_or(AppError::NotFound("Repository not found".to_string()))?;

    // Generate upload ID
    let upload_id = Uuid::new_v4().to_string();

    // Begin upload session
    let session_id = ctx.thread_manager
        .upload_service
        .begin_upload(repository.id, upload_id.clone())
        .await?;

    info!(
        upload_id = %upload_id,
        session_id = %session_id.id,
        "Docker blob upload initiated"
    );

    Ok((
        StatusCode::ACCEPTED,
        [("Docker-Upload-UUID", upload_id.as_str())],
        "Upload initiated"
    ).into_response())
}

#[instrument(skip(ctx, body))]
pub async fn handle_docker_blob_upload_chunk(
    State(ctx): State<DockerHandlerContext>,
    Path((_account_name, repository_name, upload_id)): Path<(String, String, String)>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Result<Response, AppError> {
    let upload_session = ctx.thread_manager
        .upload_service
        .get_upload_status(&upload_id)
        .await?
        .ok_or(AppError::NotFound("Upload session not found".to_string()))?;

    // Process chunk in dedicated thread pool
    let uploaded_bytes = ctx.thread_manager
        .process_chunk(upload_session.id, body.to_vec())
        .await?;

    let range_start = uploaded_bytes.saturating_sub(body.len() as u64);
    let range_end = uploaded_bytes - 1;

    info!(
        upload_id = %upload_id,
        chunk_size = %body.len(),
        range_start = %range_start,
        range_end = %range_end,
        "Processed Docker upload chunk"
    );

    Ok((
        StatusCode::ACCEPTED,
        [
            ("Range", format!("{}-{}", range_start, range_end).as_str()),
            ("Docker-Upload-UUID", upload_id.as_str()),
        ],
        "Chunk received"
    ).into_response())
}

#[instrument(skip(ctx, headers))]
pub async fn handle_docker_blob_upload_complete(
    State(ctx): State<DockerHandlerContext>,
    Path((_account_name, repository_name, upload_id)): Path<(String, String, String)>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Result<Response, AppError> {
    info!("Completing Docker blob upload");

    let upload_session = ctx.thread_manager
        .upload_service
        .get_upload_status(&upload_id)
        .await?
        .ok_or(AppError::NotFound("Upload session not found".to_string()))?;

    // If there's a final chunk, process it first
    if !body.is_empty() {
        ctx.thread_manager
            .process_chunk(upload_session.id, body.to_vec())
            .await?;
    }

    // Complete the upload
    let digest = ctx.thread_manager
        .complete_upload(upload_session.id)
        .await?;

    info!(
        upload_id = %upload_id,
        digest = %digest,
        "Docker blob upload completed"
    );

    Ok((
        StatusCode::CREATED,
        [("Docker-Content-Digest", digest.as_str())],
        "Upload completed"
    ).into_response())
}

#[instrument(skip(ctx))]
pub async fn handle_docker_blob_upload_cancel(
    State(ctx): State<DockerHandlerContext>,
    Path((_account_name, repository_name, upload_id)): Path<(String, String, String)>,
) -> Result<Response, AppError> {
    info!(upload_id = %upload_id, "Cancelling Docker blob upload");

    let upload_session = ctx.thread_manager
        .upload_service
        .get_upload_status(&upload_id)
        .await?
        .ok_or(AppError::NotFound("Upload session not found".to_string()))?;

    ctx.thread_manager
        .abandon_upload(upload_session.id)
        .await?;

    info!(upload_id = %upload_id, "Docker blob upload cancelled");

    Ok(StatusCode::NO_CONTENT.into_response())
}
```

### Phase 4: Configuration and Integration (Week 4)

#### 4.1 Update Main Application

```rust
// nitro_repo/src/app/mod.rs
use std::sync::Arc;
use crate::repository::docker::{DockerUploadService, DockerThreadManager};

impl NitroRepo {
    pub async fn new(config: &Config) -> Result<Self, AppError> {
        // ... existing initialization ...

        // Remove old blob_upload_states field
        // OLD: blob_upload_states: parking_lot::Mutex<HashMap<(Uuid, String), Arc<parking_lot::Mutex<UploadState>>>>,

        // Initialize Docker upload service
        let docker_upload_service = Arc::new(DockerUploadService::new(config.database.clone()));

        // Start background cleanup
        docker_upload_service.clone().start_cleanup_task().await;

        // Create Docker thread manager
        let docker_thread_manager = Arc::new(
            DockerThreadManager::new(docker_upload_service.clone())?
        );

        Self {
            // ... existing fields ...
            docker_upload_service,
            docker_thread_manager,
            // blob_upload_states removed
        }
    }

    // Remove old blob upload state methods
    // OLD: begin_docker_blob_upload_state, update_docker_blob_upload_state, etc.
}
```

#### 4.2 Update Routing

```rust
// nitro_repo/src/repository/repo_http.rs
use crate::repository::docker::handlers::{DockerHandlerContext, handle_docker_blob_upload_initiate, /* ... */};

impl RepositoryHttpHandler {
    pub fn docker_routes() -> Router {
        let docker_context = DockerHandlerContext {
            thread_manager: /* get from NitroRepo */,
            nitro_repo: /* get from NitroRepo */,
        };

        Router::new()
            .route("/v2/:account_name/:repo_name/blobs/uploads/", post(handle_docker_blob_upload_initiate))
            .route("/v2/:account_name/:repo_name/blobs/uploads/:upload_id",
                   patch(handle_docker_blob_upload_chunk)
                   .put(handle_docker_blob_upload_complete)
                   .delete(handle_docker_blob_upload_cancel))
            .with_state(docker_context)
    }
}

> ⚙️ *Reviewer recommendation:* once the dual-path toggle lands, emit a span attribute such as `docker.pipeline = "new" | "legacy"` so Jaeger comparisons are trivial during rollout. Consider emitting structured events to the audit log when the handler offloads work to the new service.
```

### Phase 5: Testing and Migration (Week 5-6)

#### 5.1 Load Testing

```rust
// tests/docker_upload_load_test.rs
use std::sync::Arc;
use tokio::sync::Semaphore;
use uuid::Uuid;

#[tokio::test]
async fn test_concurrent_docker_uploads() {
    let docker_service = Arc::new(create_test_docker_service().await);
    let semaphore = Arc::new(Semaphore::new(100)); // 100 concurrent uploads

    let mut handles = vec![];

    for i in 0..100 {
        let service = docker_service.clone();
        let sem = semaphore.clone();

        let handle = tokio::spawn(async move {
            let _permit = sem.acquire().await;

            let upload_id = format!("test-upload-{}", i);
            let repo_id = Uuid::new_v4();

            // Begin upload
            let session = service.begin_upload(repo_id, upload_id.clone()).await.unwrap();

            // Simulate chunk uploads
            for chunk_num in 0..10 {
                let chunk_data = vec![0u8; 64 * 1024]; // 64KB chunks
                service.process_chunk(session.id, &chunk_data).await.unwrap();
            }

            // Complete upload
            let digest = service.complete_upload(session.id).await.unwrap();

            (upload_id, digest)
        });

        handles.push(handle);
    }

    // Wait for all uploads to complete
    let results = futures::future::join_all(handles).await;

    // Verify all succeeded
    assert_eq!(results.len(), 100);
    for result in results {
        let (upload_id, digest) = result.unwrap();
        assert!(!upload_id.is_empty());
        assert!(!digest.is_empty());
    }
}
```

> 🔄 *Additional testing*: run chaos scenarios—pause the worker runtime mid-upload, restart API pods, and simulate database outages—to verify queued sessions resume cleanly and that idempotent processing holds.

#### 5.2 Migration Strategy

```rust
// nitro_repo/src/migration/docker_migration.rs
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct DockerMigrationManager {
    nitro_repo: Arc<NitroRepo>,
    use_new_docker_system: Arc<RwLock<bool>>,
}

impl DockerMigrationManager {
    pub async fn migrate_incrementally(&self, percentage: f64) -> Result<u64, AppError> {
        let mut use_new = self.use_new_docker_system.write().await;

        // Gradually increase usage of new system
        let should_use_new = rand::random::<f64>() < percentage;
        *use_new = should_use_new;

        if should_use_new {
            tracing::info!("Using new Docker upload system");
        } else {
            tracing::info!("Using legacy Docker upload system");
        }

        Ok(percentage as u64)
    }

    pub async fn get_migration_status(&self) -> MigrationStatus {
        let use_new = *self.use_new_docker_system.read().await;

        MigrationStatus {
            using_new_system: use_new,
            migration_percentage: if use_new { 100.0 } else { 0.0 },
        }
    }
}

#[derive(Debug)]
pub struct MigrationStatus {
    pub using_new_system: bool,
    pub migration_percentage: f64,
}
```

## Benefits of New Architecture

### 1. **Reliability**
- Upload state persists across application restarts
- Automatic cleanup of expired uploads
- No memory leaks from orphaned state

### 2. **Scalability**
- Dedicated thread pool prevents main thread blocking
- Concurrent uploads limited by semaphore, not single mutex
- Database can handle more concurrent operations than in-memory HashMap

### 3. **Observability**
- Upload progress visible through database queries
- Metrics can be derived from database records
- Better error tracking and debugging capabilities

### 4. **Maintainability**
- Clear separation of concerns
- Docker operations isolated from main application
- Easier to test individual components

### 5. **Performance**
- True parallel processing of Docker operations
- Backpressure handling prevents resource exhaustion
- Optimized database operations with proper indexing

## Migration Timeline

| Week | Phase | Deliverables |
|------|-------|--------------|
| 1 | Database Schema | Migration scripts, database entities |
| 2 | Upload Service | Basic DockerUploadService implementation |
| 3 | Thread Manager | Dedicated thread pool and task queue |
| 4 | Handler Refactoring | Updated Docker handlers using new service |
| 5 | Integration | Main application integration and configuration |
| 6 | Testing | Load testing, migration strategy, documentation |

## Risk Mitigation

### Technical Risks
1. **Database Performance**: Mitigate with proper indexing and connection pooling
2. **Migration Complexity**: Use feature flags and gradual rollout
3. **Backwards Compatibility**: Maintain existing API contracts
4. **Data Loss**: Comprehensive testing and backup procedures

### Operational Risks
1. **Service Disruption**: Blue-green deployment strategy
2. **Performance Regression**: Comprehensive benchmarking
3. **Complexity**: Thorough documentation and team training

## Success Metrics

1. **Zero main thread blocking** during Docker operations
2. **Handle 100+ concurrent uploads** without performance degradation
3. **Upload state persistence** across application restarts
4. **<100ms API response times** for non-Docker operations during heavy Docker load
5. **Zero data loss** during migration and operation

## Conclusion

This refactoring will transform the Docker repository from a tightly-coupled, single-threaded component to a scalable, independent service. The new architecture will provide better reliability, performance, and maintainability while supporting true concurrent operations for multiple users.

The phased approach minimizes risk while delivering incremental value, with comprehensive testing and migration strategies ensuring a smooth transition.
