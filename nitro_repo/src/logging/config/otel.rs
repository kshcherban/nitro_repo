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

impl OtelConfig {
    /// Apply environment variable fallback logic
    /// This should be called after deserialization to apply environment variable overrides
    /// Note: Config file values take precedence over environment variables
    pub fn apply_env_fallback(self) -> Self {
        // Environment variables are applied during the Default() implementation
        // Since config file deserialization overrides defaults, we don't need to
        // do anything special here - the config file values already take precedence

        // Only apply endpoint env var if it wasn't overridden in config
        let mut endpoint = self.endpoint;
        if endpoint == "http://localhost:4317" {
            if let Ok(otel_endpoint) = std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT") {
                endpoint = otel_endpoint;
            }
        }

        OtelConfig { endpoint, ..self }
    }
}
impl AppLoggerType for OtelConfig {
    fn get_levels_mut(&mut self) -> &mut LoggingLevels {
        &mut self.levels
    }
}
impl Default for OtelConfig {
    fn default() -> Self {
        // Enable tracing if NITRO_TRACING_ENABLED environment variable is set
        // This can be overridden by config file settings
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

impl Default for TracingProtocol {
    fn default() -> Self {
        TracingProtocol::GRPC
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[test]
    fn test_tracing_protocol_default() {
        let protocol = TracingProtocol::default();
        assert!(matches!(protocol, TracingProtocol::GRPC));
    }

    #[test]
    fn test_env_var_fallback() {
        // Test that the environment variable fallback works with Default() implementation

        // Clear environment variable first
        unsafe { env::remove_var("NITRO_TRACING_ENABLED") };

        // Default should be false without env var
        let config1 = OtelConfig::default();
        assert!(!config1.enabled);

        // Set environment variable
        unsafe { env::set_var("NITRO_TRACING_ENABLED", "1") };

        let config2 = OtelConfig::default();
        assert!(config2.enabled); // Should be true with env var in Default()

        // Test that explicit config construction (simulating config file) overrides env var
        let config3 = OtelConfig {
            enabled: false,
            ..Default::default()
        };
        assert!(!config3.enabled); // Should stay false as explicitly set

        // The apply_env_fallback method shouldn't change the enabled field
        let config4 = config3.apply_env_fallback();
        assert!(!config4.enabled); // Should remain false

        // Clean up
        unsafe { env::remove_var("NITRO_TRACING_ENABLED") };
    }

    #[test]
    fn test_endpoint_env_fallback() {
        // Test endpoint environment variable fallback
        unsafe { env::remove_var("OTEL_EXPORTER_OTLP_ENDPOINT") };

        let config1 = OtelConfig::default().apply_env_fallback();
        assert_eq!(config1.endpoint, "http://localhost:4317");

        // Set environment variable
        unsafe {
            env::set_var(
                "OTEL_EXPORTER_OTLP_ENDPOINT",
                "http://custom-collector:9999",
            )
        };

        let config2 = OtelConfig::default().apply_env_fallback();
        assert_eq!(config2.endpoint, "http://custom-collector:9999");

        // Test that explicit config file value overrides env var
        let config3 = OtelConfig {
            endpoint: "http://explicit-config:8080".to_string(),
            ..Default::default()
        }
        .apply_env_fallback();
        assert_eq!(config3.endpoint, "http://explicit-config:8080");

        // Clean up
        unsafe { env::remove_var("OTEL_EXPORTER_OTLP_ENDPOINT") };
    }
}
