#![allow(dead_code)]
use std::{
    borrow::Cow, env, num::NonZeroUsize, ops::Deref, path::PathBuf, str::FromStr, sync::Arc,
};

use aws_config::BehaviorVersion;
use aws_config::sts::AssumeRoleProvider;
use aws_credential_types::{Credentials as AwsCredentials, provider::SharedCredentialsProvider};
use aws_sdk_s3::{
    Client as AwsS3Client,
    types::{CommonPrefix, Tag},
};
use aws_smithy_runtime_api::client::result::SdkError;
use aws_smithy_types::byte_stream::ByteStream;
use aws_types::{SdkConfig, region::Region};
use bytes::Bytes;
use chrono::Local;
use futures::future::BoxFuture;
use hex::encode;
use lru::LruCache;
use mime::Mime;
use nr_core::storage::{FileHashes, SerdeMime, StoragePath};
use regions::{CustomRegion, S3StorageRegion};
use sha2::{Digest, Sha256};
use tokio::{fs, sync::Mutex, task};
use url::Url;

pub mod regions;
use serde::{Deserialize, Serialize};
use tracing::{debug, error, info, instrument, warn};
use utoipa::ToSchema;
pub mod tags;
use uuid::Uuid;
#[derive(Debug, thiserror::Error)]
pub enum S3StorageError {
    #[error("No Region Provided")]
    NoRegionSpecified,
    #[error("AWS SDK error: {0}")]
    AwsSdkError(String),
    #[error("Bucket Does Not Exist {0}")]
    BucketDoesNotExist(String),
    #[error("IO Error: {0}")]
    IOError(#[from] std::io::Error),
    #[error("Blocking task join error: {0}")]
    BlockingJoin(#[from] tokio::task::JoinError),
    #[error(transparent)]
    InvalidConfigType(#[from] InvalidConfigType),

    #[error("Missing Tag: {0}")]
    MissingTag(Cow<'static, str>),

    #[error(transparent)]
    PathCollision(#[from] PathCollisionError),
}
impl S3StorageError {
    pub fn static_missing_tag(tag: &'static str) -> Self {
        S3StorageError::MissingTag(tag.into())
    }
    pub fn from_sdk_error(err: impl std::fmt::Display) -> Self {
        S3StorageError::AwsSdkError(err.to_string())
    }
}
use crate::{
    BorrowedStorageConfig, BorrowedStorageTypeConfig, DirectoryFileType, DynStorage, FileContent,
    FileContentBytes, FileFileType, FileType, InvalidConfigType, PathCollisionError,
    StaticStorageFactory, Storage, StorageConfig, StorageConfigInner, StorageError, StorageFactory,
    StorageFile, StorageFileMeta, StorageTypeConfig, StorageTypeConfigTrait, meta::RepositoryMeta,
    streaming::VecDirectoryListStream, utils::new_type_arc_type,
};
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema, Default)]
pub struct S3Credentials {
    pub access_key: Option<String>,
    /// AWS secret key.
    pub secret_key: Option<String>,
    /// Session token for temporary credentials.
    pub session_token: Option<String>,
    /// Optional IAM role ARN to assume after establishing base credentials.
    pub role_arn: Option<String>,
    /// Explicit role session name override.
    pub role_session_name: Option<String>,
    /// External ID passed to STS when assuming a role.
    pub external_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaticKeyCredentials {
    pub access_key: String,
    pub secret_key: String,
    pub session_token: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoleAssumption {
    pub role_arn: String,
    pub session_name: Option<String>,
    pub external_id: Option<String>,
}

impl std::fmt::Debug for S3Credentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("S3Credentials")
            .field("access_key", &self.access_key.as_ref().map(|_| "********")) // Mask access key
            .field("secret_key", &"********") // Always mask secret key
            .finish()
    }
}
impl S3Credentials {
    pub fn new_access_key(access_key: impl Into<String>, secret_key: impl Into<String>) -> Self {
        S3Credentials {
            access_key: Some(access_key.into()),
            secret_key: Some(secret_key.into()),
            session_token: None,
            role_arn: None,
            role_session_name: None,
            external_id: None,
        }
    }
    pub fn static_keys(&self) -> Option<StaticKeyCredentials> {
        let access_key = Self::clean_string(&self.access_key)?;
        let secret_key = Self::clean_string(&self.secret_key)?;
        Some(StaticKeyCredentials {
            access_key,
            secret_key,
            session_token: Self::clean_string(&self.session_token),
        })
    }

    pub fn role_to_assume(&self) -> Option<RoleAssumption> {
        let role_arn = Self::clean_string(&self.role_arn)?;
        Some(RoleAssumption {
            role_arn,
            session_name: Self::clean_string(&self.role_session_name),
            external_id: Self::clean_string(&self.external_id),
        })
    }

    fn clean_string(value: &Option<String>) -> Option<String> {
        value
            .as_ref()
            .map(|v| v.trim())
            .filter(|v| !v.is_empty())
            .map(|v| v.to_owned())
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
pub struct S3CacheConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    #[schema(value_type = String, format = "path")]
    pub path: Option<PathBuf>,
    #[serde(default = "default_cache_max_bytes")]
    pub max_bytes: u64,
    #[serde(default = "default_cache_entry_limit")]
    pub max_entries: usize,
}

impl Default for S3CacheConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            path: None,
            max_bytes: default_cache_max_bytes(),
            max_entries: default_cache_entry_limit(),
        }
    }
}

fn default_cache_max_bytes() -> u64 {
    512 * 1024 * 1024 // 512 MiB
}

fn default_cache_entry_limit() -> usize {
    2048
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
pub struct S3Config {
    pub bucket_name: String,
    pub region: Option<S3StorageRegion>,
    /// Custom region takes precedence over the region field
    #[serde(flatten)]
    pub custom_region: Option<CustomRegion>,
    pub credentials: S3Credentials,
    #[serde(default = "default_true")]
    #[schema(default = true)]
    pub path_style: bool,
    #[serde(default)]
    pub cache: S3CacheConfig,
}

impl std::fmt::Debug for S3Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("S3Config")
            .field("bucket_name", &self.bucket_name)
            .field("region", &self.region)
            .field("custom_region", &self.custom_region)
            .field("credentials", &"********") // Mask credentials entirely
            .field("path_style", &self.path_style)
            .field("cache_enabled", &self.cache.enabled)
            .field("cache_path", &self.cache.path)
            .field("cache_max_bytes", &self.cache.max_bytes)
            .finish()
    }
}

fn default_true() -> bool {
    true
}
impl S3Config {
    pub fn resolved_region(&self) -> Result<Region, S3StorageError> {
        if let Some(custom) = &self.custom_region {
            if self.region.is_some() {
                warn!("Region set with custom region, custom region will take precedence");
            }
            let name = custom
                .custom_region
                .clone()
                .unwrap_or_else(|| "custom-endpoint".into());
            return Ok(Region::new(name));
        }
        if let Some(region) = &self.region {
            return Ok((*region).into());
        }
        Err(S3StorageError::NoRegionSpecified)
    }

    pub fn custom_endpoint(&self) -> Option<&Url> {
        self.custom_region.as_ref().map(|c| &c.endpoint)
    }

    pub fn cache_enabled(&self) -> bool {
        self.cache.enabled && self.cache.max_bytes > 0
    }
}
#[derive(Debug, Clone)]
pub struct S3MetaTags {
    pub name: String,
    pub mime_type: Option<Mime>,
    pub is_directory: bool,
}

#[derive(Debug)]
pub(super) struct S3DiskCache {
    dir: PathBuf,
    max_bytes: u64,
    state: Mutex<CacheState>,
}

#[derive(Debug)]
struct CacheState {
    entries: LruCache<String, CacheEntry>,
    current_bytes: u64,
}

#[derive(Debug, Clone)]
struct CacheEntry {
    relative_path: PathBuf,
    size: u64,
    content_type: Option<String>,
}

#[derive(Debug, Clone)]
struct CachedObject {
    bytes: Bytes,
    content_type: Option<String>,
}

impl S3DiskCache {
    async fn new(config: &S3CacheConfig, storage_name: &str) -> Result<Self, S3StorageError> {
        if config.max_bytes == 0 {
            return Err(S3StorageError::AwsSdkError(
                "cache max_bytes must be greater than zero".into(),
            ));
        }
        let dir = config
            .path
            .clone()
            .unwrap_or_else(|| default_cache_dir(storage_name));
        fs::create_dir_all(&dir).await?;
        let capacity =
            NonZeroUsize::new(config.max_entries.max(1)).unwrap_or(NonZeroUsize::MIN);
        let state = CacheState {
            entries: LruCache::new(capacity),
            current_bytes: 0,
        };
        Ok(Self {
            dir,
            max_bytes: config.max_bytes,
            state: Mutex::new(state),
        })
    }

    fn hashed_filename(key: &str) -> PathBuf {
        let digest = Sha256::digest(key.as_bytes());
        let hex = encode(digest);
        let (prefix, rest) = hex.split_at(2);
        PathBuf::from(prefix).join(rest)
    }

    async fn get(&self, key: &str) -> Result<Option<CachedObject>, S3StorageError> {
        let (relative_path, content_type) = {
            let mut state = self.state.lock().await;
            match state.entries.get(key) {
                Some(entry) => (entry.relative_path.clone(), entry.content_type.clone()),
                None => return Ok(None),
            }
        };
        let path = self.dir.join(relative_path);
        match fs::read(&path).await {
            Ok(data) => Ok(Some(CachedObject {
                bytes: Bytes::from(data),
                content_type,
            })),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(err) => Err(err.into()),
        }
    }

    async fn put(
        &self,
        key: &str,
        data: Bytes,
        content_type: Option<&str>,
    ) -> Result<(), S3StorageError> {
        let relative = Self::hashed_filename(key);
        let path = self.dir.join(&relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await?;
        }
        fs::write(&path, data.as_ref()).await?;
        let mut removed = Vec::new();
        {
            let mut state = self.state.lock().await;
            if let Some(old) = state.entries.pop(key) {
                state.current_bytes = state.current_bytes.saturating_sub(old.size);
                removed.push(old.relative_path);
            }
            state.entries.put(
                key.to_string(),
                CacheEntry {
                    relative_path: relative,
                    size: data.len() as u64,
                    content_type: content_type.map(|c| c.to_string()),
                },
            );
            state.current_bytes = state.current_bytes.saturating_add(data.len() as u64);
            while state.current_bytes > self.max_bytes {
                if let Some((_, evicted)) = state.entries.pop_lru() {
                    state.current_bytes = state.current_bytes.saturating_sub(evicted.size);
                    removed.push(evicted.relative_path);
                } else {
                    break;
                }
            }
        }
        for rel in removed {
            let _ = fs::remove_file(self.dir.join(rel)).await;
        }
        Ok(())
    }

    async fn remove(&self, key: &str) -> Result<(), S3StorageError> {
        let removed = {
            let mut state = self.state.lock().await;
            state.entries.pop(key).map(|entry| {
                state.current_bytes = state.current_bytes.saturating_sub(entry.size);
                entry.relative_path
            })
        };
        if let Some(rel) = removed {
            let _ = fs::remove_file(self.dir.join(rel)).await;
        }
        Ok(())
    }
}

fn default_cache_dir(storage_name: &str) -> PathBuf {
    let sanitized = storage_name.replace('/', "_");
    env::temp_dir()
        .join("nitro_repo")
        .join("s3-cache")
        .join(sanitized)
}
#[derive(Debug)]
pub struct S3StorageInner {
    pub config: S3Config,
    pub storage_config: StorageConfigInner,
    pub client: AwsS3Client,
    cache: Option<Arc<S3DiskCache>>,
}
impl S3StorageInner {
    fn bucket(&self) -> &str {
        &self.config.bucket_name
    }
    fn aws_client(&self) -> &AwsS3Client {
        &self.client
    }
    pub async fn load_client(config: &S3Config) -> Result<AwsS3Client, S3StorageError> {
        let region = config.resolved_region()?;
        debug!(%region, bucket = %config.bucket_name, "Connecting to S3 bucket");

        let (base_config, static_provider) = build_base_config(config, &region).await?;

        let mut builder =
            aws_sdk_s3::config::Builder::from(&base_config).force_path_style(config.path_style);

        if let Some(endpoint) = config.custom_endpoint() {
            builder = builder.endpoint_url(endpoint.to_string());
        }

        if let Some(role) = config.credentials.role_to_assume() {
            let assume_provider = build_assume_role_provider(role, &base_config).await?;
            builder = builder.credentials_provider(SharedCredentialsProvider::new(assume_provider));
        } else if let Some(provider) = static_provider {
            builder = builder.credentials_provider(provider);
        }

        let client = AwsS3Client::from_conf(builder.build());
        match client
            .head_bucket()
            .bucket(&config.bucket_name)
            .send()
            .await
        {
            Ok(_) => Ok(client),
            Err(SdkError::ServiceError(err)) if err.err().is_not_found() => Err(
                S3StorageError::BucketDoesNotExist(config.bucket_name.clone()),
            ),
            Err(err) => Err(S3StorageError::from_sdk_error(err)),
        }
    }

    pub(super) async fn build_cache(
        config: &S3Config,
        storage: &StorageConfigInner,
    ) -> Result<Option<Arc<S3DiskCache>>, S3StorageError> {
        if !config.cache_enabled() {
            return Ok(None);
        }
        let cache = S3DiskCache::new(&config.cache, &storage.storage_name).await?;
        Ok(Some(Arc::new(cache)))
    }
    pub fn s3_path(&self, repository: &Uuid, path: &StoragePath) -> String {
        format!("{}/{}", repository, path)
    }

    fn cache_key(&self, repository: &Uuid, path: &StoragePath) -> String {
        self.s3_path(repository, path)
    }

    fn should_cache(&self, path: &StoragePath) -> bool {
        self.cache.is_some() && !path.is_directory()
    }

    async fn cache_get(
        &self,
        repository: &Uuid,
        location: &StoragePath,
    ) -> Result<Option<CachedObject>, S3StorageError> {
        if !self.should_cache(location) {
            return Ok(None);
        }
        let Some(cache) = &self.cache else {
            return Ok(None);
        };
        let key = self.cache_key(repository, location);
        cache.get(&key).await
    }

    async fn cache_put(
        &self,
        repository: &Uuid,
        location: &StoragePath,
        data: Bytes,
        content_type: Option<String>,
    ) -> Result<(), S3StorageError> {
        if !self.should_cache(location) {
            return Ok(());
        }
        if let Some(cache) = &self.cache {
            let key = self.cache_key(repository, location);
            cache.put(&key, data, content_type.as_deref()).await?;
        }
        Ok(())
    }

    async fn cache_remove(
        &self,
        repository: &Uuid,
        location: &StoragePath,
    ) -> Result<(), S3StorageError> {
        if !self.should_cache(location) {
            return Ok(());
        }
        if let Some(cache) = &self.cache {
            let key = self.cache_key(repository, location);
            cache.remove(&key).await?;
        }
        Ok(())
    }
    pub async fn get_path_for_creation(
        &self,
        repository: Uuid,
        location: &StoragePath,
    ) -> Result<String, S3StorageError> {
        let mut path = repository.to_string();
        let mut conflicting_path = StoragePath::default();
        for part in location.clone().into_iter() {
            path.push('/');
            path.push_str(part.as_ref());
            conflicting_path.push_mut(part.as_ref());
            if !self.is_directory(&path).await? {
                return Err(PathCollisionError {
                    path: location.clone(),
                    conflicts_with: conflicting_path,
                }
                .into());
            }
        }
        Ok(path)
    }
    #[instrument]
    async fn does_path_exist(&self, path: &str) -> Result<bool, S3StorageError> {
        let result = self
            .aws_client()
            .head_object()
            .bucket(self.bucket())
            .key(path)
            .send()
            .await;

        match result {
            Ok(_) => Ok(true),
            Err(SdkError::ServiceError(err)) if err.err().is_not_found() => Ok(false),
            Err(err) => Err(S3StorageError::from_sdk_error(err)),
        }
    }
    #[instrument]
    fn is_directory_from_result(
        &self,
        result: &aws_sdk_s3::operation::list_objects_v2::ListObjectsV2Output,
        path: &str,
    ) -> (bool, Option<String>) {
        let contents = result.contents();
        let prefixes = result.common_prefixes();
        let is_contents_empty = contents.is_empty();
        let has_prefixes = !prefixes.is_empty();

        if is_contents_empty && !has_prefixes {
            return (true, None);
        }
        if path.ends_with('/') && !is_contents_empty {
            return (true, None);
        }

        let path_with_slash = format!("{}/", path);
        if let Some(match_prefix) = prefixes
            .iter()
            .filter_map(CommonPrefix::prefix)
            .find(|prefix| *prefix == path_with_slash)
        {
            return (true, Some(match_prefix.to_string()));
        }

        (false, None)
    }

    #[instrument]
    async fn is_directory(&self, path: &str) -> Result<bool, S3StorageError> {
        let list = self
            .aws_client()
            .list_objects_v2()
            .bucket(self.bucket())
            .prefix(path.to_owned())
            .delimiter("/")
            .send()
            .await
            .map_err(S3StorageError::from_sdk_error)?;

        Ok(self.is_directory_from_result(&list, path).0)
    }

    /// Returns None if the path is not a directory
    #[instrument]
    async fn index_directory(&self, path: &str) -> Result<Option<StorageFile>, S3StorageError> {
        let path = if !path.ends_with("/") {
            format!("{}/", path)
        } else {
            path.to_owned()
        };

        let first = self
            .aws_client()
            .list_objects_v2()
            .bucket(self.bucket())
            .prefix(path.clone())
            .delimiter("/")
            .send()
            .await
            .map_err(S3StorageError::from_sdk_error)?;

        let mut files = Vec::new();
        for file in first.contents() {
            if let Some(key) = file.key()
                && let Some(meta) = self.get_meta(key).await?
            {
                files.push(meta);
            }
        }
        for sub_directory in first
            .common_prefixes()
            .iter()
            .filter_map(CommonPrefix::prefix)
        {
            if let Some(meta) = self.get_directory_meta(sub_directory).await? {
                files.push(meta);
            }
        }

        Ok(Some(StorageFile::Directory {
            meta: StorageFileMeta {
                name: path.to_owned(),
                file_type: DirectoryFileType {
                    file_count: files.len() as u64,
                },
                modified: Local::now().fixed_offset(),
                created: Local::now().fixed_offset(),
            },
            files,
        }))
    }
    async fn get_meta(
        &self,
        path: &str,
    ) -> Result<Option<StorageFileMeta<FileType>>, S3StorageError> {
        let file_file = FileType::File(FileFileType {
            file_size: 0,
            mime_type: None,
            file_hash: FileHashes::default(),
        });

        let meta = StorageFileMeta {
            name: path.to_owned(),
            file_type: file_file,
            modified: Local::now().fixed_offset(),
            created: Local::now().fixed_offset(),
        };

        Ok(Some(meta))
    }
    async fn get_directory_meta(
        &self,
        path: &str,
    ) -> Result<Option<StorageFileMeta<FileType>>, S3StorageError> {
        let file_file = FileType::Directory(DirectoryFileType { file_count: 0 });

        let meta = StorageFileMeta {
            name: path.to_owned(),
            file_type: file_file,
            modified: Local::now().fixed_offset(),
            created: Local::now().fixed_offset(),
        };

        Ok(Some(meta))
    }
    #[instrument]
    async fn get_object_tagging(&self, path: &str) -> Result<Option<Vec<Tag>>, S3StorageError> {
        let response = self
            .aws_client()
            .get_object_tagging()
            .bucket(self.bucket())
            .key(path)
            .send()
            .await;

        match response {
            Ok(output) => Ok(Some(output.tag_set().to_vec())),
            Err(SdkError::ServiceError(err))
                if err
                    .err()
                    .meta()
                    .code()
                    .is_some_and(|code| code == "NoSuchKey") =>
            {
                Ok(None)
            }
            Err(err) => Err(S3StorageError::from_sdk_error(err)),
        }
    }

    async fn get_meta_tags(&self, path: &str) -> Result<Option<S3MetaTags>, S3StorageError> {
        let Some(tags) = self.get_object_tagging(path).await? else {
            return Ok(None);
        };

        let name = tags
            .iter()
            .find(|tag| tag.key() == tags::NAME)
            .map(|tag| tag.value().to_string())
            .ok_or_else(|| S3StorageError::static_missing_tag(tags::NAME))?;

        let mime_type = tags
            .iter()
            .find(|tag| tag.key() == tags::MIME_TYPE)
            .map(|tag| Mime::from_str(tag.value()))
            .transpose();
        let mime_type = match mime_type {
            Ok(ok) => ok,
            Err(e) => {
                error!(?e, ?path, "Failed to parse mime type");
                None
            }
        };

        Ok(Some(S3MetaTags {
            name,
            mime_type,
            is_directory: false,
        }))
    }
}

async fn build_base_config(
    config: &S3Config,
    region: &Region,
) -> Result<(SdkConfig, Option<SharedCredentialsProvider>), S3StorageError> {
    let mut loader = aws_config::defaults(BehaviorVersion::latest()).region(region.clone());
    let mut static_provider = None;
    if let Some(keys) = config.credentials.static_keys() {
        let credentials = AwsCredentials::new(
            keys.access_key,
            keys.secret_key,
            keys.session_token,
            None,
            "nitro-repo-static",
        );
        let provider = SharedCredentialsProvider::new(credentials);
        loader = loader.credentials_provider(provider.clone());
        static_provider = Some(provider);
    }

    let shared_config = loader.load().await;
    Ok((shared_config, static_provider))
}

async fn build_assume_role_provider(
    role: RoleAssumption,
    base_config: &SdkConfig,
) -> Result<AssumeRoleProvider, S3StorageError> {
    let session_name = role.session_name.unwrap_or_else(default_session_name);
    let mut builder = AssumeRoleProvider::builder(role.role_arn).session_name(session_name);
    if let Some(external_id) = role.external_id {
        builder = builder.external_id(external_id);
    }
    let provider = builder.configure(base_config).build().await;
    Ok(provider)
}

fn default_session_name() -> String {
    format!("nitro-repo-{}", Uuid::new_v4().simple())
}

fn bytes_to_stream(bytes: FileContentBytes) -> (ByteStream, usize) {
    match bytes {
        FileContentBytes::Content(content) => {
            let len = content.len();
            (ByteStream::from(content), len)
        }
        FileContentBytes::Bytes(bytes) => {
            let len = bytes.len();
            (ByteStream::from(bytes.to_vec()), len)
        }
    }
}

async fn file_into_bytes(file: FileContent) -> Result<FileContentBytes, S3StorageError> {
    let bytes = task::spawn_blocking(move || FileContentBytes::try_from(file)).await??;
    Ok(bytes)
}

async fn collect_body(stream: ByteStream) -> Result<Bytes, S3StorageError> {
    let aggregated = stream
        .collect()
        .await
        .map_err(|err| S3StorageError::AwsSdkError(err.to_string()))?;
    Ok(aggregated.into_bytes())
}
#[derive(Debug, Clone)]
pub struct S3Storage(Arc<S3StorageInner>);
new_type_arc_type!(S3Storage(S3StorageInner));
impl Storage for S3Storage {
    type Error = S3StorageError;
    type DirectoryStream = VecDirectoryListStream;
    fn storage_type_name(&self) -> &'static str {
        "s3"
    }
    #[instrument(name = "Storage::unload", fields(storage_type = "s3"))]
    async fn unload(&self) -> Result<(), S3StorageError> {
        info!("Unloading S3 Storage");
        Ok(())
    }
    #[instrument(fields(storage_type = "s3"))]
    fn storage_config(&self) -> BorrowedStorageConfig<'_> {
        BorrowedStorageConfig {
            storage_config: &self.storage_config,
            config: BorrowedStorageTypeConfig::S3(&self.config),
        }
    }
    #[instrument(name = "Storage::save_file", fields(storage_type = "s3"))]
    async fn save_file(
        &self,
        repository: uuid::Uuid,
        file: FileContent,
        location: &StoragePath,
    ) -> Result<(usize, bool), S3StorageError> {
        let path = self.get_path_for_creation(repository, location).await?;
        let already_exists = self.does_path_exist(&path).await?;
        if already_exists {
            debug!("File already exists, overwriting");
        }
        let content_type = if location.is_directory() {
            "application/x-directory"
        } else {
            "application/octet-stream"
        };
        let file_as_bytes = file_into_bytes(file).await?;
        let cache_buffer = file_as_bytes.clone_into_bytes();
        let (body, size) = bytes_to_stream(file_as_bytes);
        self.aws_client()
            .put_object()
            .bucket(self.bucket())
            .key(&path)
            .body(body)
            .content_type(content_type)
            .send()
            .await
            .map_err(S3StorageError::from_sdk_error)?;
        debug!(path = %path, "File saved to S3");
        self.cache_put(
            &repository,
            location,
            cache_buffer,
            Some(content_type.to_string()),
        )
        .await?;
        Ok((size, !already_exists))
    }
    #[instrument(name = "Storage::append_file", fields(storage_type = "s3"))]
    async fn append_file(
        &self,
        repository: uuid::Uuid,
        file: FileContent,
        location: &StoragePath,
    ) -> Result<usize, S3StorageError> {
        // S3 doesn't support native append operations
        // We need to read, append, and write back
        // This is still O(n) for S3 since network I/O dominates
        let path = self.get_path_for_creation(repository, location).await?;

        let mut combined_buffer = if self.does_path_exist(&path).await? {
            let response = self
                .aws_client()
                .get_object()
                .bucket(self.bucket())
                .key(&path)
                .send()
                .await
                .map_err(S3StorageError::from_sdk_error)?;
            collect_body(response.body).await?.to_vec()
        } else {
            Vec::new()
        };

        let appended = file_into_bytes(file).await?;
        combined_buffer.extend_from_slice(appended.as_ref());

        let combined_bytes = Bytes::from(combined_buffer);

        let content_type = if location.is_directory() {
            "application/x-directory"
        } else {
            "application/octet-stream"
        };
        let size = combined_bytes.len();
        self.aws_client()
            .put_object()
            .bucket(self.bucket())
            .key(&path)
            .content_type(content_type)
            .body(ByteStream::from(combined_bytes.clone()))
            .send()
            .await
            .map_err(S3StorageError::from_sdk_error)?;
        self.cache_put(
            &repository,
            location,
            combined_bytes,
            Some(content_type.to_string()),
        )
        .await?;
        Ok(size)
    }
    #[instrument(name = "Storage::put_repository_meta", fields(storage_type = "s3"))]
    async fn put_repository_meta(
        &self,
        repository: uuid::Uuid,
        location: &StoragePath,
        value: RepositoryMeta,
    ) -> Result<(), S3StorageError> {
        todo!()
    }
    #[instrument(name = "Storage::get_repository_meta", fields(storage_type = "s3"))]
    async fn get_repository_meta(
        &self,
        repository: uuid::Uuid,
        location: &StoragePath,
    ) -> Result<Option<RepositoryMeta>, S3StorageError> {
        todo!()
    }
    #[instrument(name = "Storage::delete_file", fields(storage_type = "s3"))]
    async fn delete_file(
        &self,
        repository: uuid::Uuid,
        location: &StoragePath,
    ) -> Result<bool, S3StorageError> {
        let path = self.s3_path(&repository, location);
        let exists = self.does_path_exist(&path).await?;
        if !exists {
            return Ok(false);
        }
        self.aws_client()
            .delete_object()
            .bucket(self.bucket())
            .key(&path)
            .send()
            .await
            .map_err(S3StorageError::from_sdk_error)?;
        self.cache_remove(&repository, location).await?;
        Ok(true)
    }
    #[instrument(name = "Storage::move_file", fields(storage_type = "s3"))]
    async fn move_file(
        &self,
        repository: uuid::Uuid,
        from: &StoragePath,
        to: &StoragePath,
    ) -> Result<bool, S3StorageError> {
        let from_path = self.s3_path(&repository, from);
        let to_path = self.s3_path(&repository, to);

        // Check if source exists
        if !self.does_path_exist(&from_path).await? {
            return Ok(false);
        }

        // For S3, we need to copy and then delete since there's no native rename
        // Read the object
        let response = self
            .aws_client()
            .get_object()
            .bucket(self.bucket())
            .key(&from_path)
            .send()
            .await
            .map_err(S3StorageError::from_sdk_error)?;
        let bytes = collect_body(response.body).await?;

        // Get content type from original object metadata (if available)
        let content_type = if to.is_directory() {
            "application/x-directory"
        } else {
            "application/octet-stream"
        };

        // Write to new location
        self.aws_client()
            .put_object()
            .bucket(self.bucket())
            .key(&to_path)
            .content_type(content_type)
            .body(ByteStream::from(bytes.clone()))
            .send()
            .await
            .map_err(S3StorageError::from_sdk_error)?;

        // Delete original
        self.aws_client()
            .delete_object()
            .bucket(self.bucket())
            .key(&from_path)
            .send()
            .await
            .map_err(S3StorageError::from_sdk_error)?;

        self.cache_remove(&repository, from).await?;
        self.cache_put(&repository, to, bytes, Some(content_type.to_string()))
            .await?;

        Ok(true)
    }
    #[instrument(name = "Storage::get_file_information", fields(storage_type = "s3"))]
    async fn get_file_information(
        &self,
        repository: uuid::Uuid,
        location: &StoragePath,
    ) -> Result<Option<crate::StorageFileMeta<FileType>>, S3StorageError> {
        todo!()
    }
    #[instrument(name = "Storage::open_file", fields(storage_type = "s3"))]
    async fn open_file(
        &self,
        repository: uuid::Uuid,
        location: &StoragePath,
    ) -> Result<Option<crate::StorageFile>, S3StorageError> {
        if let Some(cached) = self.cache_get(&repository, location).await? {
            let mime_type = cached
                .content_type
                .as_deref()
                .and_then(|ct| Mime::from_str(ct).ok())
                .map(SerdeMime);
            let size = cached.bytes.len() as u64;
            let meta = StorageFileMeta::<FileFileType> {
                name: location.to_string(),
                file_type: FileFileType {
                    file_size: size,
                    mime_type,
                    file_hash: FileHashes::default(),
                },
                modified: Local::now().fixed_offset(),
                created: Local::now().fixed_offset(),
            };
            let result = StorageFile::File {
                meta,
                content: crate::StorageFileReader::Bytes(FileContentBytes::Bytes(cached.bytes)),
            };
            return Ok(Some(result));
        }
        let path = self.s3_path(&repository, location);
        let response = match self
            .aws_client()
            .get_object()
            .bucket(self.bucket())
            .key(&path)
            .send()
            .await
        {
            Ok(resp) => resp,
            Err(SdkError::ServiceError(err)) if err.err().is_no_such_key() => return Ok(None),
            Err(err) => return Err(S3StorageError::from_sdk_error(err)),
        };

        let response_content_type = response.content_type().map(|ct| ct.to_string());
        if response_content_type
            .as_deref()
            .map(|ct| ct == "application/x-directory")
            .unwrap_or(false)
        {
            return self.index_directory(&path).await;
        }
        let response_length = response
            .content_length()
            .unwrap_or_default()
            .try_into()
            .unwrap_or_default();

        let meta = StorageFileMeta::<FileFileType> {
            name: location.to_string(),
            file_type: FileFileType {
                file_size: response_length,
                mime_type: response_content_type
                    .as_deref()
                    .map(Mime::from_str)
                    .transpose()
                    .unwrap_or_default()
                    .map(SerdeMime),
                file_hash: FileHashes::default(),
            },
            modified: Local::now().fixed_offset(),
            created: Local::now().fixed_offset(),
        };
        let body = collect_body(response.body).await?;
        self.cache_put(
            &repository,
            location,
            body.clone(),
            response_content_type.clone(),
        )
        .await?;
        let result = StorageFile::File {
            meta,
            content: crate::StorageFileReader::Bytes(FileContentBytes::Bytes(body)),
        };

        Ok(Some(result))
    }
    #[instrument(name = "Storage::validate_config_change", fields(storage_type = "s3"))]
    async fn validate_config_change(
        &self,
        config: StorageTypeConfig,
    ) -> Result<(), S3StorageError> {
        let s3_config = S3Config::from_type_config(config)?;
        S3StorageInner::load_client(&s3_config).await?;
        S3StorageInner::build_cache(&s3_config, &self.storage_config).await?;
        info!(bucket = %s3_config.bucket_name, "Successfully connected to S3 bucket");
        Ok(())
    }
    #[instrument(name = "Storage::file_exists", fields(storage_type = "s3"))]
    async fn file_exists(
        &self,
        repository: uuid::Uuid,
        location: &StoragePath,
    ) -> Result<bool, S3StorageError> {
        let path = self.s3_path(&repository, location);
        self.does_path_exist(&path).await
    }

    async fn stream_directory(
        &self,
        _repository: Uuid,
        _location: &StoragePath,
    ) -> Result<Option<Self::DirectoryStream>, Self::Error> {
        // Streaming directories is not supported for S3; callers can fall back to listings.
        Ok(None)
    }
}
#[derive(Debug, Default)]
pub struct S3StorageFactory;
impl StaticStorageFactory for S3StorageFactory {
    type StorageType = S3Storage;
    type ConfigType = S3Config;
    type Error = S3StorageError;

    fn storage_type_name() -> &'static str {
        "s3"
    }

    async fn test_storage_config(config: StorageTypeConfig) -> Result<(), S3StorageError> {
        let s3_config = S3Config::from_type_config(config)?;
        S3StorageInner::load_client(&s3_config).await?;
        info!(bucket = %s3_config.bucket_name, "Successfully connected to S3 bucket");
        Ok(())
    }

    async fn create_storage(
        inner: StorageConfigInner,
        type_config: Self::ConfigType,
    ) -> Result<Self::StorageType, S3StorageError> {
        let client = S3StorageInner::load_client(&type_config).await?;
        let cache = S3StorageInner::build_cache(&type_config, &inner).await?;
        let inner = S3StorageInner {
            config: type_config,
            storage_config: inner,
            client,
            cache,
        };
        let storage = S3Storage::from(inner);
        Ok(storage)
    }
}
impl StorageFactory for S3StorageFactory {
    fn storage_name(&self) -> &'static str {
        "s3"
    }

    fn test_storage_config(
        &self,
        config: StorageTypeConfig,
    ) -> BoxFuture<'static, Result<(), StorageError>> {
        Box::pin(async move {
            let s3_config = S3Config::from_type_config(config)?;

            S3StorageInner::load_client(&s3_config).await?;
            info!(bucket = %s3_config.bucket_name, "Successfully connected to S3 bucket");

            Ok(())
        })
    }

    fn create_storage(
        &self,
        config: StorageConfig,
    ) -> BoxFuture<'static, Result<DynStorage, StorageError>> {
        Box::pin(async move {
            let s3_config = S3Config::from_type_config(config.type_config)?;
            let storage_config = config.storage_config;
            let client = S3StorageInner::load_client(&s3_config).await?;
            let cache = S3StorageInner::build_cache(&s3_config, &storage_config).await?;
            let inner = S3StorageInner {
                config: s3_config,
                storage_config,
                client,
                cache,
            };
            let storage = S3Storage::from(inner);
            Ok(DynStorage::S3(storage))
        })
    }
}
#[cfg(test)]
mod tests {
    use tracing::warn;

    use super::{CustomRegion, S3CacheConfig, S3Config, S3Credentials, S3StorageRegion};
    use crate::{StaticStorageFactory, s3::S3StorageFactory, testing::storage::TestingStorage};

    #[tokio::test]
    pub async fn generic_test() -> anyhow::Result<()> {
        let Some(config) = crate::testing::start_storage_test("s3")? else {
            warn!("S3 Storage Test Skipped");
            return Ok(());
        };
        let local_storage =
            <S3StorageFactory as StaticStorageFactory>::create_storage_from_config(config).await?;
        let testing_storage = TestingStorage::new(local_storage);
        crate::testing::tests::full_test(testing_storage).await?;

        Ok(())
    }

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
}
