#![allow(clippy::expect_used, clippy::panic, clippy::todo, clippy::unwrap_used)]
use super::*;
use axum_extra::extract::cookie::Cookie;
use chrono::{DateTime, FixedOffset};
use http_body_util::BodyExt;
use nr_core::{
    database::entities::user::auth_token::AuthToken,
    user::{Email, Username, permissions::RepositoryActions},
};
use serde_json::json;

fn fixed_time() -> DateTime<FixedOffset> {
    DateTime::parse_from_rfc3339("2024-01-01T00:00:00+00:00").unwrap()
}

fn sample_user() -> UserSafeData {
    UserSafeData {
        id: 1,
        name: "Test User".into(),
        username: Username::new("test_user".into()).unwrap(),
        email: Email::new("user@example.com".into()).unwrap(),
        require_password_change: false,
        active: true,
        admin: false,
        user_manager: false,
        system_manager: false,
        default_repository_actions: vec![RepositoryActions::Read],
        updated_at: fixed_time(),
        created_at: fixed_time(),
    }
}

fn sample_session() -> Session {
    Session {
        user_id: 1,
        session_id: "session-id".into(),
        user_agent: "agent".into(),
        ip_address: "127.0.0.1".into(),
        expires: fixed_time(),
        created: fixed_time(),
    }
}

fn sample_auth_token() -> AuthToken {
    AuthToken {
        id: 1,
        user_id: 1,
        name: Some("token".into()),
        description: None,
        token: "token".into(),
        active: true,
        source: "test".into(),
        expires_at: None,
        created_at: fixed_time(),
    }
}

fn sample_me_with_session() -> MeWithSession {
    MeWithSession::from((sample_session(), sample_user()))
}

#[tokio::test]
async fn me_returns_bad_request_for_auth_token() {
    let response = me(Authentication::AuthToken(
        sample_auth_token(),
        sample_user(),
    ))
    .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(
        std::str::from_utf8(&bytes).unwrap(),
        "Use whoami instead of me for Auth Tokens"
    );
}

#[tokio::test]
async fn login_response_sets_cookie_and_body() {
    let cookie = Cookie::build(("session", "abc123"))
        .secure(true)
        .path("/")
        .build();
    let me = sample_me_with_session();
    let cookie_value = cookie.encoded().to_string();

    let response = login_success_response(cookie.clone(), me.clone());

    assert_eq!(response.status(), StatusCode::OK);
    let headers = response.headers();
    assert_eq!(
        headers
            .get(SET_COOKIE)
            .and_then(|value| value.to_str().ok()),
        Some(cookie_value.as_str())
    );
    assert_eq!(
        headers
            .get(http::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok()),
        Some("application/json")
    );

    let collected = response.into_body().collect().await.unwrap();
    let body = collected.to_bytes();
    let payload: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(payload, json!(me));
}
