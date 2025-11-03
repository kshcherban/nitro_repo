use std::{net::SocketAddr, str::FromStr};

use axum::{
    body::Body,
    extract::{ConnectInfo, Path, Query, State},
    http::{
        StatusCode,
        header::{LOCATION, SET_COOKIE},
    },
    response::{IntoResponse, Response},
};
use axum_extra::{
    TypedHeader,
    extract::cookie::{Cookie, Expiration},
    headers::UserAgent,
};
use chrono::Duration;
use jsonwebtoken::dangerous::insecure_decode;
use nr_core::database::entities::user::{UserSafeData, UserType};
use oauth2::AuthorizationCode;
use serde::{Deserialize, Serialize};
use tracing::{error, instrument, warn};
use utoipa::{IntoParams, ToSchema};

use crate::{
    app::{
        NitroRepo,
        authentication::oauth::OAuth2ServiceError,
        config::{OAuth2GroupRoleMapping, OAuth2ProviderKind, OAuth2Settings},
    },
    error::InternalError,
    utils::{ResponseBuilder, api_error_response::APIErrorResponse},
};

use super::sso::{SsoPrincipal, create_user, normalize_username, sanitize_redirect};

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct OAuthAuthorizeQuery {
    redirect: Option<String>,
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct OAuthCallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
    redirect: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct OAuthProviderDescriptor {
    pub provider: String,
    pub login_path: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct OAuthProvidersResponse {
    pub providers: Vec<OAuthProviderDescriptor>,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Deserialize)]
struct IdTokenClaims {
    sub: String,
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    preferred_username: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    given_name: Option<String>,
    #[serde(default)]
    family_name: Option<String>,
    #[serde(default)]
    roles: Option<Vec<String>>,
    #[serde(default)]
    groups: Option<Vec<String>>,
}

#[utoipa::path(
    get,
    path = "/oauth2/providers",
    responses((status = 200, body = OAuthProvidersResponse)),
    tag = "user",
    security(())
)]
#[instrument(skip(site), fields(project_module = "Authentication", auth.oauth2 = true))]
pub async fn list_providers(State(site): State<NitroRepo>) -> Result<Response, InternalError> {
    let Some(settings) = site.oauth2_settings() else {
        return Ok(ResponseBuilder::not_found().body("OAuth2 login is not enabled"));
    };

    let mut providers = Vec::new();
    if settings.google.is_some() {
        providers.push(OAuthProviderDescriptor {
            provider: OAuth2ProviderKind::Google.to_string(),
            login_path: format!("{}/{}", settings.login_path.trim_end_matches('/'), "google"),
        });
    }
    if settings.microsoft.is_some() {
        providers.push(OAuthProviderDescriptor {
            provider: OAuth2ProviderKind::Microsoft.to_string(),
            login_path: format!(
                "{}/{}",
                settings.login_path.trim_end_matches('/'),
                "microsoft"
            ),
        });
    }

    Ok(ResponseBuilder::ok().json(&OAuthProvidersResponse { providers }))
}

#[utoipa::path(
    get,
    path = "/oauth2/login/{provider}",
    params(
        ("provider" = String, Path, description = "OAuth2 provider identifier"),
        OAuthAuthorizeQuery
    ),
    responses(
        (status = 303, description = "Redirect to external provider"),
        (status = 404, description = "OAuth2 provider not configured")
    ),
    tag = "user",
    security(())
)]
#[instrument(
    skip(site, query),
    fields(project_module = "Authentication", auth.oauth2 = true, auth.oauth2.provider = %provider)
)]
pub async fn authorize(
    State(site): State<NitroRepo>,
    Path(provider): Path<String>,
    Query(query): Query<OAuthAuthorizeQuery>,
) -> Result<Response, InternalError> {
    let Some(service) = site.oauth2_service() else {
        return Ok(ResponseBuilder::not_found().body("OAuth2 login is not enabled"));
    };
    let provider_kind = match OAuth2ProviderKind::from_str(&provider) {
        Ok(kind) => kind,
        Err(_) => {
            return Ok(ResponseBuilder::not_found().body("Unknown OAuth2 provider"));
        }
    };

    let base_url = resolve_base_url(&site);

    let auth_redirect = match service.begin_authorization(
        provider_kind,
        base_url.as_deref(),
        query.redirect.clone(),
    ) {
        Ok(redirect) => redirect,
        Err(err) => {
            warn!(%err, "Failed to start OAuth2 authorization");
            return Ok(oauth_service_error_response(err));
        }
    };

    let response = Response::builder()
        .status(StatusCode::SEE_OTHER)
        .header(LOCATION, auth_redirect.authorization_url.as_str())
        .body(Body::empty())
        .expect("Failed to build redirect response");

    Ok(response)
}

#[utoipa::path(
    get,
    path = "/oauth2/callback",
    params(OAuthCallbackQuery),
    responses(
        (status = 303, description = "OAuth2 login completed"),
        (status = 400, description = "OAuth2 provider returned an error"),
        (status = 404, description = "OAuth2 login disabled")
    ),
    tag = "user",
    security(())
)]
#[instrument(
    skip(site, query, user_agent),
    fields(project_module = "Authentication", auth.oauth2 = true)
)]
pub async fn callback(
    State(site): State<NitroRepo>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    user_agent: Option<TypedHeader<UserAgent>>,
    Query(query): Query<OAuthCallbackQuery>,
) -> Result<Response, InternalError> {
    if let Some(error) = query.error.as_ref() {
        let api_error: APIErrorResponse<String, ()> = APIErrorResponse {
            message: error.clone().into(),
            details: query.error_description.clone(),
            error: None,
        };
        return Ok(ResponseBuilder::bad_request().json(&api_error));
    }

    let code = match query.code.as_ref() {
        Some(code) if !code.is_empty() => code,
        _ => {
            let api_error: APIErrorResponse<(), ()> = APIErrorResponse {
                message: "Missing authorization code".into(),
                details: None,
                error: None,
            };
            return Ok(ResponseBuilder::bad_request().json(&api_error));
        }
    };
    let state = match query.state.as_ref() {
        Some(state) if !state.is_empty() => state,
        _ => {
            let api_error: APIErrorResponse<(), ()> = APIErrorResponse {
                message: "Missing state parameter".into(),
                details: None,
                error: None,
            };
            return Ok(ResponseBuilder::bad_request().json(&api_error));
        }
    };

    let Some(service) = site.oauth2_service() else {
        return Ok(ResponseBuilder::not_found().body("OAuth2 login is not enabled"));
    };
    let base_url = resolve_base_url(&site);

    let exchange = match service
        .exchange_code(
            base_url.as_deref(),
            AuthorizationCode::new(code.clone()),
            state,
        )
        .await
    {
        Ok(exchange) => exchange,
        Err(err) => {
            warn!(%err, "OAuth2 code exchange failed");
            return Ok(oauth_service_error_response(err));
        }
    };

    let id_token = match exchange.token_response.extra_fields().id_token.as_ref() {
        Some(token) => token,
        None => {
            warn!("OAuth2 provider did not return an id_token");
            let api_error: APIErrorResponse<(), ()> = APIErrorResponse {
                message: "OAuth2 provider did not return an id_token".into(),
                details: None,
                error: None,
            };
            return Ok(ResponseBuilder::internal_server_error().json(&api_error));
        }
    };

    let claims = match insecure_decode::<IdTokenClaims>(id_token) {
        Ok(token_data) => token_data.claims,
        Err(err) => {
            error!(%err, "Failed to decode id_token claims");
            let api_error: APIErrorResponse<(), ()> = APIErrorResponse {
                message: "Unable to decode identity token".into(),
                details: None,
                error: None,
            };
            return Ok(ResponseBuilder::internal_server_error().json(&api_error));
        }
    };

    let Some(oauth_settings) = site.oauth2_settings_raw() else {
        return Ok(ResponseBuilder::not_found().body("OAuth2 configuration missing"));
    };

    let principal = build_principal(&claims);
    let user = match resolve_oauth_user(&site, &oauth_settings, &principal).await {
        Ok(user) => user,
        Err(response) => return Ok(response),
    };

    if !user.active {
        warn!(user_id = user.id, "Inactive user attempted OAuth2 login");
        let api_error: APIErrorResponse<(), ()> = APIErrorResponse {
            message: "User account is disabled".into(),
            details: None,
            error: None,
        };
        return Ok(ResponseBuilder::forbidden().json(&api_error));
    }

    let mut roles = extract_roles(exchange.provider, &claims);
    let mapped_roles = map_roles_from_claims(
        exchange.provider,
        &roles,
        &oauth_settings.group_role_mappings,
    );
    roles.extend(mapped_roles);
    roles.retain(|role| !role.trim().is_empty());
    roles.sort();
    roles.dedup();

    let email = user.email.to_string();
    if let Err(err) = site.apply_oauth_roles(&email, &roles).await {
        warn!(%err, user_id = user.id, "Failed to apply OAuth2 RBAC roles");
    }

    let user_agent = user_agent
        .map(|ua| ua.to_string())
        .unwrap_or_else(|| "Nitro Repo OAuth2".to_string());
    let ip = addr.ip().to_string();
    let session =
        match site
            .session_manager
            .create_session(user.id, user_agent, ip, Duration::days(1))
        {
            Ok(session) => session,
            Err(err) => {
                error!(%err, "Failed to create session for OAuth2 login");
                return Ok(err.into_response());
            }
        };

    let cookie = Cookie::build(("session", session.session_id.clone()))
        .secure(true)
        .same_site(axum_extra::extract::cookie::SameSite::None)
        .path("/")
        .http_only(true)
        .expires(Expiration::Session)
        .build();

    let redirect_header =
        sanitize_redirect(exchange.redirect.as_deref().or(query.redirect.as_deref()));

    let response = Response::builder()
        .status(StatusCode::SEE_OTHER)
        .header(SET_COOKIE, cookie.encoded().to_string())
        .header(LOCATION, redirect_header)
        .body(Body::empty())
        .expect("Failed to build OAuth2 callback response");

    Ok(response)
}

fn resolve_base_url(site: &NitroRepo) -> Option<String> {
    if let Some(settings) = site.oauth2_settings_raw() {
        if let Some(base) = settings.redirect_base_url.clone() {
            if !base.is_empty() {
                return Some(base);
            }
        }
    }

    let instance = site.inner.instance.lock();
    if !instance.app_url.is_empty() {
        return Some(instance.app_url.clone());
    }
    None
}

async fn resolve_oauth_user(
    site: &NitroRepo,
    settings: &OAuth2Settings,
    principal: &SsoPrincipal,
) -> Result<UserSafeData, Response> {
    if let Some(email) = principal.email.as_ref() {
        match UserSafeData::get_by_email(email, &site.database).await {
            Ok(Some(user)) => return Ok(user),
            Ok(None) => {}
            Err(err) => {
                error!(%err, "Failed to lookup user by email during OAuth2 login");
                return Err(internal_login_error());
            }
        }
    }

    match UserSafeData::get_by_username_or_email(&principal.username, &site.database).await {
        Ok(Some(user)) => return Ok(user),
        Ok(None) => {}
        Err(err) => {
            error!(%err, "Failed to lookup user by username during OAuth2 login");
            return Err(internal_login_error());
        }
    }

    if !settings.auto_create_users {
        let api_error: APIErrorResponse<(), ()> = APIErrorResponse {
            message: "Account not found".into(),
            details: None,
            error: None,
        };
        return Err(ResponseBuilder::forbidden().json(&api_error));
    }

    create_user(site, principal).await
}

fn build_principal(claims: &IdTokenClaims) -> SsoPrincipal {
    let source_username = claims
        .preferred_username
        .clone()
        .or_else(|| claims.email.clone())
        .unwrap_or_else(|| claims.sub.clone());
    let username = normalize_username(&source_username);

    let display_name = claims
        .name
        .clone()
        .or_else(|| claims.given_name.clone())
        .or_else(|| claims.email.clone())
        .unwrap_or_else(|| username.clone());

    SsoPrincipal {
        username,
        email: claims.email.clone(),
        display_name,
    }
}

fn oauth_service_error_response(err: OAuth2ServiceError) -> Response {
    match err {
        OAuth2ServiceError::Disabled
        | OAuth2ServiceError::MissingProviders
        | OAuth2ServiceError::ProviderNotConfigured(_) => {
            ResponseBuilder::not_found().body("OAuth2 provider not available")
        }
        OAuth2ServiceError::InvalidState => {
            ResponseBuilder::bad_request().body("OAuth2 login state is invalid or has expired")
        }
        OAuth2ServiceError::InvalidRedirectUrl(details)
        | OAuth2ServiceError::ClientConstruction(details) => {
            let api_error: APIErrorResponse<String, ()> = APIErrorResponse {
                message: "OAuth2 configuration error".into(),
                details: Some(details),
                error: None,
            };
            ResponseBuilder::internal_server_error().json(&api_error)
        }
        OAuth2ServiceError::TokenRequestFailed(details) => {
            let api_error: APIErrorResponse<String, ()> = APIErrorResponse {
                message: "OAuth2 provider rejected the request".into(),
                details: Some(details),
                error: None,
            };
            ResponseBuilder::internal_server_error().json(&api_error)
        }
    }
}

fn internal_login_error() -> Response {
    let api_error: APIErrorResponse<(), ()> = APIErrorResponse {
        message: "Unexpected error processing OAuth2 login".into(),
        details: None,
        error: None,
    };
    ResponseBuilder::internal_server_error().json(&api_error)
}

fn extract_roles(provider: OAuth2ProviderKind, claims: &IdTokenClaims) -> Vec<String> {
    let mut collected = Vec::new();
    if let Some(roles) = claims.roles.as_ref() {
        collected.extend(roles.iter().cloned());
    }
    if let Some(groups) = claims.groups.as_ref() {
        collected.extend(groups.iter().cloned());
    }

    if collected.is_empty() && provider == OAuth2ProviderKind::Google {
        collected.extend(claims.email.iter().map(|email| format!("group:{email}")));
    }

    collected.retain(|role| !role.trim().is_empty());
    collected.sort();
    collected.dedup();
    collected
}

fn map_roles_from_claims(
    provider: OAuth2ProviderKind,
    claims: &[String],
    mappings: &[OAuth2GroupRoleMapping],
) -> Vec<String> {
    let mut assigned = Vec::new();
    for mapping in mappings.iter().filter(|m| m.provider == provider) {
        let group = mapping.group.trim();
        if group.is_empty() || mapping.roles.is_empty() {
            continue;
        }
        if claims.iter().any(|claim| claim.eq_ignore_ascii_case(group)) {
            assigned.extend(mapping.roles.clone());
        }
    }
    assigned
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_roles_combines_roles_and_groups() {
        let claims = IdTokenClaims {
            sub: "user-1".to_string(),
            email: Some("user@example.com".to_string()),
            preferred_username: None,
            name: None,
            given_name: None,
            family_name: None,
            roles: Some(vec!["admin".to_string(), "admin".to_string()]),
            groups: Some(vec!["team-a".to_string()]),
        };

        let roles = extract_roles(OAuth2ProviderKind::Microsoft, &claims);
        assert_eq!(roles, vec!["admin", "team-a"]);
    }

    #[test]
    fn extract_roles_adds_google_fallback_when_missing() {
        let claims = IdTokenClaims {
            sub: "user-2".to_string(),
            email: Some("user@example.com".to_string()),
            preferred_username: None,
            name: None,
            given_name: None,
            family_name: None,
            roles: None,
            groups: None,
        };

        let roles = extract_roles(OAuth2ProviderKind::Google, &claims);
        assert_eq!(roles, vec!["group:user@example.com".to_string()]);
    }

    #[test]
    fn map_roles_from_claims_matches_configured_groups() {
        let claims = vec!["GROUP-Admins".to_string(), "team-engineering".to_string()];
        let mappings = vec![
            OAuth2GroupRoleMapping {
                provider: OAuth2ProviderKind::Microsoft,
                group: "group-admins".to_string(),
                roles: vec!["admin".to_string()],
            },
            OAuth2GroupRoleMapping {
                provider: OAuth2ProviderKind::Google,
                group: "team-engineering".to_string(),
                roles: vec!["engineering".to_string()],
            },
        ];

        let result = map_roles_from_claims(OAuth2ProviderKind::Microsoft, &claims, &mappings);
        assert_eq!(result, vec!["admin".to_string()]);
    }
}
