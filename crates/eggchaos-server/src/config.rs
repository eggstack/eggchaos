use std::{net::SocketAddr, path::Path, time::Duration};

use eggchaos_core::{Direction, FaultId, FaultPlan, FaultSpec, Probability};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    DatagramFaultSpecV1, FaultKindV1, NativeDatagramProxyRequestV1, ProxySpec, RuntimeConfigV1,
    NATIVE_DEFAULT_BANDWIDTH_BURST_BYTES, NATIVE_DEFAULT_BANDWIDTH_BYTES_PER_SECOND,
    NATIVE_DEFAULT_BUFFER_BYTES, NATIVE_DEFAULT_LIMIT_BYTES, NATIVE_DEFAULT_PROXY_TIMEOUT_MS,
    NATIVE_DEFAULT_SLICE_AVERAGE_SIZE,
};

/// Versioned native TOML configuration.
#[derive(Clone, Serialize, Deserialize)]
pub struct NativeConfig {
    /// Schema version.
    pub version: u32,
    /// Run seed.
    #[serde(default)]
    pub seed: u64,
    /// Admin listener settings.
    #[serde(default)]
    pub admin: AdminFileConfig,
    /// Service-wide runtime bounds and termination controls.
    #[serde(default)]
    pub runtime: RuntimeConfigV1,
    /// Fixed-target proxy definitions.
    #[serde(rename = "proxy", default)]
    pub proxies: Vec<ProxyFileConfig>,
    /// Fixed-target UDP datagram proxy definitions (optional in schema v1).
    #[serde(default, rename = "datagram_proxies")]
    pub datagram_proxies: Vec<DatagramProxyFileConfig>,
}

/// TOML admin settings.
#[derive(Clone, Serialize, Deserialize)]
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

impl std::fmt::Debug for AdminFileConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AdminFileConfig")
            .field("bind", &self.bind)
            .field("public_admin", &self.public_admin)
            .field(
                "auth_token",
                &self.auth_token.as_ref().map(|_| "[REDACTED]"),
            )
            .finish()
    }
}

impl std::fmt::Debug for NativeConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NativeConfig")
            .field("version", &self.version)
            .field("seed", &self.seed)
            .field("admin", &self.admin)
            .field("proxies", &self.proxies)
            .field("datagram_proxies", &self.datagram_proxies)
            .finish()
    }
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
    /// Bounded direct-connect timeout in milliseconds.
    #[serde(default = "default_connect_timeout_ms")]
    pub connect_timeout_ms: u64,
    /// Per-proxy deterministic seed namespace.
    #[serde(default)]
    pub seed: u64,
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

fn default_connect_timeout_ms() -> u64 {
    NATIVE_DEFAULT_PROXY_TIMEOUT_MS
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
    /// Maximum bytes buffered by latency, default 64 KiB.
    pub max_buffer_bytes: Option<u64>,
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
        if config.datagram_proxies.len() > 128 {
            return Err(NativeConfigError::Field {
                field: "datagram_proxies".into(),
                message: "too many datagram proxies".into(),
            });
        }
        config
            .runtime
            .limits()
            .map_err(|message| NativeConfigError::Field {
                field: "runtime".into(),
                message,
            })?;
        config
            .runtime
            .relay_buffer()
            .map_err(|message| NativeConfigError::Field {
                field: "runtime.relay_buffer_bytes".into(),
                message,
            })?;
        config
            .runtime
            .termination_grace()
            .map_err(|message| NativeConfigError::Field {
                field: "runtime.termination_grace_ms".into(),
                message,
            })?;
        config
            .runtime
            .datagram_limits()
            .map_err(|message| NativeConfigError::Field {
                field: "runtime.datagram".into(),
                message,
            })?;
        if config.datagram_proxies.len() > config.runtime.datagram.max_proxies {
            return Err(NativeConfigError::Field {
                field: "datagram_proxies".into(),
                message: "datagram proxy count exceeds runtime.datagram.max_proxies".into(),
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
        let mut datagram_names = std::collections::HashSet::new();
        for proxy in &config.datagram_proxies {
            if !datagram_names.insert(&proxy.name) {
                return Err(NativeConfigError::Field {
                    field: "datagram_proxies.name".into(),
                    message: format!("duplicate datagram proxy {}", proxy.name),
                });
            }
            let compiled = proxy.compile()?;
            let global =
                config
                    .runtime
                    .datagram_limits()
                    .map_err(|message| NativeConfigError::Field {
                        field: "runtime.datagram".into(),
                        message,
                    })?;
            compiled
                .validate(global.max_associations)
                .map_err(|error| NativeConfigError::Field {
                    field: format!("datagram_proxies.{}", proxy.name),
                    message: error.to_string(),
                })?;
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
    /// Compile datagram definitions through the same v1 authority as HTTP.
    pub fn compile_datagram_proxies(
        &self,
    ) -> Result<Vec<crate::DatagramProxySpec>, NativeConfigError> {
        let global =
            self.runtime
                .datagram_limits()
                .map_err(|message| NativeConfigError::Field {
                    field: "runtime.datagram".into(),
                    message,
                })?;
        self.datagram_proxies
            .iter()
            .map(|proxy| {
                let compiled = proxy.compile()?;
                compiled
                    .validate(global.max_associations)
                    .map_err(|error| NativeConfigError::Field {
                        field: format!("datagram_proxies.{}", proxy.name),
                        message: error.to_string(),
                    })?;
                Ok(compiled)
            })
            .collect()
    }
}

/// TOML fixed-target UDP proxy settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DatagramProxyFileConfig {
    pub name: String,
    pub listen: SocketAddr,
    pub upstream: SocketAddr,
    #[serde(default = "default_datagram_associations")]
    pub max_associations: usize,
    #[serde(default = "default_datagram_idle_ms")]
    pub association_idle_timeout_ms: u64,
    #[serde(default = "default_datagram_queue_count")]
    pub max_queued_datagrams: u64,
    #[serde(default = "default_datagram_queue_bytes")]
    pub max_queued_bytes: u64,
    #[serde(default = "default_datagram_size")]
    pub max_datagram_size: u64,
    #[serde(default)]
    pub seed: u64,
    #[serde(default)]
    pub upstream_faults: Vec<DatagramFaultSpecV1>,
    #[serde(default)]
    pub downstream_faults: Vec<DatagramFaultSpecV1>,
}

fn default_datagram_associations() -> usize {
    256
}
fn default_datagram_idle_ms() -> u64 {
    60_000
}
fn default_datagram_queue_count() -> u64 {
    1024
}
fn default_datagram_queue_bytes() -> u64 {
    4 * 1024 * 1024
}
fn default_datagram_size() -> u64 {
    65_507
}

impl DatagramProxyFileConfig {
    fn compile(&self) -> Result<crate::DatagramProxySpec, NativeConfigError> {
        let request = NativeDatagramProxyRequestV1 {
            name: self.name.clone(),
            listen: self.listen,
            upstream: self.upstream,
            max_associations: self.max_associations,
            association_idle_timeout_ms: self.association_idle_timeout_ms,
            max_queued_datagrams: self.max_queued_datagrams,
            max_queued_bytes: self.max_queued_bytes,
            max_datagram_size: self.max_datagram_size,
            seed: self.seed,
            upstream_faults: self.upstream_faults.clone(),
            downstream_faults: self.downstream_faults.clone(),
        };
        request
            .into_runtime()
            .map_err(|message| NativeConfigError::Field {
                field: format!("datagram_proxies.{}", self.name),
                message,
            })
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
        proxy.connect_timeout = Duration::from_millis(self.connect_timeout_ms);
        proxy.seed = self.seed;
        proxy.upstream_faults = FaultPlan::new(upstream).map_err(|e| NativeConfigError::Field {
            field: "fault".into(),
            message: e.to_string(),
        })?;
        proxy.downstream_faults =
            FaultPlan::new(downstream).map_err(|e| NativeConfigError::Field {
                field: "fault".into(),
                message: e.to_string(),
            })?;
        // Live policies initialize from these plans on import; the native
        // authority owns publication from there.
        proxy.validate().map_err(|error| NativeConfigError::Field {
            field: "proxy".into(),
            message: error.to_string(),
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
        let to_ns = |value: Duration, field: &str| {
            u64::try_from(value.as_nanos()).map_err(|_| NativeConfigError::Field {
                field: field.into(),
                message: "duration exceeds native v1 nanosecond range".into(),
            })
        };
        let kind = match self.kind.as_str() {
            "latency" => FaultKindV1::Latency {
                delay_ns: to_ns(duration(&self.delay, "fault.delay")?, "fault.delay")?,
                jitter_ns: to_ns(duration(&self.jitter, "fault.jitter")?, "fault.jitter")?,
                max_buffer_bytes: self.max_buffer_bytes.unwrap_or(NATIVE_DEFAULT_BUFFER_BYTES),
            },
            "bandwidth" => FaultKindV1::Bandwidth {
                bytes_per_second: self
                    .bytes_per_second
                    .unwrap_or(NATIVE_DEFAULT_BANDWIDTH_BYTES_PER_SECOND),
                burst_bytes: self
                    .burst_bytes
                    .unwrap_or(NATIVE_DEFAULT_BANDWIDTH_BURST_BYTES),
            },
            "blackhole" | "timeout" => FaultKindV1::Blackhole {
                close_after_ns: self
                    .delay
                    .as_deref()
                    .map(parse_duration)
                    .transpose()
                    .map_err(|message| NativeConfigError::Field {
                        field: "fault.delay".into(),
                        message,
                    })?
                    .map(|value| to_ns(value, "fault.delay"))
                    .transpose()?,
            },
            "limit_data" | "limit-data" => FaultKindV1::LimitData {
                bytes: self.bytes.unwrap_or(NATIVE_DEFAULT_LIMIT_BYTES),
            },
            "slow_close" | "slow-close" => FaultKindV1::SlowClose {
                delay_ns: to_ns(duration(&self.delay, "fault.delay")?, "fault.delay")?,
            },
            "slicer" | "slice" => FaultKindV1::Slice {
                average_size: self
                    .average_size
                    .unwrap_or(NATIVE_DEFAULT_SLICE_AVERAGE_SIZE),
                variation: self.variation.unwrap_or(0),
                delay_ns: to_ns(duration(&self.delay, "fault.delay")?, "fault.delay")?,
            },
            "disconnect" | "reset_peer" => FaultKindV1::Disconnect {
                after_ns: to_ns(duration(&self.delay, "fault.delay")?, "fault.delay")?,
                hard_reset: self.hard_reset,
            },
            other => {
                return Err(NativeConfigError::Field {
                    field: "fault.type".into(),
                    message: format!("unsupported type {other}"),
                })
            }
        };
        let kind = kind.into_runtime().map_err(|message| {
            let (field, detail) = message
                .split_once(' ')
                .unwrap_or(("kind", message.as_str()));
            NativeConfigError::Field {
                field: format!("fault.{field}"),
                message: detail.to_owned(),
            }
        })?;
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
    fn schema_v1_datagram_collection_compiles_with_defaults_and_faults() {
        let config = NativeConfig::parse(
            r#"
version = 1
[[datagram_proxies]]
name = "dns"
listen = "127.0.0.1:0"
upstream = "127.0.0.1:5353"
upstream_faults = [{ id = "loss", probability = 0.5, kind = { type = "loss" } }]
"#,
        )
        .unwrap();
        let proxies = config.compile_datagram_proxies().unwrap();
        assert_eq!(proxies.len(), 1);
        assert_eq!(proxies[0].max_associations, 256);
        assert_eq!(proxies[0].upstream_policy.snapshot().plan.faults().len(), 1);
        let default_config = NativeConfig::parse("version = 1\n").unwrap();
        assert!(default_config
            .compile_datagram_proxies()
            .unwrap()
            .is_empty());
    }

    #[test]
    fn schema_v1_rejects_invalid_datagram_global_bounds_and_proxy_addresses() {
        let limits = NativeConfig::parse("version=1\n[runtime.datagram]\nmax_associations=0\n");
        assert!(limits.is_err());
        let target = NativeConfig::parse("version=1\n[[datagram_proxies]]\nname='dns'\nlisten='127.0.0.1:0'\nupstream='0.0.0.0:53'\n");
        assert!(target.is_err());
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

    #[test]
    fn zero_bandwidth_burst_is_a_field_error_without_panicking() {
        let result = std::panic::catch_unwind(|| {
            NativeConfig::parse(
                r#"
version = 1
[[proxy]]
name = "redis"
listen = "127.0.0.1:0"
upstream = "127.0.0.1:6379"
[[proxy.fault]]
id = "rate"
direction = "downstream"
type = "bandwidth"
bytes_per_second = 1000
burst_bytes = 0
"#,
            )
        });
        let error = result
            .expect("config validation must not panic")
            .unwrap_err();
        assert!(error.to_string().contains("fault.burst_bytes"));
    }

    #[test]
    fn admin_tokens_are_redacted_from_debug_output() {
        let config =
            NativeConfig::parse("version = 1\n[admin]\nauth_token = 'secret-token'\n").unwrap();
        assert!(!format!("{config:?}").contains("secret-token"));
        assert!(format!("{config:?}").contains("[REDACTED]"));
    }

    #[test]
    fn schema_v1_runtime_and_proxy_bounds_compile_with_omission_defaults() {
        let config = NativeConfig::parse(
            r#"
version = 1
[[proxy]]
name = "redis"
listen = "127.0.0.1:0"
upstream = "127.0.0.1:6379"
"#,
        )
        .unwrap();
        let proxy = config.compile_proxies().unwrap().remove(0);
        assert_eq!(proxy.connect_timeout, Duration::from_secs(5));
        assert_eq!(proxy.seed, 0);
        assert_eq!(config.runtime.global_connections, 1024);
        assert_eq!(config.runtime.history, 256);
        assert_eq!(config.runtime.relay_buffer().unwrap().get(), 64 * 1024);
        assert_eq!(
            config.runtime.termination_grace().unwrap(),
            Duration::from_secs(5)
        );
    }

    #[test]
    fn schema_v1_exposes_runtime_bounds_proxy_seed_timeout_and_latency_buffer() {
        let config = NativeConfig::parse(
            r#"
version = 1
[runtime]
global_connections = 17
history = 23
relay_buffer_bytes = 4096
termination_grace_ms = 700
[[proxy]]
name = "redis"
listen = "127.0.0.1:0"
upstream = "127.0.0.1:6379"
connect_timeout_ms = 1200
seed = 91
[[proxy.fault]]
id = "delay"
direction = "downstream"
type = "latency"
max_buffer_bytes = 1234
"#,
        )
        .unwrap();
        let proxy = config.compile_proxies().unwrap().remove(0);
        assert_eq!(config.runtime.global_connections, 17);
        assert_eq!(config.runtime.history, 23);
        assert_eq!(config.runtime.relay_buffer().unwrap().get(), 4096);
        assert_eq!(
            config.runtime.termination_grace().unwrap(),
            Duration::from_millis(700)
        );
        assert_eq!(proxy.connect_timeout, Duration::from_millis(1200));
        assert_eq!(proxy.seed, 91);
        let eggchaos_core::FaultKind::Latency(latency) = proxy.downstream_faults.faults()[0].kind
        else {
            panic!("latency config compiled as latency");
        };
        assert_eq!(latency.max_buffer_bytes.get(), 1234);
    }

    #[test]
    fn schema_v1_rejects_zero_runtime_buffers_and_timeout() {
        let runtime = NativeConfig::parse("version=1\n[runtime]\nrelay_buffer_bytes=0\n");
        assert!(runtime
            .unwrap_err()
            .to_string()
            .contains("relay_buffer_bytes"));
        let timeout = NativeConfig::parse("version=1\n[[proxy]]\nname='p'\nlisten='127.0.0.1:0'\nupstream='127.0.0.1:1'\nconnect_timeout_ms=0\n");
        assert!(timeout.unwrap_err().to_string().contains("connect timeout"));
    }
}
