use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct SecuritySettings {
    pub allow_basic_without_tokens: bool,
    pub password_rules: Option<PasswordRules>,
    pub sso: Option<SsoSettings>,
}
impl Default for SecuritySettings {
    fn default() -> Self {
        Self {
            allow_basic_without_tokens: false,
            password_rules: Some(PasswordRules::default()),
            sso: None,
        }
    }
}

#[derive(Debug, Deserialize, Serialize, Clone, ToSchema)]
#[serde(default)]
pub struct SsoSettings {
    /// Enable SSO support. Disabled configurations are ignored at runtime.
    pub enabled: bool,
    /// Path or URL the UI should direct to when initiating SSO.
    pub login_path: String,
    /// Text used for the SSO button in the UI.
    pub login_button_text: String,
    /// Optional external identity provider URL used to initiate the SSO flow.
    pub provider_login_url: Option<String>,
    /// Optional query parameter on the provider login URL that indicates where to redirect after authentication.
    pub provider_redirect_param: Option<String>,
    /// Header that contains the external principal/username value.
    pub username_header: String,
    /// Optional header containing an email address for the principal.
    pub email_header: Option<String>,
    /// Optional header containing a display name for the principal.
    pub display_name_header: Option<String>,
    /// Automatically create a Nitro Repo account when the principal does not exist.
    pub auto_create_users: bool,
}

impl Default for SsoSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            login_path: default_login_path(),
            login_button_text: default_login_button_text(),
            provider_login_url: None,
            provider_redirect_param: None,
            username_header: default_username_header(),
            email_header: Some(default_email_header()),
            display_name_header: Some(default_display_name_header()),
            auto_create_users: false,
        }
    }
}

fn default_login_path() -> String {
    "/api/user/sso/login".to_string()
}

fn default_login_button_text() -> String {
    "Sign in with SSO".to_string()
}

fn default_username_header() -> String {
    "X-Forwarded-User".to_string()
}

fn default_email_header() -> String {
    "X-Forwarded-Email".to_string()
}

fn default_display_name_header() -> String {
    "X-Forwarded-Name".to_string()
}
#[derive(Debug, Deserialize, Serialize, Clone, ToSchema)]
pub struct PasswordRules {
    pub min_length: usize,
    pub require_uppercase: bool,
    pub require_lowercase: bool,
    pub require_number: bool,
    pub require_symbol: bool,
}
impl PasswordRules {
    pub fn validate(&self, password: &str) -> bool {
        if password.len() < self.min_length {
            return false;
        }
        if self.require_uppercase && !password.chars().any(|c| c.is_uppercase()) {
            return false;
        }
        if self.require_lowercase && !password.chars().any(|c| c.is_lowercase()) {
            return false;
        }
        if self.require_number && !password.chars().any(|c| c.is_numeric()) {
            return false;
        }
        if self.require_symbol && !password.chars().any(|c| c.is_ascii_punctuation()) {
            return false;
        }
        true
    }
}
impl Default for PasswordRules {
    fn default() -> Self {
        Self {
            min_length: 8,
            require_uppercase: true,
            require_lowercase: true,
            require_number: true,
            require_symbol: true,
        }
    }
}

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct TlsConfig {
    pub private_key: PathBuf,
    pub certificate_chain: PathBuf,
}
