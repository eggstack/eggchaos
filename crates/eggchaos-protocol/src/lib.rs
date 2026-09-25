//! Stable native `/v1` wire contract for eggchaos.
//!
//! This crate owns the JSON wire DTOs, defaults, units, bounds,
//! discriminators, and unknown-field rejection for the versioned native
//! control plane. It is intentionally narrow: no sockets, no Tokio
//! runtime/tasks, no EggServe dependency, no live policy/state, no CLI
//! presentation, no Toxiproxy DTOs, and no EggReplay/EggProbe dependency.
//!
//! ```text
//! eggchaos-core
//!       ^
//!       |
//! eggchaos-experiment
//!       ^
//!       |
//! eggchaos-protocol (this crate: wire DTOs + operation inventory)
//!       ^
//!       |
//! eggchaos-server (runtime/control authority, compatibility re-exports)
//! ```
//!
//! Conversions in this crate land only on `eggchaos-core` or
//! `eggchaos-experiment` semantic types. Runtime assembly (`ProxySpec`,
//! live policies, listener ownership) stays in `eggchaos-server`.
#![deny(unsafe_code)]

pub mod common;
pub mod routes;
pub mod scenario_v2;
pub mod stream;

pub use common::{
    ErrorBodyV1, ErrorEnvelopeV1, HealthV1, VersionV1, MAX_REQUEST_BODY_BYTES,
    METRICS_CONTENT_TYPE, NATIVE_API_VERSION, NATIVE_JSON_CONTENT_TYPE,
};
pub use stream::{
    validate_proxy_name, DatagramFaultKindV1, DatagramFaultPatchV1, DatagramFaultSpecV1,
    DatagramFaultUpsertV1, DatagramProxyCoreParts, DatagramRuntimeConfigV1, FaultKindV1,
    FaultPatchV1, FaultUpsertV1, NativeDatagramAssociationViewV1, NativeDatagramEvidenceV1,
    NativeDatagramProxyPatchV1, NativeDatagramProxyRequestV1, NativeDatagramProxyViewV1,
    NativeFaultViewV1, NativeProxyPatchV1, NativeProxyRequestV1, NativeProxyViewV1,
    RuntimeConfigV1, ScenarioActionV1, ScenarioEventResultV1, ScenarioEventV1, ScenarioFaultV1,
    ScenarioRunV1, ScenarioStatusV1, ScenarioV1, MAX_CONNECTION_LIMIT, MAX_HISTORY_LIMIT,
    MAX_LATENCY_BUFFER_BYTES, MAX_RELAY_BUFFER_BYTES, MAX_TIMEOUT_MS,
    NATIVE_DEFAULT_BANDWIDTH_BURST_BYTES, NATIVE_DEFAULT_BANDWIDTH_BYTES_PER_SECOND,
    NATIVE_DEFAULT_BUFFER_BYTES, NATIVE_DEFAULT_LIMIT_BYTES, NATIVE_DEFAULT_PROXY_TIMEOUT_MS,
    NATIVE_DEFAULT_SLICE_AVERAGE_SIZE,
};

pub use routes::{NativeOperation, NATIVE_OPERATIONS, OPENAPI_CONTRACT_VERSION};
pub use scenario_v2::{
    phase_identity_string, to_canonical_json, ScenarioActionDto, ScenarioPhaseV2Dto,
    ScenarioRepeatV2Dto, ScenarioScheduleV2Dto, ScenarioScheduleV2Toml, ScheduleCleanupResourceV2,
    ScheduleCleanupV2, ScheduleCompileV2, ScheduleCompiledEventV2, ScheduleRunEventV2,
    ScheduleRunV2, ScheduleValidateV2,
};
