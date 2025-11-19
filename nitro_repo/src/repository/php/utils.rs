use nr_core::{repository::project::ReleaseType, storage::StoragePath};
use serde::{Deserialize, Serialize};

use super::PhpRepositoryError;
use crate::repository::RepositoryHandlerError;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PhpPackagePathInfo {
    pub vendor: String,
    pub package: String,
    pub version: String,
    pub file_name: String,
}

impl PhpPackagePathInfo {
    pub fn package_name(&self) -> String {
        format!("{}/{}", self.vendor, self.package)
    }

    pub fn normalized_package_name(&self) -> String {
        self.package_name().to_ascii_lowercase()
    }

    pub fn project_storage_path(&self) -> String {
        format!(
            "{}/{}",
            self.vendor.to_ascii_lowercase(),
            self.package.to_ascii_lowercase()
        )
    }

    pub fn version_storage_path(&self) -> String {
        format!(
            "{}/{}/{}",
            self.vendor.to_ascii_lowercase(),
            self.package.to_ascii_lowercase(),
            self.version
        )
    }

    pub fn release_type(&self) -> ReleaseType {
        ReleaseType::release_type_from_version(&self.version)
    }
}

impl TryFrom<&StoragePath> for PhpPackagePathInfo {
    type Error = PhpRepositoryError;

    fn try_from(path: &StoragePath) -> Result<Self, Self::Error> {
        let components: Vec<String> = path
            .clone()
            .into_iter()
            .map(|component| component.to_string())
            .collect();
        if components.len() < 4 {
            return Err(PhpRepositoryError::InvalidPath(path.to_string()));
        }
        let vendor = components[0].clone();
        let package = components[1].clone();
        let version = components[2].clone();
        let file_name = components
            .last()
            .cloned()
            .ok_or(RepositoryHandlerError::NotFound)?;
        Ok(Self {
            vendor,
            package,
            version,
            file_name,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_php_path() {
        let path = StoragePath::from("acme/example/1.0.0/example-1.0.0.zip");
        let info = PhpPackagePathInfo::try_from(&path).unwrap();
        assert_eq!(info.vendor, "acme");
        assert_eq!(info.package, "example");
        assert_eq!(info.version, "1.0.0");
        assert_eq!(info.package_name(), "acme/example");
        assert_eq!(info.version_storage_path(), "acme/example/1.0.0");
    }

    #[test]
    fn rejects_short_path() {
        let path = StoragePath::from("acme/example.zip");
        assert!(PhpPackagePathInfo::try_from(&path).is_err());
    }
}
