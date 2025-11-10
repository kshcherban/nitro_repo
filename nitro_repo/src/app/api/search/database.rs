use chrono::{DateTime, FixedOffset};
use serde_json::Value;
use sqlx::FromRow;

use super::{InternalError, PackageSearchResult, RepositorySummary, query_parser::SearchQuery};
use crate::app::NitroRepo;

#[derive(Debug, Clone, FromRow)]
pub struct DatabasePackageRow {
    pub package_name: String,
    pub package_key: String,
    pub version: String,
    pub path: String,
    #[sqlx(json)]
    pub extra: Option<Value>,
    pub updated_at: DateTime<FixedOffset>,
}

pub async fn search_database_packages(
    site: &NitroRepo,
    summary: &RepositorySummary,
    query: &SearchQuery,
    limit: usize,
) -> Result<Vec<PackageSearchResult>, InternalError> {
    if limit == 0 {
        return Ok(Vec::new());
    }
    let fetch_limit = limit.max(1).saturating_mul(4).min(500);
    let rows = sqlx::query_as::<_, DatabasePackageRow>(
        r#"
            SELECT
                p.name AS package_name,
                p.key AS package_key,
                pv.version,
                pv.path,
                pv.extra,
                pv.updated_at
            FROM projects p
            INNER JOIN project_versions pv ON pv.project_id = p.id
            WHERE p.repository_id = $1
            ORDER BY pv.updated_at DESC
            LIMIT $2
        "#,
    )
    .bind(summary.repository_id)
    .bind(fetch_limit as i64)
    .fetch_all(&site.database)
    .await?;

    filter_database_rows(summary, rows, query, limit)
}

pub fn filter_database_rows(
    summary: &RepositorySummary,
    rows: Vec<DatabasePackageRow>,
    query: &SearchQuery,
    limit: usize,
) -> Result<Vec<PackageSearchResult>, InternalError> {
    if limit == 0 {
        return Ok(Vec::new());
    }

    let mut results = Vec::new();

    for row in rows {
        if results.len() >= limit {
            break;
        }

        let name_refs = [row.package_name.as_str(), row.package_key.as_str()];
        if !query.matches_package_names(&name_refs) {
            continue;
        }

        let term_fields = [
            row.package_name.as_str(),
            row.package_key.as_str(),
            row.version.as_str(),
        ];
        if !query.matches_terms(&term_fields) {
            continue;
        }

        if !query.matches_version(row.version.as_str()) {
            continue;
        }

        let file_name = format!("{}@{}", row.package_name, row.version);
        results.push(PackageSearchResult {
            repository_id: summary.repository_id,
            repository_name: summary.repository_name.clone(),
            storage_name: summary.storage_name.clone(),
            repository_type: summary.repository_type.clone(),
            file_name,
            cache_path: row.path.clone(),
            size: extract_size(&row.extra),
            modified: row.updated_at,
        });
    }

    Ok(results)
}

fn extract_size(extra: &Option<Value>) -> u64 {
    extra
        .as_ref()
        .and_then(|value| value.get("size"))
        .and_then(Value::as_u64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use chrono::{FixedOffset, TimeZone, Utc};
    use serde_json::json;
    use uuid::Uuid;

    use super::*;
    use crate::app::api::search::{
        query_parser::{Operator, SearchQuery},
        version_constraint::VersionConstraint,
    };

    fn make_row(name: &str, version: &str) -> DatabasePackageRow {
        DatabasePackageRow {
            package_name: name.to_string(),
            package_key: name.to_string(),
            version: version.to_string(),
            path: format!("{name}/{version}/"),
            extra: Some(json!({ "size": 1024 })),
            updated_at: Utc
                .with_ymd_and_hms(2025, 11, 1, 12, 0, 0)
                .single()
                .unwrap()
                .with_timezone(&FixedOffset::east_opt(0).unwrap()),
        }
    }

    #[tokio::test]
    async fn filter_database_rows_matches_package_name() {
        let summary = RepositorySummary {
            repository_id: Uuid::new_v4(),
            repository_name: "helm-hosted".into(),
            storage_name: "primary".into(),
            repository_type: "helm".into(),
        };

        let rows = vec![make_row("postgresql", "16.1.0"), make_row("nginx", "2.0.0")];
        let query = SearchQuery {
            package_filter: Some((Operator::Equals, "postgresql".to_string())),
            ..SearchQuery::default()
        };

        let results = filter_database_rows(&summary, rows, &query, 10).expect("query to pass");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].file_name, "postgresql@16.1.0");
    }

    #[tokio::test]
    async fn filter_database_rows_applies_limit() {
        let summary = RepositorySummary {
            repository_id: Uuid::new_v4(),
            repository_name: "helm-hosted".into(),
            storage_name: "primary".into(),
            repository_type: "helm".into(),
        };

        let rows = vec![
            make_row("chart-a", "1.0.0"),
            make_row("chart-a", "1.1.0"),
            make_row("chart-a", "1.2.0"),
        ];

        let query = SearchQuery::default();
        let results = filter_database_rows(&summary, rows, &query, 2).expect("limit to apply");
        assert_eq!(results.len(), 2);
    }

    #[tokio::test]
    async fn filter_database_rows_respects_version_constraint() {
        let summary = RepositorySummary {
            repository_id: Uuid::new_v4(),
            repository_name: "helm-hosted".into(),
            storage_name: "primary".into(),
            repository_type: "helm".into(),
        };

        let rows = vec![make_row("chart-a", "1.0.0"), make_row("chart-a", "2.0.0")];
        let query = SearchQuery {
            version_constraint: Some(VersionConstraint::Exact("1.0.0".to_string())),
            ..SearchQuery::default()
        };

        let results = filter_database_rows(&summary, rows, &query, 10).expect("query to pass");
        assert_eq!(results.len(), 1);
        assert!(results[0].file_name.contains("1.0.0"));
    }
}
