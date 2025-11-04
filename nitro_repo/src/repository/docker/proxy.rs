use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Docker proxy configuration for upstream registries
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DockerProxyConfig {
    /// Upstream registry URL (e.g., "https://registry-1.docker.io")
    pub upstream_url: String,

    /// Optional authentication for upstream registry
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upstream_auth: Option<DockerProxyAuth>,

    /// Enable caching of pulled images
    #[serde(default = "default_cache_enabled")]
    pub cache_enabled: bool,
}

fn default_cache_enabled() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DockerProxyAuth {
    pub username: String,
    pub password: String,
}

// Placeholder for future proxy implementation
#[derive(Debug, Clone)]
pub struct DockerProxy {
    // Will be implemented in future iterations
}
