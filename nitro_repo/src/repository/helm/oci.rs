use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OciDescriptor {
    #[serde(rename = "mediaType")]
    pub media_type: String,
    pub digest: String,
    pub size: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotations: Option<serde_json::Map<String, serde_json::Value>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OciManifest {
    #[serde(rename = "schemaVersion")]
    pub schema_version: u32,
    pub config: OciDescriptor,
    pub layers: Vec<OciDescriptor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotations: Option<serde_json::Map<String, serde_json::Value>>,
}

#[derive(Debug)]
pub struct HelmOciManifestInput {
    pub chart_digest: String,
    pub chart_size: u64,
    pub chart_name: String,
    pub chart_version: String,
    pub config_digest: String,
    pub config_size: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum HelmOciError {
    #[error("invalid Helm OCI manifest parameters: {0}")]
    InvalidInput(String),
}

pub fn build_helm_manifest(_input: HelmOciManifestInput) -> Result<OciManifest, HelmOciError> {
    if _input.chart_digest.is_empty() {
        return Err(HelmOciError::InvalidInput(
            "chart_digest cannot be empty".to_string(),
        ));
    }
    if _input.config_digest.is_empty() {
        return Err(HelmOciError::InvalidInput(
            "config_digest cannot be empty".to_string(),
        ));
    }
    if _input.chart_size == 0 || _input.config_size == 0 {
        return Err(HelmOciError::InvalidInput(
            "descriptor sizes must be greater than zero".to_string(),
        ));
    }

    let mut manifest_annotations = Map::new();
    manifest_annotations.insert(
        "org.opencontainers.image.title".to_string(),
        Value::String(_input.chart_name.clone()),
    );
    manifest_annotations.insert(
        "org.opencontainers.image.version".to_string(),
        Value::String(_input.chart_version.clone()),
    );

    let mut config_annotations = Map::new();
    config_annotations.insert(
        "io.cncf.helm.chart.name".to_string(),
        Value::String(_input.chart_name.clone()),
    );
    config_annotations.insert(
        "io.cncf.helm.chart.version".to_string(),
        Value::String(_input.chart_version.clone()),
    );

    let config_descriptor = OciDescriptor {
        media_type: "application/vnd.cncf.helm.config.v1+json".to_string(),
        digest: _input.config_digest,
        size: _input.config_size,
        annotations: Some(config_annotations),
    };

    let layer_descriptor = OciDescriptor {
        media_type: "application/vnd.cncf.helm.chart.content.v1.tar+gzip".to_string(),
        digest: _input.chart_digest,
        size: _input.chart_size,
        annotations: None,
    };

    Ok(OciManifest {
        schema_version: 2,
        config: config_descriptor,
        layers: vec![layer_descriptor],
        annotations: Some(manifest_annotations),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_uses_expected_media_types() {
        let input = HelmOciManifestInput {
            chart_digest: "sha256:deadbeef".to_string(),
            chart_size: 42,
            chart_name: "webapp".to_string(),
            chart_version: "1.2.3".to_string(),
            config_digest: "sha256:cafebabe".to_string(),
            config_size: 128,
        };

        let manifest = build_helm_manifest(input).expect("manifest creation should succeed");
        assert_eq!(manifest.schema_version, 2);
        assert_eq!(manifest.layers.len(), 1);
        assert_eq!(
            manifest.config.media_type,
            "application/vnd.cncf.helm.config.v1+json"
        );
        assert_eq!(
            manifest.layers[0].media_type,
            "application/vnd.cncf.helm.chart.content.v1.tar+gzip"
        );
        assert_eq!(manifest.layers[0].size, 42);
        assert_eq!(manifest.layers[0].digest, "sha256:deadbeef");
    }

    #[test]
    fn manifest_includes_chart_annotations() {
        let input = HelmOciManifestInput {
            chart_digest: "sha256:feedface".to_string(),
            chart_size: 512,
            chart_name: "metrics".to_string(),
            chart_version: "0.8.0".to_string(),
            config_digest: "sha256:012345".to_string(),
            config_size: 256,
        };

        let manifest = build_helm_manifest(input).expect("manifest creation should succeed");
        let annotations = manifest.annotations.expect("annotations should be present");
        assert_eq!(
            annotations.get("org.opencontainers.image.title").unwrap(),
            &serde_json::Value::String("metrics".to_string())
        );
        assert_eq!(
            annotations.get("org.opencontainers.image.version").unwrap(),
            &serde_json::Value::String("0.8.0".to_string())
        );
    }
}
