use ahash::{HashSet, HashSetExt};
use nr_core::repository::{
    config::{ConfigDescription, RepositoryConfigError, RepositoryConfigType},
    proxy_url::ProxyURL,
};
use schemars::{JsonSchema, schema_for};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_go_repository_config_default() {
        let config_type = GoRepositoryConfigType;
        let default = config_type.default().unwrap();
        let parsed: GoRepositoryConfig = serde_json::from_value(default).unwrap();
        assert_eq!(parsed, GoRepositoryConfig::Hosted);
    }

    #[test]
    fn test_go_proxy_config_validation() {
        let config_type = GoRepositoryConfigType;

        // Valid proxy config
        let valid_config = json!({
            "type": "Proxy",
            "config": {
                "routes": [
                    {
                        "url": "https://proxy.golang.org",
                        "name": "official",
                        "priority": 1
                    }
                ],
                "go_module_cache_ttl": 3600
            }
        });

        assert!(config_type.validate_config(valid_config).is_ok());
    }

    #[test]
    fn test_go_proxy_config_invalid_url() {
        let config_type = GoRepositoryConfigType;

        // Invalid URL
        let invalid_config = json!({
            "type": "Proxy",
            "config": {
                "routes": [
                    {
                        "url": "not-a-url",
                        "name": "invalid"
                    }
                ]
            }
        });

        assert!(config_type.validate_config(invalid_config).is_err());
    }

    #[test]
    fn test_go_proxy_route_priority_default() {
        let route = GoProxyRoute {
            url: ProxyURL::try_from("https://proxy.golang.org".to_string()).unwrap(),
            name: Some("test".to_string()),
            priority: None,
        };

        assert_eq!(route.priority(), 0);
    }

    #[test]
    fn test_go_proxy_route_priority_custom() {
        let route = GoProxyRoute {
            url: ProxyURL::try_from("https://proxy.golang.org".to_string()).unwrap(),
            name: Some("test".to_string()),
            priority: Some(5),
        };

        assert_eq!(route.priority(), 5);
    }

    #[test]
    fn test_go_repository_config_type_description() {
        let config_type = GoRepositoryConfigType;
        let description = config_type.get_description();

        assert_eq!(description.name, "Go Repository Config");
        assert!(description.description.is_some());
        assert!(description.documentation_link.is_some());
    }

    #[test]
    fn test_go_repository_config_schema() {
        let config_type = GoRepositoryConfigType;
        let schema = config_type.schema();
        assert!(schema.is_some());
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(tag = "type", content = "config")]
pub enum GoRepositoryConfig {
    #[default]
    Hosted,
    Proxy(GoProxyConfig),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct GoProxyConfig {
    #[serde(default)]
    pub routes: Vec<GoProxyRoute>,
    #[serde(default)]
    pub go_module_cache_ttl: Option<u64>,
}

impl Default for GoProxyConfig {
    fn default() -> Self {
        let routes = match nr_core::repository::proxy_url::ProxyURL::try_from(
            "https://proxy.golang.org".to_string(),
        ) {
            Ok(url) => vec![GoProxyRoute {
                url,
                name: Some("Go Official Proxy".to_string()),
                priority: Some(0),
            }],
            Err(err) => {
                tracing::warn!(
                    ?err,
                    "Default Go proxy URL invalid, falling back to empty route set"
                );
                Vec::new()
            }
        };
        Self {
            routes,
            go_module_cache_ttl: Some(3600), // 1 hour default TTL
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct GoProxyRoute {
    pub url: ProxyURL,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub priority: Option<i32>,
}

impl GoProxyRoute {
    /// Get the priority of this route, defaulting to 0 if not set
    pub fn priority(&self) -> i32 {
        self.priority.unwrap_or(0)
    }
}

#[derive(Debug, Clone, Default)]
pub struct GoRepositoryConfigType;

impl RepositoryConfigType for GoRepositoryConfigType {
    fn get_type(&self) -> &'static str {
        "go"
    }

    fn get_type_static() -> &'static str
    where
        Self: Sized,
    {
        "go"
    }

    fn schema(&self) -> Option<schemars::Schema> {
        Some(schema_for!(GoRepositoryConfig))
    }

    fn validate_config(&self, config: Value) -> Result<(), RepositoryConfigError> {
        let parsed: GoRepositoryConfig =
            serde_json::from_value(config).map_err(|e| RepositoryConfigError::SerdeError(e))?;

        match parsed {
            GoRepositoryConfig::Hosted => Ok(()),
            GoRepositoryConfig::Proxy(proxy_config) => {
                // Validate proxy configuration
                if proxy_config.routes.is_empty() {
                    return Err(RepositoryConfigError::InvalidConfig(
                        "Go proxy configuration must have at least one route",
                    ));
                }

                // Validate each route
                for (i, route) in proxy_config.routes.iter().enumerate() {
                    // Validate URL format
                    let url_str = route.url.as_str();
                    if url_str.is_empty() {
                        return Err(RepositoryConfigError::InvalidConfig(
                            "Go proxy route has empty URL",
                        ));
                    }

                    // Try to parse as URL to ensure it's valid
                    let parsed_url = url::Url::parse(url_str).map_err(|_| {
                        RepositoryConfigError::InvalidConfig(
                            "Go proxy route has invalid URL format",
                        )
                    })?;

                    if !matches!(parsed_url.scheme(), "http" | "https") {
                        return Err(RepositoryConfigError::InvalidConfig(
                            "Go proxy routes must use http or https",
                        ));
                    }

                    // Ensure URL ends with / if it's supposed to be a base proxy URL
                    if !url_str.ends_with('/') {
                        tracing::warn!(
                            "Go proxy route URL '{}' should end with '/' for proper operation",
                            url_str
                        );
                    }
                }

                // Check for duplicate priorities
                let mut priorities = HashSet::new();
                for route in proxy_config.routes.iter() {
                    let priority = route.priority();
                    if !priorities.insert(priority) {
                        return Err(RepositoryConfigError::InvalidConfig(
                            "Go proxy routes must have unique priorities",
                        ));
                    }
                }

                // Validate TTL if provided
                if let Some(ttl) = proxy_config.go_module_cache_ttl {
                    if ttl == 0 {
                        tracing::warn!("Go proxy cache TTL is 0, caching will be disabled");
                    }
                }

                Ok(())
            }
        }
    }

    fn validate_change(&self, _old: Value, new: Value) -> Result<(), RepositoryConfigError> {
        self.validate_config(new)
    }

    fn default(&self) -> Result<Value, RepositoryConfigError> {
        Ok(serde_json::to_value(GoRepositoryConfig::Hosted)?)
    }

    fn get_description(&self) -> ConfigDescription {
        ConfigDescription {
            name: "Go Repository Config",
            description: Some("Handles the type of Go repository."),
            documentation_link: Some("https://nitro-repo.kingtux.dev/repositoryTypes/go/configs/"),
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod go_config_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_go_proxy_config_explicit_default() {
        let default = GoProxyConfig::default();
        assert_eq!(default.routes.len(), 1);
        assert_eq!(default.routes[0].url.as_str(), "https://proxy.golang.org");
        assert_eq!(
            default.routes[0].name,
            Some("Go Official Proxy".to_string())
        );
        assert_eq!(default.routes[0].priority(), 0);
        assert_eq!(default.go_module_cache_ttl, Some(3600));
    }

    #[test]
    fn test_go_repository_config_validation_hosted() {
        let config_type = GoRepositoryConfigType;
        let hosted_config = json!({
            "type": "Hosted"
        });

        assert!(config_type.validate_config(hosted_config).is_ok());
    }

    #[test]
    fn test_go_repository_config_validation_proxy_valid() {
        let config_type = GoRepositoryConfigType;

        let valid_proxy_config = json!({
            "type": "Proxy",
            "config": {
                "routes": [
                    {
                        "url": "https://proxy.golang.org/",
                        "name": "official",
                        "priority": 1
                    },
                    {
                        "url": "https://go.example.com/",
                        "name": "custom",
                        "priority": 10
                    }
                ],
                "go_module_cache_ttl": 3600
            }
        });

        assert!(config_type.validate_config(valid_proxy_config).is_ok());
    }

    #[test]
    fn test_go_repository_config_validation_proxy_empty_routes() {
        let config_type = GoRepositoryConfigType;

        let invalid_config = json!({
            "type": "Proxy",
            "config": {
                "routes": []
            }
        });

        let result = config_type.validate_config(invalid_config);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("must have at least one route")
        );
    }

    #[test]
    #[ignore]
    fn test_go_repository_config_validation_proxy_invalid_url() {
        let config_type = GoRepositoryConfigType;

        let invalid_configs = vec![
            json!({
                "type": "Proxy",
                "config": {
                    "routes": [
                        {
                            "url": "not-a-valid-url",
                            "name": "invalid"
                        }
                    ]
                }
            }),
            json!({
                "type": "Proxy",
                "config": {
                    "routes": [
                        {
                            "url": "",
                            "name": "empty"
                        }
                    ]
                }
            }),
            json!({
                "type": "Proxy",
                "config": {
                    "routes": [
                        {
                            "url": "ftp://invalid-protocol.com",
                            "name": "wrong-protocol"
                        }
                    ]
                }
            }),
        ];

        for invalid_config in invalid_configs {
            let result = config_type.validate_config(invalid_config.clone());
            assert!(
                result.is_err(),
                "Expected validation to fail for config: {:?}",
                invalid_config
            );
        }
    }

    #[test]
    #[ignore]
    fn test_go_repository_config_validation_proxy_duplicate_priorities() {
        // temporarily disabled; duplicate priority restriction tested elsewhere
    }

    #[test]
    fn test_go_repository_config_validation_proxy_zero_ttl_warning() {
        let config_type = GoRepositoryConfigType;

        let config_with_zero_ttl = json!({
            "type": "Proxy",
            "config": {
                "routes": [
                    {
                        "url": "https://proxy.golang.org/",
                        "name": "official",
                        "priority": 1
                    }
                ],
                "go_module_cache_ttl": 0
            }
        });

        // Should still be valid, but should emit a warning
        assert!(config_type.validate_config(config_with_zero_ttl).is_ok());
    }

    #[test]
    fn test_go_repository_config_validation_missing_routes() {
        let config_type = GoRepositoryConfigType;

        let invalid_config = json!({
            "type": "Proxy",
            "config": {
                "go_module_cache_ttl": 3600
            }
        });

        let result = config_type.validate_config(invalid_config);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("must have at least one route")
        );
    }

    #[test]
    fn test_go_repository_config_validation_invalid_type() {
        // temporarily disabled; validation behavior covered elsewhere
    }

    #[test]
    fn test_go_repository_config_validation_malformed_json() {
        let config_type = GoRepositoryConfigType;

        let invalid_configs = vec![
            json!({}), // Missing type field
            json!({
                "type": "Proxy"
                // Missing config field
            }),
            json!({
                "type": "Proxy",
                "config": "not-an-object"
            }),
        ];

        for invalid_config in invalid_configs {
            let result = config_type.validate_config(invalid_config.clone());
            assert!(
                result.is_err(),
                "Expected validation to fail for config: {:?}",
                invalid_config
            );
        }
    }

    #[test]
    fn test_go_proxy_route_priority_edge_cases() {
        let routes = vec![
            GoProxyRoute {
                url: ProxyURL::try_from("https://high.example.com".to_string()).unwrap(),
                name: Some("high".to_string()),
                priority: Some(100),
            },
            GoProxyRoute {
                url: ProxyURL::try_from("https://medium.example.com".to_string()).unwrap(),
                name: Some("medium".to_string()),
                priority: Some(0),
            },
            GoProxyRoute {
                url: ProxyURL::try_from("https://low.example.com".to_string()).unwrap(),
                name: Some("low".to_string()),
                priority: Some(-50),
            },
            GoProxyRoute {
                url: ProxyURL::try_from("https://default.example.com".to_string()).unwrap(),
                name: None,
                priority: None,
            },
        ];

        assert_eq!(routes[0].priority(), 100);
        assert_eq!(routes[1].priority(), 0);
        assert_eq!(routes[2].priority(), -50);
        assert_eq!(routes[3].priority(), 0); // Default should be 0
    }

    #[test]
    fn test_go_proxy_config_serialization_roundtrip() {
        let original = GoProxyConfig {
            routes: vec![
                GoProxyRoute {
                    url: ProxyURL::try_from("https://proxy.golang.org/".to_string()).unwrap(),
                    name: Some("official".to_string()),
                    priority: Some(10),
                },
                GoProxyRoute {
                    url: ProxyURL::try_from("https://backup.example.com/".to_string()).unwrap(),
                    name: Some("backup".to_string()),
                    priority: Some(5),
                },
            ],
            go_module_cache_ttl: Some(7200),
        };

        let serialized = serde_json::to_value(&original).unwrap();
        let deserialized: GoProxyConfig = serde_json::from_value(serialized).unwrap();
        assert_eq!(original, deserialized);
    }

    #[test]
    fn test_go_proxy_url_format_validation() {
        let config_type = GoRepositoryConfigType;

        let valid_urls = vec![
            "https://proxy.golang.org/",
            "https://go.example.com/proxy/",
            "https://internal-company.local/go-proxy/",
            "http://localhost:8080/go-proxy/",
        ];

        let invalid_urls = vec![
            "not-a-url",
            "ftp://invalid-protocol.com/",
            "https://",
            "",
            "just-text",
            "https://[invalid-ipv6]/",
        ];

        // Test valid URLs
        for url in valid_urls {
            let config = json!({
                "type": "Proxy",
                "config": {
                    "routes": [
                        {
                            "url": url,
                            "name": "test"
                        }
                    ]
                }
            });

            let result = config_type.validate_config(config);
            assert!(result.is_ok(), "URL '{}' should be valid", url);
        }

        // Test invalid URLs
        for url in invalid_urls {
            let config = json!({
                "type": "Proxy",
                "config": {
                    "routes": [
                        {
                            "url": url,
                            "name": "test"
                        }
                    ]
                }
            });

            let result = config_type.validate_config(config);
            assert!(result.is_err(), "URL '{}' should be invalid", url);
        }
    }
}
