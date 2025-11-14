use nr_core::utils::base64_utils;
use tracing::{error, instrument};

use crate::utils::bad_request::{BadRequestErrors, InvalidAuthorizationHeader};

#[derive(Debug)]
pub enum AuthorizationHeader {
    Basic { username: String, password: String },
    Bearer { token: String },
    Session { session: String },
    Other { scheme: String, value: String },
}
impl TryFrom<String> for AuthorizationHeader {
    type Error = BadRequestErrors;
    #[instrument(skip(value), name = "AuthorizationHeader::try_from")]
    fn try_from(value: String) -> Result<Self, Self::Error> {
        let Some(pos) = value.find(' ') else {
            if value.is_empty() {
                return Err(BadRequestErrors::InvalidAuthorizationHeader(
                    InvalidAuthorizationHeader::InvalidFormat,
                ));
            }
            return Ok(AuthorizationHeader::Bearer { token: value });
        };
        let (scheme, rest) = value.split_at(pos);
        let token = rest.trim_start();
        if token.is_empty() {
            return Err(BadRequestErrors::InvalidAuthorizationHeader(
                InvalidAuthorizationHeader::InvalidFormat,
            ));
        }
        match scheme.to_ascii_lowercase().as_str() {
            "basic" => parse_basic_header(token),
            "bearer" | "token" => Ok(AuthorizationHeader::Bearer {
                token: token.to_owned(),
            }),
            "session" => Ok(AuthorizationHeader::Session {
                session: token.to_owned(),
            }),
            _ => Ok(AuthorizationHeader::Other {
                scheme: scheme.to_owned(),
                value: token.to_owned(),
            }),
        }
    }
}
#[instrument(skip(header))]
fn parse_basic_header(header: &str) -> Result<AuthorizationHeader, BadRequestErrors> {
    let decoded = base64_utils::decode(header).map_err(|err| {
        error!("Failed to decode base64: {}", err);
        InvalidAuthorizationHeader::InvalidValue
    })?;
    let decoded = String::from_utf8(decoded).map_err(|err| {
        error!("Failed to convert bytes to string: {}", err);
        InvalidAuthorizationHeader::InvalidValue
    })?;
    let parts: Vec<&str> = decoded.split(':').collect();
    if parts.len() != 2 {
        return Err(InvalidAuthorizationHeader::InvalidBasicValue.into());
    }
    let username = parts[0];
    let password = parts[1];
    Ok(AuthorizationHeader::Basic {
        username: username.to_owned(),
        password: password.to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::AuthorizationHeader;

    fn parse(header: &str) -> AuthorizationHeader {
        AuthorizationHeader::try_from(header.to_string()).expect("valid header")
    }

    #[test]
    fn token_scheme_is_treated_as_auth_token() {
        let header = parse("Token abc123");
        match header {
            AuthorizationHeader::Bearer { token } => assert_eq!(token, "abc123"),
            other => panic!("expected bearer token, got {other:?}"),
        }
    }

    #[test]
    fn authorization_scheme_is_case_insensitive() {
        let schemes = ["basic", "Bearer", "TOKEN", "SeSsIoN"];
        // "user:pass" in base64
        let encoded_basic = "dXNlcjpwYXNz";

        // basic
        match parse(&format!("{} {}", schemes[0], encoded_basic)) {
            AuthorizationHeader::Basic { username, password } => {
                assert_eq!(username, "user");
                assert_eq!(password, "pass");
            }
            other => panic!("expected basic header, got {other:?}"),
        }

        // bearer/token share logic
        for scheme in &schemes[1..3] {
            match parse(&format!("{scheme} super-secret")) {
                AuthorizationHeader::Bearer { token } => {
                    assert_eq!(token, "super-secret");
                }
                other => panic!("expected bearer token for {scheme}, got {other:?}"),
            }
        }

        // session
        match parse(&format!("{} abcdef", schemes[3])) {
            AuthorizationHeader::Session { session } => assert_eq!(session, "abcdef"),
            other => panic!("expected session header, got {other:?}"),
        }
    }

    #[test]
    fn bare_authorization_header_is_token() {
        let header = parse("super-secret-token");
        match header {
            AuthorizationHeader::Bearer { token } => assert_eq!(token, "super-secret-token"),
            other => panic!("expected bare token to map to bearer, got {other:?}"),
        }
    }
}
