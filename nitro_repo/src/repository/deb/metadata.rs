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

fn push_label_value(output: &mut String, label: &str, value: impl std::fmt::Display) {
    output.push_str(label);
    output.push_str(": ");
    output.push_str(&value.to_string());
    output.push('\n');
}

fn push_empty_line(output: &mut String) {
    output.push('\n');
}

pub fn format_packages_entry(record: &PackagesRecord) -> String {
    let mut output = String::new();
    push_label_value(&mut output, "Package", &record.package);
    push_label_value(&mut output, "Version", &record.version);
    push_label_value(&mut output, "Architecture", &record.architecture);
    if let Some(section) = record.section.as_deref() {
        push_label_value(&mut output, "Section", section);
    }
    if let Some(priority) = record.priority.as_deref() {
        push_label_value(&mut output, "Priority", priority);
    }
    if let Some(maintainer) = record.maintainer.as_deref() {
        push_label_value(&mut output, "Maintainer", maintainer);
    }
    if let Some(depends) = record.depends.as_deref() {
        push_label_value(&mut output, "Depends", depends);
    }
    if let Some(installed_size) = record.installed_size {
        push_label_value(&mut output, "Installed-Size", installed_size);
    }
    if let Some(homepage) = record.homepage.as_deref() {
        push_label_value(&mut output, "Homepage", homepage);
    }
    push_label_value(&mut output, "Filename", &record.filename);
    push_label_value(&mut output, "Size", record.size);
    push_label_value(&mut output, "MD5sum", &record.md5);
    push_label_value(&mut output, "SHA1", &record.sha1);
    push_label_value(&mut output, "SHA256", &record.sha256);

    // Debian description format expects summary followed by newline and space-prefixed details.
    if let Some((summary, rest)) = record.description.split_once('\n') {
        push_label_value(&mut output, "Description", summary);
        for line in rest.lines() {
            if line.is_empty() {
                output.push_str(" .\n");
            } else {
                output.push(' ');
                output.push_str(line);
                output.push('\n');
            }
        }
    } else {
        push_label_value(&mut output, "Description", &record.description);
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

fn push_hash_section(
    output: &mut String,
    label: &str,
    entries: &[ReleaseEntry],
    hash_value: impl Fn(&ReleaseEntry) -> &str,
) {
    output.push_str(label);
    output.push_str(":\n");
    for entry in entries {
        let size = format!("{:>16}", entry.size);
        output.push(' ');
        output.push_str(hash_value(entry));
        output.push(' ');
        output.push_str(&size);
        output.push(' ');
        output.push_str(&entry.path);
        output.push('\n');
    }
}

pub fn build_release_file(
    distribution: &str,
    components: &[String],
    architectures: &[String],
    entries: &[ReleaseEntry],
) -> String {
    let mut output = String::new();
    use chrono::{DateTime, FixedOffset};

    let now: DateTime<FixedOffset> = chrono::Utc::now().into();
    push_label_value(&mut output, "Origin", "Nitro Repo");
    push_label_value(&mut output, "Label", "Nitro Repo");
    push_label_value(&mut output, "Suite", distribution);
    push_label_value(&mut output, "Codename", distribution);
    push_label_value(&mut output, "Date", now.format("%a, %d %b %Y %H:%M:%S %z"));
    push_label_value(&mut output, "Components", components.join(" "));
    push_label_value(&mut output, "Architectures", architectures.join(" "));
    push_empty_line(&mut output);

    push_hash_section(&mut output, "MD5Sum", entries, |entry| entry.md5.as_str());
    push_hash_section(&mut output, "SHA1", entries, |entry| entry.sha1.as_str());
    push_hash_section(&mut output, "SHA256", entries, |entry| {
        entry.sha256.as_str()
    });

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
    fn packages_entry_format_matches_expected_output() {
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
        let expected = "\
Package: sample
Version: 1.2.3
Architecture: amd64
Section: utils
Priority: optional
Maintainer: Nitro <dev@nitro>
Depends: libc6 (>= 2.31)
Installed-Size: 2048
Homepage: https://nitro
Filename: pool/main/s/sample/sample_1.2.3_amd64.deb
Size: 42
MD5sum: md5
SHA1: sha1
SHA256: sha256
Description: summary
 details line

";
        assert_eq!(entry, expected);
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

    #[test]
    fn release_file_aligns_hash_section_entries() {
        let entries = [ReleaseEntry {
            path: "dists/stable/main/binary-amd64/Packages".into(),
            size: 1024,
            md5: "aaa".into(),
            sha1: "bbb".into(),
            sha256: "ccc".into(),
        }];
        let release = build_release_file("stable", &["main".into()], &["amd64".into()], &entries);
        let lines: Vec<_> = release.lines().collect();

        // MD5 section header + entry directly after the empty spacer.
        let md5_section = lines
            .iter()
            .position(|line| *line == "MD5Sum:")
            .expect("MD5 section present");
        let expected_md5_entry = format!(
            " {} {:>16} {}",
            "aaa", 1024, "dists/stable/main/binary-amd64/Packages"
        );
        assert_eq!(lines[md5_section + 1], expected_md5_entry);

        let sha1_section = lines
            .iter()
            .position(|line| *line == "SHA1:")
            .expect("SHA1 section present");
        let expected_sha1_entry = format!(
            " {} {:>16} {}",
            "bbb", 1024, "dists/stable/main/binary-amd64/Packages"
        );
        assert_eq!(lines[sha1_section + 1], expected_sha1_entry);

        let sha256_section = lines
            .iter()
            .position(|line| *line == "SHA256:")
            .expect("SHA256 section present");
        let expected_sha256_entry = format!(
            " {} {:>16} {}",
            "ccc", 1024, "dists/stable/main/binary-amd64/Packages"
        );
        assert_eq!(lines[sha256_section + 1], expected_sha256_entry);
    }
}
