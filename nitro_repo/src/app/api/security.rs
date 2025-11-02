use axum::{Json, extract::State, response::Response, routing::get};
use nr_core::user::permissions::HasPermissions;
use tracing::{error, instrument};
use utoipa::OpenApi;

use crate::{
    app::{NitroRepo, authentication::Authentication, config::SsoSettings},
    error::InternalError,
    utils::ResponseBuilder,
};

#[derive(OpenApi)]
#[openapi(
    paths(get_sso_settings, update_sso_settings),
    components(schemas(SsoSettings))
)]
pub struct SecurityAPI;

pub fn security_routes() -> axum::Router<NitroRepo> {
    axum::Router::new().route("/sso", get(get_sso_settings).put(update_sso_settings))
}

#[utoipa::path(
    get,
    path = "/sso",
    tag = "security",
    responses((status = 200, description = "Current SSO configuration", body = SsoSettings)),
    security(("session" = []))
)]
#[instrument(skip(auth, site), fields(project_module = "Security"))]
pub async fn get_sso_settings(
    auth: Authentication,
    State(site): State<NitroRepo>,
) -> Result<Response, InternalError> {
    if !auth.is_admin_or_system_manager() {
        return Ok(ResponseBuilder::forbidden().body("Administrator permissions required"));
    }

    let security = site.security_settings();
    let settings = security.sso.unwrap_or_else(SsoSettings::default);
    Ok(ResponseBuilder::ok().json(&settings))
}

#[utoipa::path(
    put,
    path = "/sso",
    tag = "security",
    request_body = SsoSettings,
    responses((status = 204, description = "SSO configuration updated")),
    security(("session" = []))
)]
#[instrument(skip(auth, site, settings), fields(project_module = "Security"))]
pub async fn update_sso_settings(
    auth: Authentication,
    State(site): State<NitroRepo>,
    Json(settings): Json<SsoSettings>,
) -> Result<Response, InternalError> {
    if !auth.is_admin_or_system_manager() {
        return Ok(ResponseBuilder::forbidden().body("Administrator permissions required"));
    }

    if let Err(err) = site.update_sso_settings(Some(settings)).await {
        error!(%err, "Failed to update SSO configuration");
        return Ok(
            ResponseBuilder::internal_server_error().body("Failed to update SSO configuration")
        );
    }

    Ok(ResponseBuilder::no_content().empty())
}
