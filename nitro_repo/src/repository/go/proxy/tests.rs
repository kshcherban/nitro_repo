#![allow(clippy::expect_used, clippy::panic, clippy::todo, clippy::unwrap_used)]
use super::*;

#[test]
fn test_proxy_route_sorting() {
    let mut routes = vec![
        GoProxyRoute {
            url: ProxyURL::try_from("https://low.example.com".to_string()).unwrap(),
            name: Some("Low Priority".to_string()),
            priority: Some(1),
        },
        GoProxyRoute {
            url: ProxyURL::try_from("https://high.example.com".to_string()).unwrap(),
            name: Some("High Priority".to_string()),
            priority: Some(10),
        },
        GoProxyRoute {
            url: ProxyURL::try_from("https://medium.example.com".to_string()).unwrap(),
            name: Some("Medium Priority".to_string()),
            priority: Some(5),
        },
    ];

    routes.sort_by_key(|route| -route.priority());

    assert_eq!(routes[0].priority(), 10);
    assert_eq!(routes[1].priority(), 5);
    assert_eq!(routes[2].priority(), 1);
}

#[test]
fn test_normalize_routes() {
    let empty_routes: Vec<GoProxyRoute> = vec![];
    let normalized = normalize_routes(empty_routes);
    assert_eq!(normalized.len(), 1);
    assert_eq!(normalized[0].name.as_deref(), Some("Go Official Proxy"));

    let custom_routes = vec![
        GoProxyRoute {
            url: ProxyURL::try_from("https://custom1.example.com".to_string()).unwrap(),
            name: Some("Custom1".to_string()),
            priority: Some(10),
        },
        GoProxyRoute {
            url: ProxyURL::try_from("https://custom2.example.com".to_string()).unwrap(),
            name: Some("Custom2".to_string()),
            priority: Some(5),
        },
    ];

    let normalized = normalize_routes(custom_routes);
    assert_eq!(normalized.len(), 2);
    assert_eq!(normalized[0].priority(), 10); // Higher priority first
    assert_eq!(normalized[1].priority(), 5);
}
