use nr_core::repository::config::{ConfigDescription, RepositoryConfigError, RepositoryConfigType};
use schemars::{JsonSchema, schema_for};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(tag = "type")]
pub enum PhpRepositoryConfig {
    #[default]
    Hosted,
}

#[derive(Debug, Clone, Default)]
pub struct PhpRepositoryConfigType;

impl RepositoryConfigType for PhpRepositoryConfigType {
    fn get_type(&self) -> &'static str {
        "php"
    }

    fn get_type_static() -> &'static str
    where
        Self: Sized,
    {
        "php"
    }

    fn schema(&self) -> Option<schemars::Schema> {
        Some(schema_for!(PhpRepositoryConfig))
    }

    fn validate_config(&self, config: Value) -> Result<(), RepositoryConfigError> {
        serde_json::from_value::<PhpRepositoryConfig>(config)?;
        Ok(())
    }

    fn validate_change(&self, _old: Value, _new: Value) -> Result<(), RepositoryConfigError> {
        Ok(())
    }

    fn default(&self) -> Result<Value, RepositoryConfigError> {
        Ok(serde_json::to_value(PhpRepositoryConfig::Hosted)?)
    }

    fn get_description(&self) -> ConfigDescription {
        ConfigDescription {
            name: "PHP Repository Config",
            description: Some("Handles the type of PHP (Composer) repository."),
            documentation_link: Some("https://nitro-repo.kingtux.dev/repositoryTypes/php/configs/"),
            ..Default::default()
        }
    }
}
