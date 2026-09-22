use std::{net::SocketAddr, num::NonZeroU64, path::Path, time::Duration};

use eggchaos_core::{
    BandwidthConfig, BlackholeConfig, Direction, DisconnectConfig, FaultId, FaultKind, FaultPlan,
    FaultSpec, LatencyConfig, LimitDataConfig, Probability, SliceConfig, SlowCloseConfig,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::ProxySpec;

/// Versioned native TOML configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NativeConfig {
    /// Schema version.
    pub version: u32,
    /// Run seed.
    #[serde(default)]
    pub seed: u64,
    /// Admin listener settings.
    #[serde(default)]
    pub admin: AdminFileConfig,
    /// Fixed-target proxy definitions.
    #[serde(rename = "proxy", default)]
    pub proxies: Vec<ProxyFileConfig>,
}

/// TOML admin settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdminFileConfig {
    /// Bind address, loopback by default.
    #[serde(default = "default_admin_bind")]
    pub bind: SocketAddr,
    /// Explicit opt-in for non-loopback exposure.
    #[serde(default)]
    pub public_admin: bool,
    /// Bearer token. Prefer an environment/file indirection in deployment.
    pub auth_token: Option<String>,
}

fn default_admin_bind() -> SocketAddr {
    "127.0.0.1:8475".parse().unwrap()
}

impl Default for AdminFileConfig {
    fn default() -> Self {
        Self {
            bind: default_admin_bind(),
            public_admin: false,
            auth_token: None,
        }
    }
}

/// TOML fixed-target proxy settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyFileConfig {
    /// Proxy name.
    pub name: String,
    /// Listener address.
    pub listen: SocketAddr,
    /// Target address.
    pub upstream: SocketAddr,
    /// Whether it starts enabled.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Optional connection cap.
    pub max_connections: Option<usize>,
    /// Fault entries.
    #[serde(rename = "fault", default)]
    pub faults: Vec<FaultFileConfig>,
}

fn default_true() -> bool {
    true
}

/// A TOML fault DTO with human-friendly units.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FaultFileConfig {
    /// Stable ID.
    pub id: String,
    /// Direction.
    pub direction: Direction,
    /// Native type name.
    #[serde(rename = "type")]
    pub kind: String,
    /// Activation probability.
    #[serde(default = "default_probability")]
    pub probability: f64,
    /// Base delay/rate/limit attributes.
    pub delay: Option<String>,
    /// Jitter string.
    pub jitter: Option<String>,
    /// Rate in bytes/sec.
    pub bytes_per_second: Option<u64>,
    /// Burst bytes.
    pub burst_bytes: Option<u64>,
    /// Limit bytes.
    pub bytes: Option<u64>,
    /// Average slice size.
    pub average_size: Option<u64>,
    /// Slice variation.
    pub variation: Option<u64>,
    /// Hard reset request.
    #[serde(default)]
    pub hard_reset: bool,
}

fn default_probability() -> f64 {
    1.0
}

/// Configuration conversion failures with field context.
#[derive(Debug, Error)]
pub enum NativeConfigError {
    /// TOML parse error.
    #[error("invalid TOML: {0}")]
    Toml(#[from] toml::de::Error),
    /// File I/O.
    #[error("configuration I/O: {0}")]
    Io(#[from] std::io::Error),
    /// Schema version is unsupported.
    #[error("unsupported configuration version: {0}")]
    Version(u32),
    /// A typed field is invalid.
    #[error("invalid configuration field {field}: {message}")]
    Field { field: String, message: String },
}

impl NativeConfig {
    /// Parse bounded TOML and compile all proxy plans through core validation.
    pub fn parse(text: &str) -> Result<Self, NativeConfigError> {
        let config: Self = toml::from_str(text)?;
        if config.version != 1 {
            return Err(NativeConfigError::Version(config.version));
        }
        if config.proxies.len() > 1024 {
            return Err(NativeConfigError::Field {
                field: "proxy".into(),
                message: "too many proxies".into(),
            });
        }
        let mut names = std::collections::HashSet::new();
        for proxy in &config.proxies {
            if !names.insert(&proxy.name) {
                return Err(NativeConfigError::Field {
                    field: "proxy.name".into(),
                    message: format!("duplicate proxy {}", proxy.name),
                });
            }
            let _ = proxy.compile()?;
        }
        Ok(config)
    }
    /// Load and parse a TOML file.
    pub async fn load(path: impl AsRef<Path>) -> Result<Self, NativeConfigError> {
        Self::parse(&tokio::fs::read_to_string(path).await?)
    }
    /// Compile service proxy definitions.
    pub fn compile_proxies(&self) -> Result<Vec<ProxySpec>, NativeConfigError> {
        self.proxies.iter().map(ProxyFileConfig::compile).collect()
    }
}

impl ProxyFileConfig {
    /// Compile this DTO into the native runtime model.
    pub fn compile(&self) -> Result<ProxySpec, NativeConfigError> {
        let mut upstream = Vec::new();
        let mut downstream = Vec::new();
        for fault in &self.faults {
            let spec = fault.compile()?;
            if fault.direction == Direction::Upstream {
                upstream.push(spec);
            } else {
                downstream.push(spec);
            }
        }
        let mut proxy = ProxySpec::new(self.name.clone(), self.listen, self.upstream);
        proxy.enabled = self.enabled;
        proxy.max_connections = self.max_connections;
        proxy.upstream_faults = FaultPlan::new(upstream).map_err(|e| NativeConfigError::Field {
            field: "fault".into(),
            message: e.to_string(),
        })?;
        proxy.downstream_faults =
            FaultPlan::new(downstream).map_err(|e| NativeConfigError::Field {
                field: "fault".into(),
                message: e.to_string(),
            })?;
        proxy
            .upstream_policy
            .publish(proxy.upstream_faults.clone())
            .map_err(|e| NativeConfigError::Field {
                field: "fault".into(),
                message: e.to_string(),
            })?;
        proxy
            .downstream_policy
            .publish(proxy.downstream_faults.clone())
            .map_err(|e| NativeConfigError::Field {
                field: "fault".into(),
                message: e.to_string(),
            })?;
        Ok(proxy)
    }
}

impl FaultFileConfig {
    fn compile(&self) -> Result<FaultSpec, NativeConfigError> {
        let id = FaultId::new(self.id.clone()).map_err(|e| NativeConfigError::Field {
            field: "fault.id".into(),
            message: e.to_string(),
        })?;
        let probability =
            Probability::new(self.probability).map_err(|e| NativeConfigError::Field {
                field: "fault.probability".into(),
                message: e.to_string(),
            })?;
        let duration =
            |value: &Option<String>, field: &str| -> Result<Duration, NativeConfigError> {
                value
                    .as_deref()
                    .map(parse_duration)
                    .unwrap_or(Ok(Duration::ZERO))
                    .map_err(|message| NativeConfigError::Field {
                        field: field.into(),
                        message,
                    })
            };
        let kind = match self.kind.as_str() {
            "latency" => FaultKind::Latency(LatencyConfig {
                delay: duration(&self.delay, "fault.delay")?,
                jitter: duration(&self.jitter, "fault.jitter")?,
                max_buffer_bytes: NonZeroU64::new(64 * 1024).unwrap(),
            }),
            "bandwidth" => FaultKind::Bandwidth(BandwidthConfig {
                bytes_per_second: NonZeroU64::new(self.bytes_per_second.unwrap_or(1)).ok_or_else(
                    || NativeConfigError::Field {
                        field: "fault.bytes_per_second".into(),
                        message: "must be non-zero".into(),
                    },
                )?,
                burst_bytes: NonZeroU64::new(self.burst_bytes.unwrap_or(64 * 1024)).unwrap(),
            }),
            "blackhole" | "timeout" => FaultKind::Blackhole(BlackholeConfig {
                close_after: self
                    .delay
                    .as_deref()
                    .map(parse_duration)
                    .transpose()
                    .map_err(|message| NativeConfigError::Field {
                        field: "fault.delay".into(),
                        message,
                    })?,
            }),
            "limit_data" => FaultKind::LimitData(LimitDataConfig {
                bytes: NonZeroU64::new(self.bytes.unwrap_or(1)).ok_or_else(|| {
                    NativeConfigError::Field {
                        field: "fault.bytes".into(),
                        message: "must be non-zero".into(),
                    }
                })?,
            }),
            "slow_close" => FaultKind::SlowClose(SlowCloseConfig {
                delay: duration(&self.delay, "fault.delay")?,
            }),
            "slicer" | "slice" => FaultKind::Slice(SliceConfig {
                average_size: NonZeroU64::new(self.average_size.unwrap_or(1024)).ok_or_else(
                    || NativeConfigError::Field {
                        field: "fault.average_size".into(),
                        message: "must be non-zero".into(),
                    },
                )?,
                variation: self.variation.unwrap_or(0),
                delay: duration(&self.delay, "fault.delay")?,
            }),
            "disconnect" | "reset_peer" => FaultKind::Disconnect(DisconnectConfig {
                after: duration(&self.delay, "fault.delay")?,
                hard_reset: self.hard_reset,
            }),
            other => {
                return Err(NativeConfigError::Field {
                    field: "fault.type".into(),
                    message: format!("unsupported type {other}"),
                })
            }
        };
        Ok(FaultSpec {
            id,
            probability,
            kind,
        })
    }
}

fn parse_duration(value: &str) -> Result<Duration, String> {
    let value = value.trim();
    let (number, multiplier) = if let Some(value) = value.strip_suffix("ms") {
        (value, 1_000_000)
    } else if let Some(value) = value.strip_suffix("us") {
        (value, 1_000)
    } else if let Some(value) = value.strip_suffix('s') {
        (value, 1_000_000_000)
    } else {
        return Err("duration must use ms, us, or s".into());
    };
    let number: u64 = number
        .parse()
        .map_err(|_| "duration number is invalid".to_string())?;
    Ok(Duration::from_nanos(
        number
            .checked_mul(multiplier)
            .ok_or_else(|| "duration overflows".to_string())?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_versioned_toml_and_compiles_faults() {
        let config = NativeConfig::parse(
            r#"
version = 1
seed = 42
[[proxy]]
name = "redis"
listen = "127.0.0.1:0"
upstream = "127.0.0.1:6379"
[[proxy.fault]]
id = "delay"
direction = "downstream"
type = "latency"
delay = "250ms"
"#,
        )
        .unwrap();
        let proxies = config.compile_proxies().unwrap();
        assert_eq!(proxies.len(), 1);
        assert_eq!(proxies[0].downstream_faults.faults().len(), 1);
    }

    #[test]
    fn rejects_duplicate_proxy_names() {
        let result = NativeConfig::parse(
            r#"
version = 1
[[proxy]]
name = "same"
listen = "127.0.0.1:0"
upstream = "127.0.0.1:1"
[[proxy]]
name = "same"
listen = "127.0.0.1:0"
upstream = "127.0.0.1:2"
"#,
        );
        assert!(result.is_err());
    }
}
