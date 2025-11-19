use std::sync::{
    Arc,
    atomic::{self, AtomicBool},
};

use derive_more::derive::Deref;
use nr_core::{
    database::entities::repository::DBRepository,
    repository::{
        Visibility,
        config::{RepositoryConfigType, get_repository_config_or_default},
    },
};
use nr_storage::DynStorage;
use parking_lot::RwLock;
use tracing::{debug, error, instrument};
use uuid::Uuid;

use super::{
    DockerError, DockerPushRules, DockerPushRulesConfigType, REPOSITORY_TYPE_ID, RepoResponse,
    RepositoryRequest,
};
use crate::{
    app::NitroRepo,
    repository::{Repository, RepositoryAuthConfigType, RepositoryFactoryError},
};

#[derive(derive_more::Debug)]
pub struct DockerHostedInner {
    pub id: Uuid,
    pub name: String,
    pub active: AtomicBool,
    pub visibility: RwLock<Visibility>,
    pub push_rules: RwLock<DockerPushRules>,
    #[debug(skip)]
    pub storage: DynStorage,
    #[debug(skip)]
    pub site: NitroRepo,
}

#[derive(Debug, Clone, Deref)]
pub struct DockerHosted(Arc<DockerHostedInner>);

impl DockerHosted {
    pub async fn load(
        repository: DBRepository,
        storage: DynStorage,
        site: NitroRepo,
    ) -> Result<Self, RepositoryFactoryError> {
        let push_rules_db = get_repository_config_or_default::<
            DockerPushRulesConfigType,
            DockerPushRules,
        >(repository.id, site.as_ref())
        .await?;
        debug!("Loaded Docker Push Rules Config: {:?}", push_rules_db);

        let active = AtomicBool::new(repository.active);

        let inner = DockerHostedInner {
            id: repository.id,
            name: repository.name.into(),
            active,
            visibility: RwLock::new(repository.visibility),
            push_rules: RwLock::new(push_rules_db.value.0),
            storage,
            site,
        };

        Ok(Self(Arc::new(inner)))
    }
}

impl Repository for DockerHosted {
    type Error = DockerError;

    #[inline(always)]
    fn site(&self) -> NitroRepo {
        self.0.site.clone()
    }

    #[inline(always)]
    fn get_storage(&self) -> DynStorage {
        self.0.storage.clone()
    }

    #[inline(always)]
    fn visibility(&self) -> Visibility {
        *self.visibility.read()
    }

    #[inline(always)]
    fn get_type(&self) -> &'static str {
        REPOSITORY_TYPE_ID
    }

    fn full_type(&self) -> &'static str {
        "docker/hosted"
    }

    #[inline(always)]
    fn name(&self) -> String {
        self.0.name.clone()
    }

    #[inline(always)]
    fn id(&self) -> Uuid {
        self.0.id
    }

    #[inline(always)]
    fn is_active(&self) -> bool {
        self.active.load(atomic::Ordering::Relaxed)
    }

    fn config_types(&self) -> Vec<&str> {
        vec![
            DockerPushRulesConfigType::get_type_static(),
            RepositoryAuthConfigType::get_type_static(),
        ]
    }

    #[instrument(fields(repository_type = "docker/hosted"))]
    async fn reload(&self) -> Result<(), RepositoryFactoryError> {
        let Some(is_active) = DBRepository::get_active_by_id(self.id, self.site.as_ref()).await?
        else {
            error!("Failed to get repository");
            self.0.active.store(false, atomic::Ordering::Relaxed);
            return Ok(());
        };
        self.0.active.store(is_active, atomic::Ordering::Relaxed);

        let push_rules_db = get_repository_config_or_default::<
            DockerPushRulesConfigType,
            DockerPushRules,
        >(self.id, self.site.as_ref())
        .await?;

        {
            let mut push_rules = self.push_rules.write();
            *push_rules = push_rules_db.value.0;
        }

        Ok(())
    }

    async fn handle_get(&self, request: RepositoryRequest) -> Result<RepoResponse, Self::Error> {
        super::handlers::handle_get(self.clone(), request).await
    }

    async fn handle_put(&self, request: RepositoryRequest) -> Result<RepoResponse, Self::Error> {
        super::handlers::handle_put(self.clone(), request).await
    }

    async fn handle_post(&self, request: RepositoryRequest) -> Result<RepoResponse, Self::Error> {
        super::handlers::handle_post(self.clone(), request).await
    }

    async fn handle_patch(&self, request: RepositoryRequest) -> Result<RepoResponse, Self::Error> {
        super::handlers::handle_patch(self.clone(), request).await
    }

    async fn handle_delete(&self, request: RepositoryRequest) -> Result<RepoResponse, Self::Error> {
        super::handlers::handle_delete(self.clone(), request).await
    }

    async fn handle_head(&self, request: RepositoryRequest) -> Result<RepoResponse, Self::Error> {
        super::handlers::handle_head(self.clone(), request).await
    }
}
