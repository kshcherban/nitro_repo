use std::{net::SocketAddr, str::FromStr};

use axum::{
    body::Body,
    extract::{ConnectInfo, Query, State},
    http::{
        HeaderMap, HeaderName, StatusCode,
        header::{COOKIE, LOCATION, SET_COOKIE},
    },
    response::{IntoResponse, Response},
};
use axum_extra::{
    TypedHeader,
    extract::{
        CookieJar,
        cookie::{Cookie, Expiration},
    },
    headers::UserAgent,
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::Duration;
use http::HeaderValue;
use nr_core::{
    database::entities::user::{NewUserRequest, UserSafeData, UserType},
    user::{Email, Username},
};
use serde::Deserialize;
use sqlx::Error as SqlxError;
use tracing::{debug, error, instrument, trace, warn};
use utoipa::IntoParams;
use uuid::Uuid;

use crate::{
    app::{NitroRepo, config::SsoSettings},
    error::InternalError,
    utils::{ResponseBuilder, api_error_response::APIErrorResponse},
};

#[derive(Debug, Deserialize, IntoParams)]
pub struct SsoLoginQuery {
    redirect: Option<String>,
}

#[derive(Debug)]
struct SsoPrincipal {
    username: String,
    email: Option<String>,
    display_name: String,
}

#[derive(Debug, Deserialize)]
struct CfAccessJwtClaims {
    email: Option<String>,
    name: Option<String>,
    common_name: Option<String>,
    sub: Option<String>,
}

pub type SsoLoginResponse = Result<Response, InternalError>;

#[utoipa::path(
    get,
    path = "/sso/login",
    params(SsoLoginQuery),
    responses(
        (status = 303, description = "SSO login succeeded"),
        (status = 401, description = "SSO headers missing"),
        (status = 403, description = "Account not authorized for SSO"),
        (status = 404, description = "SSO login disabled")
    ),
    security(()),
    operation_id = "ssoLogin"
)]
#[instrument(
    skip(site, user_agent, headers),
    fields(project_module = "Authentication", auth.sso = true)
)]
pub async fn login(
    State(site): State<NitroRepo>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    user_agent: Option<TypedHeader<UserAgent>>,
    headers: HeaderMap,
    Query(query): Query<SsoLoginQuery>,
) -> SsoLoginResponse {
    let Some(config) = site.sso_settings() else {
        trace!("SSO login attempted without configuration");
        return Ok(ResponseBuilder::not_found().body("SSO login is not enabled"));
    };

    let principal = match extract_principal(&config, &headers) {
        Ok(principal) => principal,
        Err(response) => return Ok(response),
    };

    let user = match resolve_or_create_user(&site, &config, &principal).await {
        Ok(user) => user,
        Err(response) => return Ok(response),
    };

    if !user.active {
        warn!(user_id = user.id, "Inactive user attempted SSO login");
        let api_error: APIErrorResponse<(), ()> = APIErrorResponse {
            message: "User account is disabled".into(),
            details: None,
            error: None,
        };
        return Ok(ResponseBuilder::forbidden().json(&api_error));
    }

    let user_agent = user_agent
        .map(|ua| ua.to_string())
        .unwrap_or_else(|| "Nitro Repo SSO".to_string());
    let ip = addr.ip().to_string();
    let duration = Duration::days(1);
    let session = match site
        .session_manager
        .create_session(user.id, user_agent, ip, duration)
    {
        Ok(session) => session,
        Err(err) => {
            error!(error = %err, "Failed to create session for SSO principal");
            return Ok(err.into_response());
        }
    };

    let cookie = Cookie::build(("session", session.session_id.clone()))
        .secure(true)
        .path("/")
        .expires(Expiration::Session)
        .build();

    let redirect_target = sanitize_redirect(query.redirect.as_deref());

    let response = Response::builder()
        .status(StatusCode::SEE_OTHER)
        .header(SET_COOKIE, cookie.encoded().to_string())
        .header(LOCATION, redirect_target)
        .body(Body::empty())
        .expect("Failed to build SSO redirect response");

    Ok(response)
}

fn extract_principal(config: &SsoSettings, headers: &HeaderMap) -> Result<SsoPrincipal, Response> {
    if let Some(principal) = extract_principal_from_headers(config, headers)? {
        return Ok(principal);
    }

    if let Some(principal) = extract_principal_from_cf_jwt(headers)? {
        return Ok(principal);
    }

    let api_error: APIErrorResponse<(), ()> = APIErrorResponse {
        message: "SSO username header missing".into(),
        details: None,
        error: None,
    };
    Err(ResponseBuilder::unauthorized().json(&api_error))
}

fn extract_principal_from_headers(
    config: &SsoSettings,
    headers: &HeaderMap,
) -> Result<Option<SsoPrincipal>, Response> {
    let Some(username_raw) = header_value(headers, &config.username_header)? else {
        return Ok(None);
    };

    let email = match config.email_header.as_ref() {
        Some(header) => header_value(headers, header)?,
        None => None,
    };
    let display_name = match config.display_name_header.as_ref() {
        Some(header) => header_value(headers, header)?,
        None => None,
    }
    .filter(|value| !value.is_empty())
    .unwrap_or_else(|| username_raw.clone());

    let username = normalize_username(&username_raw);

    Ok(Some(SsoPrincipal {
        username,
        email,
        display_name,
    }))
}

fn extract_principal_from_cf_jwt(headers: &HeaderMap) -> Result<Option<SsoPrincipal>, Response> {
    let Some(token) = extract_cf_access_token(headers) else {
        trace!("CF Access JWT missing from headers and cookies");
        return Ok(None);
    };

    let claims = match decode_cf_access_jwt(&token) {
        Ok(Some(claims)) => claims,
        Ok(None) => {
            trace!("CF Access JWT missing payload");
            return Ok(None);
        }
        Err(response) => return Err(response),
    };

    let Some(identifier) = claims
        .email
        .clone()
        .or(claims.sub.clone())
        .filter(|value| !value.is_empty())
    else {
        trace!("CF Access JWT missing identifier claims");
        return Ok(None);
    };

    let display_name = claims
        .name
        .or(claims.common_name)
        .unwrap_or_else(|| identifier.clone());

    let username = normalize_username(&identifier);

    Ok(Some(SsoPrincipal {
        username,
        email: claims.email,
        display_name,
    }))
}

fn decode_cf_access_jwt(token: &str) -> Result<Option<CfAccessJwtClaims>, Response> {
    let mut segments = token.split('.');
    let _header = segments.next();
    let payload = segments.next();

    let Some(payload) = payload else {
        return Ok(None);
    };

    let decoded = URL_SAFE_NO_PAD.decode(payload).map_err(|error| {
        error!(%error, "Unable to base64 decode CF Access JWT payload");
        ResponseBuilder::internal_server_error().body("Failed to decode Cloudflare Access token")
    })?;

    let claims: CfAccessJwtClaims = serde_json::from_slice(&decoded).map_err(|error| {
        error!(%error, "Unable to parse CF Access JWT payload");
        ResponseBuilder::internal_server_error().body("Failed to parse Cloudflare Access token")
    })?;

    Ok(Some(claims))
}

fn extract_cf_access_token(headers: &HeaderMap) -> Option<String> {
    if let Some(token) = headers
        .get("Cf-Access-Jwt-Assertion")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Some(token.to_owned());
    }

    let jar = CookieJar::from_headers(headers);
    if let Some(cookie) = jar
        .get("CF_Authorization")
        .map(|cookie| cookie.value().trim())
        .filter(|value| !value.is_empty())
    {
        return Some(cookie.to_owned());
    }

    for value in headers.get_all(COOKIE).iter() {
        if let Ok(cookie_header) = value.to_str() {
            for pair in cookie_header.split(';') {
                let trimmed = pair.trim();
                if let Some(rest) = trimmed.strip_prefix("CF_Authorization=") {
                    let token = rest.trim();
                    if !token.is_empty() {
                        return Some(token.to_owned());
                    }
                }
            }
        }
    }

    None
}

fn header_value(headers: &HeaderMap, name: &str) -> Result<Option<String>, Response> {
    let header_name = HeaderName::from_str(name).map_err(|error| {
        error!(%error, header = name, "Invalid SSO header configuration");
        let api_error: APIErrorResponse<(), ()> = APIErrorResponse {
            message: "Invalid SSO header configuration".into(),
            details: None,
            error: None,
        };
        ResponseBuilder::internal_server_error().json(&api_error)
    })?;

    Ok(headers
        .get(header_name)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty()))
}

async fn resolve_or_create_user(
    site: &NitroRepo,
    config: &SsoSettings,
    principal: &SsoPrincipal,
) -> Result<UserSafeData, Response> {
    if let Some(email) = principal.email.as_ref() {
        trace!(email, "Attempting to match SSO user by email");
        match UserSafeData::get_by_email(email, &site.database).await {
            Ok(Some(user)) => return Ok(user),
            Ok(None) => {}
            Err(err) => {
                error!(%err, "Failed to lookup user by email during SSO login");
                return Err(ResponseBuilder::internal_server_error()
                    .body("Unexpected error processing SSO login"));
            }
        }
    }

    trace!(username = %principal.username, "Attempting to match SSO user by username");
    match UserSafeData::get_by_username_or_email(&principal.username, &site.database).await {
        Ok(Some(user)) => return Ok(user),
        Ok(None) => {}
        Err(err) => {
            error!(%err, "Failed to lookup user by username during SSO login");
            return Err(ResponseBuilder::internal_server_error()
                .body("Unexpected error processing SSO login"));
        }
    }

    if !config.auto_create_users {
        let api_error: APIErrorResponse<(), ()> = APIErrorResponse {
            message: "Account not found".into(),
            details: None,
            error: None,
        };
        return Err(ResponseBuilder::forbidden().json(&api_error));
    }

    create_user(site, principal).await
}

async fn create_user(site: &NitroRepo, principal: &SsoPrincipal) -> Result<UserSafeData, Response> {
    debug!(username = %principal.username, "Auto-provisioning SSO user");
    let base_username = principal.username.clone();

    for attempt in 0..=20 {
        let candidate = generate_username_candidate(&base_username, attempt);
        let username = match Username::from_str(&candidate) {
            Ok(username) => username,
            Err(err) => {
                warn!(%err, candidate, "Generated username rejected");
                continue;
            }
        };

        let email = match build_user_email(principal.email.as_deref(), &candidate) {
            Ok(email) => email,
            Err(response) => return Err(response),
        };

        let new_user = NewUserRequest {
            name: principal.display_name.clone(),
            username,
            email,
            password: None,
        };

        match new_user.insert(&site.database).await {
            Ok(user) => {
                trace!(username = %user.username, "Provisioned new SSO user");
                return Ok(UserSafeData::from(user));
            }
            Err(SqlxError::Database(db_err)) => {
                if db_err
                    .code()
                    .map(|code| code.as_ref() == "23505")
                    .unwrap_or(false)
                {
                    debug!(
                        candidate,
                        "Username or email collision when auto-provisioning SSO user"
                    );
                    continue;
                }
                error!(error = %db_err, "Database error creating SSO user");
                let api_error: APIErrorResponse<(), ()> = APIErrorResponse {
                    message: "Failed to create user".into(),
                    details: None,
                    error: None,
                };
                return Err(ResponseBuilder::internal_server_error().json(&api_error));
            }
            Err(err) => {
                error!(%err, "Unexpected error creating SSO user");
                let api_error: APIErrorResponse<(), ()> = APIErrorResponse {
                    message: "Failed to create user".into(),
                    details: None,
                    error: None,
                };
                return Err(ResponseBuilder::internal_server_error().json(&api_error));
            }
        }
    }

    warn!(username = %principal.username, "Unable to provision unique username for SSO user");
    let api_error: APIErrorResponse<(), ()> = APIErrorResponse {
        message: "Unable to provision account".into(),
        details: None,
        error: None,
    };
    Err(ResponseBuilder::conflict().json(&api_error))
}

fn build_user_email(raw_email: Option<&str>, username: &str) -> Result<Email, Response> {
    if let Some(raw) = raw_email {
        match Email::from_str(raw) {
            Ok(email) => return Ok(email),
            Err(err) => warn!(%err, "Invalid email supplied by SSO provider"),
        }
    }

    let mut local_part = username.to_owned();
    const DOMAIN: &str = "@sso.local";
    let max_local_len = 32 - DOMAIN.len();
    if max_local_len <= 0 {
        let api_error: APIErrorResponse<(), ()> = APIErrorResponse {
            message: "Invalid SSO email configuration".into(),
            details: None,
            error: None,
        };
        return Err(ResponseBuilder::internal_server_error().json(&api_error));
    }
    if local_part.len() > max_local_len {
        local_part.truncate(max_local_len);
    }
    if local_part.len() < 3 {
        local_part.push_str("usr");
    }
    let fallback = format!("{}{}", local_part, DOMAIN);
    Email::from_str(&fallback).map_err(|err| {
        error!(%err, fallback, "Failed to construct fallback email for SSO user");
        let api_error: APIErrorResponse<(), ()> = APIErrorResponse {
            message: "Invalid SSO email".into(),
            details: None,
            error: None,
        };
        ResponseBuilder::internal_server_error().json(&api_error)
    })
}

fn normalize_username(raw: &str) -> String {
    let mut normalized = raw.trim().to_lowercase();
    if normalized.is_empty() {
        normalized = format!("user{}", &Uuid::new_v4().simple().to_string()[..6]);
    }

    let mut cleaned = String::with_capacity(normalized.len());
    for ch in normalized.chars() {
        match ch {
            'a'..='z' | '0'..='9' | '_' | '-' => cleaned.push(ch),
            ' ' | '.' | '@' => cleaned.push('_'),
            _ => {
                // Skip any other characters
            }
        }
    }

    while cleaned.starts_with('_') || cleaned.starts_with('-') {
        cleaned.remove(0);
        if cleaned.is_empty() {
            break;
        }
    }
    while cleaned.ends_with('_') || cleaned.ends_with('-') {
        cleaned.pop();
        if cleaned.is_empty() {
            break;
        }
    }

    if cleaned.len() < 3 {
        cleaned = format!("usr{}", &Uuid::new_v4().simple().to_string()[..5]);
    }

    if cleaned.len() > 32 {
        cleaned.truncate(32);
    }

    cleaned
}

fn generate_username_candidate(base: &str, attempt: usize) -> String {
    if attempt == 0 {
        return base.chars().take(32).collect();
    }
    let suffix = attempt.to_string();
    let max_base_len = 32usize.saturating_sub(suffix.len());
    let mut trimmed: String = base.chars().take(max_base_len.max(1)).collect();
    if trimmed.len() < 2 {
        trimmed.push('u');
        trimmed.push('s');
    }
    trimmed.push_str(&suffix);
    trimmed
}

fn sanitize_redirect(target: Option<&str>) -> HeaderValue {
    let default = HeaderValue::from_static("/");
    let Some(target) = target.filter(|value| !value.is_empty()) else {
        return default;
    };
    if !target.starts_with('/') || target.starts_with("//") {
        return default;
    }
    HeaderValue::from_str(target).unwrap_or(default)
}
