use std::sync::Arc;

use http::StatusCode;
use nr_core::repository::config::RepositoryConfigType;
use nr_core::{
    database::entities::{
        project::{DBProject, ProjectDBType, versions::DBProjectVersion},
        repository::DBRepository,
    },
    repository::{
        Visibility,
        config::{project::ProjectConfigType, repository_page::RepositoryPageType},
        project::{PythonPackageMetadata, VersionData},
    },
    user::permissions::RepositoryActions,
};
use nr_storage::{DynStorage, Storage};
use parking_lot::RwLock;
use serde_json::to_value;
use tracing::{debug, info, instrument};
use uuid::Uuid;

use super::{
    PythonRepositoryError,
    configs::{PythonRepositoryConfig, PythonRepositoryConfigType},
    utils::{PythonPackagePathInfo, normalize_package_name},
};
use crate::{
    app::NitroRepo,
    repository::utils::{RepositoryExt, can_read_repository},
    repository::{RepoResponse, Repository, RepositoryFactoryError, RepositoryRequest},
    utils::ResponseBuilder,
};

#[derive(Debug)]
pub struct PythonRepositoryInner {
    pub id: Uuid,
    pub name: String,
    pub visibility: RwLock<Visibility>,
    pub repository: DBRepository,
    #[allow(dead_code)]
    pub config: PythonRepositoryConfig,
    pub storage: DynStorage,
    pub site: NitroRepo,
}

#[derive(Debug, Clone)]
pub struct PythonHosted(pub Arc<PythonRepositoryInner>);

impl PythonHosted {
    pub async fn load(
        site: NitroRepo,
        storage: DynStorage,
        repository: DBRepository,
        config: PythonRepositoryConfig,
    ) -> Result<Self, RepositoryFactoryError> {
        let visibility = RwLock::new(repository.visibility);
        Ok(Self(Arc::new(PythonRepositoryInner {
            id: repository.id,
            name: repository.name.to_string(),
            visibility,
            repository,
            config,
            storage,
            site,
        })))
    }

    #[instrument(skip(self, request))]
    async fn handle_upload(
        &self,
        request: RepositoryRequest,
    ) -> Result<RepoResponse, PythonRepositoryError> {
        let Some(user) = request
            .authentication
            .get_user_if_has_action(RepositoryActions::Write, self.id(), self.site().as_ref())
            .await?
        else {
            return Ok(RepoResponse::unauthorized());
        };
        let bytes = request.body.body_as_bytes().await?;
        let info = PythonPackagePathInfo::try_from(&request.path)?;
        info!(path = %request.path, ?info, "Saving Python package");
        self.storage()
            .save_file(self.id(), bytes.into(), &request.path)
            .await?;

        self.upsert_metadata(user.id, &info).await?;

        Ok(RepoResponse::Other(ResponseBuilder::created().empty()))
    }

    #[instrument(skip(self, info))]
    async fn upsert_metadata(
        &self,
        publisher: i32,
        info: &PythonPackagePathInfo,
    ) -> Result<(), PythonRepositoryError> {
        let project_key = info.project_key();
        let project = if let Some(project) =
            DBProject::find_by_project_key(&project_key, self.id(), self.site().as_ref()).await?
        {
            project
        } else {
            let new_project = nr_core::database::entities::project::NewProject {
                scope: None,
                project_key: project_key.clone(),
                name: info.package.clone(),
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
            debug!(
                project_id = %project.id,
                version = %info.version,
                "Python version already exists"
            );
            return Ok(());
        }

        let metadata = PythonPackageMetadata {
            filename: info.file_name.clone(),
            normalized_name: Some(normalize_package_name(&info.package)),
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

    fn visibility(&self) -> Visibility {
        *self.0.visibility.read()
    }
    fn site(&self) -> NitroRepo {
        self.0.site.clone()
    }
    fn storage(&self) -> DynStorage {
        self.0.storage.clone()
    }
    fn id(&self) -> Uuid {
        self.0.id
    }
}

impl RepositoryExt for PythonHosted {}

impl Repository for PythonHosted {
    type Error = PythonRepositoryError;

    fn get_storage(&self) -> DynStorage {
        self.storage()
    }

    fn get_type(&self) -> &'static str {
        "python"
    }

    fn full_type(&self) -> &'static str {
        "python/hosted"
    }

    fn config_types(&self) -> Vec<&str> {
        vec![
            PythonRepositoryConfigType::get_type_static(),
            ProjectConfigType::get_type_static(),
            RepositoryPageType::get_type_static(),
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
        let visibility = self.visibility();
        let site = self.site();
        let storage = self.storage();
        let repository_id = self.id();
        async move {
            if !can_read_repository(
                &request.authentication,
                visibility,
                repository_id,
                site.as_ref(),
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

    fn handle_head(
        &self,
        request: RepositoryRequest,
    ) -> impl std::future::Future<Output = Result<RepoResponse, Self::Error>> + Send {
        let visibility = self.visibility();
        let site = self.site();
        let storage = self.storage();
        let repository_id = self.id();
        async move {
            if !can_read_repository(
                &request.authentication,
                visibility,
                repository_id,
                site.as_ref(),
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

    fn handle_post(
        &self,
        request: RepositoryRequest,
    ) -> impl std::future::Future<Output = Result<RepoResponse, Self::Error>> + Send {
        let this = self.clone();
        async move { this.handle_upload(request).await }
    }
}
