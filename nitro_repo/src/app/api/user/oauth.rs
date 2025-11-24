use std::{io, net::SocketAddr, str::FromStr};

use axum::{
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
use chrono::{DateTime, Duration, Utc};
use jsonwebtoken::dangerous::insecure_decode;
use nr_core::database::entities::user::{UserSafeData, UserType};
use nr_core::user::permissions::UpdatePermissions;
use oauth2::AuthorizationCode;
use serde::{Deserialize, Serialize};
use sqlx::Row;
use tokio::sync::OnceCell;
use tracing::{error, info, instrument, warn};
use utoipa::{IntoParams, ToSchema};

use crate::{
    app::{
        NitroRepo,
        authentication::oauth::{OAuth2ServiceError, OAuthStateExport},
        config::{OAuth2GroupRoleMapping, OAuth2ProviderKind, OAuth2Settings},
    },
    error::{InternalError, OtherInternalError},
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

const OAUTH_STATE_TTL_SECONDS: i64 = 300;
static OAUTH_STATE_TABLE_INIT: OnceCell<()> = OnceCell::const_new();

struct PersistedOAuthState {
    provider: String,
    pkce_verifier: String,
    redirect: Option<String>,
    created_at: DateTime<Utc>,
}

impl PersistedOAuthState {
    fn is_expired(&self) -> bool {
        let cutoff = Utc::now() - Duration::seconds(OAUTH_STATE_TTL_SECONDS);
        self.created_at < cutoff
    }

    fn into_export(self) -> Result<OAuthStateExport, OAuth2ServiceError> {
        let provider = OAuth2ProviderKind::from_str(&self.provider)
            .map_err(|_| OAuth2ServiceError::InvalidState)?;
        Ok(OAuthStateExport {
            provider,
            pkce_verifier: self.pkce_verifier,
            redirect: self.redirect,
        })
    }
}

async fn ensure_oauth_state_storage(site: &NitroRepo) -> Result<(), InternalError> {
    let pool = site.database.clone();
    OAUTH_STATE_TABLE_INIT
        .get_or_try_init(|| async move {
            sqlx::query(
                r#"
                CREATE TABLE IF NOT EXISTS oauth2_states (
                    state TEXT PRIMARY KEY,
                    provider TEXT NOT NULL,
                    pkce_verifier TEXT NOT NULL,
                    redirect TEXT NULL,
                    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
                )
                "#,
            )
            .execute(&pool)
            .await?;

            sqlx::query(
                r#"
                CREATE INDEX IF NOT EXISTS idx_oauth2_states_created_at
                    ON oauth2_states (created_at)
                "#,
            )
            .execute(&pool)
            .await?;
            Ok::<(), InternalError>(())
        })
        .await?;
    Ok(())
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

    let snapshot = service.export_state(&auth_redirect.state).ok_or_else(|| {
        InternalError::from(OtherInternalError::new(io::Error::new(
            io::ErrorKind::Other,
            "OAuth2 state initialization failed",
        )))
    })?;
    persist_oauth_state(&site, &auth_redirect.state, &snapshot).await?;

    let response = ResponseBuilder::default()
        .status(StatusCode::SEE_OTHER)
        .header(LOCATION, auth_redirect.authorization_url.as_str())
        .empty();

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
    let code_value = code.clone();
    let state_value = state.clone();

    let exchange = match service
        .exchange_code(
            base_url.as_deref(),
            AuthorizationCode::new(code_value.clone()),
            state,
        )
        .await
    {
        Ok(exchange) => {
            if let Err(err) = delete_persisted_oauth_state(&site, state).await {
                warn!(%err, "Failed to delete persisted OAuth2 state after successful exchange");
            }
            exchange
        }
        Err(OAuth2ServiceError::InvalidState) => {
            match load_persisted_oauth_state(&site, state).await? {
                Some(persisted) => {
                    if persisted.is_expired() {
                        if let Err(err) = delete_persisted_oauth_state(&site, state).await {
                            warn!(%err, "Failed to delete expired OAuth2 state");
                        }
                        return Ok(oauth_service_error_response(
                            OAuth2ServiceError::InvalidState,
                        ));
                    }
                    let export = match persisted.into_export() {
                        Ok(export) => export,
                        Err(err) => {
                            if let Err(cleanup_err) =
                                delete_persisted_oauth_state(&site, state).await
                            {
                                warn!(%cleanup_err, "Failed to delete invalid OAuth2 state");
                            }
                            return Ok(oauth_service_error_response(err));
                        }
                    };
                    match service
                        .exchange_code_with_export(
                            base_url.as_deref(),
                            AuthorizationCode::new(code_value.clone()),
                            export,
                        )
                        .await
                    {
                        Ok(exchange) => {
                            if let Err(err) = delete_persisted_oauth_state(&site, state).await {
                                warn!(
                                    %err,
                                    "Failed to delete persisted OAuth2 state after fallback exchange"
                                );
                            }
                            exchange
                        }
                        Err(err) => {
                            if let Err(cleanup_err) =
                                delete_persisted_oauth_state(&site, state).await
                            {
                                warn!(%cleanup_err, "Failed to delete persisted OAuth2 state after fallback failure");
                            }
                            warn!(%err, "OAuth2 code exchange failed using persisted state");
                            return Ok(oauth_service_error_response(err));
                        }
                    }
                }
                None => {
                    warn!(state = %state_value, "OAuth2 state not found in persistent store");
                    return Ok(oauth_service_error_response(
                        OAuth2ServiceError::InvalidState,
                    ));
                }
            }
        }
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

    let claim_groups = extract_roles(exchange.provider, &claims);
    let mapped_roles = map_roles_from_claims(
        exchange.provider,
        &claim_groups,
        &oauth_settings.group_role_mappings,
    );
    let has_mapped_roles = !mapped_roles.is_empty();
    let principal = build_principal(&claims);
    let subject_identifier = principal.email.as_deref().unwrap_or(&principal.username);

    let rbac = site.oauth2_rbac();
    let mut existing_roles: Vec<String> = Vec::new();
    if let Some(ref rbac_engine) = rbac {
        match rbac_engine.roles_for_user(subject_identifier).await {
            Ok(roles) => existing_roles = roles,
            Err(err) => {
                warn!(
                    %err,
                    provider = %exchange.provider,
                    subject = subject_identifier,
                    "Failed to load existing OAuth2 RBAC roles"
                );
            }
        }
    }
    let has_existing_roles = !existing_roles.is_empty();

    if rbac.is_some() && !has_mapped_roles && !has_existing_roles {
        warn!(
            provider = %exchange.provider,
            subject = subject_identifier,
            "OAuth2 login denied: no roles mapped for subject"
        );
        return Ok(oauth_denied_redirect("no_roles"));
    }

    let rbac_enabled = rbac.is_some();

    let mut user = match resolve_oauth_user(&site, &oauth_settings, &principal).await {
        Ok(user) => user,
        Err(response) => return Ok(response),
    };

    if !user.active {
        warn!(user_id = user.id, "Inactive user attempted OAuth2 login");
        return Ok(oauth_denied_redirect("inactive"));
    }

    let email = user.email.to_string();
    if existing_roles.is_empty() && subject_identifier != email {
        if let Some(ref rbac_engine) = rbac {
            match rbac_engine.roles_for_user(&email).await {
                Ok(roles) => existing_roles = roles,
                Err(err) => {
                    warn!(
                        %err,
                        provider = %exchange.provider,
                        subject = %email,
                        "Failed to load existing OAuth2 RBAC roles for resolved email"
                    );
                }
            }
        }
    }

    if has_mapped_roles {
        if let Err(err) = site.apply_oauth_roles(&email, &mapped_roles).await {
            warn!(%err, user_id = user.id, "Failed to apply OAuth2 RBAC roles");
        }
    }

    let effective_roles = if has_mapped_roles {
        mapped_roles.clone()
    } else {
        existing_roles.clone()
    };

    let admin_role = effective_roles
        .iter()
        .any(|role| role.eq_ignore_ascii_case("admin"));
    let user_manager_role = effective_roles
        .iter()
        .any(|role| role.eq_ignore_ascii_case("user_manager"));
    let system_manager_role = effective_roles
        .iter()
        .any(|role| role.eq_ignore_ascii_case("system_manager"));

    if rbac_enabled
        && (!effective_roles.is_empty())
        && (admin_role != user.admin
            || user_manager_role != user.user_manager
            || system_manager_role != user.system_manager)
    {
        let update = UpdatePermissions {
            admin: Some(admin_role),
            user_manager: Some(user_manager_role),
            system_manager: Some(system_manager_role),
            default_repository_actions: None,
            repository_permissions: Default::default(),
        };
        if let Err(err) = update.update_permissions(user.id, &site.database).await {
            warn!(%err, user_id = user.id, "Failed to synchronize OAuth2 user flags");
        } else {
            user.admin = admin_role;
            user.user_manager = user_manager_role;
            user.system_manager = system_manager_role;
        }
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

    let redirect_str = redirect_header.to_str().unwrap_or("/").to_string();
    info!(
        user_id = user.id,
        email = %user.email,
        provider = %exchange.provider,
        redirect = %redirect_str,
        "OAuth2 login succeeded"
    );

    let response = ResponseBuilder::default()
        .status(StatusCode::SEE_OTHER)
        .header(SET_COOKIE, cookie.encoded().to_string())
        .header(LOCATION, redirect_header)
        .empty();

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
        return Err(oauth_denied_redirect("no_account"));
    }

    create_user(site, principal).await
}

async fn persist_oauth_state(
    site: &NitroRepo,
    state: &str,
    snapshot: &OAuthStateExport,
) -> Result<(), InternalError> {
    ensure_oauth_state_storage(site).await?;
    prune_persisted_oauth_states(site).await?;
    sqlx::query(
        r#"
        INSERT INTO oauth2_states (state, provider, pkce_verifier, redirect)
        VALUES ($1, $2, $3, $4)
        ON CONFLICT (state) DO UPDATE
        SET provider = EXCLUDED.provider,
            pkce_verifier = EXCLUDED.pkce_verifier,
            redirect = EXCLUDED.redirect,
            created_at = NOW()
        "#,
    )
    .bind(state)
    .bind(snapshot.provider.to_string())
    .bind(snapshot.pkce_verifier.as_str())
    .bind(snapshot.redirect.as_deref())
    .execute(&site.database)
    .await?;
    Ok(())
}

async fn load_persisted_oauth_state(
    site: &NitroRepo,
    state: &str,
) -> Result<Option<PersistedOAuthState>, InternalError> {
    ensure_oauth_state_storage(site).await?;
    let row = sqlx::query(
        r#"
        SELECT provider, pkce_verifier, redirect, created_at
        FROM oauth2_states
        WHERE state = $1
        "#,
    )
    .bind(state)
    .fetch_optional(&site.database)
    .await?;

    Ok(row.map(|record| PersistedOAuthState {
        provider: record.get::<String, _>("provider"),
        pkce_verifier: record.get::<String, _>("pkce_verifier"),
        redirect: record.get::<Option<String>, _>("redirect"),
        created_at: record.get::<DateTime<Utc>, _>("created_at"),
    }))
}

async fn delete_persisted_oauth_state(site: &NitroRepo, state: &str) -> Result<(), InternalError> {
    ensure_oauth_state_storage(site).await?;
    sqlx::query(
        r#"
        DELETE FROM oauth2_states
        WHERE state = $1
        "#,
    )
    .bind(state)
    .execute(&site.database)
    .await?;
    Ok(())
}

async fn prune_persisted_oauth_states(site: &NitroRepo) -> Result<(), InternalError> {
    ensure_oauth_state_storage(site).await?;
    sqlx::query(
        r#"
        DELETE FROM oauth2_states
        WHERE created_at < NOW() - INTERVAL '15 minutes'
        "#,
    )
    .execute(&site.database)
    .await?;
    Ok(())
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
        roles: Vec::new(),
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

fn oauth_denied_redirect(reason: &str) -> Response {
    let location = format!("/oauth/denied?reason={reason}");
    ResponseBuilder::default()
        .status(StatusCode::SEE_OTHER)
        .header(LOCATION, location)
        .empty()
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

    #[test]
    fn oauth_denied_redirect_sets_location_header() {
        let response = super::oauth_denied_redirect("invalid_state");
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        let location = response
            .headers()
            .get(LOCATION)
            .and_then(|value| value.to_str().ok());
        assert_eq!(location, Some("/oauth/denied?reason=invalid_state"));
    }
}
