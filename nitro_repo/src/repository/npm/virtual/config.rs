use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, ToSchema, PartialEq, Eq)]
pub struct NpmVirtualConfig {
    #[serde(default)]
    pub member_repositories: Vec<VirtualRepositoryMemberConfig>,
    #[serde(default)]
    pub resolution_order: VirtualResolutionOrder,
    #[serde(default = "default_cache_ttl_seconds")]
    pub cache_ttl_seconds: u64,
    #[serde(default)]
    #[schemars(with = "Option<String>")]
    #[schema(value_type = Option<String>, format = "uuid")]
    pub publish_to: Option<Uuid>,
}

impl Default for NpmVirtualConfig {
    fn default() -> Self {
        Self {
            member_repositories: Vec::new(),
            resolution_order: VirtualResolutionOrder::default(),
            cache_ttl_seconds: default_cache_ttl_seconds(),
            publish_to: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, ToSchema, PartialEq, Eq)]
pub struct VirtualRepositoryMemberConfig {
    #[schemars(with = "String")]
    #[schema(value_type = String, format = "uuid")]
    pub repository_id: Uuid,
    pub repository_name: String,
    #[serde(default)]
    pub priority: u32,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, ToSchema, PartialEq, Eq, Default)]
pub enum VirtualResolutionOrder {
    #[default]
    Priority,
}

const fn default_enabled() -> bool {
    true
}

const fn default_cache_ttl_seconds() -> u64 {
    60
}
