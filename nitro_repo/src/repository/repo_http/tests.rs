#![allow(clippy::expect_used, clippy::panic, clippy::todo, clippy::unwrap_used)]
use super::*;

#[test]
fn docker_v2_ok_response_sets_headers() {
    let response = docker_v2_ok_response();
    assert_eq!(response.status(), StatusCode::OK);
    let headers = response.headers();
    assert_eq!(
        headers
            .get("Docker-Distribution-API-Version")
            .and_then(|value| value.to_str().ok()),
        Some(DOCKER_API_VERSION)
    );
    assert_eq!(
        headers
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok()),
        Some(DOCKER_JSON_CONTENT_TYPE)
    );
}

#[test]
fn docker_v2_unauthorized_response_sets_challenge() {
    let response = docker_v2_unauthorized_response("Bearer realm=\"test\"", "{}");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let headers = response.headers();
    assert_eq!(
        headers
            .get("WWW-Authenticate")
            .and_then(|value| value.to_str().ok()),
        Some("Bearer realm=\"test\"")
    );
}

#[test]
fn www_authenticate_response_sets_header_and_body() {
    let response =
        RepoResponse::www_authenticate("Basic realm=\"Nitro Repo\"").into_response_default();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let headers = response.headers();
    assert_eq!(
        headers
            .get("WWW-Authenticate")
            .and_then(|value| value.to_str().ok()),
        Some("Basic realm=\"Nitro Repo\"")
    );
}

#[test]
fn forbidden_response_returns_expected_status_and_message() {
    let response = RepoResponse::forbidden().into_response_default();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[test]
fn unsupported_method_response_mentions_method() {
    let response =
        RepoResponse::unsupported_method_response(Method::POST, "docker").into_response_default();
    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
}
