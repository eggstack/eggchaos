//! Stable native `/v1` stream, proxy, and Scenario V1 wire DTOs.
//!
//! This module owns the JSON contract: field spellings, defaults, units,
//! bounds, discriminators, and unknown-field rejection. Conversions land
//! only on `eggchaos-core` semantic types. Runtime assembly (`ProxySpec`,
//! live policies, listener ownership) stays in `eggchaos-server`, which
//! re-exports these DTOs for source compatibility.
use std::{
    net::SocketAddr,
    num::{NonZeroU64, NonZeroUsize},
    time::Duration,
};

use eggchaos_core::{
    BandwidthConfig, BlackholeConfig, DatagramFaultKind, DatagramFaultSpec, DatagramPlan,
    DatagramQueueLimits, Direction, DisconnectConfig, FaultId, FaultKind, FaultPlan, FaultSpec,
    LatencyConfig, LimitDataConfig, Probability, SliceConfig, SlowCloseConfig,
};
use serde::{Deserialize, Serialize};

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
/// Maximum proxy/connect timeout in milliseconds shared by wire validation.
pub const MAX_TIMEOUT_MS: u64 = 300_000;
/// Maximum connections/history bound shared by wire validation.
pub const MAX_CONNECTION_LIMIT: usize = 1_000_000;
/// Maximum retained-history bound shared by wire validation.
pub const MAX_HISTORY_LIMIT: usize = 1_000_000;
/// Maximum relay buffer in bytes shared by wire validation.
pub const MAX_RELAY_BUFFER_BYTES: usize = 16 * 1024 * 1024;
/// Maximum latency buffer in bytes shared by wire validation.
pub const MAX_LATENCY_BUFFER_BYTES: u64 = 64 * 1024 * 1024;

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
    /// Validate wire bounds and proxy identity without assembling runtime state.
    ///
    /// The name rule mirrors `ProxySpec::validate` in `eggchaos-server`;
    /// `contract_proxy_name_parity` pins agreement on a shared corpus.
    pub fn validate(&self) -> Result<(), String> {
        validate_proxy_name(&self.name)?;
        if self.connect_timeout_ms == 0 || self.connect_timeout_ms > MAX_TIMEOUT_MS {
            return Err(format!(
                "connect_timeout_ms must be in 1..={MAX_TIMEOUT_MS}"
            ));
        }
        Ok(())
    }
}

/// Validate a proxy name with the rule shared by the runtime authority.
///
/// Names travel as single URL path segments, so the charset is restricted
/// to unreserved characters.
pub fn validate_proxy_name(name: &str) -> Result<(), String> {
    if name.is_empty() || name.len() > 128 {
        return Err("proxy name must be 1..=128 bytes".into());
    }
    if !name
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err("proxy name must match [A-Za-z0-9._-]+".into());
    }
    Ok(())
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

impl NativeProxyPatchV1 {
    /// Validate supplied timeout bounds before a patch is applied.
    pub fn validate(&self) -> Result<(), String> {
        if self
            .connect_timeout_ms
            .is_some_and(|timeout| timeout == 0 || timeout > MAX_TIMEOUT_MS)
        {
            return Err(format!(
                "connect_timeout_ms must be in 1..={MAX_TIMEOUT_MS}"
            ));
        }
        Ok(())
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
    /// Validate this typed fault configuration before publication.
    pub fn validate(&self) -> Result<(), String> {
        let kind = self.clone().into_core()?;
        let id = FaultId::new("validation").map_err(|error| error.to_string())?;
        let probability = Probability::new(1.0).map_err(|error| error.to_string())?;
        FaultPlan::new(vec![FaultSpec {
            id,
            probability,
            kind,
        }])
        .map(|_| ())
        .map_err(|error| error.to_string())
    }

    pub fn into_core(self) -> Result<FaultKind, String> {
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

    pub fn from_core(kind: FaultKind) -> Self {
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
            // M036 temporary compile-only arm: no native `stream-loss` DTO
            // exists yet, so a core-constructed StreamLoss has no wire
            // representation. Unreachable through every M036 construction
            // path (native authoring cannot name the kind); M037 replaces
            // this arm with the versioned `stream-loss` DTO.
            FaultKind::StreamLoss(_) => unreachable!(
                "native stream-loss DTO is owned by M037; \
                 core StreamLoss cannot round-trip through protocol in M036"
            ),
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
    /// Validate the identifier, probability, and typed fault configuration.
    pub fn validate(&self) -> Result<(), String> {
        FaultId::new(self.id.clone()).map_err(|error| error.to_string())?;
        Probability::new(self.probability).map_err(|error| error.to_string())?;
        self.kind.validate()
    }

    pub fn into_core(self) -> Result<(Direction, FaultSpec), String> {
        let id = FaultId::new(self.id.clone()).map_err(|error| error.to_string())?;
        let probability = Probability::new(self.probability).map_err(|error| error.to_string())?;
        let kind = self.kind.into_core()?;
        Ok((
            self.direction,
            FaultSpec {
                id,
                probability,
                kind,
            },
        ))
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
    /// Validate any supplied probability and replacement fault configuration.
    pub fn validate(&self) -> Result<(), String> {
        if let Some(probability) = self.probability {
            Probability::new(probability).map_err(|error| error.to_string())?;
        }
        if let Some(kind) = &self.kind {
            kind.validate()?;
        }
        Ok(())
    }

    pub fn into_core(self) -> Result<(Option<f64>, Option<FaultKind>), String> {
        if let Some(probability) = self.probability {
            Probability::new(probability).map_err(|error| error.to_string())?;
        }
        let kind = self.kind.map(FaultKindV1::into_core).transpose()?;
        Ok((self.probability, kind))
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
            kind: FaultKindV1::from_core(value.kind),
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

// Assembled by `eggchaos-server` from its runtime proxy view; the field
// layout above is the contract.

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
    /// Datagram runtime global bounds.
    #[serde(default)]
    pub datagram: DatagramRuntimeConfigV1,
}

/// Global limits for the separate datagram resource family.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DatagramRuntimeConfigV1 {
    #[serde(default = "default_dgram_proxies")]
    pub max_proxies: usize,
    #[serde(default = "default_dgram_associations")]
    pub max_associations: usize,
    #[serde(default = "default_dgram_history")]
    pub history: usize,
    #[serde(default = "default_dgram_ingress_slots")]
    pub ingress_per_association: usize,
    #[serde(default = "default_dgram_ingress_bytes")]
    pub max_ingress_queue_bytes: usize,
}

const fn default_dgram_proxies() -> usize {
    128
}
const fn default_dgram_associations() -> usize {
    4096
}
const fn default_dgram_history() -> usize {
    1024
}
const fn default_dgram_ingress_slots() -> usize {
    16
}
const fn default_dgram_ingress_bytes() -> usize {
    64 * 1024 * 1024
}

impl Default for DatagramRuntimeConfigV1 {
    fn default() -> Self {
        Self {
            max_proxies: default_dgram_proxies(),
            max_associations: default_dgram_associations(),
            history: default_dgram_history(),
            ingress_per_association: default_dgram_ingress_slots(),
            max_ingress_queue_bytes: default_dgram_ingress_bytes(),
        }
    }
}

// Assembled into `DatagramRuntimeLimits` by the `eggchaos-server` adapter;
// the field layout and defaults above are the contract.

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
            datagram: DatagramRuntimeConfigV1::default(),
        }
    }
}

impl RuntimeConfigV1 {
    /// Validate admission bounds without assembling the server authority.
    ///
    /// The `eggchaos-server` adapter reuses these exact bounds when it
    /// constructs `AdmissionLimits`; the parity test pins agreement.
    pub fn validate(&self) -> Result<(), String> {
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
        Ok(())
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
    /// Replace one directional datagram fault plan.
    SetDatagramPlan {
        proxy: String,
        direction: eggchaos_core::Direction,
        faults: Vec<DatagramFaultSpecV1>,
    },
    /// Remove one datagram fault by identity.
    RemoveDatagramFault {
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

impl ScenarioFaultV1 {
    /// Convert to the core fault specification.
    pub fn into_core(self) -> Result<FaultSpec, String> {
        Ok(FaultSpec {
            id: FaultId::new(self.id).map_err(|error| error.to_string())?,
            probability: Probability::new(self.probability).map_err(|error| error.to_string())?,
            kind: self.kind.into_core()?,
        })
    }
}

impl ScenarioV1 {
    /// Validate version, event ordering/limits, fault identities, and kinds.
    pub fn validate(&self) -> Result<(), String> {
        if self.version != 1 {
            return Err("unsupported scenario version".into());
        }
        if self.events.len() > 1024 {
            return Err("too many scenario events".into());
        }
        let mut previous = 0;
        for event in &self.events {
            if event.at_ms < previous {
                return Err("scenario events must be ordered".into());
            }
            previous = event.at_ms;
            self.validate_action(&event.action)?;
        }
        Ok(())
    }

    fn validate_action(&self, action: &ScenarioActionV1) -> Result<(), String> {
        match action {
            ScenarioActionV1::SetPlan { faults, .. } => {
                let converted = faults
                    .iter()
                    .cloned()
                    .map(ScenarioFaultV1::into_core)
                    .collect::<Result<Vec<_>, _>>()?;
                FaultPlan::new(converted).map_err(|error| error.to_string())?;
            }
            ScenarioActionV1::RemoveFault { id, .. } => {
                FaultId::new(id.clone()).map_err(|error| error.to_string())?;
            }
            ScenarioActionV1::SetDatagramPlan { faults, .. } => {
                let converted = faults
                    .iter()
                    .cloned()
                    .map(DatagramFaultSpecV1::into_core)
                    .collect::<Result<Vec<_>, _>>()?;
                DatagramPlan::new(converted).map_err(|error| error.to_string())?;
            }
            ScenarioActionV1::RemoveDatagramFault { id, .. } => {
                FaultId::new(id.clone()).map_err(|error| error.to_string())?;
            }
        }
        Ok(())
    }

    /// Convert to `(at_ms, action)` pairs over the consumer-neutral
    /// experiment action authority. The `eggchaos-server` adapter wraps
    /// these in its runtime `Scenario` document.
    pub fn into_core_actions(
        self,
    ) -> Result<Vec<(u64, eggchaos_experiment::ScenarioAction)>, String> {
        self.validate()?;
        let mut events = Vec::with_capacity(self.events.len());
        for event in self.events {
            let action = match event.action {
                ScenarioActionV1::SetPlan {
                    proxy,
                    direction,
                    faults,
                } => {
                    let converted = faults
                        .into_iter()
                        .map(ScenarioFaultV1::into_core)
                        .collect::<Result<Vec<_>, _>>()?;
                    FaultPlan::new(converted.clone()).map_err(|error| error.to_string())?;
                    eggchaos_experiment::ScenarioAction::SetPlan {
                        proxy,
                        direction,
                        faults: converted,
                    }
                }
                ScenarioActionV1::RemoveFault {
                    proxy,
                    direction,
                    id,
                } => {
                    FaultId::new(id.clone()).map_err(|error| error.to_string())?;
                    eggchaos_experiment::ScenarioAction::RemoveFault {
                        proxy,
                        direction,
                        id,
                    }
                }
                ScenarioActionV1::SetDatagramPlan {
                    proxy,
                    direction,
                    faults,
                } => {
                    let converted = faults
                        .into_iter()
                        .map(DatagramFaultSpecV1::into_core)
                        .collect::<Result<Vec<_>, _>>()?;
                    DatagramPlan::new(converted.clone()).map_err(|error| error.to_string())?;
                    eggchaos_experiment::ScenarioAction::SetDatagramPlan {
                        proxy,
                        direction,
                        faults: converted,
                    }
                }
                ScenarioActionV1::RemoveDatagramFault {
                    proxy,
                    direction,
                    id,
                } => {
                    FaultId::new(id.clone()).map_err(|error| error.to_string())?;
                    eggchaos_experiment::ScenarioAction::RemoveDatagramFault {
                        proxy,
                        direction,
                        id,
                    }
                }
            };
            events.push((event.at_ms, action));
        }
        Ok(events)
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

impl ScenarioRunV1 {
    /// Assemble a run view from already-mapped parts.
    ///
    /// The `eggchaos-server` adapter maps its runtime record through this
    /// constructor, so the JSON layout above stays the single contract.
    pub fn from_parts(
        run_id: u64,
        seed: u64,
        status: ScenarioStatusV1,
        applied: usize,
        failure: Option<String>,
        trail: Vec<ScenarioEventResultV1>,
    ) -> Self {
        Self {
            run_id,
            seed,
            status,
            applied,
            failure,
            trail,
        }
    }
}

/// Explicit native schema for one datagram fault stage.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DatagramFaultSpecV1 {
    pub id: String,
    #[serde(default = "default_one")]
    pub probability: f64,
    pub kind: DatagramFaultKindV1,
}

/// Versioned datagram fault vocabulary. Durations are integer nanoseconds.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case", deny_unknown_fields)]
pub enum DatagramFaultKindV1 {
    Delay {
        delay_ns: u64,
        #[serde(default)]
        jitter_ns: u64,
    },
    Loss,
    Duplicate {
        additional_copies: u8,
    },
    Reorder {
        hold_ns: u64,
    },
    PayloadCorrupt {
        bytes: u64,
    },
    Bandwidth {
        bytes_per_second: u64,
        burst_bytes: u64,
    },
}

impl DatagramFaultKindV1 {
    fn into_core(self) -> Result<DatagramFaultKind, String> {
        let duration = Duration::from_nanos;
        Ok(match self {
            Self::Delay {
                delay_ns,
                jitter_ns,
            } => DatagramFaultKind::Delay {
                delay: duration(delay_ns),
                jitter: duration(jitter_ns),
            },
            Self::Loss => DatagramFaultKind::Loss,
            Self::Duplicate { additional_copies } => {
                DatagramFaultKind::Duplicate { additional_copies }
            }
            Self::Reorder { hold_ns } => DatagramFaultKind::Reorder {
                hold: duration(hold_ns),
            },
            Self::PayloadCorrupt { bytes } => DatagramFaultKind::PayloadCorrupt {
                bytes: NonZeroU64::new(bytes).ok_or("bytes must be nonzero")?,
            },
            Self::Bandwidth {
                bytes_per_second,
                burst_bytes,
            } => DatagramFaultKind::Bandwidth {
                bytes_per_second: NonZeroU64::new(bytes_per_second)
                    .ok_or("bytes_per_second must be nonzero")?,
                burst_bytes: NonZeroU64::new(burst_bytes).ok_or("burst_bytes must be nonzero")?,
            },
        })
    }

    fn from_core(kind: DatagramFaultKind) -> Self {
        let ns = |duration: Duration| duration.as_nanos().min(u64::MAX as u128) as u64;
        match kind {
            DatagramFaultKind::Delay { delay, jitter } => Self::Delay {
                delay_ns: ns(delay),
                jitter_ns: ns(jitter),
            },
            DatagramFaultKind::Loss => Self::Loss,
            DatagramFaultKind::Duplicate { additional_copies } => {
                Self::Duplicate { additional_copies }
            }
            DatagramFaultKind::Reorder { hold } => Self::Reorder { hold_ns: ns(hold) },
            DatagramFaultKind::PayloadCorrupt { bytes } => {
                Self::PayloadCorrupt { bytes: bytes.get() }
            }
            DatagramFaultKind::Bandwidth {
                bytes_per_second,
                burst_bytes,
            } => Self::Bandwidth {
                bytes_per_second: bytes_per_second.get(),
                burst_bytes: burst_bytes.get(),
            },
        }
    }
}

impl DatagramFaultSpecV1 {
    /// Convert to the core datagram fault specification.
    pub fn into_core(self) -> Result<DatagramFaultSpec, String> {
        Ok(DatagramFaultSpec {
            id: FaultId::new(self.id).map_err(|error| error.to_string())?,
            probability: Probability::new(self.probability).map_err(|error| error.to_string())?,
            kind: self.kind.into_core()?,
        })
    }
}

/// Add one datagram fault to a proxy direction.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DatagramFaultUpsertV1 {
    pub direction: eggchaos_core::Direction,
    #[serde(flatten)]
    pub fault: DatagramFaultSpecV1,
}

impl From<DatagramFaultSpec> for DatagramFaultSpecV1 {
    fn from(value: DatagramFaultSpec) -> Self {
        Self {
            id: value.id.to_string(),
            probability: value.probability.get(),
            kind: DatagramFaultKindV1::from_core(value.kind),
        }
    }
}

/// Explicit datagram proxy create schema for `/v1`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeDatagramProxyRequestV1 {
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

/// Core-level parts of a datagram proxy definition.
///
/// The `eggchaos-server` adapter assembles these into its
/// `DatagramProxySpec` authority (policies, seed namespaces, global
/// association validation) without re-implementing wire validation.
#[derive(Debug, Clone)]
pub struct DatagramProxyCoreParts {
    /// Proxy name.
    pub name: String,
    /// Listener address.
    pub listen: SocketAddr,
    /// Fixed target address.
    pub upstream: SocketAddr,
    /// Per-proxy association cap.
    pub max_associations: usize,
    /// Idle expiry in milliseconds.
    pub association_idle_timeout_ms: u64,
    /// Shared directional engine queue bounds.
    pub queue_limits: DatagramQueueLimits,
    /// Per-proxy deterministic seed.
    pub seed: u64,
    /// Client-to-target faults.
    pub upstream_faults: Vec<DatagramFaultSpec>,
    /// Target-to-client faults.
    pub downstream_faults: Vec<DatagramFaultSpec>,
}

impl NativeDatagramProxyRequestV1 {
    /// Validate wire bounds and convert faults/queue limits to core parts.
    pub fn into_core_parts(self) -> Result<DatagramProxyCoreParts, String> {
        validate_proxy_name(&self.name)?;
        if self.association_idle_timeout_ms == 0 || self.association_idle_timeout_ms > 86_400_000 {
            return Err("association_idle_timeout_ms must be in 1..=86400000".into());
        }
        let queue_limits = DatagramQueueLimits {
            max_queued_datagrams: NonZeroU64::new(self.max_queued_datagrams)
                .ok_or("max_queued_datagrams must be nonzero")?,
            max_queued_bytes: NonZeroU64::new(self.max_queued_bytes)
                .ok_or("max_queued_bytes must be nonzero")?,
            max_datagram_bytes: NonZeroU64::new(self.max_datagram_size)
                .ok_or("max_datagram_size must be nonzero")?,
        }
        .validate()
        .map_err(|error| error.to_string())?;
        let upstream = self
            .upstream_faults
            .into_iter()
            .map(DatagramFaultSpecV1::into_core)
            .collect::<Result<Vec<_>, _>>()?;
        let downstream = self
            .downstream_faults
            .into_iter()
            .map(DatagramFaultSpecV1::into_core)
            .collect::<Result<Vec<_>, _>>()?;
        let mut ids = std::collections::HashSet::new();
        for fault in upstream.iter().chain(downstream.iter()) {
            if !ids.insert(fault.id.as_str()) {
                return Err(
                    "datagram fault IDs must be unique across directions for path lookup".into(),
                );
            }
        }
        DatagramPlan::new(upstream.clone()).map_err(|error| error.to_string())?;
        DatagramPlan::new(downstream.clone()).map_err(|error| error.to_string())?;
        Ok(DatagramProxyCoreParts {
            name: self.name,
            listen: self.listen,
            upstream: self.upstream,
            max_associations: self.max_associations,
            association_idle_timeout_ms: self.association_idle_timeout_ms,
            queue_limits,
            seed: self.seed,
            upstream_faults: upstream,
            downstream_faults: downstream,
        })
    }

    /// Validate wire bounds without consuming the request.
    pub fn validate(&self) -> Result<(), String> {
        self.clone().into_core_parts().map(|_| ())
    }
}

/// Enable or disable a datagram listener without changing its definition.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeDatagramProxyPatchV1 {
    pub enabled: Option<bool>,
    pub listen: Option<SocketAddr>,
    pub upstream: Option<SocketAddr>,
    pub max_associations: Option<usize>,
    pub association_idle_timeout_ms: Option<u64>,
}

/// Patch one datagram fault's probability or behavior.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DatagramFaultPatchV1 {
    pub probability: Option<f64>,
    pub kind: Option<DatagramFaultKindV1>,
}

impl DatagramFaultPatchV1 {
    /// Convert to validated probability/kind parts.
    pub fn into_core_parts(self) -> Result<(Option<f64>, Option<DatagramFaultKind>), String> {
        let probability = self
            .probability
            .map(|value| Probability::new(value).map_err(|error| error.to_string()))
            .transpose()?;
        let kind = self.kind.map(DatagramFaultKindV1::into_core).transpose()?;
        Ok((probability.map(Probability::get), kind))
    }
}

/// Stable v1 representation of per-direction datagram evidence.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NativeDatagramEvidenceV1 {
    pub admitted_datagrams: u64,
    pub admitted_bytes: u64,
    pub emitted_datagrams: u64,
    pub emitted_bytes: u64,
    pub configured_loss: u64,
    pub queue_overflow: u64,
    pub oversize_datagrams: u64,
    pub duplicated_copies: u64,
    pub corrupted_candidates: u64,
    pub reorder_activations: u64,
    pub queued_datagrams: u64,
    pub queued_bytes: u64,
    pub high_water_datagrams: u64,
    pub high_water_bytes: u64,
    pub fault_activations: [u64; 6],
    pub injected_delay_nanos: u128,
    pub bandwidth_delay_nanos: u128,
    pub last_generation: u64,
    pub last_seed_namespace: u64,
    pub rng_version: eggchaos_core::RngVersion,
}

impl From<eggchaos_core::DatagramEvidence> for NativeDatagramEvidenceV1 {
    fn from(value: eggchaos_core::DatagramEvidence) -> Self {
        Self {
            admitted_datagrams: value.admitted_datagrams,
            admitted_bytes: value.admitted_bytes,
            emitted_datagrams: value.emitted_datagrams,
            emitted_bytes: value.emitted_bytes,
            configured_loss: value.configured_loss,
            queue_overflow: value.queue_overflow,
            oversize_datagrams: value.oversize_datagrams,
            duplicated_copies: value.duplicated_copies,
            corrupted_candidates: value.corrupted_candidates,
            reorder_activations: value.reorder_activations,
            queued_datagrams: value.queued_datagrams,
            queued_bytes: value.queued_bytes,
            high_water_datagrams: value.high_water_datagrams,
            high_water_bytes: value.high_water_bytes,
            fault_activations: value.fault_activations,
            injected_delay_nanos: value.injected_delay_nanos,
            bandwidth_delay_nanos: value.bandwidth_delay_nanos,
            last_generation: value.last_generation,
            last_seed_namespace: value.last_seed_namespace,
            rng_version: value.rng_version,
        }
    }
}

/// Explicit datagram proxy view DTO.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NativeDatagramProxyViewV1 {
    pub name: String,
    pub listen: SocketAddr,
    pub bound_addr: Option<SocketAddr>,
    pub upstream: SocketAddr,
    pub running: bool,
    pub max_associations: usize,
    pub active_associations: usize,
    pub association_idle_timeout_ms: u64,
    pub max_datagram_size: u64,
    pub max_queued_datagrams: u64,
    pub max_queued_bytes: u64,
    pub seed: u64,
    pub upstream_generation: u64,
    pub downstream_generation: u64,
    pub oversize_datagrams: u64,
    pub association_capacity_rejections: u64,
    pub ingress_queue_overflow: u64,
    pub association_setup_failures: u64,
}

// Assembled by `eggchaos-server` from its runtime datagram proxy view.

/// Explicit datagram association evidence DTO. Payloads are never captured.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NativeDatagramAssociationViewV1 {
    pub id: u64,
    pub proxy: String,
    pub client: SocketAddr,
    pub upstream: SocketAddr,
    pub age_ms: u64,
    pub idle_ms: u64,
    pub ingress_datagrams: u64,
    pub ingress_bytes: u64,
    pub egress_datagrams: u64,
    pub egress_bytes: u64,
    pub ingress_queue_overflow: u64,
    pub oversize_datagrams: u64,
    pub administrative_discards: u64,
    pub send_errors: u64,
    pub upstream_evidence: NativeDatagramEvidenceV1,
    pub downstream_evidence: NativeDatagramEvidenceV1,
}

// Assembled by `eggchaos-server` from its runtime association snapshot.

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
    fn datagram_fault_v1_roundtrips_all_six_explicit_kinds() {
        let fixtures = [
            r#"{"id":"delay","probability":1.0,"kind":{"type":"delay","delay_ns":10,"jitter_ns":2}}"#,
            r#"{"id":"loss","probability":0.25,"kind":{"type":"loss"}}"#,
            r#"{"id":"dupe","probability":0.5,"kind":{"type":"duplicate","additional_copies":2}}"#,
            r#"{"id":"reorder","probability":1.0,"kind":{"type":"reorder","hold_ns":30}}"#,
            r#"{"id":"corrupt","probability":1.0,"kind":{"type":"payload-corrupt","bytes":1}}"#,
            r#"{"id":"bandwidth","probability":1.0,"kind":{"type":"bandwidth","bytes_per_second":100,"burst_bytes":50}}"#,
        ];
        for fixture in fixtures {
            let dto: DatagramFaultSpecV1 = serde_json::from_str(fixture).unwrap();
            dto.clone().into_core().unwrap();
            assert_eq!(serde_json::to_string(&dto).unwrap(), fixture);
        }
    }

    #[test]
    fn datagram_proxy_v1_defaults_and_rejects_empty_queue_limits() {
        let request: NativeDatagramProxyRequestV1 = serde_json::from_str(
            r#"{"name":"dns","listen":"127.0.0.1:0","upstream":"127.0.0.1:5353"}"#,
        )
        .unwrap();
        let parts = request.into_core_parts().unwrap();
        assert_eq!(parts.max_associations, 256);
        assert_eq!(
            Duration::from_millis(parts.association_idle_timeout_ms),
            Duration::from_secs(60)
        );
        let invalid: NativeDatagramProxyRequestV1 = serde_json::from_str(
            r#"{"name":"dns","listen":"127.0.0.1:0","upstream":"127.0.0.1:5353","max_queued_bytes":0}"#,
        ).unwrap();
        assert!(invalid.into_core_parts().is_err());
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
    fn runtime_configuration_defaults_match_documented_bounds() {
        let runtime = RuntimeConfigV1::default();
        assert_eq!(runtime.global_connections, 1024);
        assert_eq!(runtime.history, 256);
        assert_eq!(runtime.relay_buffer().unwrap().get(), 64 * 1024);
        assert_eq!(runtime.termination_grace().unwrap(), Duration::from_secs(5));
        assert!(RuntimeConfigV1 {
            global_connections: 1_000_001,
            ..runtime
        }
        .validate()
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
        let actions = scenario.into_core_actions().unwrap();
        assert_eq!(actions.len(), 1);
        assert!(serde_json::from_str::<ScenarioV1>(r#"{"version":1,"seed":7,"events":[{"at_ms":25,"legacy":true,"action":{"type":"remove-fault","proxy":"cache","direction":"upstream","id":"delay"}}]}"#).is_err());
        assert_eq!(
            serde_json::to_string(&ScenarioStatusV1::Completed).unwrap(),
            "\"completed\""
        );
        let datagram = r#"{"version":1,"seed":4,"events":[{"at_ms":0,"action":{"type":"set-datagram-plan","proxy":"dns","direction":"upstream","faults":[{"id":"loss","probability":1.0,"kind":{"type":"loss"}}]}}]}"#;
        let dto: ScenarioV1 = serde_json::from_str(datagram).unwrap();
        assert_eq!(serde_json::to_string(&dto).unwrap(), datagram);
        assert!(matches!(
            dto.into_core_actions().unwrap()[0].1,
            eggchaos_experiment::ScenarioAction::SetDatagramPlan { .. }
        ));
    }
}
