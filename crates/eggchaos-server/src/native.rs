//! Explicit `/v1` wire types and conversion into the runtime model.
use std::{
    net::SocketAddr,
    num::{NonZeroU64, NonZeroUsize},
    time::Duration,
};

use eggchaos_core::{
    BandwidthConfig, BlackholeConfig, DisconnectConfig, FaultKind, LatencyConfig, LimitDataConfig,
    SliceConfig, SlowCloseConfig,
};
use serde::{Deserialize, Serialize};

use crate::{AdmissionLimits, FaultPatch, FaultUpsert, ProxyPatch, ProxySpec, ProxyView};

/// Default native/config buffer size (64 KiB).
pub const NATIVE_DEFAULT_BUFFER_BYTES: u64 = 64 * 1024;
/// Default proxy connect timeout.
pub const NATIVE_DEFAULT_PROXY_TIMEOUT_MS: u64 = 5_000;
/// Default bandwidth rate in bytes per second.
pub const NATIVE_DEFAULT_BANDWIDTH_BYTES_PER_SECOND: u64 = 1;
/// Default bandwidth burst capacity.
pub const NATIVE_DEFAULT_BANDWIDTH_BURST_BYTES: u64 = 64 * 1024;
/// Default limit-data byte count.
pub const NATIVE_DEFAULT_LIMIT_BYTES: u64 = 1;
/// Default slice average size.
pub const NATIVE_DEFAULT_SLICE_AVERAGE_SIZE: u64 = 1024;

const fn default_true() -> bool {
    true
}
const fn default_one() -> f64 {
    1.0
}
const fn default_buffer_bytes() -> u64 {
    NATIVE_DEFAULT_BUFFER_BYTES
}
const fn default_timeout_ms() -> u64 {
    NATIVE_DEFAULT_PROXY_TIMEOUT_MS
}
const fn default_bandwidth_rate() -> u64 {
    NATIVE_DEFAULT_BANDWIDTH_BYTES_PER_SECOND
}
const fn default_bandwidth_burst() -> u64 {
    NATIVE_DEFAULT_BANDWIDTH_BURST_BYTES
}
const fn default_limit_bytes() -> u64 {
    NATIVE_DEFAULT_LIMIT_BYTES
}
const fn default_slice_average() -> u64 {
    NATIVE_DEFAULT_SLICE_AVERAGE_SIZE
}
const MAX_CONNECTION_LIMIT: usize = 1_000_000;
const MAX_HISTORY_LIMIT: usize = 1_000_000;
const MAX_RELAY_BUFFER_BYTES: usize = 16 * 1024 * 1024;
const MAX_TIMEOUT_MS: u64 = 300_000;
const MAX_LATENCY_BUFFER_BYTES: u64 = 64 * 1024 * 1024;

/// Explicit native proxy create schema for `/v1`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeProxyRequestV1 {
    /// Proxy name.
    pub name: String,
    /// Listener address.
    pub listen: SocketAddr,
    /// Fixed target address.
    pub upstream: SocketAddr,
    /// Whether the listener starts enabled.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Optional per-proxy active connection limit.
    pub max_connections: Option<usize>,
    /// Connect timeout in milliseconds.
    #[serde(default = "default_timeout_ms")]
    pub connect_timeout_ms: u64,
    /// Per-proxy deterministic seed namespace.
    #[serde(default)]
    pub seed: u64,
}

impl NativeProxyRequestV1 {
    pub(crate) fn into_runtime(self) -> Result<ProxySpec, String> {
        if self.connect_timeout_ms == 0 || self.connect_timeout_ms > MAX_TIMEOUT_MS {
            return Err(format!(
                "connect_timeout_ms must be in 1..={MAX_TIMEOUT_MS}"
            ));
        }
        let mut proxy = ProxySpec::new(self.name, self.listen, self.upstream);
        proxy.enabled = self.enabled;
        proxy.max_connections = self.max_connections;
        proxy.connect_timeout = Duration::from_millis(self.connect_timeout_ms);
        proxy.seed = self.seed;
        Ok(proxy)
    }
}

/// Explicit native proxy patch schema for `/v1`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeProxyPatchV1 {
    /// Replacement listener address.
    pub listen: Option<SocketAddr>,
    /// Replacement fixed target.
    pub upstream: Option<SocketAddr>,
    /// Enable or disable the listener.
    pub enabled: Option<bool>,
    /// Replace or clear the per-proxy connection limit.
    #[serde(default)]
    pub max_connections: Option<Option<usize>>,
    /// Replacement connect timeout in milliseconds.
    pub connect_timeout_ms: Option<u64>,
}

impl From<NativeProxyPatchV1> for ProxyPatch {
    fn from(value: NativeProxyPatchV1) -> Self {
        Self {
            listen: value.listen,
            upstream: value.upstream,
            enabled: value.enabled,
            max_connections: value.max_connections,
            connect_timeout_ms: value.connect_timeout_ms,
        }
    }
}

/// Explicit fault behavior schema. Time values are integer nanoseconds.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case", deny_unknown_fields)]
pub enum FaultKindV1 {
    /// Delay writes with optional symmetric jitter.
    Latency {
        delay_ns: u64,
        #[serde(default)]
        jitter_ns: u64,
        #[serde(default = "default_buffer_bytes")]
        max_buffer_bytes: u64,
    },
    /// Limit sustained write rate with a token bucket.
    Bandwidth {
        #[serde(default = "default_bandwidth_rate")]
        bytes_per_second: u64,
        #[serde(default = "default_bandwidth_burst")]
        burst_bytes: u64,
    },
    /// Discard writes until an optional deadline.
    Blackhole {
        #[serde(default)]
        close_after_ns: Option<u64>,
    },
    /// Forward at most this many bytes.
    #[serde(rename = "limit-data")]
    LimitData {
        #[serde(default = "default_limit_bytes")]
        bytes: u64,
    },
    /// Delay write shutdown.
    #[serde(rename = "slow-close")]
    SlowClose { delay_ns: u64 },
    /// Split writes into slices.
    Slice {
        #[serde(default = "default_slice_average")]
        average_size: u64,
        #[serde(default)]
        variation: u64,
        #[serde(default)]
        delay_ns: u64,
    },
    /// Terminate after an optional delay.
    Disconnect {
        #[serde(default)]
        after_ns: u64,
        #[serde(default)]
        hard_reset: bool,
    },
}

impl FaultKindV1 {
    pub(crate) fn into_runtime(self) -> Result<FaultKind, String> {
        let nonzero = |field: &str, value| {
            NonZeroU64::new(value).ok_or_else(|| format!("{field} must be non-zero"))
        };
        let duration = |nanos: u64| Duration::from_nanos(nanos);
        Ok(match self {
            Self::Latency {
                delay_ns,
                jitter_ns,
                max_buffer_bytes,
            } => {
                if max_buffer_bytes > MAX_LATENCY_BUFFER_BYTES {
                    return Err(format!(
                        "max_buffer_bytes must be at most {MAX_LATENCY_BUFFER_BYTES}"
                    ));
                }
                FaultKind::Latency(LatencyConfig {
                    delay: duration(delay_ns),
                    jitter: duration(jitter_ns),
                    max_buffer_bytes: nonzero("max_buffer_bytes", max_buffer_bytes)?,
                })
            }
            Self::Bandwidth {
                bytes_per_second,
                burst_bytes,
            } => FaultKind::Bandwidth(BandwidthConfig {
                bytes_per_second: nonzero("bytes_per_second", bytes_per_second)?,
                burst_bytes: nonzero("burst_bytes", burst_bytes)?,
            }),
            Self::Blackhole { close_after_ns } => FaultKind::Blackhole(BlackholeConfig {
                close_after: close_after_ns.map(duration),
            }),
            Self::LimitData { bytes } => FaultKind::LimitData(LimitDataConfig {
                bytes: nonzero("bytes", bytes)?,
            }),
            Self::SlowClose { delay_ns } => FaultKind::SlowClose(SlowCloseConfig {
                delay: duration(delay_ns),
            }),
            Self::Slice {
                average_size,
                variation,
                delay_ns,
            } => FaultKind::Slice(SliceConfig {
                average_size: nonzero("average_size", average_size)?,
                variation,
                delay: duration(delay_ns),
            }),
            Self::Disconnect {
                after_ns,
                hard_reset,
            } => FaultKind::Disconnect(DisconnectConfig {
                after: duration(after_ns),
                hard_reset,
            }),
        })
    }

    fn from_runtime(kind: FaultKind) -> Self {
        let ns = |duration: Duration| duration.as_nanos().min(u64::MAX as u128) as u64;
        match kind {
            FaultKind::Latency(v) => Self::Latency {
                delay_ns: ns(v.delay),
                jitter_ns: ns(v.jitter),
                max_buffer_bytes: v.max_buffer_bytes.get(),
            },
            FaultKind::Bandwidth(v) => Self::Bandwidth {
                bytes_per_second: v.bytes_per_second.get(),
                burst_bytes: v.burst_bytes.get(),
            },
            FaultKind::Blackhole(v) => Self::Blackhole {
                close_after_ns: v.close_after.map(ns),
            },
            FaultKind::LimitData(v) => Self::LimitData {
                bytes: v.bytes.get(),
            },
            FaultKind::SlowClose(v) => Self::SlowClose {
                delay_ns: ns(v.delay),
            },
            FaultKind::Slice(v) => Self::Slice {
                average_size: v.average_size.get(),
                variation: v.variation,
                delay_ns: ns(v.delay),
            },
            FaultKind::Disconnect(v) => Self::Disconnect {
                after_ns: ns(v.after),
                hard_reset: v.hard_reset,
            },
        }
    }
}

/// Native fault creation request.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FaultUpsertV1 {
    /// Direction of writes to impair.
    pub direction: eggchaos_core::Direction,
    /// Opaque fault identity.
    pub id: String,
    /// Per-connection activation probability.
    #[serde(default = "default_one")]
    pub probability: f64,
    /// Explicit fault behavior.
    pub kind: FaultKindV1,
}

impl FaultUpsertV1 {
    pub(crate) fn into_runtime(self) -> Result<FaultUpsert, String> {
        Ok(FaultUpsert {
            direction: self.direction,
            id: self.id,
            probability: self.probability,
            kind: self.kind.into_runtime()?,
        })
    }
}

/// Native fault patch request; the `kind` object is identical to create.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FaultPatchV1 {
    /// Replacement probability.
    pub probability: Option<f64>,
    /// Replacement behavior using the create schema.
    pub kind: Option<FaultKindV1>,
}

impl FaultPatchV1 {
    pub(crate) fn into_runtime(self) -> Result<FaultPatch, String> {
        Ok(FaultPatch {
            probability: self.probability,
            kind: self.kind.map(FaultKindV1::into_runtime).transpose()?,
        })
    }
}

/// Native fault response representation.
#[derive(Debug, Clone, Serialize)]
pub struct NativeFaultViewV1 {
    /// Opaque identifier.
    pub id: String,
    /// Activation probability.
    pub probability: f64,
    /// Explicit native fault behavior.
    pub kind: FaultKindV1,
}

impl From<&eggchaos_core::FaultSpec> for NativeFaultViewV1 {
    fn from(value: &eggchaos_core::FaultSpec) -> Self {
        Self {
            id: value.id.to_string(),
            probability: value.probability.get(),
            kind: FaultKindV1::from_runtime(value.kind),
        }
    }
}

/// Native proxy view with explicit fault representations.
#[derive(Debug, Clone, Serialize)]
pub struct NativeProxyViewV1 {
    /// Proxy name.
    pub name: String,
    /// Configured listen address.
    pub listen: SocketAddr,
    /// Fixed target address.
    pub upstream: SocketAddr,
    /// Bound address when running.
    pub bound_addr: Option<SocketAddr>,
    /// Whether listener is running.
    pub running: bool,
    /// Desired enabled state.
    pub enabled: bool,
    /// Upstream faults.
    pub upstream_faults: Vec<NativeFaultViewV1>,
    /// Downstream faults.
    pub downstream_faults: Vec<NativeFaultViewV1>,
    /// Upstream policy generation.
    pub upstream_generation: u64,
    /// Downstream policy generation.
    pub downstream_generation: u64,
    /// Upstream deterministic seed namespace.
    pub upstream_seed_namespace: u64,
    /// Downstream deterministic seed namespace.
    pub downstream_seed_namespace: u64,
    /// Per-proxy connection cap.
    pub max_connections: Option<usize>,
    /// Direct-connect timeout in milliseconds.
    pub connect_timeout_ms: u64,
    /// Per-proxy deterministic seed.
    pub seed: u64,
}

impl From<ProxyView> for NativeProxyViewV1 {
    fn from(value: ProxyView) -> Self {
        Self {
            name: value.name,
            listen: value.listen,
            upstream: value.upstream,
            bound_addr: value.bound_addr,
            running: value.running,
            enabled: value.enabled,
            upstream_faults: value
                .upstream_faults
                .faults()
                .iter()
                .map(NativeFaultViewV1::from)
                .collect(),
            downstream_faults: value
                .downstream_faults
                .faults()
                .iter()
                .map(NativeFaultViewV1::from)
                .collect(),
            upstream_generation: value.upstream_generation,
            downstream_generation: value.downstream_generation,
            upstream_seed_namespace: value.upstream_seed_namespace,
            downstream_seed_namespace: value.downstream_seed_namespace,
            max_connections: value.max_connections,
            connect_timeout_ms: value.connect_timeout_ms,
            seed: value.seed,
        }
    }
}

/// Runtime tuning passed to `ServiceBuilder` from schema-v1 configuration.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeConfigV1 {
    /// Maximum active connections across proxies.
    #[serde(default = "default_global_connections")]
    pub global_connections: usize,
    /// Maximum retained closed connection records.
    #[serde(default = "default_history")]
    pub history: usize,
    /// Bounded relay buffer in bytes.
    #[serde(default = "default_buffer_bytes")]
    pub relay_buffer_bytes: u64,
    /// Graceful termination drain period in milliseconds.
    #[serde(default = "default_termination_grace_ms")]
    pub termination_grace_ms: u64,
}

const fn default_global_connections() -> usize {
    1024
}
const fn default_history() -> usize {
    256
}
const fn default_termination_grace_ms() -> u64 {
    NATIVE_DEFAULT_PROXY_TIMEOUT_MS
}

impl Default for RuntimeConfigV1 {
    fn default() -> Self {
        Self {
            global_connections: default_global_connections(),
            history: default_history(),
            relay_buffer_bytes: default_buffer_bytes(),
            termination_grace_ms: default_termination_grace_ms(),
        }
    }
}

impl RuntimeConfigV1 {
    /// Convert the wire limits to the runtime admission bound type.
    pub fn limits(self) -> Result<AdmissionLimits, String> {
        if self.global_connections == 0 || self.global_connections > MAX_CONNECTION_LIMIT {
            return Err(format!(
                "runtime.global_connections must be in 1..={MAX_CONNECTION_LIMIT}"
            ));
        }
        if self.history > MAX_HISTORY_LIMIT {
            return Err(format!(
                "runtime.history must be at most {MAX_HISTORY_LIMIT}"
            ));
        }
        Ok(AdmissionLimits {
            global_connections: self.global_connections,
            history: self.history,
        })
    }
    /// Convert the relay buffer size to the runtime's non-zero byte count.
    pub fn relay_buffer(self) -> Result<NonZeroUsize, String> {
        let bytes = usize::try_from(self.relay_buffer_bytes)
            .map_err(|_| "runtime.relay_buffer_bytes exceeds platform range")?;
        if bytes > MAX_RELAY_BUFFER_BYTES {
            return Err(format!(
                "runtime.relay_buffer_bytes must be at most {MAX_RELAY_BUFFER_BYTES}"
            ));
        }
        NonZeroUsize::new(bytes).ok_or_else(|| "runtime.relay_buffer_bytes must be non-zero".into())
    }
    /// Return the termination drain grace as a duration.
    pub fn termination_grace(self) -> Result<Duration, String> {
        if self.termination_grace_ms > MAX_TIMEOUT_MS {
            return Err(format!(
                "runtime.termination_grace_ms must be at most {MAX_TIMEOUT_MS}"
            ));
        }
        Ok(Duration::from_millis(self.termination_grace_ms))
    }
}

/// Native scenario input document for `/v1/scenarios/apply`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScenarioV1 {
    /// Scenario document version.
    pub version: u32,
    /// Deterministic scenario seed.
    pub seed: u64,
    /// Ordered events.
    pub events: Vec<ScenarioEventV1>,
}

/// Native scenario event with its action fields flattened.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScenarioEventV1 {
    /// Milliseconds after scenario start.
    pub at_ms: u64,
    /// Action data.
    pub action: ScenarioActionV1,
}

/// Supported scenario action schema.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case", deny_unknown_fields)]
pub enum ScenarioActionV1 {
    /// Replace one directional fault plan.
    SetPlan {
        proxy: String,
        direction: eggchaos_core::Direction,
        faults: Vec<ScenarioFaultV1>,
    },
    /// Remove one fault by identity.
    RemoveFault {
        proxy: String,
        direction: eggchaos_core::Direction,
        id: String,
    },
}

/// Scenario fault in the same v1 vocabulary as native CRUD.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScenarioFaultV1 {
    /// Opaque fault ID.
    pub id: String,
    /// Activation probability.
    #[serde(default = "default_one")]
    pub probability: f64,
    /// Typed fault behavior.
    pub kind: FaultKindV1,
}

impl ScenarioV1 {
    pub(crate) fn into_runtime(self) -> Result<crate::Scenario, String> {
        let mut events = Vec::with_capacity(self.events.len());
        for event in self.events {
            let action = match event.action {
                ScenarioActionV1::SetPlan {
                    proxy,
                    direction,
                    faults,
                } => {
                    let mut converted = Vec::with_capacity(faults.len());
                    for fault in faults {
                        converted.push(eggchaos_core::FaultSpec {
                            id: eggchaos_core::FaultId::new(fault.id)
                                .map_err(|error| error.to_string())?,
                            probability: eggchaos_core::Probability::new(fault.probability)
                                .map_err(|error| error.to_string())?,
                            kind: fault.kind.into_runtime()?,
                        });
                    }
                    crate::ScenarioAction::SetPlan {
                        proxy,
                        direction,
                        faults: converted,
                    }
                }
                ScenarioActionV1::RemoveFault {
                    proxy,
                    direction,
                    id,
                } => crate::ScenarioAction::RemoveFault {
                    proxy,
                    direction,
                    id,
                },
            };
            events.push(crate::ScenarioEvent {
                at_ms: event.at_ms,
                action,
            });
        }
        Ok(crate::Scenario {
            version: self.version,
            seed: self.seed,
            events,
        })
    }
}

/// Native scenario status spellings.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ScenarioStatusV1 {
    Pending,
    Running,
    Cancelling,
    Cancelled,
    Completed,
    Failed,
}

/// Native scenario event evidence.
#[derive(Debug, Clone, Serialize)]
pub struct ScenarioEventResultV1 {
    /// Event index.
    pub index: usize,
    /// Scheduled time in milliseconds.
    pub at_ms: u64,
    /// Stable action name.
    pub action: String,
    /// Target proxy.
    pub proxy: String,
    /// Direction.
    pub direction: eggchaos_core::Direction,
    /// Global generation after action.
    pub global_generation: u64,
    /// Upstream generation after action.
    pub upstream_generation: u64,
    /// Downstream generation after action.
    pub downstream_generation: u64,
}

/// Native scenario run response.
#[derive(Debug, Clone, Serialize)]
pub struct ScenarioRunV1 {
    /// Run ID.
    pub run_id: u64,
    /// Scenario seed.
    pub seed: u64,
    /// Current lifecycle status.
    pub status: ScenarioStatusV1,
    /// Applied event count.
    pub applied: usize,
    /// Failure detail, if any.
    pub failure: Option<String>,
    /// Event generation trail.
    pub trail: Vec<ScenarioEventResultV1>,
}

impl From<crate::ScenarioRunRecord> for ScenarioRunV1 {
    fn from(record: crate::ScenarioRunRecord) -> Self {
        let status = match record.status {
            crate::ScenarioRunStatus::Pending => ScenarioStatusV1::Pending,
            crate::ScenarioRunStatus::Running => ScenarioStatusV1::Running,
            crate::ScenarioRunStatus::Cancelling => ScenarioStatusV1::Cancelling,
            crate::ScenarioRunStatus::Cancelled => ScenarioStatusV1::Cancelled,
            crate::ScenarioRunStatus::Completed => ScenarioStatusV1::Completed,
            crate::ScenarioRunStatus::Failed => ScenarioStatusV1::Failed,
        };
        Self {
            run_id: record.run_id,
            seed: record.seed,
            status,
            applied: record.applied,
            failure: record.failure,
            trail: record
                .trail
                .into_iter()
                .map(|event| ScenarioEventResultV1 {
                    index: event.index,
                    at_ms: event.at_ms,
                    action: event.action,
                    proxy: event.proxy,
                    direction: event.direction,
                    global_generation: event.global_generation,
                    upstream_generation: event.upstream_generation,
                    downstream_generation: event.downstream_generation,
                })
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_fault_create_patch_and_response_share_the_versioned_fixture() {
        let kind = FaultKindV1::Latency {
            delay_ns: 250_000_000,
            jitter_ns: 10_000_000,
            max_buffer_bytes: 65_536,
        };
        let create = FaultUpsertV1 {
            direction: eggchaos_core::Direction::Downstream,
            id: "delay".into(),
            probability: 1.0,
            kind: kind.clone(),
        };
        let create_json = serde_json::to_string(&create).unwrap();
        assert_eq!(
            create_json,
            r#"{"direction":"downstream","id":"delay","probability":1.0,"kind":{"type":"latency","delay_ns":250000000,"jitter_ns":10000000,"max_buffer_bytes":65536}}"#
        );
        let patch = FaultPatchV1 {
            probability: Some(0.5),
            kind: Some(kind),
        };
        let patch_value: serde_json::Value = serde_json::to_value(patch).unwrap();
        let create_value: serde_json::Value = serde_json::from_str(&create_json).unwrap();
        assert_eq!(patch_value["kind"], create_value["kind"]);
        let roundtrip: FaultUpsertV1 = serde_json::from_str(&create_json).unwrap();
        assert_eq!(serde_json::to_string(&roundtrip).unwrap(), create_json);
    }

    #[test]
    fn every_native_fault_kind_has_an_exact_wire_fixture() {
        let fixtures = [
            (
                FaultKindV1::Latency {
                    delay_ns: 1,
                    jitter_ns: 2,
                    max_buffer_bytes: 3,
                },
                r#"{"type":"latency","delay_ns":1,"jitter_ns":2,"max_buffer_bytes":3}"#,
            ),
            (
                FaultKindV1::Bandwidth {
                    bytes_per_second: 12,
                    burst_bytes: 4,
                },
                r#"{"type":"bandwidth","bytes_per_second":12,"burst_bytes":4}"#,
            ),
            (
                FaultKindV1::Blackhole {
                    close_after_ns: None,
                },
                r#"{"type":"blackhole","close_after_ns":null}"#,
            ),
            (
                FaultKindV1::LimitData { bytes: 7 },
                r#"{"type":"limit-data","bytes":7}"#,
            ),
            (
                FaultKindV1::SlowClose { delay_ns: 9 },
                r#"{"type":"slow-close","delay_ns":9}"#,
            ),
            (
                FaultKindV1::Slice {
                    average_size: 10,
                    variation: 2,
                    delay_ns: 11,
                },
                r#"{"type":"slice","average_size":10,"variation":2,"delay_ns":11}"#,
            ),
            (
                FaultKindV1::Disconnect {
                    after_ns: 0,
                    hard_reset: false,
                },
                r#"{"type":"disconnect","after_ns":0,"hard_reset":false}"#,
            ),
        ];
        for (kind, expected) in fixtures {
            assert_eq!(serde_json::to_string(&kind).unwrap(), expected);
            let parsed: FaultKindV1 = serde_json::from_str(expected).unwrap();
            assert_eq!(serde_json::to_string(&parsed).unwrap(), expected);
        }
        let bandwidth: FaultKindV1 = serde_json::from_str(r#"{"type":"bandwidth"}"#).unwrap();
        assert_eq!(
            serde_json::to_string(&bandwidth).unwrap(),
            format!(
                r#"{{"type":"bandwidth","bytes_per_second":{},"burst_bytes":{}}}"#,
                NATIVE_DEFAULT_BANDWIDTH_BYTES_PER_SECOND, NATIVE_DEFAULT_BANDWIDTH_BURST_BYTES
            )
        );
        let limit_data: FaultKindV1 = serde_json::from_str(r#"{"type":"limit-data"}"#).unwrap();
        assert_eq!(
            serde_json::to_string(&limit_data).unwrap(),
            format!(r#"{{"type":"limit-data","bytes":{NATIVE_DEFAULT_LIMIT_BYTES}}}"#)
        );
        let slice: FaultKindV1 = serde_json::from_str(r#"{"type":"slice"}"#).unwrap();
        assert_eq!(
            serde_json::to_string(&slice).unwrap(),
            format!(
                r#"{{"type":"slice","average_size":{},"variation":0,"delay_ns":0}}"#,
                NATIVE_DEFAULT_SLICE_AVERAGE_SIZE
            )
        );
    }

    #[test]
    fn native_proxy_create_and_patch_use_connect_timeout_ms() {
        let create = NativeProxyRequestV1 {
            name: "cache".into(),
            listen: "127.0.0.1:0".parse().unwrap(),
            upstream: "127.0.0.1:6379".parse().unwrap(),
            enabled: true,
            max_connections: None,
            connect_timeout_ms: 2_500,
            seed: 4,
        };
        let patch = NativeProxyPatchV1 {
            connect_timeout_ms: Some(2_500),
            ..NativeProxyPatchV1::default()
        };
        assert_eq!(
            serde_json::to_value(create).unwrap()["connect_timeout_ms"],
            2500
        );
        assert_eq!(
            serde_json::to_value(patch).unwrap()["connect_timeout_ms"],
            2500
        );
    }

    #[test]
    fn runtime_configuration_defaults_match_existing_runtime_defaults() {
        let runtime = RuntimeConfigV1::default();
        assert_eq!(
            runtime.global_connections,
            AdmissionLimits::default().global_connections
        );
        assert_eq!(runtime.history, AdmissionLimits::default().history);
        assert_eq!(runtime.relay_buffer().unwrap().get(), 64 * 1024);
        assert_eq!(runtime.termination_grace().unwrap(), Duration::from_secs(5));
        assert!(RuntimeConfigV1 {
            global_connections: 1_000_001,
            ..runtime
        }
        .limits()
        .is_err());
        assert!(RuntimeConfigV1 {
            termination_grace_ms: 300_001,
            ..runtime
        }
        .termination_grace()
        .is_err());
    }

    #[test]
    fn scenario_v1_has_explicit_action_and_status_contract() {
        let json = r#"{"version":1,"seed":7,"events":[{"at_ms":25,"action":{"type":"remove-fault","proxy":"cache","direction":"upstream","id":"delay"}}]}"#;
        let scenario: ScenarioV1 = serde_json::from_str(json).unwrap();
        let runtime = scenario.into_runtime().unwrap();
        assert_eq!(runtime.events.len(), 1);
        assert!(serde_json::from_str::<ScenarioV1>(r#"{"version":1,"seed":7,"events":[{"at_ms":25,"legacy":true,"action":{"type":"remove-fault","proxy":"cache","direction":"upstream","id":"delay"}}]}"#).is_err());
        assert_eq!(
            serde_json::to_string(&ScenarioStatusV1::Completed).unwrap(),
            "\"completed\""
        );
    }
}
