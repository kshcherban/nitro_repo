use nr_core::repository::config::{ConfigDescription, RepositoryConfigError, RepositoryConfigType};
use schemars::{JsonSchema, schema_for};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum HelmRepositoryMode {
    Http,
    Oci,
    Hybrid,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct HelmRepositoryConfig {
    #[serde(default)]
    pub overwrite: bool,
    #[serde(default)]
    pub index_cache_ttl: Option<u64>,
    #[serde(default = "HelmRepositoryMode::default_mode")]
    pub mode: HelmRepositoryMode,
    #[serde(default)]
    pub public_base_url: Option<String>,
    #[serde(default)]
    pub max_chart_size: Option<usize>,
    #[serde(default)]
    pub max_file_count: Option<usize>,
}

impl HelmRepositoryMode {
    fn default_mode() -> Self {
        HelmRepositoryMode::Hybrid
    }
}

impl Default for HelmRepositoryConfig {
    fn default() -> Self {
        Self {
            overwrite: false,
            index_cache_ttl: Some(300),
            mode: HelmRepositoryMode::Hybrid,
            public_base_url: None,
            max_chart_size: Some(10 * 1024 * 1024),
            max_file_count: Some(1024),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct HelmRepositoryConfigType;

impl RepositoryConfigType for HelmRepositoryConfigType {
    fn get_type(&self) -> &'static str {
        "helm"
    }

    fn get_type_static() -> &'static str
    where
        Self: Sized,
    {
        "helm"
    }

    fn schema(&self) -> Option<schemars::Schema> {
        Some(schema_for!(HelmRepositoryConfig))
    }

    fn get_description(&self) -> ConfigDescription {
        ConfigDescription {
            name: "Helm Repository Config",
            description: Some(
                "Configures Nitro's Helm repository behavior including index caching and modes",
            ),
            documentation_link: Some("https://nitro-repo.kingtux.dev/repositoryTypes/helm/config/"),
            ..Default::default()
        }
    }

    fn default(&self) -> Result<Value, RepositoryConfigError> {
        serde_json::to_value(HelmRepositoryConfig::default())
            .map_err(RepositoryConfigError::SerdeError)
    }

    fn validate_config(&self, config: Value) -> Result<(), RepositoryConfigError> {
        let parsed: HelmRepositoryConfig =
            serde_json::from_value(config).map_err(RepositoryConfigError::SerdeError)?;

        if let Some(url) = parsed.public_base_url.as_deref() {
            let parsed_url = url::Url::parse(url).map_err(|_| {
                RepositoryConfigError::InvalidConfig(
                    "public_base_url must be a fully-qualified URL",
                )
            })?;
            let scheme = parsed_url.scheme();
            if scheme != "http" && scheme != "https" {
                return Err(RepositoryConfigError::InvalidConfig(
                    "public_base_url must use http or https",
                ));
            }
        }

        if let Some(size) = parsed.max_chart_size {
            if size == 0 {
                return Err(RepositoryConfigError::InvalidConfig(
                    "max_chart_size must be greater than zero",
                ));
            }
        }

        if let Some(count) = parsed.max_file_count {
            if count == 0 {
                return Err(RepositoryConfigError::InvalidConfig(
                    "max_file_count must be greater than zero",
                ));
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn default_config_sets_expected_values() {
        let config_type = HelmRepositoryConfigType;
        let default_value = config_type.default().expect("default config should build");
        let parsed: HelmRepositoryConfig = serde_json::from_value(default_value).unwrap();
        assert!(!parsed.overwrite);
        assert_eq!(parsed.mode, HelmRepositoryMode::Hybrid);
        assert_eq!(parsed.max_chart_size, Some(10 * 1024 * 1024));
        assert_eq!(parsed.max_file_count, Some(1024));
    }

    #[test]
    fn validates_public_base_url_format() {
        let config_type = HelmRepositoryConfigType;
        let valid = json!({
            "overwrite": true,
            "mode": "hybrid",
            "public_base_url": "https://charts.example.com/helm",
            "max_chart_size": 20971520
        });
        assert!(config_type.validate_config(valid).is_ok());

        let invalid = json!({
            "mode": "hybrid",
            "public_base_url": "not a url"
        });
        assert!(config_type.validate_config(invalid).is_err());
    }

    #[test]
    fn rejects_negative_chart_limits() {
        let config_type = HelmRepositoryConfigType;
        let invalid = json!({
            "max_chart_size": -1,
            "max_file_count": -10
        });
        assert!(config_type.validate_config(invalid).is_err());
    }
}
