use std::{fmt::Debug, path::PathBuf, sync::Arc};

use ahash::{HashMap, HashMapExt};
use anyhow::{Context, anyhow};
use authentication::{
    oauth::{OAuth2Rbac, OAuth2Service},
    session::{SessionManager, SessionManagerConfig},
};
use axum::extract::State;
use config::{Mode, OAuth2Settings, PasswordRules, SecuritySettings, SiteSetting, SsoSettings};
use derive_more::{AsRef, derive::Deref};
use email::EmailSetting;
use email_service::{EmailAccess, EmailService};
use http::{HeaderName, Uri};
pub mod frontend;
pub mod resources;
use nr_core::{
    database::{
        DatabaseConfig,
        entities::{
            repository::{DBRepository, DBRepositoryConfig},
            settings::ApplicationSettings,
            storage::{DBStorage, StorageDBType},
            user::user_utils,
        },
    },
    repository::config::{
        RepositoryConfigType, project::ProjectConfigType, repository_page::RepositoryPageType,
    },
};
use nr_core::{storage::FileHashes, utils::base64_utils};
use nr_storage::{DynStorage, STORAGE_FACTORIES, Storage, StorageConfig, StorageFactory};
use opentelemetry::{
    InstrumentationScope, global,
    metrics::{Histogram, Meter, UpDownCounter},
};
use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};
pub mod authentication;
pub mod config;
pub mod email;
pub mod email_service;
pub mod request_logging;
use current_semver::current_semver;
use digest::Digest;
use md5::Md5;
use sha1::Sha1;
use sha2::Sha256;
use sha3::Sha3_256;
use sqlx::PgPool;
use tokio::task::JoinHandle;
use tracing::{debug, info, instrument, warn};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;
pub mod open_api;
use crate::{
    repository::{
        DynRepository, RepositoryAuthConfig, RepositoryAuthConfigType, RepositoryType,
        StagingConfig,
        docker::{DockerPushRulesConfigType, DockerRegistryConfigType, DockerRepositoryType},
        go::{GoRepositoryConfigType, GoRepositoryType},
        helm::{HelmRepositoryConfigType, HelmRepositoryType},
        maven::{MavenPushRulesConfigType, MavenRepositoryConfigType, MavenRepositoryType},
        npm::{NPMRegistryConfigType, NpmRegistryType},
        php::{PhpRepositoryConfigType, PhpRepositoryType},
        python::{PythonRepositoryConfigType, PythonRepositoryType},
        repo_tracing::RepositoryMetricsMeter,
    },
    utils::ip_addr::HasForwardedHeader,
};
pub mod api;
pub mod badge;
pub mod responses;
pub mod web;
#[derive(Debug, Serialize, Clone, ToSchema)]
pub struct Instance {
    pub app_url: String,
    pub name: String,
    pub description: String,
    pub is_https: bool,
    pub is_installed: bool,
    #[schema(value_type=String)]
    pub version: semver::Version,
    pub mode: Mode,
    pub password_rules: Option<PasswordRules>,
    pub sso: Option<InstanceSsoSettings>,
    pub oauth2: Option<InstanceOAuth2Settings>,
}

#[derive(Debug, Serialize, Clone, ToSchema)]
pub struct InstanceSsoSettings {
    pub login_path: String,
    pub login_button_text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_login_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_redirect_param: Option<String>,
    pub auto_create_users: bool,
}

impl From<&SsoSettings> for InstanceSsoSettings {
    fn from(settings: &SsoSettings) -> Self {
        Self {
            login_path: settings.login_path.clone(),
            login_button_text: settings.login_button_text.clone(),
            provider_login_url: settings.provider_login_url.clone(),
            provider_redirect_param: settings.provider_redirect_param.clone(),
            auto_create_users: settings.auto_create_users,
        }
    }
}

#[derive(Debug, Serialize, Clone, ToSchema)]
pub struct InstanceOAuth2Settings {
    pub login_path: String,
    pub callback_path: String,
    pub providers: Vec<InstanceOAuth2Provider>,
    pub auto_create_users: bool,
    pub group_role_mappings: Vec<config::OAuth2GroupRoleMapping>,
}

#[derive(Debug, Serialize, Clone, ToSchema)]
pub struct InstanceOAuth2Provider {
    pub provider: String,
    pub redirect_path: Option<String>,
}

impl From<&OAuth2Settings> for InstanceOAuth2Settings {
    fn from(settings: &OAuth2Settings) -> Self {
        let mut providers = Vec::new();
        if settings.google.is_some() {
            providers.push(InstanceOAuth2Provider {
                provider: "google".to_string(),
                redirect_path: settings
                    .google
                    .as_ref()
                    .and_then(|cfg| cfg.redirect_path.clone()),
            });
        }
        if settings.microsoft.is_some() {
            providers.push(InstanceOAuth2Provider {
                provider: "microsoft".to_string(),
                redirect_path: settings
                    .microsoft
                    .as_ref()
                    .and_then(|cfg| cfg.redirect_path.clone()),
            });
        }
        Self {
            login_path: settings.login_path.clone(),
            callback_path: settings.callback_path.clone(),
            providers,
            auto_create_users: settings.auto_create_users,
            group_role_mappings: settings.group_role_mappings.clone(),
        }
    }
}

#[derive(Debug)]
struct UploadState {
    md5: Option<Md5>,
    sha1: Option<Sha1>,
    sha2: Sha256,
    sha3: Option<Sha3_256>,
    length: u64,
    sha256_only: bool,
}

#[derive(Clone)]
pub struct BlobUploadStateHandle(Arc<parking_lot::Mutex<UploadState>>);

#[derive(Debug, Clone)]
pub struct FinalizedUpload {
    pub digest: String,
    pub hashes: FileHashes,
    pub length: u64,
}

impl UploadState {
    /// Create upload state with all hash algorithms (for general use)
    fn new() -> Self {
        Self {
            md5: Some(Md5::new()),
            sha1: Some(Sha1::new()),
            sha2: Sha256::new(),
            sha3: Some(Sha3_256::new()),
            length: 0,
            sha256_only: false,
        }
    }

    /// Create upload state with only SHA256 (for Docker)
    fn new_sha256_only() -> Self {
        Self {
            md5: None,
            sha1: None,
            sha2: Sha256::new(),
            sha3: None,
            length: 0,
            sha256_only: true,
        }
    }

    fn update(&mut self, chunk: &[u8]) {
        if chunk.is_empty() {
            return;
        }

        if let Some(md5) = &mut self.md5 {
            md5.update(chunk);
        }
        if let Some(sha1) = &mut self.sha1 {
            sha1.update(chunk);
        }
        self.sha2.update(chunk);
        if let Some(sha3) = &mut self.sha3 {
            sha3.update(chunk);
        }
        self.length += chunk.len() as u64;
    }

    fn finalize(self) -> FinalizedUpload {
        let sha2_bytes = self.sha2.finalize();
        let digest = format!("sha256:{:x}", sha2_bytes);
        let hashes = FileHashes {
            md5: self.md5.map(|h| base64_utils::encode(h.finalize())),
            sha1: self.sha1.map(|h| base64_utils::encode(h.finalize())),
            sha2_256: Some(base64_utils::encode(&sha2_bytes)),
            sha3_256: self.sha3.map(|h| base64_utils::encode(h.finalize())),
        };

        FinalizedUpload {
            digest,
            hashes,
            length: self.length,
        }
    }

    fn take(&mut self) -> Self {
        let sha_only = self.sha256_only;
        let mut replacement = if sha_only {
            UploadState::new_sha256_only()
        } else {
            UploadState::new()
        };
        std::mem::swap(self, &mut replacement);
        replacement
    }
}

impl BlobUploadStateHandle {
    fn new(state: UploadState) -> Self {
        Self(Arc::new(parking_lot::Mutex::new(state)))
    }

    fn lock(&self) -> parking_lot::MutexGuard<'_, UploadState> {
        self.0.lock()
    }

    fn try_into_state(self) -> Result<UploadState, Self> {
        match Arc::try_unwrap(self.0) {
            Ok(inner) => Ok(inner.into_inner()),
            Err(arc) => Err(Self(arc)),
        }
    }
}

#[cfg(test)]
mod upload_state_tests {
    use super::*;

    #[test]
    fn docker_upload_state_only_emits_sha256() {
        let mut state = UploadState::new_sha256_only();
        state.update(b"hello world");

        let finalized = state.finalize();
        assert!(finalized.hashes.md5.is_none());
        assert!(finalized.hashes.sha1.is_none());
        assert!(finalized.hashes.sha3_256.is_none());
        assert!(finalized.hashes.sha2_256.is_some());
        assert_eq!(
            finalized.digest,
            "sha256:b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        );
    }
}
#[derive(Debug, Clone, Hash, PartialEq, Eq, IntoParams, Deserialize)]
#[into_params(parameter_in = Path)]
pub struct RepositoryStorageName {
    /// The name of the storage
    pub storage_name: String,
    /// The name of the repository
    pub repository_name: String,
}

impl RepositoryStorageName {
    pub async fn query_db(&self, database: &PgPool) -> Result<Option<Uuid>, sqlx::Error> {
        let query: Option<Uuid> = sqlx::query_scalar(
            r#"SELECT repositories.id FROM repositories INNER JOIN storages
                    ON storages.id = repositories.storage_id AND storages.name = $1
                    WHERE repositories.name = $2"#,
        )
        .bind(&self.storage_name)
        .bind(&self.repository_name)
        .fetch_optional(database)
        .await?;
        Ok(query)
    }
}
impl From<(&str, &str)> for RepositoryStorageName {
    fn from((storage_name, repository_name): (&str, &str)) -> Self {
        Self {
            storage_name: storage_name.to_lowercase(),
            repository_name: repository_name.to_lowercase(),
        }
    }
}
impl From<(String, String)> for RepositoryStorageName {
    fn from((storage_name, repository_name): (String, String)) -> Self {
        Self {
            storage_name: storage_name.to_lowercase(),
            repository_name: repository_name.to_lowercase(),
        }
    }
}
#[derive(Debug, Default)]
pub struct InternalServices {
    pub session_cleaner: Option<JoinHandle<()>>,
    pub email: Option<EmailService>,
}
pub struct NitroRepoInner {
    pub instance: Mutex<Instance>,
    pub storages: RwLock<HashMap<Uuid, DynStorage>>,
    pub repositories: RwLock<HashMap<Uuid, DynRepository>>,
    pub name_lookup_table: Mutex<HashMap<RepositoryStorageName, Uuid>>,
    pub general_security_settings: RwLock<SecuritySettings>,
    pub oauth2_service: RwLock<Option<Arc<OAuth2Service>>>,
    pub oauth2_rbac: RwLock<Option<Arc<OAuth2Rbac>>>,
    #[cfg(feature = "frontend")]
    pub frontend: frontend::HostedFrontend,
    pub staging_config: StagingConfig,
    services: Mutex<InternalServices>,
    blob_upload_states: parking_lot::Mutex<HashMap<(Uuid, String), BlobUploadStateHandle>>,
    pub suggested_local_storage_path: PathBuf,
}
macro_rules! take_service {
    ($(
        $fn_name:ident => $field:ident -> $type:ty
    ),*) => {
        $(
            pub fn $fn_name(&self) -> Option<$type> {
                let mut services = self.services.lock();
                services.$field.take()
            }
        )*
    }
}
impl NitroRepoInner {
    take_service! {
        take_session_cleaner => session_cleaner -> JoinHandle<()>,
        take_email => email -> EmailService
    }
    /// Notifies services that have waiters that the application is shutting down
    pub fn notify_shutdown(&self) {
        let services = self.services.lock();
        if let Some(email) = services.email.as_ref() {
            email.notify_shutdown.notify_waiters();
        }
    }
}
impl Debug for NitroRepo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // TODO: Improve the Debug implementation
        f.debug_struct("NitroRepo")
            .field("instance", &self.inner.instance.lock())
            .field("active_storages", &self.inner.storages.read().len())
            .field("active_repositories", &self.inner.repositories.read().len())
            .field("database", &self.database)
            .finish()
    }
}
/// Request Metrics based on [HTTP Server Semantic Conventions](https://opentelemetry.io/docs/specs/semconv/http/http-metrics/#http-server)
#[derive(Debug, Clone)]
pub struct AppMetrics {
    pub meter: Meter,
    pub request_size_bytes: Histogram<u64>,
    pub response_size_bytes: Histogram<u64>,
    pub request_duration: Histogram<f64>,
    pub active_sessions: UpDownCounter<i64>,
}
impl Default for AppMetrics {
    fn default() -> Self {
        let scope = InstrumentationScope::builder("nitro-repo")
        .with_schema_url("https://github.com/open-telemetry/semantic-conventions/blob/v1.29.0/docs/http/http-metrics.md")
        .with_version(env!("CARGO_PKG_VERSION")).build();
        let meter = global::meter_with_scope(scope);

        Self {
            active_sessions: meter
                .i64_up_down_counter("http.server.active_sessions")
                .with_description("The number of active sessions")
                .build(),
            request_size_bytes: meter
                .u64_histogram("http.server.request.body.size")
                .with_unit("By")
                .build(),
            response_size_bytes: meter
                .u64_histogram("http.server.response.body.size")
                .with_unit("By")
                .build(),
            request_duration: meter
                .f64_histogram("http.server.request.duration")
                .with_boundaries(vec![
                    0.005, 0.01, 0.025, 0.05, 0.075, 0.1, 0.25, 0.5, 0.75, 1f64, 2.5, 5f64, 7.5,
                    10f64,
                ])
                .with_unit("s")
                .build(),
            meter,
        }
    }
}
#[derive(Clone, AsRef, Deref)]
pub struct NitroRepo {
    #[deref(forward)]
    pub inner: Arc<NitroRepoInner>,
    pub database: PgPool,
    pub session_manager: Arc<SessionManager>,
    pub email_access: Arc<EmailAccess>,
    pub metrics: AppMetrics,
    pub repository_metrics: RepositoryMetricsMeter,
    pub auth_token_cache: Arc<
        moka::future::Cache<
            String,
            (
                nr_core::database::entities::user::auth_token::AuthToken,
                nr_core::database::entities::user::UserSafeData,
            ),
        >,
    >,
}
static X_FORWARDED_FOR_HEADER: HeaderName = HeaderName::from_static("x-forwarded-for");

impl HasForwardedHeader for NitroRepo {
    fn forwarded_header(&self) -> Option<&http::HeaderName> {
        Some(&X_FORWARDED_FOR_HEADER)
    }
}
impl NitroRepo {
    #[instrument]
    async fn load_database(database: DatabaseConfig) -> anyhow::Result<PgPool> {
        info!(
            user = %database.user,
            database = %database.database,
            host = %database.host,
            port = ?database.port,
            "Connecting to database"
        );
        let options = database.try_into()?;
        info!("Database connection established successfully (password masked in logs)");
        let database = PgPool::connect_with(options)
            .await
            .context("Could not connect to database")?;
        nr_core::database::migration::run_migrations(&database).await?;
        Ok(database)
    }
    #[allow(clippy::too_many_arguments)]
    pub async fn new(
        mode: Mode,
        site: SiteSetting,
        security: SecuritySettings,
        session_manager: SessionManagerConfig,
        staging_config: StagingConfig,
        email_settings: Option<EmailSetting>,
        database: DatabaseConfig,
        suggested_local_storage_path: Option<PathBuf>,
    ) -> anyhow::Result<Self> {
        let database = Self::load_database(database).await?;
        let stored_sso = ApplicationSettings::get::<SsoSettings>("security.sso", &database)
            .await
            .context("Failed to load stored SSO settings")?;
        let stored_oauth2 =
            ApplicationSettings::get::<OAuth2Settings>("security.oauth2", &database)
                .await
                .context("Failed to load stored OAuth2 settings")?;
        let mut security = security;
        if let Some(stored_sso) = stored_sso {
            security.sso = Some(stored_sso);
        }
        if let Some(stored_oauth2) = stored_oauth2 {
            security.oauth2 = Some(stored_oauth2);
        }

        let oauth2_service = match security.oauth2.clone() {
            Some(cfg) if cfg.enabled => match OAuth2Service::new(cfg.clone()) {
                Ok(Some(service)) => Some(Arc::new(service)),
                Ok(None) => None,
                Err(err) => {
                    warn!(%err, "Failed to initialize OAuth2 service");
                    None
                }
            },
            _ => None,
        };

        let oauth2_rbac = if let Some(cfg) = security.oauth2.clone() {
            if cfg.enabled {
                if let Some(casbin_cfg) = cfg.casbin.as_ref() {
                    match OAuth2Rbac::from_config(casbin_cfg).await {
                        Ok(rbac) => Some(Arc::new(rbac)),
                        Err(err) => {
                            warn!(%err, "Failed to initialize OAuth2 RBAC");
                            None
                        }
                    }
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        let is_installed = user_utils::does_user_exist(&database).await?;
        let mut instance = Instance {
            mode,
            version: current_semver!(),
            app_url: site.app_url.unwrap_or_default(),
            is_installed,
            name: site.name,
            description: site.description,
            is_https: site.is_https,
            password_rules: security.password_rules.clone(),
            sso: security
                .sso
                .as_ref()
                .filter(|cfg| cfg.enabled)
                .map(InstanceSsoSettings::from),
            oauth2: security
                .oauth2
                .as_ref()
                .filter(|cfg| cfg.enabled)
                .map(InstanceOAuth2Settings::from),
        };
        if oauth2_service.is_none() {
            instance.oauth2 = None;
        }
        let mut services = InternalServices::default();

        let (email_access, service) = EmailService::start(email_settings).await?;
        services.email = Some(service);
        let suggested_local_storage_path = if let Some(path) = suggested_local_storage_path {
            path
        } else {
            std::env::current_dir()?.join("storages")
        };
        let nitro_repo = NitroRepoInner {
            instance: Mutex::new(instance),
            storages: RwLock::new(HashMap::new()),
            repositories: RwLock::new(HashMap::new()),
            name_lookup_table: Mutex::new(HashMap::new()),
            general_security_settings: RwLock::new(security),
            oauth2_service: RwLock::new(oauth2_service),
            oauth2_rbac: RwLock::new(oauth2_rbac),
            staging_config,
            services: Mutex::new(services),
            blob_upload_states: parking_lot::Mutex::new(HashMap::new()),
            #[cfg(feature = "frontend")]
            frontend: frontend::HostedFrontend::new(site.frontend_path)?,
            suggested_local_storage_path,
        };

        let session_manager = Arc::new(SessionManager::new(session_manager, mode)?);

        // Initialize auth token cache with 5 minute TTL
        // Tokens expire in 15 minutes, so 5 minute cache is safe
        let auth_token_cache = Arc::new(
            moka::future::Cache::builder()
                .max_capacity(10_000)
                .time_to_live(std::time::Duration::from_secs(300))
                .build(),
        );

        let nitro_repo = NitroRepo {
            inner: Arc::new(nitro_repo),
            session_manager,
            database,
            email_access: Arc::new(email_access),
            metrics: AppMetrics::default(),
            repository_metrics: RepositoryMetricsMeter::default(),
            auth_token_cache,
        };
        nitro_repo.load_storages().await?;
        nitro_repo.load_repositories().await?;
        Ok(nitro_repo)
    }

    /// # Notes
    ///
    /// Lock is held intentionally to prevent anything else touching the storages while they are being loaded
    #[allow(clippy::await_holding_lock)]
    async fn load_storages(&self) -> anyhow::Result<()> {
        let mut storages = self.storages.write();
        storages.clear();

        let db_storages = DBStorage::get_all(&self.database).await?;
        let storage_configs = db_storages
            .into_iter()
            .map(StorageConfig::try_from)
            .collect::<Result<Vec<_>, _>>()?;

        for storage_config in storage_configs {
            let id = storage_config.storage_config.storage_id;
            info!(?storage_config, "Loading storage");
            let Some(factory) =
                self.get_storage_factory(&storage_config.storage_config.storage_type)
            else {
                warn!(
                    "No storage factory found for {}",
                    storage_config.storage_config.storage_type
                );
                continue;
            };
            let storage = factory.create_storage(storage_config).await?;
            storages.insert(id, storage);
        }
        info!("Loaded {} storages", storages.len());
        Ok(())
    }
    /// # Notes
    ///
    /// Lock is held intentionally to prevent anything else touching the repositories while they are being loaded
    #[allow(clippy::await_holding_lock)]
    async fn load_repositories(&self) -> anyhow::Result<()> {
        let mut repositories = self.repositories.write();
        repositories.clear();
        let db_repositories = DBRepository::get_all(&self.database).await?;
        for db_repository in db_repositories {
            let storage = self
                .get_storage(db_repository.storage_id)
                .context("Storage not found")?;
            let repository_type = self
                .get_repository_type(&db_repository.repository_type)
                .context("Repository type not found")?;
            let repository_id = db_repository.id;
            let repository = repository_type
                .load_repo(db_repository, storage, self.clone())
                .await?;
            repositories.insert(repository_id, repository);
        }
        info!("Loaded {} repositories", repositories.len());
        Ok(())
    }
    pub fn get_storage_factory(&self, storage_name: &str) -> Option<&'static dyn StorageFactory> {
        STORAGE_FACTORIES
            .iter()
            .find(|factory| factory.storage_name() == storage_name)
            .copied()
    }
    pub async fn close(self) {
        self.session_manager.shutdown();
        self.inner.notify_shutdown();
        //TODO: Close Repositories
        let storages = {
            let mut storages = self.storages.write();
            // Take the values out of the hashmap and clear it
            std::mem::take(&mut *storages)
        };
        for (id, storage) in storages.into_iter() {
            info!(?id, "Unloading storage");
            storage.unload().await.unwrap_or_else(|err| {
                warn!(?id, "Failed to unload storage: {}", err);
            });
        }
        info!("Removing Logger");

        info!("Removing Email");
        let email = self.inner.take_email();
        info!("Email State has been taken");
        if let Some(email) = email {
            email.handle.abort();
        }
        let session_cleaner = self.inner.take_session_cleaner();
        if let Some(handle) = session_cleaner {
            handle.abort();
        }
    }
    pub fn get_repository_config_type(
        &self,
        name: &str,
    ) -> Option<&'static dyn RepositoryConfigType> {
        REPOSITORY_CONFIG_TYPES
            .iter()
            .find(|config_type| config_type.get_type().eq_ignore_ascii_case(name))
            .copied()
    }

    pub fn security_settings(&self) -> SecuritySettings {
        self.inner.general_security_settings.read().clone()
    }

    pub fn sso_settings(&self) -> Option<SsoSettings> {
        self.inner
            .general_security_settings
            .read()
            .sso
            .clone()
            .filter(|cfg| cfg.enabled)
    }

    pub fn sso_settings_raw(&self) -> Option<SsoSettings> {
        self.inner.general_security_settings.read().sso.clone()
    }

    pub fn oauth2_settings(&self) -> Option<OAuth2Settings> {
        self.inner
            .general_security_settings
            .read()
            .oauth2
            .clone()
            .filter(|cfg| cfg.enabled)
    }

    pub fn oauth2_settings_raw(&self) -> Option<OAuth2Settings> {
        self.inner.general_security_settings.read().oauth2.clone()
    }

    pub fn oauth2_service(&self) -> Option<Arc<OAuth2Service>> {
        self.inner.oauth2_service.read().clone()
    }

    pub fn oauth2_rbac(&self) -> Option<Arc<OAuth2Rbac>> {
        self.inner.oauth2_rbac.read().clone()
    }

    pub async fn apply_oauth_roles(&self, subject: &str, roles: &[String]) -> anyhow::Result<()> {
        if let Some(rbac) = self.oauth2_rbac() {
            rbac.set_roles_for_user(subject, roles).await?;
        }
        Ok(())
    }

    pub async fn check_oauth_permission(
        &self,
        subject: &str,
        object: &str,
        action: &str,
    ) -> anyhow::Result<Option<bool>> {
        if let Some(rbac) = self.oauth2_rbac() {
            let decision = rbac.enforce(subject, object, action).await?;
            Ok(Some(decision))
        } else {
            Ok(None)
        }
    }

    pub async fn update_oauth2_settings(
        &self,
        settings: Option<OAuth2Settings>,
    ) -> anyhow::Result<()> {
        let mut new_service: Option<Arc<OAuth2Service>> = None;
        let mut new_rbac: Option<Arc<OAuth2Rbac>> = None;

        if let Some(cfg) = settings.clone() {
            if cfg.enabled {
                let service = OAuth2Service::new(cfg.clone())
                    .map_err(|err| anyhow!("Failed to initialize OAuth2 service: {err}"))?
                    .ok_or_else(|| {
                        anyhow!("OAuth2 configuration is missing provider credentials")
                    })?;
                new_service = Some(Arc::new(service));

                if let Some(casbin_cfg) = cfg.casbin.as_ref() {
                    let rbac = OAuth2Rbac::from_config(casbin_cfg)
                        .await
                        .map_err(|err| anyhow!("Failed to initialize OAuth2 RBAC: {err}"))?;
                    new_rbac = Some(Arc::new(rbac));
                }
            }
        }

        {
            let mut security = self.inner.general_security_settings.write();
            security.oauth2 = settings.clone();
        }
        {
            let mut instance = self.inner.instance.lock();
            instance.oauth2 = settings
                .as_ref()
                .filter(|cfg| cfg.enabled)
                .map(InstanceOAuth2Settings::from);
        }
        {
            let mut service_lock = self.inner.oauth2_service.write();
            *service_lock = new_service;
        }
        {
            let mut rbac_lock = self.inner.oauth2_rbac.write();
            *rbac_lock = new_rbac;
        }

        if let Some(settings) = settings {
            ApplicationSettings::upsert("security.oauth2", &settings, &self.database).await?;
        } else {
            ApplicationSettings::delete("security.oauth2", &self.database).await?;
        }

        Ok(())
    }
    pub fn get_repository(&self, id: Uuid) -> Option<DynRepository> {
        let repository = self.repositories.read();
        repository.get(&id).cloned()
    }
    pub fn add_storage(&self, id: Uuid, storage: DynStorage) {
        let mut storages = self.storages.write();
        storages.insert(id, storage);
    }
    pub fn add_repository(&self, id: Uuid, repository: DynRepository) {
        let mut repositories = self.repositories.write();
        repositories.insert(id, repository);
    }

    pub fn loaded_repositories(&self) -> Vec<(Uuid, DynRepository)> {
        let repositories = self.repositories.read();
        repositories
            .iter()
            .map(|(id, repository)| (*id, repository.clone()))
            .collect()
    }

    pub async fn get_repository_auth_config(
        &self,
        repository_id: Uuid,
    ) -> Result<RepositoryAuthConfig, sqlx::Error> {
        let config = DBRepositoryConfig::<RepositoryAuthConfig>::get_config(
            repository_id,
            RepositoryAuthConfigType::get_type_static(),
            &self.database,
        )
        .await?;
        Ok(config.map(|cfg| cfg.value.0).unwrap_or_default())
    }

    fn ensure_upload_state_handle(
        &self,
        repository: Uuid,
        upload_id: &str,
        sha256_only: bool,
    ) -> BlobUploadStateHandle {
        let mut states = self.inner.blob_upload_states.lock();
        states
            .entry((repository, upload_id.to_owned()))
            .or_insert_with(|| {
                if sha256_only {
                    BlobUploadStateHandle::new(UploadState::new_sha256_only())
                } else {
                    BlobUploadStateHandle::new(UploadState::new())
                }
            })
            .clone()
    }

    pub fn get_upload_state_handle(
        &self,
        repository: Uuid,
        upload_id: &str,
    ) -> Option<BlobUploadStateHandle> {
        let states = self.inner.blob_upload_states.lock();
        states.get(&(repository, upload_id.to_owned())).cloned()
    }

    pub fn ensure_docker_blob_upload_state_handle(
        &self,
        repository: Uuid,
        upload_id: &str,
    ) -> BlobUploadStateHandle {
        self.ensure_upload_state_handle(repository, upload_id, true)
    }

    fn ensure_blob_upload_state_handle(
        &self,
        repository: Uuid,
        upload_id: &str,
    ) -> BlobUploadStateHandle {
        self.ensure_upload_state_handle(repository, upload_id, false)
    }

    pub fn update_upload_state_handle(&self, handle: &BlobUploadStateHandle, chunk: &[u8]) -> u64 {
        let mut guard = handle.lock();
        guard.update(chunk);
        guard.length
    }

    pub fn blob_upload_state_length(&self, handle: &BlobUploadStateHandle) -> u64 {
        handle.lock().length
    }

    pub fn begin_blob_upload_state(&self, repository: Uuid, upload_id: &str) {
        self.ensure_blob_upload_state_handle(repository, upload_id);
    }

    /// Begin blob upload state for Docker (SHA256 only)
    pub fn begin_docker_blob_upload_state(&self, repository: Uuid, upload_id: &str) {
        self.ensure_docker_blob_upload_state_handle(repository, upload_id);
    }

    pub fn update_blob_upload_state(&self, repository: Uuid, upload_id: &str, chunk: &[u8]) -> u64 {
        let state = self.ensure_blob_upload_state_handle(repository, upload_id);
        self.update_upload_state_handle(&state, chunk)
    }

    /// Update blob upload state for Docker (ensures SHA256-only hashing)
    pub fn update_docker_blob_upload_state(
        &self,
        repository: Uuid,
        upload_id: &str,
        chunk: &[u8],
    ) -> u64 {
        let state = self.ensure_docker_blob_upload_state_handle(repository, upload_id);
        self.update_upload_state_handle(&state, chunk)
    }

    pub fn current_blob_upload_length(&self, repository: Uuid, upload_id: &str) -> Option<u64> {
        self.get_upload_state_handle(repository, upload_id)
            .map(|handle| handle.lock().length)
    }

    pub fn finalize_blob_upload_state(
        &self,
        repository: Uuid,
        upload_id: &str,
    ) -> Option<FinalizedUpload> {
        let state = {
            let mut states = self.inner.blob_upload_states.lock();
            states.remove(&(repository, upload_id.to_owned()))
        };
        state.map(|handle| match handle.try_into_state() {
            Ok(state) => state.finalize(),
            Err(handle) => {
                let mut guard = handle.lock();
                let state = guard.take();
                state.finalize()
            }
        })
    }

    pub fn abandon_blob_upload_state(&self, repository: Uuid, upload_id: &str) {
        let mut states = self.inner.blob_upload_states.lock();
        states.remove(&(repository, upload_id.to_owned()));
    }

    pub fn update_app_url(&self, app_url: &Uri) {
        info!(?app_url, "Updating app url");
        // TODO:
    }
    pub async fn update_sso_settings(&self, settings: Option<SsoSettings>) -> anyhow::Result<()> {
        {
            let mut security = self.inner.general_security_settings.write();
            security.sso = settings.clone();
        }

        {
            let mut instance = self.inner.instance.lock();
            instance.sso = settings
                .as_ref()
                .filter(|cfg| cfg.enabled)
                .map(InstanceSsoSettings::from);
        }

        if let Some(settings) = settings {
            ApplicationSettings::upsert("security.sso", &settings, &self.database).await?;
        } else {
            ApplicationSettings::delete("security.sso", &self.database).await?;
        }

        Ok(())
    }
    /// Checks if a repository name and storage pair are found in the lookup table. If not queries the database.
    /// If found in the database, adds the pair to the lookup table
    ///
    /// ## Notes
    /// [RepositoryStorageName] is case insensitive. It will be converted to lowercase before being queried. Database queries are case insensitive
    #[instrument(skip(name))]
    pub async fn get_repository_from_names(
        &self,
        name: &RepositoryStorageName,
    ) -> Result<Option<DynRepository>, sqlx::Error> {
        let id = {
            let lookup_table = self.inner.name_lookup_table.lock();
            lookup_table.get(name).cloned()
        };
        if let Some(id) = id {
            debug!(?id, ?name, "Found id in lookup table");
            let repository: Option<DynRepository> = self.get_repository(id);
            if repository.is_none() {
                warn!(?name, "Unregistered database id found in lookup table");
                {
                    let mut lookup_table = self.inner.name_lookup_table.lock();
                    lookup_table.remove(name);
                }
                return Ok(repository);
            }
            return Ok(repository);
        }
        debug!(
            ?name,
            "Name not found in lookup table. Attempting to query database"
        );
        let id = name.query_db(&self.database).await?;
        if let Some(id) = id {
            debug!(?id, ?name, "Found id in database");
            let repository: Option<DynRepository> = self.get_repository(id);
            if repository.is_none() {
                warn!(
                    ?name,
                    "Unregistered database id found. Repositories in database do not match loaded repositories"
                );
                // TODO: Reload Everything
                return Ok(repository);
            }
            // Add the name to the lookup table
            let mut lookup_table = self.inner.name_lookup_table.lock();
            lookup_table.insert(name.clone(), id);

            return Ok(repository);
        }
        // No repository found in the database
        Ok(None)
    }
    pub fn get_storage(&self, id: Uuid) -> Option<DynStorage> {
        let storages = self.storages.read();
        storages.get(&id).cloned()
    }
    pub fn get_repository_type(&self, name: &str) -> Option<&'static dyn RepositoryType> {
        REPOSITORY_TYPES
            .iter()
            .find(|repo_type| repo_type.get_type().eq_ignore_ascii_case(name))
            .copied()
    }
    pub fn remove_repository(&self, id: Uuid) {
        {
            let mut repositories = self.repositories.write();
            repositories.remove(&id);
        }
        {
            let mut lookup_table = self.inner.name_lookup_table.lock();
            lookup_table.retain(|_, value| *value != id);
        }
    }
    fn set_session_cleaner(&self, cleaner: JoinHandle<()>) {
        let mut services = self.inner.services.lock();
        services.session_cleaner = Some(cleaner);
    }
    fn start_session_cleaner(&self) {
        let result = SessionManager::start_cleaner(self.clone());
        if let Some(handle) = result {
            self.set_session_cleaner(handle);
            info!("Session cleaner started");
        }
    }
}

pub type NitroRepoState = State<NitroRepo>;

pub static REPOSITORY_CONFIG_TYPES: &[&dyn RepositoryConfigType] = &[
    &ProjectConfigType,
    &RepositoryPageType,
    &DockerRegistryConfigType,
    &DockerPushRulesConfigType,
    &GoRepositoryConfigType,
    &HelmRepositoryConfigType,
    &MavenRepositoryConfigType,
    &MavenPushRulesConfigType,
    &NPMRegistryConfigType,
    &PythonRepositoryConfigType,
    &PhpRepositoryConfigType,
    &RepositoryAuthConfigType,
];
pub static REPOSITORY_TYPES: &[&dyn RepositoryType] = &[
    &DockerRepositoryType,
    &GoRepositoryType,
    &HelmRepositoryType,
    &MavenRepositoryType,
    &NpmRegistryType,
    &PythonRepositoryType,
    &PhpRepositoryType,
];

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;

    #[test]
    fn upload_state_streaming_hashes_match_reference() {
        let chunks: &[&[u8]] = &[b"streamed ", b"payload ", b"verification"];
        let mut state = UploadState::new();
        for chunk in chunks {
            state.update(chunk);
        }

        let finalized = state.finalize();
        let combined = b"streamed payload verification";

        assert_eq!(finalized.length, combined.len() as u64);

        let expected_digest = format!("sha256:{:x}", Sha256::digest(combined));
        assert_eq!(finalized.digest, expected_digest);

        let expected_md5 = BASE64_STANDARD.encode(md5::Md5::digest(combined).as_slice());
        assert_eq!(finalized.hashes.md5.as_deref(), Some(expected_md5.as_str()));

        let expected_sha1 = BASE64_STANDARD.encode(sha1::Sha1::digest(combined).as_slice());
        assert_eq!(
            finalized.hashes.sha1.as_deref(),
            Some(expected_sha1.as_str())
        );

        let expected_sha2 = BASE64_STANDARD.encode(sha2::Sha256::digest(combined).as_slice());
        assert_eq!(
            finalized.hashes.sha2_256.as_deref(),
            Some(expected_sha2.as_str())
        );

        let expected_sha3 = BASE64_STANDARD.encode(sha3::Sha3_256::digest(combined).as_slice());
        assert_eq!(
            finalized.hashes.sha3_256.as_deref(),
            Some(expected_sha3.as_str())
        );
    }
}
