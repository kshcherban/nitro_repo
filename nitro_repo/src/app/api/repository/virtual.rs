use axum::{
    Json, Router,
    extract::{Path, State},
    response::IntoResponse,
    routing::{get, put},
};
use nr_core::{
    database::entities::repository::{
        DBRepository, DBRepositoryConfig, DBVirtualRepositoryMember, GenericDBRepositoryConfig,
        NewVirtualRepositoryMember,
    },
    repository::config::RepositoryConfigType,
    user::permissions::{HasPermissions, RepositoryActions},
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    app::{
        NitroRepo,
        authentication::Authentication,
        responses::{MissingPermission, RepositoryNotFound},
    },
    error::{InternalError, OtherInternalError},
    repository::{
        DynRepository, Repository,
        npm::{
            NPMRegistry, NPMRegistryConfig, NPMRegistryConfigType,
            npm_virtual::{
                NpmVirtualConfig, VirtualRepositoryMemberConfig, VirtualResolutionOrder,
            },
        },
    },
    utils::ResponseBuilder,
};

pub fn virtual_routes() -> Router<NitroRepo> {
    Router::new()
        .route(
            "/{repository_id}/virtual/members",
            get(list_members).post(update_members),
        )
        .route(
            "/{repository_id}/virtual/resolution-order",
            put(update_resolution_order),
        )
}

#[derive(Debug, Serialize, ToSchema)]
pub struct VirtualMemberView {
    #[schema(value_type = String, format = "uuid")]
    pub repository_id: Uuid,
    pub repository_name: String,
    pub priority: u32,
    pub enabled: bool,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct VirtualConfigView {
    pub resolution_order: VirtualResolutionOrder,
    pub cache_ttl_seconds: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(value_type = Option<String>, format = "uuid")]
    pub publish_to: Option<Uuid>,
    pub members: Vec<VirtualMemberView>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateMembersRequest {
    pub members: Vec<VirtualRepositoryMemberConfig>,
    #[serde(default)]
    pub resolution_order: Option<VirtualResolutionOrder>,
    #[serde(default)]
    pub cache_ttl_seconds: Option<u64>,
    #[serde(default)]
    #[schema(value_type = Option<String>, format = "uuid")]
    pub publish_to: Option<Uuid>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdateResolutionOrderRequest {
    pub resolution_order: VirtualResolutionOrder,
    #[serde(default)]
    pub cache_ttl_seconds: Option<u64>,
    #[serde(default)]
    #[schema(value_type = Option<String>, format = "uuid")]
    pub publish_to: Option<Uuid>,
}

#[utoipa::path(
    get,
    path = "/api/repository/{repository_id}/virtual/members",
    params(("repository_id" = Uuid, Path, description = "Virtual repository id")),
    responses((status = 200, description = "Virtual members", body = VirtualConfigView))
)]
async fn list_members(
    State(site): State<NitroRepo>,
    auth: Authentication,
    Path(repository_id): Path<Uuid>,
) -> Result<axum::response::Response, InternalError> {
    if !auth
        .has_action(RepositoryActions::Edit, repository_id, site.as_ref())
        .await?
    {
        return Ok(MissingPermission::EditRepository(repository_id).into_response());
    }
    let Some(repository) = DBRepository::get_by_id(repository_id, site.as_ref()).await? else {
        return Ok(RepositoryNotFound::Uuid(repository_id).into_response());
    };
    if !repository.repository_type.eq_ignore_ascii_case("npm") {
        return Ok(ResponseBuilder::bad_request().body("Repository is not NPM"));
    }

    let config = DBRepositoryConfig::<NPMRegistryConfig>::get_config(
        repository_id,
        NPMRegistryConfigType::get_type_static(),
        site.as_ref(),
    )
    .await?
    .map(|cfg| cfg.value.0)
    .unwrap_or_default();

    let virtual_config = match config {
        NPMRegistryConfig::Virtual(cfg) => cfg,
        _ => return Ok(ResponseBuilder::bad_request().body("Repository is not virtual")),
    };

    let members = DBVirtualRepositoryMember::list_for_virtual(repository_id, site.as_ref()).await?;
    let views = hydrate_members(&members, &site).await?;

    let view = VirtualConfigView {
        resolution_order: virtual_config.resolution_order,
        cache_ttl_seconds: virtual_config.cache_ttl_seconds,
        publish_to: virtual_config.publish_to,
        members: views,
    };
    Ok(ResponseBuilder::ok().json(&view))
}

#[utoipa::path(
    post,
    path = "/api/repository/{repository_id}/virtual/members",
    request_body = UpdateMembersRequest,
    params(("repository_id" = Uuid, Path, description = "Virtual repository id")),
    responses((status = 200, description = "Updated virtual members", body = VirtualConfigView))
)]
async fn update_members(
    State(site): State<NitroRepo>,
    auth: Authentication,
    Path(repository_id): Path<Uuid>,
    Json(payload): Json<UpdateMembersRequest>,
) -> Result<axum::response::Response, InternalError> {
    if !auth
        .has_action(RepositoryActions::Edit, repository_id, site.as_ref())
        .await?
    {
        return Ok(MissingPermission::EditRepository(repository_id).into_response());
    }

    let Some(repository) = DBRepository::get_by_id(repository_id, site.as_ref()).await? else {
        return Ok(RepositoryNotFound::Uuid(repository_id).into_response());
    };
    if !repository.repository_type.eq_ignore_ascii_case("npm") {
        return Ok(ResponseBuilder::bad_request().body("Repository is not NPM"));
    }
    let config = DBRepositoryConfig::<NPMRegistryConfig>::get_config(
        repository_id,
        NPMRegistryConfigType::get_type_static(),
        site.as_ref(),
    )
    .await?
    .map(|cfg| cfg.value.0)
    .unwrap_or_default();
    let mut config = match config {
        NPMRegistryConfig::Virtual(cfg) => cfg,
        _ => return Ok(ResponseBuilder::bad_request().body("Repository is not virtual")),
    };

    if payload.members.is_empty() {
        return Ok(ResponseBuilder::bad_request().body("Member list cannot be empty"));
    }
    if has_duplicate_members(&payload.members) {
        return Ok(ResponseBuilder::bad_request().body("Duplicate member repositories provided"));
    }

    if let Some(order) = payload.resolution_order {
        config.resolution_order = order;
    }
    if let Some(ttl) = payload.cache_ttl_seconds {
        if ttl == 0 {
            return Ok(ResponseBuilder::bad_request().body("cache_ttl_seconds must be > 0"));
        }
        config.cache_ttl_seconds = ttl;
    }
    if let Some(publish_to) = payload.publish_to {
        config.publish_to = Some(publish_to);
    }

    config.member_repositories = payload.members;

    if !publish_target_is_member(&config) {
        return Ok(
            ResponseBuilder::bad_request().body("publish_to must reference a member repository")
        );
    }

    persist_virtual_config(repository_id, &config, site.as_ref()).await?;
    replace_members(repository_id, &config, site.as_ref()).await?;
    reload_runtime_virtual(&site, repository_id).await;

    list_members(State(site), auth, Path(repository_id)).await
}

#[utoipa::path(
    put,
    path = "/api/repository/{repository_id}/virtual/resolution-order",
    request_body = UpdateResolutionOrderRequest,
    params(("repository_id" = Uuid, Path, description = "Virtual repository id")),
    responses((status = 200, description = "Updated resolution order", body = VirtualConfigView))
)]
async fn update_resolution_order(
    State(site): State<NitroRepo>,
    auth: Authentication,
    Path(repository_id): Path<Uuid>,
    Json(payload): Json<UpdateResolutionOrderRequest>,
) -> Result<axum::response::Response, InternalError> {
    if !auth
        .has_action(RepositoryActions::Edit, repository_id, site.as_ref())
        .await?
    {
        return Ok(MissingPermission::EditRepository(repository_id).into_response());
    }

    let Some(repository) = DBRepository::get_by_id(repository_id, site.as_ref()).await? else {
        return Ok(RepositoryNotFound::Uuid(repository_id).into_response());
    };
    if !repository.repository_type.eq_ignore_ascii_case("npm") {
        return Ok(ResponseBuilder::bad_request().body("Repository is not NPM"));
    }

    let config = DBRepositoryConfig::<NPMRegistryConfig>::get_config(
        repository_id,
        NPMRegistryConfigType::get_type_static(),
        site.as_ref(),
    )
    .await?
    .map(|cfg| cfg.value.0)
    .unwrap_or_default();

    let mut config = match config {
        NPMRegistryConfig::Virtual(cfg) => cfg,
        _ => return Ok(ResponseBuilder::bad_request().body("Repository is not virtual")),
    };

    config.resolution_order = payload.resolution_order;
    if let Some(ttl) = payload.cache_ttl_seconds {
        config.cache_ttl_seconds = ttl.max(1);
    }
    config.publish_to = payload.publish_to.or(config.publish_to);

    if !publish_target_is_member(&config) {
        return Ok(
            ResponseBuilder::bad_request().body("publish_to must reference a member repository")
        );
    }

    persist_virtual_config(repository_id, &config, site.as_ref()).await?;
    reload_runtime_virtual(&site, repository_id).await;

    list_members(State(site), auth, Path(repository_id)).await
}

async fn hydrate_members(
    members: &[DBVirtualRepositoryMember],
    site: &NitroRepo,
) -> Result<Vec<VirtualMemberView>, InternalError> {
    let mut views = Vec::with_capacity(members.len());
    for member in members {
        let Some(repo) =
            DBRepository::get_by_id(member.member_repository_id, site.as_ref()).await?
        else {
            continue;
        };
        views.push(VirtualMemberView {
            repository_id: repo.id,
            repository_name: repo.name.to_string(),
            priority: member.priority.max(0) as u32,
            enabled: member.enabled,
        });
    }
    Ok(views)
}

async fn persist_virtual_config(
    repository_id: Uuid,
    config: &NpmVirtualConfig,
    database: &sqlx::PgPool,
) -> Result<(), InternalError> {
    let value = serde_json::to_value(NPMRegistryConfig::Virtual(config.clone()))
        .map_err(|err| InternalError::from(OtherInternalError::new(err)))?;
    GenericDBRepositoryConfig::add_or_update(
        repository_id,
        NPMRegistryConfigType::get_type_static().to_string(),
        value,
        database,
    )
    .await?;
    Ok(())
}

async fn replace_members(
    repository_id: Uuid,
    config: &NpmVirtualConfig,
    database: &sqlx::PgPool,
) -> Result<(), InternalError> {
    let members: Vec<_> = config
        .member_repositories
        .iter()
        .map(|member| NewVirtualRepositoryMember {
            member_repository_id: member.repository_id,
            priority: member.priority as i32,
            enabled: member.enabled,
        })
        .collect();
    DBVirtualRepositoryMember::replace_all(repository_id, &members, database).await?;
    Ok(())
}

fn has_duplicate_members(members: &[VirtualRepositoryMemberConfig]) -> bool {
    let mut seen = HashSet::new();
    members
        .iter()
        .any(|member| !seen.insert(member.repository_id))
}

fn publish_target_is_member(config: &NpmVirtualConfig) -> bool {
    match config.publish_to {
        Some(target) => config
            .member_repositories
            .iter()
            .any(|member| member.repository_id == target),
        None => true,
    }
}

async fn reload_runtime_virtual(site: &NitroRepo, repository_id: Uuid) {
    if let Some(DynRepository::NPM(NPMRegistry::Virtual(virtual_repo))) =
        site.get_repository(repository_id)
    {
        if let Err(err) = virtual_repo.reload().await {
            tracing::warn!(repository = %repository_id, error = %err, "Failed to reload virtual repository after config update");
        }
    }
}
