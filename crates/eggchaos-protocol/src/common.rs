//! Shared native service/version/error wire DTOs and contract constants.
//!
//! These shapes are returned by every native route family, so they live in
//! one place instead of being re-declared per resource. `eggchaos-server`
//! renders its bounded runtime errors through [`ErrorEnvelopeV1`].

use serde::{Deserialize, Serialize};

/// Native API version segment (`/v1`).
pub const NATIVE_API_VERSION: &str = "v1";
/// Media type for native JSON request/response bodies.
pub const NATIVE_JSON_CONTENT_TYPE: &str = "application/json";
/// Media type for `GET /metrics` (Prometheus text exposition format).
pub const METRICS_CONTENT_TYPE: &str = "text/plain; version=0.0.4";
/// Maximum native request body in bytes (1 MiB), enforced by the server.
pub const MAX_REQUEST_BODY_BYTES: usize = 1024 * 1024;

/// Bounded native error envelope: `{"error":{"code":...,"message":...}}`.
///
/// `code` is a stable machine string (`not_found`, `conflict`, `invalid`,
/// `invalid_json`, `unauthorized`, `bind_failed`, `restart_failed`,
/// `serialization`); `message` is a bounded human detail that never echoes
/// bearer tokens.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ErrorEnvelopeV1 {
    /// Typed error body.
    pub error: ErrorBodyV1,
}

/// Error body inside [`ErrorEnvelopeV1`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ErrorBodyV1 {
    /// Stable machine code.
    pub code: String,
    /// Bounded human detail.
    pub message: String,
}

impl ErrorEnvelopeV1 {
    /// Build an envelope from a code and a message.
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            error: ErrorBodyV1 {
                code: code.into(),
                message: message.into(),
            },
        }
    }
}

/// `GET /v1/health` response body.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HealthV1 {
    /// Whether the service loop is running.
    pub running: bool,
    /// Global configuration generation.
    pub generation: u64,
}

/// `GET /v1/version` response body.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VersionV1 {
    /// Crate/service version.
    pub version: String,
    /// Native API version (`"v1"`).
    pub api: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_envelope_has_the_documented_shape() {
        let envelope = ErrorEnvelopeV1::new("not_found", "proxy not found: cache");
        assert_eq!(
            serde_json::to_string(&envelope).unwrap(),
            r#"{"error":{"code":"not_found","message":"proxy not found: cache"}}"#
        );
        let parsed: ErrorEnvelopeV1 =
            serde_json::from_str(r#"{"error":{"code":"invalid","message":"bad"}}"#).unwrap();
        assert_eq!(parsed.error.code, "invalid");
        assert!(
            serde_json::from_str::<ErrorEnvelopeV1>(r#"{"error":{"code":"x"},"extra":1}"#).is_err()
        );
    }
}
