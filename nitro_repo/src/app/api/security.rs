use axum::{Json, extract::State, response::Response, routing::get};
use nr_core::user::permissions::HasPermissions;
use tracing::{error, instrument};
use utoipa::{OpenApi, ToSchema};

use crate::{
    app::{
        NitroRepo,
        authentication::Authentication,
        authentication::oauth::normalize_scopes,
        config::{
            OAuth2CasbinConfig, OAuth2GoogleConfig, OAuth2GroupRoleMapping, OAuth2MicrosoftConfig,
            OAuth2Settings, SsoSettings,
        },
    },
    error::InternalError,
    utils::ResponseBuilder,
};

use serde::{Deserialize, Serialize};

#[derive(OpenApi)]
#[openapi(
    paths(
        get_sso_settings,
        update_sso_settings,
        get_oauth2_settings,
        update_oauth2_settings
    ),
    components(schemas(SsoSettings, OAuth2Settings))
)]
pub struct SecurityAPI;

pub fn security_routes() -> axum::Router<NitroRepo> {
    axum::Router::new()
        .route("/sso", get(get_sso_settings).put(update_sso_settings))
        .route(
            "/oauth2",
            get(get_oauth2_settings).put(update_oauth2_settings),
        )
}

#[derive(Debug, Serialize, ToSchema)]
struct OAuth2ProviderSettingsResponse {
    client_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    redirect_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tenant_id: Option<String>,
    scopes: Vec<String>,
    client_secret_configured: bool,
}

#[derive(Debug, Serialize, ToSchema)]
struct OAuth2SettingsResponse {
    enabled: bool,
    login_path: String,
    callback_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    redirect_base_url: Option<String>,
    auto_create_users: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    google: Option<OAuth2ProviderSettingsResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    microsoft: Option<OAuth2ProviderSettingsResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    casbin: Option<OAuth2CasbinConfig>,
    #[serde(default)]
    group_role_mappings: Vec<OAuth2GroupRoleMapping>,
    #[serde(default)]
    available_roles: Vec<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
struct OAuth2ProviderSettingsRequest {
    client_id: String,
    #[serde(default)]
    client_secret: Option<String>,
    #[serde(default)]
    scopes: Vec<String>,
    #[serde(default)]
    redirect_path: Option<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
struct OAuth2MicrosoftSettingsRequest {
    client_id: String,
    #[serde(default)]
    client_secret: Option<String>,
    #[serde(default)]
    tenant_id: Option<String>,
    #[serde(default)]
    scopes: Vec<String>,
    #[serde(default)]
    redirect_path: Option<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct OAuth2SettingsRequest {
    enabled: bool,
    login_path: String,
    callback_path: String,
    #[serde(default)]
    redirect_base_url: Option<String>,
    auto_create_users: bool,
    #[serde(default)]
    google: Option<OAuth2ProviderSettingsRequest>,
    #[serde(default)]
    microsoft: Option<OAuth2MicrosoftSettingsRequest>,
    #[serde(default)]
    casbin: Option<OAuth2CasbinConfig>,
    #[serde(default)]
    group_role_mappings: Vec<OAuth2GroupRoleMapping>,
}

fn sanitize_optional(value: Option<String>) -> Option<String> {
    value.and_then(|val| {
        let trimmed = val.trim().to_string();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    })
}

fn sanitize_relative_path(value: &str, fallback: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        fallback.to_string()
    } else if trimmed.starts_with('/') {
        trimmed.to_string()
    } else {
        format!("/{trimmed}")
    }
}

fn sanitize_scopes(scopes: Vec<String>) -> Vec<String> {
    let cleaned: Vec<String> = scopes
        .into_iter()
        .map(|scope| scope.trim().to_string())
        .filter(|scope| !scope.is_empty())
        .collect();
    normalize_scopes(&cleaned)
}

fn collect_available_roles(policy: &str) -> Vec<String> {
    let mut roles = Vec::new();
    for line in policy.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let mut segments = trimmed.split(',').map(|segment| segment.trim());
        let Some(rule_type) = segments.next() else {
            continue;
        };
        if rule_type != "p" {
            continue;
        }
        if let Some(role) = segments.next() {
            if !role.is_empty() {
                roles.push(role.to_string());
            }
        }
    }
    roles.sort();
    roles.dedup();
    roles
}

fn sanitize_casbin_config(mut cfg: OAuth2CasbinConfig) -> OAuth2CasbinConfig {
    let defaults = OAuth2CasbinConfig::default();
    let model = {
        let trimmed = cfg.model.trim();
        if trimmed.is_empty() {
            defaults.model.clone()
        } else {
            trimmed.to_string()
        }
    };
    let policy = {
        let trimmed = cfg.policy.trim();
        if trimmed.is_empty() {
            defaults.policy.clone()
        } else {
            trimmed.to_string()
        }
    };
    cfg.model = model;
    cfg.policy = policy;
    cfg
}

fn build_oauth2_response(settings: &OAuth2Settings) -> OAuth2SettingsResponse {
    let google = settings
        .google
        .as_ref()
        .map(|cfg| OAuth2ProviderSettingsResponse {
            client_id: cfg.client_id.clone(),
            redirect_path: cfg.redirect_path.clone(),
            tenant_id: None,
            scopes: cfg.scopes.clone(),
            client_secret_configured: !cfg.client_secret.is_empty(),
        });
    let microsoft = settings
        .microsoft
        .as_ref()
        .map(|cfg| OAuth2ProviderSettingsResponse {
            client_id: cfg.client_id.clone(),
            redirect_path: cfg.redirect_path.clone(),
            tenant_id: cfg.tenant_id.clone(),
            scopes: cfg.scopes.clone(),
            client_secret_configured: !cfg.client_secret.is_empty(),
        });

    OAuth2SettingsResponse {
        enabled: settings.enabled,
        login_path: settings.login_path.clone(),
        callback_path: settings.callback_path.clone(),
        redirect_base_url: settings.redirect_base_url.clone(),
        auto_create_users: settings.auto_create_users,
        google,
        microsoft,
        casbin: settings.casbin.clone(),
        group_role_mappings: settings.group_role_mappings.clone(),
        available_roles: settings
            .casbin
            .as_ref()
            .map(|cfg| collect_available_roles(&cfg.policy))
            .unwrap_or_default(),
    }
}

fn merge_oauth2_settings(
    current: Option<&OAuth2Settings>,
    request: OAuth2SettingsRequest,
) -> Result<OAuth2Settings, String> {
    let login_path = sanitize_relative_path(&request.login_path, "/api/user/oauth2/login");
    let callback_path = sanitize_relative_path(&request.callback_path, "/api/user/oauth2/callback");

    let google =
        merge_google_settings(current.and_then(|cfg| cfg.google.as_ref()), request.google)?;
    let microsoft = merge_microsoft_settings(
        current.and_then(|cfg| cfg.microsoft.as_ref()),
        request.microsoft,
    )?;

    let mut casbin = match request.casbin {
        Some(cfg) => Some(sanitize_casbin_config(cfg)),
        None => current.and_then(|cfg| cfg.casbin.clone()),
    };
    if request.enabled && casbin.is_none() {
        casbin = Some(OAuth2CasbinConfig::default());
    }

    let group_role_mappings = request
        .group_role_mappings
        .into_iter()
        .filter_map(|mapping| {
            let OAuth2GroupRoleMapping {
                provider,
                group,
                mut roles,
            } = mapping;
            let group_trimmed = group.trim();
            if group_trimmed.is_empty() {
                return None;
            }
            roles = roles
                .into_iter()
                .map(|role| role.trim().to_string())
                .filter(|role| !role.is_empty())
                .collect();
            if roles.is_empty() {
                return None;
            }
            roles.sort();
            roles.dedup();
            Some(OAuth2GroupRoleMapping {
                provider,
                group: group_trimmed.to_string(),
                roles,
            })
        })
        .collect();

    Ok(OAuth2Settings {
        enabled: request.enabled,
        login_path,
        callback_path,
        redirect_base_url: sanitize_optional(request.redirect_base_url),
        auto_create_users: request.auto_create_users,
        google,
        microsoft,
        casbin,
        group_role_mappings,
    })
}

fn merge_google_settings(
    current: Option<&OAuth2GoogleConfig>,
    request: Option<OAuth2ProviderSettingsRequest>,
) -> Result<Option<OAuth2GoogleConfig>, String> {
    let Some(req) = request else {
        return Ok(None);
    };

    let client_id = req.client_id.trim();
    if client_id.is_empty() {
        return Err("Google client ID is required".into());
    }

    let secret = match req.client_secret {
        Some(secret) => {
            let trimmed = secret.trim();
            if trimmed.is_empty() {
                return Err("Google client secret cannot be empty".into());
            }
            trimmed.to_string()
        }
        None => current
            .map(|cfg| cfg.client_secret.clone())
            .ok_or_else(|| "Provide a Google client secret".to_string())?,
    };

    Ok(Some(OAuth2GoogleConfig {
        client_id: client_id.to_string(),
        client_secret: secret,
        scopes: sanitize_scopes(req.scopes),
        redirect_path: sanitize_optional(req.redirect_path),
    }))
}

fn merge_microsoft_settings(
    current: Option<&OAuth2MicrosoftConfig>,
    request: Option<OAuth2MicrosoftSettingsRequest>,
) -> Result<Option<OAuth2MicrosoftConfig>, String> {
    let Some(req) = request else {
        return Ok(None);
    };

    let client_id = req.client_id.trim();
    if client_id.is_empty() {
        return Err("Microsoft client ID is required".into());
    }

    let secret = match req.client_secret {
        Some(secret) => {
            let trimmed = secret.trim();
            if trimmed.is_empty() {
                return Err("Microsoft client secret cannot be empty".into());
            }
            trimmed.to_string()
        }
        None => current
            .map(|cfg| cfg.client_secret.clone())
            .ok_or_else(|| "Provide a Microsoft client secret".to_string())?,
    };

    Ok(Some(OAuth2MicrosoftConfig {
        client_id: client_id.to_string(),
        client_secret: secret,
        tenant_id: sanitize_optional(req.tenant_id),
        scopes: sanitize_scopes(req.scopes),
        redirect_path: sanitize_optional(req.redirect_path),
    }))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::config::OAuth2ProviderKind;

    #[test]
    fn merge_google_settings_keeps_existing_secret_when_omitted() {
        let current = OAuth2GoogleConfig {
            client_id: "google-client".into(),
            client_secret: "super-secret".into(),
            scopes: vec!["openid".into(), "email".into()],
            redirect_path: None,
        };
        let request = Some(OAuth2ProviderSettingsRequest {
            client_id: "google-client".into(),
            client_secret: None,
            scopes: vec!["openid".into(), "email".into()],
            redirect_path: None,
        });

        let merged = merge_google_settings(Some(&current), request).expect("merge success");
        let new_config = merged.expect("google config");

        assert_eq!(new_config.client_id, "google-client");
        assert_eq!(new_config.client_secret, "super-secret");
        assert_eq!(
            new_config.scopes,
            vec!["openid".to_string(), "email".to_string()]
        );
    }

    #[test]
    fn merge_google_settings_requires_secret_initially() {
        let request = Some(OAuth2ProviderSettingsRequest {
            client_id: "google-client".into(),
            client_secret: None,
            scopes: vec!["openid".into()],
            redirect_path: None,
        });

        let merged = merge_google_settings(None, request);

        assert!(merged.is_err());
    }

    #[test]
    fn merge_oauth2_settings_sanitizes_paths_and_mappings() {
        let current = OAuth2Settings {
            enabled: false,
            login_path: "/api/user/oauth2/login".into(),
            callback_path: "/api/user/oauth2/callback".into(),
            redirect_base_url: None,
            auto_create_users: false,
            google: Some(OAuth2GoogleConfig {
                client_id: "google-client".into(),
                client_secret: "secret".into(),
                scopes: vec!["openid".into()],
                redirect_path: None,
            }),
            microsoft: None,
            casbin: None,
            group_role_mappings: vec![],
        };

        let request = OAuth2SettingsRequest {
            enabled: true,
            login_path: "oauth/login".into(),
            callback_path: "/oauth/callback".into(),
            redirect_base_url: Some("  ".into()),
            auto_create_users: true,
            google: Some(OAuth2ProviderSettingsRequest {
                client_id: "google-client".into(),
                client_secret: None,
                scopes: vec!["OpenID".into(), "email".into(), "".into()],
                redirect_path: Some("callback".into()),
            }),
            microsoft: None,
            casbin: None,
            group_role_mappings: vec![
                OAuth2GroupRoleMapping {
                    provider: OAuth2ProviderKind::Google,
                    group: "engineering".into(),
                    roles: vec!["admin".into(), "admin".into(), " ".into()],
                },
                OAuth2GroupRoleMapping {
                    provider: OAuth2ProviderKind::Google,
                    group: "   ".into(),
                    roles: vec!["ignored".into()],
                },
            ],
        };

        let merged =
            merge_oauth2_settings(Some(&current), request).expect("settings should merge safely");

        assert_eq!(merged.login_path, "/oauth/login");
        assert_eq!(merged.callback_path, "/oauth/callback");
        assert!(merged.redirect_base_url.is_none());
        assert!(merged.google.is_some());
        assert_eq!(merged.group_role_mappings.len(), 1);
        let mapping = &merged.group_role_mappings[0];
        assert_eq!(mapping.group, "engineering");
        assert_eq!(mapping.roles, vec!["admin"]);
        assert_eq!(
            merged
                .google
                .as_ref()
                .and_then(|cfg| cfg.redirect_path.clone())
                .as_deref(),
            Some("callback")
        );
        assert_eq!(
            merged.google.as_ref().map(|cfg| cfg.scopes.clone()),
            Some(vec!["OpenID".into(), "email".into()])
        );
    }
}

#[utoipa::path(
    get,
    path = "/oauth2",
    tag = "security",
    responses((status = 200, description = "Current OAuth2 configuration", body = OAuth2Settings)),
    security(("session" = []))
)]
#[instrument(skip(auth, site), fields(project_module = "Security"))]
pub async fn get_oauth2_settings(
    auth: Authentication,
    State(site): State<NitroRepo>,
) -> Result<Response, InternalError> {
    if !auth.is_admin_or_system_manager() {
        return Ok(ResponseBuilder::forbidden().body("Administrator permissions required"));
    }

    let security = site.security_settings();
    let settings = security.oauth2.unwrap_or_else(OAuth2Settings::default);
    let response = build_oauth2_response(&settings);
    Ok(ResponseBuilder::ok().json(&response))
}

#[utoipa::path(
    put,
    path = "/oauth2",
    tag = "security",
    request_body = OAuth2Settings,
    responses((status = 204, description = "OAuth2 configuration updated")),
    security(("session" = []))
)]
#[instrument(skip(auth, site, settings), fields(project_module = "Security"))]
pub async fn update_oauth2_settings(
    auth: Authentication,
    State(site): State<NitroRepo>,
    Json(settings): Json<OAuth2SettingsRequest>,
) -> Result<Response, InternalError> {
    if !auth.is_admin_or_system_manager() {
        return Ok(ResponseBuilder::forbidden().body("Administrator permissions required"));
    }

    let current = site.oauth2_settings_raw();
    let merged = match merge_oauth2_settings(current.as_ref(), settings) {
        Ok(settings) => settings,
        Err(err) => {
            error!(%err, "Invalid OAuth2 configuration submitted");
            return Ok(ResponseBuilder::bad_request().body(err));
        }
    };

    if let Err(err) = site.update_oauth2_settings(Some(merged)).await {
        error!(%err, "Failed to update OAuth2 configuration");
        return Ok(
            ResponseBuilder::internal_server_error().body("Failed to update OAuth2 configuration")
        );
    }

    Ok(ResponseBuilder::no_content().empty())
}
