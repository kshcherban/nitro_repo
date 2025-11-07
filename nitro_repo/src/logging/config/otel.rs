use ahash::{HashMap, HashMapExt};
use opentelemetry::{KeyValue, StringValue};
use serde::{Deserialize, Serialize};

use super::{AppLoggerType, LoggingLevels};
/// Tracing Config Resource Values.
///
/// ```toml
/// "service.name" = "nitro-repo"
/// "service.version" = "3.0.0-BETA"
/// "service.environment" = "development"
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OtelResourceMap(pub HashMap<String, String>);
impl Default for OtelResourceMap {
    fn default() -> Self {
        let mut trace_config = HashMap::new();
        trace_config.insert("service.name".to_string(), "nitro-repo".to_string());
        trace_config.insert(
            "service.version".to_string(),
            env!("CARGO_PKG_VERSION").to_string(),
        );
        trace_config.insert("service.environment".to_string(), "development".to_string());
        Self(trace_config)
    }
}
impl From<OtelResourceMap> for opentelemetry_sdk::Resource {
    fn from(mut value: OtelResourceMap) -> Self {
        if !value.0.contains_key("service.name") {
            value
                .0
                .insert("service.name".to_string(), "nitro-repo".to_string());
        }
        let resources: Vec<KeyValue> = value
            .0
            .into_iter()
            .map(|(k, v)| KeyValue::new(k, Into::<StringValue>::into(v)))
            .collect();
        opentelemetry_sdk::Resource::builder()
            .with_attributes(resources)
            .build()
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct MetricsConfig {
    pub enabled: bool,
    pub protocol: TracingProtocol,
    /// Endpoint for the tracing collector.
    pub endpoint: String,
    /// Tracing Config Resource Values.
    pub config: OtelResourceMap,
}
impl Default for MetricsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            protocol: TracingProtocol::GRPC,
            endpoint: "http://localhost:4317".to_owned(),
            config: OtelResourceMap::default(),
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct OtelConfig {
    pub enabled: bool,
    pub protocol: TracingProtocol,
    /// Endpoint for the tracing collector.
    pub endpoint: String,
    /// Tracing Config Resource Values.
    pub config: OtelResourceMap,
    pub traces: bool,
    pub logs: bool,
    pub levels: LoggingLevels,
}
impl AppLoggerType for OtelConfig {
    fn get_levels_mut(&mut self) -> &mut LoggingLevels {
        &mut self.levels
    }
}
impl Default for OtelConfig {
    fn default() -> Self {
        // Enable tracing if NITRO_TRACING_ENABLED environment variable is set
        let enabled = std::env::var("NITRO_TRACING_ENABLED").is_ok();

        Self {
            enabled,
            protocol: TracingProtocol::GRPC,
            endpoint: std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT")
                .unwrap_or_else(|_| "http://localhost:4317".to_owned()),
            config: OtelResourceMap::default(),
            traces: true,
            logs: false, // Don't send logs to OTLP - logs should always be available locally
            levels: LoggingLevels::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum TracingProtocol {
    GRPC,
    /// Not Implemented Yet
    HttpBinary,
    HttpJson,
}
impl From<TracingProtocol> for opentelemetry_otlp::Protocol {
    fn from(value: TracingProtocol) -> Self {
        match value {
            TracingProtocol::GRPC => opentelemetry_otlp::Protocol::Grpc,
            TracingProtocol::HttpBinary => opentelemetry_otlp::Protocol::HttpBinary,
            TracingProtocol::HttpJson => opentelemetry_otlp::Protocol::HttpJson,
        }
    }
}
