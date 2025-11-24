use std::{net::SocketAddr, str::FromStr};

use axum::{
    extract::{ConnectInfo, Query, State},
    http::{
        HeaderMap, HeaderName, StatusCode,
        header::{LOCATION, SET_COOKIE},
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
    app::{
        NitroRepo,
        authentication::jwks::{JwksError, JwksFetcher, JwksManager, JwksResolver},
        config::{OidcProviderConfig, SsoSettings, TokenSource},
    },
    error::InternalError,
    utils::{ResponseBuilder, api_error_response::APIErrorResponse},
};

#[derive(Debug, Deserialize, IntoParams)]
pub struct SsoLoginQuery {
    redirect: Option<String>,
}

#[derive(Debug)]
pub(super) struct SsoPrincipal {
    pub(super) username: String,
    pub(super) email: Option<String>,
    pub(super) display_name: String,
    pub(super) roles: Vec<String>,
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

    let principal = match extract_principal(&site.jwks, &config, &headers).await {
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
        .same_site(axum_extra::extract::cookie::SameSite::None)
        .path("/")
        .expires(Expiration::Session)
        .build();

    let redirect_target = sanitize_redirect(query.redirect.as_deref());

    let response = ResponseBuilder::default()
        .status(StatusCode::SEE_OTHER)
        .header(SET_COOKIE, cookie.encoded().to_string())
        .header(LOCATION, redirect_target)
        .empty();

    if !principal.roles.is_empty() {
        if let Err(err) = site
            .apply_oauth_roles(&user.username, &principal.roles)
            .await
        {
            error!(%err, "Failed to apply SSO roles to user");
        }
    }

    Ok(response)
}

async fn extract_principal<F>(
    jwks: &JwksManager<F>,
    config: &SsoSettings,
    headers: &HeaderMap,
) -> Result<SsoPrincipal, Response>
where
    F: JwksFetcher + JwksResolver + Clone,
{
    if let Some(principal) = extract_principal_from_providers(jwks, config, headers).await? {
        return Ok(principal);
    }

    let api_error: APIErrorResponse<(), ()> = APIErrorResponse {
        message: "No valid SSO provider token found".into(),
        details: None,
        error: None,
    };
    Err(ResponseBuilder::unauthorized().json(&api_error))
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

pub(super) async fn resolve_or_create_user(
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

pub(super) async fn create_user(
    site: &NitroRepo,
    principal: &SsoPrincipal,
) -> Result<UserSafeData, Response> {
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

pub(super) fn build_user_email(raw_email: Option<&str>, username: &str) -> Result<Email, Response> {
    if let Some(raw) = raw_email {
        match Email::from_str(raw) {
            Ok(email) => return Ok(email),
            Err(err) => warn!(%err, "Invalid email supplied by SSO provider"),
        }
    }

    let mut local_part = username.to_owned();
    const DOMAIN: &str = "@sso.local";
    const MAX_LOCAL_LEN: usize = 32 - DOMAIN.len();
    if local_part.len() > MAX_LOCAL_LEN {
        local_part.truncate(MAX_LOCAL_LEN);
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

pub(super) fn normalize_username(raw: &str) -> String {
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

pub(super) fn generate_username_candidate(base: &str, attempt: usize) -> String {
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

pub(super) fn sanitize_redirect(target: Option<&str>) -> HeaderValue {
    let default = HeaderValue::from_static("/");
    let Some(target) = target.filter(|value| !value.is_empty()) else {
        return default;
    };
    if !target.starts_with('/') || target.starts_with("//") {
        return default;
    }
    HeaderValue::from_str(target).unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use super::{
        SsoSettings, build_user_email, extract_principal, normalize_username, sanitize_redirect,
    };
    use crate::app::authentication::jwks::{
        JwkDocument, JwkKey, JwksError, JwksFetcher, JwksManager, JwksResolver,
    };
    use crate::app::config::{OidcProviderConfig, TokenSource};
    use async_trait::async_trait;
    use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
    use http::{HeaderMap, HeaderValue};
    use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
    use rsa::{
        RsaPrivateKey, pkcs1::EncodeRsaPrivateKey, rand_core::OsRng, traits::PublicKeyParts,
    };
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    #[test]
    fn build_user_email_uses_raw_value_when_valid() {
        let email = build_user_email(Some("user@example.com"), "username").unwrap();
        assert_eq!(email.to_string(), "user@example.com");
    }

    #[test]
    fn build_user_email_generates_fallback_with_domain() {
        let email = build_user_email(None, "ab").unwrap();
        assert!(email.to_string().starts_with("abusr"));
        assert!(email.to_string().ends_with("@sso.local"));
    }

    #[tokio::test]
    async fn oidc_provider_principal_is_used_when_token_present() -> anyhow::Result<()> {
        let kid = "kid-1";
        let issuer = "https://issuer.example";
        let audience = "nitro";
        let (encoding_key, jwks) = generate_rsa_material(kid)?;
        let token = sign_test_token(kid, &encoding_key, issuer, audience)?;

        let fetcher = StaticFetcher::new(jwks);
        let manager = JwksManager::new(fetcher, Duration::from_secs(3600));

        let settings = SsoSettings {
            enabled: true,
            providers: vec![OidcProviderConfig {
                name: "example".into(),
                issuer: issuer.into(),
                audience: audience.into(),
                jwks_url: Some("https://issuer.example/keys".into()),
                token_source: TokenSource::Header {
                    name: "Authorization".into(),
                    prefix: Some("Bearer ".into()),
                },
                subject_claim: None,
                email_claim: None,
                display_name_claim: None,
                role_claims: vec!["roles".into()],
            }],
            ..SsoSettings::default()
        };

        let mut headers = HeaderMap::new();
        headers.insert(
            http::header::AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {token}"))?,
        );

        let principal = extract_principal(&manager, &settings, &headers)
            .await
            .expect("principal");

        assert_eq!(principal.username, normalize_username("user-123"));
        assert_eq!(principal.email.as_deref(), Some("user@example.com"));
        assert_eq!(principal.display_name, "Test User");
        assert!(principal.roles.contains(&"admin".to_string()));
        Ok(())
    }
    #[tokio::test]
    async fn sanitize_redirect_rejects_external_urls() {
        let external = sanitize_redirect(Some("https://example.com"));
        assert_eq!(external, HeaderValue::from_static("/"));
    }

    #[derive(Clone)]
    struct StaticFetcher {
        doc: JwkDocument,
    }

    impl StaticFetcher {
        fn new(doc: JwkDocument) -> Self {
            Self { doc }
        }
    }

    #[async_trait]
    impl JwksFetcher for StaticFetcher {
        async fn fetch(&self, _url: &str) -> Result<JwkDocument, JwksError> {
            Ok(self.doc.clone())
        }
    }

    #[async_trait]
    impl JwksResolver for StaticFetcher {
        async fn discover_jwks_url(&self, _issuer: &str) -> Result<String, JwksError> {
            Err(JwksError::MissingJwksUrl)
        }
    }

    fn generate_rsa_material(kid: &str) -> anyhow::Result<(EncodingKey, JwkDocument)> {
        let mut rng = OsRng;
        let private_key = RsaPrivateKey::new(&mut rng, 2048)?;
        let n = URL_SAFE_NO_PAD.encode(private_key.n().to_bytes_be());
        let e = URL_SAFE_NO_PAD.encode(private_key.e().to_bytes_be());
        let jwk = JwkKey {
            kid: kid.to_string(),
            kty: Some("RSA".to_string()),
            n: Some(n),
            e: Some(e),
            x: None,
            y: None,
            crv: None,
        };
        let der = private_key.to_pkcs1_der()?;
        let encoding_key = EncodingKey::from_rsa_der(der.as_bytes());
        Ok((encoding_key, JwkDocument { keys: vec![jwk] }))
    }

    fn sign_test_token(
        kid: &str,
        encoding_key: &EncodingKey,
        issuer: &str,
        audience: &str,
    ) -> anyhow::Result<String> {
        #[derive(serde::Serialize)]
        struct Claims {
            sub: String,
            email: String,
            iss: String,
            aud: String,
            exp: usize,
            name: String,
            roles: Vec<String>,
        }

        let mut header = Header::new(Algorithm::RS256);
        header.kid = Some(kid.to_string());
        let exp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() + 900;
        let claims = Claims {
            sub: "user-123".into(),
            email: "user@example.com".into(),
            iss: issuer.into(),
            aud: audience.into(),
            exp: exp as usize,
            name: "Test User".into(),
            roles: vec!["admin".into(), "editor".into()],
        };
        let token = encode(&header, &claims, encoding_key)?;
        Ok(token)
    }
}
async fn extract_principal_from_providers<F>(
    jwks: &JwksManager<F>,
    config: &SsoSettings,
    headers: &HeaderMap,
) -> Result<Option<SsoPrincipal>, Response>
where
    F: JwksFetcher + JwksResolver + Clone,
{
    if config.providers.is_empty() {
        return Ok(None);
    }

    for provider in &config.providers {
        let token = match extract_token_from_source(headers, &provider.token_source)? {
            Some(token) => token,
            None => continue,
        };

        match jwks.verify(&token, provider).await {
            Ok(claims) => {
                let principal = map_claims_to_principal(provider, &config.role_claims, &claims)?;
                return Ok(Some(principal));
            }
            Err(JwksError::MissingJwksUrl) => {
                error!(provider = %provider.name, "JWKS URL missing for provider");
                let api_error: APIErrorResponse<(), ()> = APIErrorResponse {
                    message: "SSO provider JWKS URL not configured".into(),
                    details: None,
                    error: None,
                };
                return Err(ResponseBuilder::internal_server_error().json(&api_error));
            }
            Err(error) => {
                warn!(provider = %provider.name, %error, "OIDC token verification failed");
                continue;
            }
        }
    }

    Ok(None)
}
fn extract_token_from_source(
    headers: &HeaderMap,
    source: &TokenSource,
) -> Result<Option<String>, Response> {
    match source {
        TokenSource::Header { name, prefix } => {
            let value = header_value(headers, name)?;
            Ok(value.map(|raw| strip_prefix(raw, prefix)))
        }
        TokenSource::Cookie { name } => {
            let jar = CookieJar::from_headers(headers);
            let token = jar
                .get(name)
                .map(|cookie| cookie.value().trim())
                .filter(|value| !value.is_empty())
                .map(|value| value.to_string());
            Ok(token)
        }
    }
}

fn strip_prefix(raw: String, prefix: &Option<String>) -> String {
    if let Some(prefix) = prefix {
        raw.strip_prefix(prefix)
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or(raw.trim().to_string())
    } else {
        raw.trim().to_string()
    }
}

fn claim_value<'a>(
    claims: &'a serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Option<&'a str> {
    claims
        .get(key)
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn map_claims_to_principal(
    provider: &OidcProviderConfig,
    global_role_claims: &[String],
    claims: &serde_json::Map<String, serde_json::Value>,
) -> Result<SsoPrincipal, Response> {
    let username_claim = provider
        .subject_claim
        .as_deref()
        .and_then(|name| claim_value(claims, name))
        .or_else(|| claim_value(claims, "preferred_username"))
        .or_else(|| claim_value(claims, "cognito:username"))
        .or_else(|| claim_value(claims, "sub"));

    let Some(raw_username) = username_claim else {
        let api_error: APIErrorResponse<(), ()> = APIErrorResponse {
            message: "Token missing username claim".into(),
            details: None,
            error: None,
        };
        return Err(ResponseBuilder::forbidden().json(&api_error));
    };

    let email = provider
        .email_claim
        .as_deref()
        .and_then(|name| claim_value(claims, name))
        .or_else(|| claim_value(claims, "email"))
        .map(str::to_string);

    let display_name = provider
        .display_name_claim
        .as_deref()
        .and_then(|name| claim_value(claims, name))
        .or_else(|| claim_value(claims, "name"))
        .or_else(|| claim_value(claims, "common_name"))
        .unwrap_or(raw_username);

    let mut roles = extract_roles(claims, &provider.role_claims);
    let mut global = extract_roles(claims, global_role_claims);
    roles.append(&mut global);
    roles.sort();
    roles.dedup();

    Ok(SsoPrincipal {
        username: normalize_username(raw_username),
        email,
        display_name: display_name.to_string(),
        roles,
    })
}

fn extract_roles(
    claims: &serde_json::Map<String, serde_json::Value>,
    role_claims: &[String],
) -> Vec<String> {
    let mut roles = Vec::new();
    for key in role_claims {
        if let Some(value) = claims.get(key) {
            match value {
                serde_json::Value::String(s) => {
                    if !s.trim().is_empty() {
                        roles.push(s.trim().to_string());
                    }
                }
                serde_json::Value::Array(items) => {
                    for item in items {
                        if let Some(s) = item.as_str() {
                            if !s.trim().is_empty() {
                                roles.push(s.trim().to_string());
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }
    roles.sort();
    roles.dedup();
    roles
}
