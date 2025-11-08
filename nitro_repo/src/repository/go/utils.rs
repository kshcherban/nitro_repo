use axum::extract::Path as AxumPath;
use nr_core::storage::StoragePath;

use super::types::{GoModuleError, GoModulePath, GoVersion};

/// Extract Go module information from a request path
#[derive(Debug, Clone)]
pub struct GoModuleRequest {
    pub module_path: GoModulePath,
    pub version: Option<GoVersion>,
    pub request_type: GoRequestType,
    pub sumdb_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GoRequestType {
    ListVersions,        // GET /{module}/@v/list
    VersionInfo,         // GET /{module}/@v/{version}.info
    GoMod,               // GET /{module}/@v/{version}.mod
    ModuleZip,           // GET /{module}/@v/{version}.zip
    Latest,              // GET /{module}/@latest
    GoModWithoutVersion, // GET /{module}/go.mod (deprecated)
    SumdbSupported,      // GET /sumdb/{sumdb-name}/supported
    SumdbLookup,         // GET /sumdb/{sumdb-name}/lookup/{@domain}/{path}
    SumdbTile,           // GET /sumdb/{sumdb-name}/tile/{height}/{hash}
}

impl GoModuleRequest {
    /// Parse a request path into Go module information
    pub fn from_path(path: &str) -> Result<Self, GoModuleError> {
        let path = path.trim_start_matches('/');

        if path.is_empty() {
            return Err(GoModuleError::EmptyPath);
        }

        // Handle sumdb requests
        if let Some(sumdb_path) = path.strip_prefix("sumdb/") {
            return Self::parse_sumdb_request(sumdb_path);
        }

        // Handle special endpoints
        if let Some(module_part) = path.strip_suffix("/@latest") {
            let module_path = GoModulePath::new(module_part)?;
            return Ok(GoModuleRequest {
                module_path,
                version: None,
                request_type: GoRequestType::Latest,
                sumdb_path: None,
            });
        }

        if let Some(module_part) = path.strip_suffix("/@v/list") {
            let module_path = GoModulePath::new(module_part)?;
            return Ok(GoModuleRequest {
                module_path,
                version: None,
                request_type: GoRequestType::ListVersions,
                sumdb_path: None,
            });
        }

        if let Some((module_part, file_part)) = path.split_once("/@v/") {
            let module_path = GoModulePath::new(module_part)?;

            if let Some(version_str) = file_part.strip_suffix(".info") {
                let version = GoVersion::new(version_str)?;
                return Ok(GoModuleRequest {
                    module_path,
                    version: Some(version),
                    request_type: GoRequestType::VersionInfo,
                    sumdb_path: None,
                });
            }

            if let Some(version_str) = file_part.strip_suffix(".mod") {
                let version = GoVersion::new(version_str)?;
                return Ok(GoModuleRequest {
                    module_path,
                    version: Some(version),
                    request_type: GoRequestType::GoMod,
                    sumdb_path: None,
                });
            }

            if let Some(version_str) = file_part.strip_suffix(".zip") {
                let version = GoVersion::new(version_str)?;
                return Ok(GoModuleRequest {
                    module_path,
                    version: Some(version),
                    request_type: GoRequestType::ModuleZip,
                    sumdb_path: None,
                });
            }
        }

        // Handle deprecated go.mod without version
        if let Some(module_part) = path.strip_suffix("/go.mod") {
            let module_path = GoModulePath::new(module_part)?;
            return Ok(GoModuleRequest {
                module_path,
                version: None,
                request_type: GoRequestType::GoModWithoutVersion,
                sumdb_path: None,
            });
        }

        // If we can't parse it as a specific endpoint, treat it as an invalid path
        Err(GoModuleError::InvalidModulePath(path.to_string()))
    }

    /// Get the storage path for this request
    pub fn storage_path(&self) -> StoragePath {
        match &self.request_type {
            GoRequestType::ListVersions => {
                StoragePath::from(format!("{}/@v/list", self.module_path.as_str()))
            }
            GoRequestType::VersionInfo => {
                let version = self.version.as_ref().unwrap();
                StoragePath::from(format!(
                    "{}/@v/{}.info",
                    self.module_path.as_str(),
                    version.as_str()
                ))
            }
            GoRequestType::GoMod => {
                let version = self.version.as_ref().unwrap();
                StoragePath::from(format!(
                    "{}/@v/{}.mod",
                    self.module_path.as_str(),
                    version.as_str()
                ))
            }
            GoRequestType::ModuleZip => {
                let version = self.version.as_ref().unwrap();
                StoragePath::from(format!(
                    "{}/@v/{}.zip",
                    self.module_path.as_str(),
                    version.as_str()
                ))
            }
            GoRequestType::Latest => {
                StoragePath::from(format!("{}/@latest", self.module_path.as_str()))
            }
            GoRequestType::GoModWithoutVersion => {
                StoragePath::from(format!("{}/go.mod", self.module_path.as_str()))
            }
            GoRequestType::SumdbSupported => StoragePath::from("sumdb/supported"),
            GoRequestType::SumdbLookup => StoragePath::from("sumdb/lookup"),
            GoRequestType::SumdbTile => StoragePath::from("sumdb/tile"),
        }
    }

    /// Check if this is a read request
    pub fn is_read_request(&self) -> bool {
        matches!(
            self.request_type,
            GoRequestType::ListVersions
                | GoRequestType::VersionInfo
                | GoRequestType::GoMod
                | GoRequestType::ModuleZip
                | GoRequestType::Latest
                | GoRequestType::GoModWithoutVersion
                | GoRequestType::SumdbSupported
                | GoRequestType::SumdbLookup
                | GoRequestType::SumdbTile
        )
    }

    /// Check if this request requires a version
    pub fn requires_version(&self) -> bool {
        matches!(
            self.request_type,
            GoRequestType::VersionInfo | GoRequestType::GoMod | GoRequestType::ModuleZip
        )
    }

    /// Parse a sumdb request path
    fn parse_sumdb_request(path: &str) -> Result<Self, GoModuleError> {
        tracing::debug!("Parsing sumdb request path: {}", path);

        // Create a dummy module path for sumdb requests (they don't follow normal module patterns)
        let dummy_module = GoModulePath::new("sumdb.request")?;

        if path.ends_with("/supported") {
            // Handle /sumdb/{sumdb-name}/supported
            let normalized = path
                .trim_start_matches("sum.golang.org/")
                .trim_start_matches('/');
            let stored = if normalized.is_empty() {
                "supported".to_string()
            } else {
                normalized.to_string()
            };
            if stored.contains("..") {
                return Err(GoModuleError::InvalidRequest(format!(
                    "Invalid sumdb path: {}",
                    path
                )));
            }
            Ok(GoModuleRequest {
                module_path: dummy_module,
                version: None,
                request_type: GoRequestType::SumdbSupported,
                sumdb_path: Some(stored),
            })
        } else if let Some(lookup_path) = path.strip_prefix("sum.golang.org/lookup/") {
            let lookup_path = lookup_path.trim_start_matches('/');
            if lookup_path.contains("..") {
                return Err(GoModuleError::InvalidRequest(format!(
                    "Invalid sumdb lookup path: {}",
                    path
                )));
            }
            // Handle /sumdb/sum.golang.org/lookup/{@domain}/{path}
            Ok(GoModuleRequest {
                module_path: dummy_module,
                version: None,
                request_type: GoRequestType::SumdbLookup,
                sumdb_path: Some(format!("lookup/{}", lookup_path)),
            })
        } else if let Some(tile_path) = path.strip_prefix("sum.golang.org/tile/") {
            let tile_path = tile_path.trim_start_matches('/');
            if tile_path.contains("..") {
                return Err(GoModuleError::InvalidRequest(format!(
                    "Invalid sumdb tile path: {}",
                    path
                )));
            }
            // Handle /sumdb/sum.golang.org/tile/{height}/{hash}
            Ok(GoModuleRequest {
                module_path: dummy_module,
                version: None,
                request_type: GoRequestType::SumdbTile,
                sumdb_path: Some(format!("tile/{}", tile_path)),
            })
        } else {
            tracing::warn!("No match for sumdb path: {}", path);
            Err(GoModuleError::InvalidRequest(format!(
                "Unsupported sumdb path: {}",
                path
            )))
        }
    }
}

/// Convert an Axum path extractor to a GoModuleRequest
impl TryFrom<AxumPath<String>> for GoModuleRequest {
    type Error = crate::utils::bad_request::BadRequestErrors;

    fn try_from(path: AxumPath<String>) -> Result<Self, Self::Error> {
        GoModuleRequest::from_path(&path.0).map_err(|e| {
            crate::utils::bad_request::BadRequestErrors::Other(format!(
                "Invalid Go module path: {}",
                e
            ))
        })
    }
}

/// Generate Go module info JSON content
pub fn generate_go_module_info(
    module_path: &GoModulePath,
    version: &GoVersion,
    time: chrono::DateTime<chrono::Utc>,
) -> Result<String, GoModuleError> {
    #[derive(serde::Serialize)]
    struct GoModuleInfoJson {
        #[serde(rename = "Version")]
        version: String,
        #[serde(rename = "Time")]
        time: String,
    }

    let info = GoModuleInfoJson {
        version: version.as_str().to_string(),
        time: time.format("%Y-%m-%dT%H:%M:%SZ").to_string(),
    };

    serde_json::to_string(&info)
        .map_err(|e| GoModuleError::InvalidModulePath(format!("Failed to serialize info: {}", e)))
}

/// Generate a basic go.mod file content
pub fn generate_go_mod(module_path: &str) -> String {
    format!("module {}\n\ngo 1.21\n", module_path)
}

/// Validate that a version string is compatible with Go module requirements
pub fn validate_version_for_go(version: &str) -> Result<(), GoModuleError> {
    let _go_version = GoVersion::new(version)?;
    Ok(())
}

/// Check if a module path is a major version suffix
pub fn has_major_version_suffix(module_path: &str) -> bool {
    if let Some((_, suffix)) = module_path.rsplit_once('/') {
        suffix.starts_with('v') && suffix[1..].chars().all(|c| c.is_ascii_digit())
    } else {
        false
    }
}

/// Extract major version from module path if present
pub fn extract_major_version(module_path: &str) -> Option<u32> {
    if let Some((_, suffix)) = module_path.rsplit_once('/') {
        if suffix.starts_with('v') && suffix.len() > 1 {
            suffix[1..].parse::<u32>().ok()
        } else {
            None
        }
    } else {
        None
    }
}
