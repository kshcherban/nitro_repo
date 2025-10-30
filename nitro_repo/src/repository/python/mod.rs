use ahash::HashMap;
use futures::future::BoxFuture;
use nr_core::{
    database::entities::repository::{DBRepository, DBRepositoryConfig},
    repository::config::RepositoryConfigType,
};
use nr_macros::DynRepositoryHandler;
use nr_storage::DynStorage;

pub use super::prelude::*;
mod configs;
pub use configs::*;
pub mod hosted;
pub mod proxy;
pub mod utils;

use proxy::PythonProxy;

use super::{DynRepository, NewRepository, RepositoryType, RepositoryTypeDescription};

#[derive(Debug, Clone, DynRepositoryHandler)]
#[repository_handler(error = PythonRepositoryError)]
pub enum PythonRepository {
    Hosted(hosted::PythonHosted),
    Proxy(PythonProxy),
}

#[derive(Debug, thiserror::Error)]
pub enum PythonRepositoryError {
    #[error("Invalid package path: {0}")]
    InvalidPath(String),
    #[error("Unsupported operation: {0}")]
    Unsupported(&'static str),
    #[error("{0}")]
    Other(Box<dyn crate::utils::IntoErrorResponse>),
}
impl From<PythonRepositoryError> for super::RepositoryHandlerError {
    fn from(value: PythonRepositoryError) -> Self {
        super::RepositoryHandlerError::Other(Box::new(value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repository::{RepositoryType, test_helpers::test_storage};
    use ahash::HashMap;
    use serde_json::json;
    use uuid::Uuid;

    #[tokio::test]
    async fn create_new_python_hosted_accepts_default_config() {
        let storage = test_storage().await;
        let mut configs = HashMap::default();
        configs.insert(
            PythonRepositoryConfigType::get_type_static().to_string(),
            json!({ "type": "Hosted" }),
        );
        let result = PythonRepositoryType::default()
            .create_new("python-hosted".into(), Uuid::new_v4(), configs, storage)
            .await;
        let repository = result.expect("hosted repository to be created");
        assert_eq!(repository.repository_type, "python");
        assert!(
            repository
                .configs
                .contains_key(PythonRepositoryConfigType::get_type_static()),
            "expected python config to be persisted",
        );
    }

    #[tokio::test]
    async fn create_new_python_rejects_config_missing_type() {
        let storage = test_storage().await;
        let mut configs = HashMap::default();
        configs.insert(
            PythonRepositoryConfigType::get_type_static().to_string(),
            json!({}),
        );
        let result = PythonRepositoryType::default()
            .create_new("python-invalid".into(), Uuid::new_v4(), configs, storage)
            .await;
        match result {
            Err(RepositoryFactoryError::InvalidConfig(repository, message)) => {
                assert_eq!(repository, "python");
                assert!(
                    message.contains("type"),
                    "expected error message to mention missing type, got: {message}"
                );
            }
            other => panic!("expected invalid config error, got: {other:?}"),
        }
    }
}
impl From<PythonRepositoryError> for super::DynRepositoryHandlerError {
    fn from(value: PythonRepositoryError) -> Self {
        super::DynRepositoryHandlerError(Box::new(value))
    }
}
macro_rules! impl_from_other {
    ($from:ty) => {
        impl From<$from> for PythonRepositoryError {
            fn from(value: $from) -> Self {
                PythonRepositoryError::Other(Box::new(value))
            }
        }
    };
}
impl_from_other!(crate::repository::RepositoryHandlerError);
impl_from_other!(crate::utils::bad_request::BadRequestErrors);
impl_from_other!(nr_storage::StorageError);
impl_from_other!(sqlx::Error);
impl_from_other!(serde_json::Error);
impl_from_other!(crate::app::authentication::AuthenticationError);

impl crate::utils::IntoErrorResponse for PythonRepositoryError {
    fn into_response_boxed(self: Box<Self>) -> axum::response::Response {
        use axum::response::IntoResponse;
        use http::StatusCode;
        match *self {
            PythonRepositoryError::InvalidPath(message) => {
                crate::utils::ResponseBuilder::bad_request()
                    .body(message)
                    .into_response()
            }
            PythonRepositoryError::Unsupported(message) => crate::utils::ResponseBuilder::default()
                .status(StatusCode::NOT_IMPLEMENTED)
                .body(message),
            PythonRepositoryError::Other(inner) => inner.into_response_boxed(),
        }
    }
}

#[derive(Debug, Default)]
pub struct PythonRepositoryType;

impl RepositoryType for PythonRepositoryType {
    fn get_type(&self) -> &'static str {
        "python"
    }

    fn config_types(&self) -> Vec<&str> {
        vec![PythonRepositoryConfigType::get_type_static()]
    }

    fn get_description(&self) -> RepositoryTypeDescription {
        RepositoryTypeDescription {
            type_name: "python",
            name: "Python",
            description: "A Python package repository compatible with Nitro Repo clients.",
            documentation_url: Some("https://nitro-repo.kingtux.dev/repositoryTypes/python/"),
            is_stable: false,
            required_configs: vec![PythonRepositoryConfigType::get_type_static()],
        }
    }

    fn create_new(
        &self,
        name: String,
        uuid: uuid::Uuid,
        configs: HashMap<String, serde_json::Value>,
        _storage: DynStorage,
    ) -> BoxFuture<'static, Result<NewRepository, RepositoryFactoryError>> {
        Box::pin(async move {
            let config = configs
                .get(PythonRepositoryConfigType::get_type_static())
                .ok_or(RepositoryFactoryError::MissingConfig(
                    PythonRepositoryConfigType::get_type_static(),
                ))?
                .clone();
            let _: PythonRepositoryConfig = serde_json::from_value(config)
                .map_err(|err| RepositoryFactoryError::InvalidConfig("python", err.to_string()))?;
            Ok(NewRepository {
                name,
                uuid,
                repository_type: "python".to_string(),
                configs,
            })
        })
    }

    fn load_repo(
        &self,
        repo: DBRepository,
        storage: DynStorage,
        website: NitroRepo,
    ) -> BoxFuture<'static, Result<DynRepository, RepositoryFactoryError>> {
        Box::pin(async move {
            let config = DBRepositoryConfig::<PythonRepositoryConfig>::get_config(
                repo.id,
                PythonRepositoryConfigType::get_type_static(),
                &website.database,
            )
            .await?
            .map(|cfg| cfg.value.0)
            .unwrap_or_default();

            match config {
                PythonRepositoryConfig::Hosted => {
                    let hosted = hosted::PythonHosted::load(
                        website,
                        storage,
                        repo,
                        PythonRepositoryConfig::Hosted,
                    )
                    .await?;
                    Ok(DynRepository::Python(PythonRepository::Hosted(hosted)))
                }
                PythonRepositoryConfig::Proxy(proxy_config) => {
                    let proxy =
                        proxy::PythonProxy::load(website, storage, repo, proxy_config).await?;
                    Ok(DynRepository::Python(PythonRepository::Proxy(proxy)))
                }
            }
        })
    }
}
