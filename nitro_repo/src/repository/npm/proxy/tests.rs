#![allow(clippy::expect_used, clippy::panic, clippy::todo, clippy::unwrap_used)]
use super::*;

#[test]
fn cache_path_for_scoped_package() {
    let path = StoragePath::from("@scope/package/-/package-1.0.0.tgz");
    let cache = cache_path_for_npm_proxy(&path).expect("cache path");
    assert_eq!(
        cache.to_string(),
        "packages/@scope/package/package-1.0.0.tgz"
    );
}

#[test]
fn cache_path_for_unscoped_package() {
    let path = StoragePath::from("left-pad/-/left-pad-1.3.0.tgz");
    let cache = cache_path_for_npm_proxy(&path).expect("cache path");
    assert_eq!(cache.to_string(), "packages/left-pad/left-pad-1.3.0.tgz");
}

#[test]
fn cache_path_requires_tarball_segment() {
    let path = StoragePath::from("left-pad/latest");
    assert!(cache_path_for_npm_proxy(&path).is_none());
}

#[test]
fn normalize_routes_adds_default_when_empty() {
    let routes = normalize_routes(Vec::new());
    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0], DEFAULT_ROUTE.clone());
}

#[test]
fn normalize_routes_keeps_existing_entries() {
    let custom = NpmProxyRoute {
        url: ProxyURL::try_from(String::from("https://mirror.npm.internal")).expect("valid url"),
        name: Some("mirror".to_string()),
    };
    let routes = normalize_routes(vec![custom.clone()]);
    assert_eq!(routes, vec![custom]);
}
