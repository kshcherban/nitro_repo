use async_trait::async_trait;
use serde_json::Value;
use uuid::Uuid;

use super::{InternalError, PackageSearchResult, RepositorySummary, query_parser::SearchQuery};
use crate::search::query::{DatabasePackageRow, PackageSearchRepository};
use nr_core::repository::project::DebPackageMetadata;

#[async_trait]
pub trait SearchBackend: Send + Sync {
    async fn fetch_repository_rows(
        &self,
        repository_id: Uuid,
        query: &SearchQuery,
        limit: usize,
    ) -> Result<Vec<DatabasePackageRow>, sqlx::Error>;

    async fn repository_has_index_rows(&self, repository_id: Uuid) -> Result<bool, sqlx::Error>;
}

#[async_trait]
impl<'a> SearchBackend for PackageSearchRepository<'a> {
    async fn fetch_repository_rows(
        &self,
        repository_id: Uuid,
        query: &SearchQuery,
        limit: usize,
    ) -> Result<Vec<DatabasePackageRow>, sqlx::Error> {
        self.fetch_repository_rows(repository_id, query, limit)
            .await
    }

    async fn repository_has_index_rows(&self, repository_id: Uuid) -> Result<bool, sqlx::Error> {
        self.repository_has_index_rows(repository_id).await
    }
}

pub async fn search_database_packages<B: SearchBackend + ?Sized>(
    searcher: &B,
    summary: &RepositorySummary,
    query: &SearchQuery,
    limit: usize,
) -> Result<Vec<PackageSearchResult>, InternalError> {
    if limit == 0 {
        return Ok(Vec::new());
    }
    let rows = searcher
        .fetch_repository_rows(summary.repository_id, query, limit)
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

        let repo_type = summary.repository_type.as_str();
        let mut metadata_terms: Vec<String> = Vec::new();
        let mut metadata_name_refs: Vec<String> = Vec::new();
        let metadata = if repo_type.eq_ignore_ascii_case("deb") {
            deb_metadata(&row.extra)
        } else {
            None
        };

        if let Some(meta) = &metadata {
            let terms = deb_metadata_terms(meta);
            metadata_name_refs.extend(terms.clone());
            metadata_terms = terms;
        }

        let mut name_refs: Vec<&str> = vec![row.package_name.as_str(), row.package_key.as_str()];
        name_refs.extend(metadata_name_refs.iter().map(String::as_str));

        if !query.matches_package_names(&name_refs) {
            continue;
        }

        let mut term_fields: Vec<&str> = vec![
            row.package_name.as_str(),
            row.package_key.as_str(),
            row.version.as_str(),
        ];
        term_fields.extend(metadata_terms.iter().map(String::as_str));

        if !query.matches_terms(&term_fields) {
            continue;
        }

        if !query.matches_version(row.version.as_str()) {
            continue;
        }

        let file_name = metadata
            .as_ref()
            .map(|meta| deb_file_name(meta))
            .unwrap_or_else(|| format!("{}@{}", row.package_name, row.version));
        let cache_path = metadata
            .as_ref()
            .map(|meta| meta.filename.clone())
            .unwrap_or_else(|| row.path.clone());

        results.push(PackageSearchResult {
            repository_id: summary.repository_id,
            repository_name: summary.repository_name.clone(),
            storage_name: summary.storage_name.clone(),
            repository_type: summary.repository_type.clone(),
            file_name,
            cache_path,
            size: extract_size(&row.extra),
            modified: row.updated_at,
        });
    }

    Ok(results)
}

fn deb_metadata(extra: &Option<Value>) -> Option<DebPackageMetadata> {
    extra
        .as_ref()
        .and_then(|value| serde_json::from_value(value.clone()).ok())
}

fn deb_metadata_terms(metadata: &DebPackageMetadata) -> Vec<String> {
    let mut terms: Vec<String> = Vec::new();
    terms.push(metadata.architecture.clone());
    terms.push(metadata.component.clone());
    terms.push(metadata.distribution.clone());
    if let Some(section) = &metadata.section {
        terms.push(section.clone());
    }
    if let Some(priority) = &metadata.priority {
        terms.push(priority.clone());
    }
    if let Some(ref maintainer) = metadata.maintainer {
        terms.push(maintainer.clone());
    }
    if let Some(ref homepage) = metadata.homepage {
        terms.push(homepage.clone());
    }
    if let Some(ref description) = metadata.description {
        terms.push(description.clone());
    }
    terms.push(metadata.filename.clone());
    terms.extend(metadata.depends.iter().cloned());
    terms.retain(|value| !value.trim().is_empty());
    terms
}

fn deb_file_name(metadata: &DebPackageMetadata) -> String {
    metadata
        .filename
        .rsplit('/')
        .next()
        .unwrap_or(&metadata.filename)
        .to_string()
}

fn extract_size(extra: &Option<Value>) -> u64 {
    extra
        .as_ref()
        .and_then(|value| value.get("size"))
        .and_then(Value::as_u64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests;
