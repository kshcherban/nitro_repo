use semver::{Version, VersionReq};

use super::query_parser::Operator;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VersionConstraint {
    Exact(String),
    Range { op: Operator, version: String },
    Semver(VersionReq),
}

impl VersionConstraint {
    pub fn matches(&self, version: &str) -> bool {
        match self {
            VersionConstraint::Exact(expected) => {
                if let (Some(lhs), Some(rhs)) = (
                    parse_semver_version(version),
                    parse_semver_version(expected),
                ) {
                    lhs == rhs
                } else {
                    version.eq_ignore_ascii_case(expected)
                }
            }
            VersionConstraint::Range {
                op,
                version: target,
            } => match op {
                Operator::Contains => version.to_lowercase().contains(&target.to_lowercase()),
                Operator::Equals => version.eq_ignore_ascii_case(target),
                Operator::GreaterThan
                | Operator::GreaterOrEqual
                | Operator::LessThan
                | Operator::LessOrEqual => compare_range(op, version, target),
            },
            VersionConstraint::Semver(req) => match parse_semver_version(version) {
                Some(parsed) => req.matches(&parsed),
                None => false,
            },
        }
    }
}

fn compare_range(op: &Operator, candidate: &str, target: &str) -> bool {
    if let (Some(lhs), Some(rhs)) = (
        parse_semver_version(candidate),
        parse_semver_version(target),
    ) {
        return match op {
            Operator::GreaterThan => lhs > rhs,
            Operator::GreaterOrEqual => lhs >= rhs,
            Operator::LessThan => lhs < rhs,
            Operator::LessOrEqual => lhs <= rhs,
            Operator::Equals => lhs == rhs,
            Operator::Contains => lhs
                .to_string()
                .to_lowercase()
                .contains(&target.to_lowercase()),
        };
    }

    let lhs = candidate.to_lowercase();
    let rhs = target.to_lowercase();
    match op {
        Operator::GreaterThan => lhs > rhs,
        Operator::GreaterOrEqual => lhs >= rhs,
        Operator::LessThan => lhs < rhs,
        Operator::LessOrEqual => lhs <= rhs,
        Operator::Equals => lhs == rhs,
        Operator::Contains => lhs.contains(&rhs),
    }
}

fn parse_semver_version(value: &str) -> Option<Version> {
    let trimmed = value.trim();
    let normalized = trimmed.strip_prefix('v').unwrap_or(trimmed);
    Version::parse(normalized).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_constraint_matches_semver() {
        let req = VersionReq::parse(">=1.2.0").unwrap();
        let constraint = VersionConstraint::Semver(req);
        assert!(constraint.matches("1.3.0"));
        assert!(!constraint.matches("1.1.9"));
    }

    #[test]
    fn version_constraint_matches_string_comparison() {
        let constraint = VersionConstraint::Range {
            op: Operator::GreaterOrEqual,
            version: "2024.10".to_string(),
        };
        assert!(constraint.matches("2024.11"));
        assert!(!constraint.matches("2024.09"));
    }

    #[test]
    fn version_constraint_contains_handles_substrings() {
        let constraint = VersionConstraint::Range {
            op: Operator::Contains,
            version: "beta".to_string(),
        };
        assert!(constraint.matches("1.0.0-beta.1"));
        assert!(!constraint.matches("1.0.0"));
    }

    #[test]
    fn version_constraint_exact_handles_leading_v() {
        let constraint = VersionConstraint::Exact("v1.2.3".to_string());
        assert!(constraint.matches("1.2.3"));
        assert!(constraint.matches("v1.2.3"));
        assert!(!constraint.matches("1.2.4"));
    }
}
