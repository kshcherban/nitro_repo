use nr_core::{
    database::entities::project::{DBProject, ProjectDBType, versions::DBProjectVersion},
    repository::Visibility,
    user::permissions::{HasPermissions, RepositoryActions},
};
use sqlx::PgPool;
use uuid::Uuid;

use super::{Repository, RepositoryAuthConfig, RepositoryHandlerError};

pub async fn can_read_repository<A: HasPermissions>(
    auth: &A,
    visibility: Visibility,
    repository_id: Uuid,
    database: &PgPool,
) -> Result<bool, sqlx::Error> {
    if matches!(visibility, Visibility::Private | Visibility::Hidden) {
        return auth
            .has_action(RepositoryActions::Read, repository_id, database)
            .await;
    }
    Ok(true)
}

pub async fn can_read_repository_with_auth<A: HasPermissions>(
    auth: &A,
    visibility: Visibility,
    repository_id: Uuid,
    database: &PgPool,
    auth_config: &RepositoryAuthConfig,
) -> Result<bool, sqlx::Error> {
    if auth_config.enabled {
        return auth
            .has_action(RepositoryActions::Read, repository_id, database)
            .await;
    }
    can_read_repository(auth, visibility, repository_id, database).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::PgConnectOptions;

    struct StubAuth {
        allow: bool,
    }

    impl HasPermissions for StubAuth {
        fn user_id(&self) -> Option<i32> {
            Some(1)
        }

        fn get_permissions(&self) -> Option<UserPermissions> {
            None
        }

        async fn has_action(
            &self,
            _action: RepositoryActions,
            _repository: Uuid,
            _db: &PgPool,
        ) -> Result<bool, sqlx::Error> {
            Ok(self.allow)
        }
    }

    use nr_core::user::permissions::UserPermissions;

    fn test_pool() -> PgPool {
        PgPool::connect_lazy_with(
            PgConnectOptions::new()
                .host("localhost")
                .username("postgres")
                .database("postgres"),
        )
    }

    #[tokio::test]
    async fn auth_config_enabled_requires_permission() {
        let pool = test_pool();
        let repository_id = Uuid::new_v4();
        let config = RepositoryAuthConfig { enabled: true };
        let allowed = StubAuth { allow: true };
        let denied = StubAuth { allow: false };

        assert!(
            can_read_repository_with_auth(
                &allowed,
                Visibility::Public,
                repository_id,
                &pool,
                &config
            )
            .await
            .unwrap()
        );
        assert!(
            !can_read_repository_with_auth(
                &denied,
                Visibility::Public,
                repository_id,
                &pool,
                &config
            )
            .await
            .unwrap()
        );
    }

    #[tokio::test]
    async fn public_repository_without_auth_config_allows_guests() {
        let pool = test_pool();
        let repository_id = Uuid::new_v4();
        let config = RepositoryAuthConfig { enabled: false };
        let denied = StubAuth { allow: false };

        assert!(
            can_read_repository_with_auth(
                &denied,
                Visibility::Public,
                repository_id,
                &pool,
                &config
            )
            .await
            .unwrap()
        );
    }
}

pub trait RepositoryExt: Repository {
    async fn get_project_from_key(
        &self,
        project_key: &str,
    ) -> Result<Option<DBProject>, RepositoryHandlerError> {
        let project =
            DBProject::find_by_project_key(project_key, self.id(), self.site().as_ref()).await?;
        Ok(project)
    }
    async fn get_project_version(
        &self,
        project: Uuid,
        version: &str,
    ) -> Result<Option<DBProjectVersion>, RepositoryHandlerError> {
        let version =
            DBProjectVersion::find_by_version_and_project(version, project, &self.site().database)
                .await?;
        Ok(version)
    }
}
