use ahash::AHashMap;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ControlFile {
    fields: AHashMap<String, String>,
}

impl ControlFile {
    pub fn parse(contents: &str) -> Result<Self, ControlParseError> {
        let mut fields = AHashMap::new();
        let mut current_key: Option<String> = None;
        let mut current_value = String::new();

        for line in contents.lines() {
            if line.trim_start().is_empty() {
                if let Some(key) = current_key.take() {
                    fields.insert(key, current_value.trim_end().to_string());
                    current_value.clear();
                }
                continue;
            }

            if let Some(stripped) = line.strip_prefix(' ') {
                if current_key.is_none() {
                    return Err(ControlParseError::InvalidContinuation);
                }
                if stripped == "." {
                    current_value.push('\n');
                } else {
                    if !current_value.is_empty() {
                        current_value.push('\n');
                    }
                    current_value.push_str(stripped);
                }
                continue;
            }

            if let Some(key) = current_key.take() {
                fields.insert(key, current_value.trim_end().to_string());
                current_value.clear();
            }

            let Some((key, value)) = line.split_once(':') else {
                return Err(ControlParseError::MissingSeparator(line.to_string()));
            };
            current_key = Some(key.trim().to_string());
            current_value.push_str(value.trim_start());
        }

        if let Some(key) = current_key.take() {
            fields.insert(key, current_value.trim_end().to_string());
        }

        Ok(Self { fields })
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.fields.get(key).map(|value| value.as_str())
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ControlParseError {
    #[error("control field is missing ':' separator near `{0}`")]
    MissingSeparator(String),
    #[error("found continuation line before any field header")]
    InvalidContinuation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackagesRecord {
    pub package: String,
    pub version: String,
    pub architecture: String,
    pub section: Option<String>,
    pub priority: Option<String>,
    pub maintainer: Option<String>,
    pub installed_size: Option<u64>,
    pub depends: Option<String>,
    pub description: String,
    pub homepage: Option<String>,
    pub filename: String,
    pub size: u64,
    pub md5: String,
    pub sha1: String,
    pub sha256: String,
}

pub fn format_packages_entry(record: &PackagesRecord) -> String {
    let mut output = String::new();
    use std::fmt::Write;

    writeln!(&mut output, "Package: {}", record.package).unwrap();
    writeln!(&mut output, "Version: {}", record.version).unwrap();
    writeln!(&mut output, "Architecture: {}", record.architecture).unwrap();
    if let Some(section) = record.section.as_deref() {
        writeln!(&mut output, "Section: {}", section).unwrap();
    }
    if let Some(priority) = record.priority.as_deref() {
        writeln!(&mut output, "Priority: {}", priority).unwrap();
    }
    if let Some(maintainer) = record.maintainer.as_deref() {
        writeln!(&mut output, "Maintainer: {}", maintainer).unwrap();
    }
    if let Some(depends) = record.depends.as_deref() {
        writeln!(&mut output, "Depends: {}", depends).unwrap();
    }
    if let Some(installed_size) = record.installed_size {
        writeln!(&mut output, "Installed-Size: {}", installed_size).unwrap();
    }
    if let Some(homepage) = record.homepage.as_deref() {
        writeln!(&mut output, "Homepage: {}", homepage).unwrap();
    }
    writeln!(&mut output, "Filename: {}", record.filename).unwrap();
    writeln!(&mut output, "Size: {}", record.size).unwrap();
    writeln!(&mut output, "MD5sum: {}", record.md5).unwrap();
    writeln!(&mut output, "SHA1: {}", record.sha1).unwrap();
    writeln!(&mut output, "SHA256: {}", record.sha256).unwrap();

    // Debian description format expects summary followed by newline and space-prefixed details.
    if let Some((summary, rest)) = record.description.split_once('\n') {
        writeln!(&mut output, "Description: {}", summary).unwrap();
        for line in rest.lines() {
            if line.is_empty() {
                writeln!(&mut output, " .").unwrap();
            } else {
                writeln!(&mut output, " {}", line).unwrap();
            }
        }
    } else {
        writeln!(&mut output, "Description: {}", record.description).unwrap();
    }

    output.push('\n');
    output
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseEntry {
    pub path: String,
    pub size: u64,
    pub md5: String,
    pub sha1: String,
    pub sha256: String,
}

pub fn build_release_file(
    distribution: &str,
    components: &[String],
    architectures: &[String],
    entries: &[ReleaseEntry],
) -> String {
    let mut output = String::new();
    use chrono::{DateTime, FixedOffset};
    use std::fmt::Write;

    let now: DateTime<FixedOffset> = chrono::Utc::now().into();
    writeln!(&mut output, "Origin: Nitro Repo").unwrap();
    writeln!(&mut output, "Label: Nitro Repo").unwrap();
    writeln!(&mut output, "Suite: {}", distribution).unwrap();
    writeln!(&mut output, "Codename: {}", distribution).unwrap();
    writeln!(
        &mut output,
        "Date: {}",
        now.format("%a, %d %b %Y %H:%M:%S %z")
    )
    .unwrap();
    writeln!(&mut output, "Components: {}", components.join(" ")).unwrap();
    writeln!(&mut output, "Architectures: {}", architectures.join(" ")).unwrap();
    writeln!(&mut output).unwrap();

    writeln!(&mut output, "MD5Sum:").unwrap();
    for entry in entries {
        writeln!(
            &mut output,
            " {} {:>16} {}",
            entry.md5, entry.size, entry.path
        )
        .unwrap();
    }
    writeln!(&mut output, "SHA1:").unwrap();
    for entry in entries {
        writeln!(
            &mut output,
            " {} {:>16} {}",
            entry.sha1, entry.size, entry.path
        )
        .unwrap();
    }
    writeln!(&mut output, "SHA256:").unwrap();
    for entry in entries {
        writeln!(
            &mut output,
            " {} {:>16} {}",
            entry.sha256, entry.size, entry.path
        )
        .unwrap();
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_control_file_handles_multiline_description() {
        let control = "Package: sample\nVersion: 1.0\nDescription: summary line\n more details\n .\n final line\n";
        let parsed = ControlFile::parse(control).expect("control to parse");
        let description = parsed.get("Description").expect("description field");
        assert_eq!(
            description, "summary line\nmore details\n\nfinal line",
            "description should preserve folded lines"
        );
    }

    #[test]
    fn parse_control_rejects_invalid_lines() {
        let control = "Package sample";
        let err = ControlFile::parse(control).expect_err("validation error");
        assert!(matches!(err, ControlParseError::MissingSeparator(_)));
    }

    #[test]
    fn packages_entry_contains_required_fields() {
        let record = PackagesRecord {
            package: "sample".into(),
            version: "1.2.3".into(),
            architecture: "amd64".into(),
            section: Some("utils".into()),
            priority: Some("optional".into()),
            maintainer: Some("Nitro <dev@nitro>".into()),
            installed_size: Some(2048),
            depends: Some("libc6 (>= 2.31)".into()),
            description: "summary\ndetails line".into(),
            homepage: Some("https://nitro".into()),
            filename: "pool/main/s/sample/sample_1.2.3_amd64.deb".into(),
            size: 42,
            md5: "md5".into(),
            sha1: "sha1".into(),
            sha256: "sha256".into(),
        };
        let entry = format_packages_entry(&record);
        assert!(entry.contains("Package: sample"));
        assert!(entry.contains("Depends: libc6"));
        assert!(entry.contains("Filename: pool/main/s/sample/sample_1.2.3_amd64.deb"));
        assert!(entry.contains(" details line"));
    }

    #[test]
    fn release_file_lists_all_hashes() {
        let entries = [ReleaseEntry {
            path: "dists/stable/main/binary-amd64/Packages".into(),
            size: 1024,
            md5: "aaa".into(),
            sha1: "bbb".into(),
            sha256: "ccc".into(),
        }];
        let release = build_release_file("stable", &["main".into()], &["amd64".into()], &entries);
        assert!(release.contains("Suite: stable"));
        assert!(release.contains("Components: main"));
        assert!(release.contains("aaa"));
        assert!(release.contains("SHA256:"));
        assert!(release.contains("dists/stable/main/binary-amd64/Packages"));
    }
}
