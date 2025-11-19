use std::sync::Arc;

use http::StatusCode;
use nr_core::repository::config::RepositoryConfigType;
use nr_core::{
    database::entities::{
        project::{DBProject, ProjectDBType, versions::DBProjectVersion},
        repository::{DBRepository, DBRepositoryConfig},
    },
    repository::{
        Visibility,
        config::repository_page::RepositoryPageType,
        project::{PhpPackageMetadata, VersionData},
    },
    user::permissions::RepositoryActions,
};
use nr_storage::{DynStorage, Storage};
use parking_lot::RwLock;
use serde_json::to_value;
use uuid::Uuid;

use super::{
    PhpRepositoryError,
    configs::{PhpRepositoryConfig, PhpRepositoryConfigType},
    utils::PhpPackagePathInfo,
};
use crate::{
    app::NitroRepo,
    repository::{RepoResponse, Repository, RepositoryFactoryError, RepositoryRequest},
    repository::{
        RepositoryAuthConfigType,
        utils::{RepositoryExt, can_read_repository_with_auth},
    },
    utils::ResponseBuilder,
};

#[derive(Debug)]
pub struct PhpRepositoryInner {
    pub id: Uuid,
    pub name: String,
    pub visibility: RwLock<Visibility>,
    pub repository: DBRepository,
    pub config: PhpRepositoryConfig,
    pub storage: DynStorage,
    pub site: NitroRepo,
}

#[derive(Debug, Clone)]
pub struct PhpHosted(pub Arc<PhpRepositoryInner>);

impl PhpHosted {
    pub async fn load(
        site: NitroRepo,
        storage: DynStorage,
        repository: DBRepository,
    ) -> Result<Self, RepositoryFactoryError> {
        let config = DBRepositoryConfig::<PhpRepositoryConfig>::get_config(
            repository.id,
            PhpRepositoryConfigType::get_type_static(),
            site.as_ref(),
        )
        .await?
        .map(|cfg| cfg.value.0)
        .unwrap_or_default();
        Ok(Self(Arc::new(PhpRepositoryInner {
            id: repository.id,
            name: repository.name.to_string(),
            visibility: RwLock::new(repository.visibility),
            repository,
            config,
            storage,
            site,
        })))
    }

    async fn upsert_metadata(
        &self,
        publisher: i32,
        info: &PhpPackagePathInfo,
    ) -> Result<(), PhpRepositoryError> {
        let project_key = info.normalized_package_name();
        let project = if let Some(project) =
            DBProject::find_by_project_key(&project_key, self.id(), self.site().as_ref()).await?
        {
            project
        } else {
            let new_project = nr_core::database::entities::project::NewProject {
                scope: Some(info.vendor.clone()),
                project_key: project_key.clone(),
                name: info.package_name(),
                description: None,
                repository: self.id(),
                storage_path: info.project_storage_path(),
            };
            new_project.insert(self.site().as_ref()).await?
        };

        if DBProjectVersion::find_by_version_and_project(
            &info.version,
            project.id,
            &self.site().database,
        )
        .await?
        .is_some()
        {
            return Ok(());
        }

        let metadata = PhpPackageMetadata {
            filename: info.file_name.clone(),
            ..Default::default()
        };

        let new_version = nr_core::database::entities::project::versions::NewVersion {
            project_id: project.id,
            version: info.version.clone(),
            release_type: info.release_type(),
            version_path: info.version_storage_path(),
            publisher: Some(publisher),
            version_page: None,
            extra: VersionData {
                extra: Some(to_value(metadata)?),
                ..Default::default()
            },
        };
        new_version.insert(&self.site().database).await?;
        Ok(())
    }

    async fn handle_upload(
        &self,
        request: RepositoryRequest,
    ) -> Result<RepoResponse, PhpRepositoryError> {
        let Some(user) = request
            .authentication
            .get_user_if_has_action(RepositoryActions::Write, self.id(), self.site().as_ref())
            .await?
        else {
            return Ok(RepoResponse::unauthorized());
        };
        let info = PhpPackagePathInfo::try_from(&request.path)?;
        let bytes = request.body.body_as_bytes().await?;
        self.storage()
            .save_file(self.id(), bytes.into(), &request.path)
            .await?;
        self.upsert_metadata(user.id, &info).await?;
        Ok(RepoResponse::Other(ResponseBuilder::created().empty()))
    }

    fn id(&self) -> Uuid {
        self.0.id
    }
    fn site(&self) -> NitroRepo {
        self.0.site.clone()
    }
    fn storage(&self) -> DynStorage {
        self.0.storage.clone()
    }
    fn visibility(&self) -> Visibility {
        *self.0.visibility.read()
    }
}

impl RepositoryExt for PhpHosted {}

impl Repository for PhpHosted {
    type Error = PhpRepositoryError;

    fn get_storage(&self) -> DynStorage {
        self.storage()
    }

    fn get_type(&self) -> &'static str {
        "php"
    }

    fn full_type(&self) -> &'static str {
        "php/hosted"
    }

    fn config_types(&self) -> Vec<&str> {
        vec![
            PhpRepositoryConfigType::get_type_static(),
            RepositoryPageType::get_type_static(),
            RepositoryAuthConfigType::get_type_static(),
        ]
    }

    fn name(&self) -> String {
        self.0.name.clone()
    }

    fn id(&self) -> Uuid {
        self.id()
    }

    fn visibility(&self) -> Visibility {
        self.visibility()
    }

    fn is_active(&self) -> bool {
        self.0.repository.active
    }

    fn site(&self) -> NitroRepo {
        self.site()
    }

    fn handle_get(
        &self,
        request: RepositoryRequest,
    ) -> impl std::future::Future<Output = Result<RepoResponse, Self::Error>> + Send {
        let repository_id = self.id();
        let visibility = self.visibility();
        let site = self.site();
        let storage = self.storage();
        async move {
            if !can_read_repository_with_auth(
                &request.authentication,
                visibility,
                repository_id,
                site.as_ref(),
                &request.auth_config,
            )
            .await?
            {
                return Ok(RepoResponse::basic_text_response(
                    StatusCode::UNAUTHORIZED,
                    "Missing permission to read repository",
                ));
            }
            let file = storage.open_file(repository_id, &request.path).await?;
            Ok(file.into())
        }
    }

    fn handle_put(
        &self,
        request: RepositoryRequest,
    ) -> impl std::future::Future<Output = Result<RepoResponse, Self::Error>> + Send {
        let this = self.clone();
        async move { this.handle_upload(request).await }
    }

    fn handle_post(
        &self,
        request: RepositoryRequest,
    ) -> impl std::future::Future<Output = Result<RepoResponse, Self::Error>> + Send {
        let this = self.clone();
        async move { this.handle_upload(request).await }
    }

    fn handle_head(
        &self,
        request: RepositoryRequest,
    ) -> impl std::future::Future<Output = Result<RepoResponse, Self::Error>> + Send {
        let repository_id = self.id();
        let visibility = self.visibility();
        let site = self.site();
        let storage = self.storage();
        async move {
            if !can_read_repository_with_auth(
                &request.authentication,
                visibility,
                repository_id,
                site.as_ref(),
                &request.auth_config,
            )
            .await?
            {
                return Ok(RepoResponse::basic_text_response(
                    StatusCode::UNAUTHORIZED,
                    "Missing permission to read repository",
                ));
            }
            let meta = storage
                .get_file_information(repository_id, &request.path)
                .await?;
            Ok(meta.into())
        }
    }
}
