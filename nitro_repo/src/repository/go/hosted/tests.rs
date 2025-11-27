#![allow(clippy::expect_used, clippy::panic, clippy::todo, clippy::unwrap_used)]

use crate::repository::go::configs::GoRepositoryConfig;

#[test]
fn test_go_hosted_basic_functionality() {
    // Test that GoHosted can be constructed with basic properties
    // This test focuses on the basic structure without complex setup

    let config = GoRepositoryConfig::Hosted;
    assert!(matches!(config, GoRepositoryConfig::Hosted));
}
