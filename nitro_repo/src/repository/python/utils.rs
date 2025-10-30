use nr_core::{repository::project::ReleaseType, storage::StoragePath};
use serde::{Deserialize, Serialize};

use super::PythonRepositoryError;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PythonPackagePathInfo {
    pub package: String,
    pub version: String,
    pub file_name: String,
}

impl PythonPackagePathInfo {
    pub fn project_key(&self) -> String {
        normalize_package_name(&self.package)
    }

    pub fn release_type(&self) -> ReleaseType {
        ReleaseType::release_type_from_version(&self.version)
    }

    pub fn project_storage_path(&self) -> String {
        format!("{}/", self.project_key())
    }

    pub fn version_storage_path(&self) -> String {
        format!("{}/{}", self.project_key(), self.version)
    }
}

impl TryFrom<&StoragePath> for PythonPackagePathInfo {
    type Error = PythonRepositoryError;

    fn try_from(path: &StoragePath) -> Result<Self, Self::Error> {
        let components: Vec<String> = path
            .clone()
            .into_iter()
            .map(|component| component.to_string())
            .collect();
        if components.len() < 3 {
            return Err(PythonRepositoryError::InvalidPath(path.to_string()));
        }
        let package = components.first().cloned().unwrap();
        let version = components.get(1).cloned().unwrap();
        let file_name = components.last().cloned().unwrap();
        Ok(Self {
            package,
            version,
            file_name,
        })
    }
}

pub fn normalize_package_name(name: &str) -> String {
    name.to_ascii_lowercase()
        .chars()
        .map(|char| match char {
            '-' | '_' | '.' => '-',
            _ => char,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_python_path() {
        let path = StoragePath::from("example_pkg/1.0.0/example_pkg-1.0.0-py3-none-any.whl");
        let info = PythonPackagePathInfo::try_from(&path).unwrap();
        assert_eq!(info.package, "example_pkg");
        assert_eq!(info.version, "1.0.0");
        assert_eq!(
            info.file_name,
            "example_pkg-1.0.0-py3-none-any.whl".to_string()
        );
        assert_eq!(info.project_key(), "example-pkg");
        assert_eq!(info.version_storage_path(), "example-pkg/1.0.0");
    }

    #[test]
    fn normalizes_package_name() {
        assert_eq!(normalize_package_name("Example_Pkg"), "example-pkg");
        assert_eq!(normalize_package_name("Example.Pkg"), "example-pkg");
    }

    #[test]
    fn rejects_short_path() {
        let path = StoragePath::from("example_pkg/file.whl");
        let result = PythonPackagePathInfo::try_from(&path);
        assert!(result.is_err());
    }
}
