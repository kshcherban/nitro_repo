#![allow(clippy::expect_used, clippy::panic, clippy::todo, clippy::unwrap_used)]
use super::*;

#[test]
fn cache_path_for_simple_package() {
    let path = StoragePath::from("simple/Example_Pkg/example-1.0.0.whl");
    let cache = cache_path_for_python_proxy(&path).expect("cache path");
    assert_eq!(cache.to_string(), "packages/example-pkg/example-1.0.0.whl");
}

#[test]
fn cache_path_for_directory_returns_none() {
    let path = StoragePath::from("simple/example/");
    assert!(cache_path_for_python_proxy(&path).is_none());
}

#[test]
fn cache_path_preserves_existing_packages_path() {
    let path = StoragePath::from("packages/example/example-1.0.0.whl");
    let cache = cache_path_for_python_proxy(&path).expect("cache path");
    assert_eq!(cache.to_string(), "packages/example/example-1.0.0.whl");
}

#[test]
fn normalize_routes_injects_default_when_empty() {
    let routes = normalize_routes(Vec::new());
    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0], DEFAULT_ROUTE.clone());
}

#[test]
fn normalize_routes_retains_existing_entries() {
    let custom = PythonProxyRoute {
        url: ProxyURL::try_from(String::from("https://internal.example/simple"))
            .expect("valid url"),
        name: Some("Internal".to_string()),
    };
    let routes = normalize_routes(vec![custom.clone()]);
    assert_eq!(routes, vec![custom]);
}
