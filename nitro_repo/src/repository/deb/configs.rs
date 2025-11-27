use nr_core::repository::config::{ConfigDescription, RepositoryConfigError, RepositoryConfigType};
use schemars::{JsonSchema, schema_for};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct DebRepositoryConfig {
    pub distributions: Vec<String>,
    pub components: Vec<String>,
    pub architectures: Vec<String>,
}

impl Default for DebRepositoryConfig {
    fn default() -> Self {
        Self {
            distributions: vec!["stable".into()],
            components: vec!["main".into()],
            architectures: vec!["amd64".into(), "all".into()],
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct DebRepositoryConfigType;

impl RepositoryConfigType for DebRepositoryConfigType {
    fn get_type(&self) -> &'static str {
        "deb"
    }

    fn get_type_static() -> &'static str
    where
        Self: Sized,
    {
        "deb"
    }

    fn schema(&self) -> Option<schemars::Schema> {
        Some(schema_for!(DebRepositoryConfig))
    }

    fn validate_config(&self, config: Value) -> Result<(), RepositoryConfigError> {
        let decoded: DebRepositoryConfig = serde_json::from_value(config)?;
        validate_identifier_list(
            &decoded.distributions,
            "At least one distribution is required",
        )?;
        validate_identifier_list(&decoded.components, "At least one component is required")?;
        validate_identifier_list(
            &decoded.architectures,
            "At least one architecture is required",
        )?;
        Ok(())
    }

    fn validate_change(&self, _old: Value, new: Value) -> Result<(), RepositoryConfigError> {
        self.validate_config(new)
    }

    fn default(&self) -> Result<Value, RepositoryConfigError> {
        Ok(serde_json::to_value(DebRepositoryConfig::default())?)
    }

    fn get_description(&self) -> ConfigDescription {
        ConfigDescription {
            name: "Debian Repository Config".into(),
            description: Some("Configure distributions, components, and architectures.".into()),
            documentation_link: None,
            ..Default::default()
        }
    }
}

fn validate_identifier_list(
    values: &[String],
    empty_error: &'static str,
) -> Result<(), RepositoryConfigError> {
    if values.is_empty() {
        return Err(RepositoryConfigError::InvalidConfig(empty_error));
    }
    for value in values {
        if !is_valid_identifier(value) {
            return Err(RepositoryConfigError::InvalidConfig(
                "Values may only contain alphanumeric characters, '-', '_' or '.'",
            ));
        }
    }
    Ok(())
}

fn is_valid_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
}

#[cfg(test)]
mod tests;
