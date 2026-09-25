//! Fixed-target TCP runtime, bounded connection registry, and native control
//! model for eggchaos.
#![deny(unsafe_code)]

mod admin;
mod config;
mod native;
mod native_v2;
mod runtime;
pub mod scenario;
pub mod scenario_v2;

pub use admin::{AdminConfig, AdminError, AdminHandle, NativeAdmin};
pub use config::{
    AdminFileConfig, DatagramProxyFileConfig, FaultFileConfig, NativeConfig, NativeConfigError,
    ProxyFileConfig,
};
pub use eggchaos_core::{ActiveFault, FAULT_TYPE_NAMES};
pub use native::{
    runtime_admission_limits, runtime_datagram_limits, DatagramFaultKindV1, DatagramFaultPatchV1,
    DatagramFaultSpecV1, DatagramFaultUpsertV1, DatagramRuntimeConfigV1, FaultKindV1, FaultPatchV1,
    FaultUpsertV1, NativeDatagramAssociationViewV1, NativeDatagramEvidenceV1,
    NativeDatagramProxyPatchV1, NativeDatagramProxyRequestV1, NativeDatagramProxyViewV1,
    NativeFaultViewV1, NativeProxyPatchV1, NativeProxyRequestV1, NativeProxyViewV1,
    RuntimeConfigV1, ScenarioActionV1, ScenarioEventV1, ScenarioFaultV1, ScenarioRunV1,
    ScenarioStatusV1, ScenarioV1, NATIVE_DEFAULT_BANDWIDTH_BURST_BYTES,
    NATIVE_DEFAULT_BANDWIDTH_BYTES_PER_SECOND, NATIVE_DEFAULT_BUFFER_BYTES,
    NATIVE_DEFAULT_LIMIT_BYTES, NATIVE_DEFAULT_PROXY_TIMEOUT_MS, NATIVE_DEFAULT_SLICE_AVERAGE_SIZE,
};
pub use native_v2::{
    phase_identity_string, to_canonical_json, ScenarioActionDto, ScenarioPhaseV2Dto,
    ScenarioRepeatV2Dto, ScenarioScheduleV2Dto, ScenarioScheduleV2Toml, ScheduleCleanupResourceV2,
    ScheduleCleanupV2, ScheduleCompileV2, ScheduleCompiledEventV2, ScheduleRunEventV2,
    ScheduleRunV2, ScheduleValidateV2,
};
pub use runtime::{
    AdmissionLimits, ClosedConnection, ConnectionEvidence, ConnectionOutcome, ConnectionSnapshot,
    ConnectionState, ControlError, ControlState, DatagramAssociationSnapshot, DatagramProxySpec,
    DatagramProxyView, DatagramRuntime, DatagramRuntimeError, DatagramRuntimeLimits,
    DirectionBytes, EggchaosError, EggchaosService, ExpectedPublish, FaultPatch, FaultUpsert,
    MetricTables, MetricsCounters, PerProxyMetrics, ProxyPatch, ProxySpec, ProxyView, ResetReport,
    ResetResult, ResettableTcpStream, RuntimeParams, ServiceBuilder, ServiceHandle, TcpResetHandle,
    MAX_METRIC_ACTIVATIONS, MAX_METRIC_PROXIES, OUTCOME_CLASS_NAMES, VERSION,
};
pub use scenario::{
    drive_scenario_run, validate_scenario, Scenario, ScenarioAction, ScenarioEvent,
    ScenarioEventResult, ScenarioRunRecord, ScenarioRunStatus,
};
pub use scenario_v2::{
    compile_schedule, compiled_fingerprint, expanded_event_count, fingerprint_hex, CleanupOutcome,
    CleanupPolicyV2, CleanupResourceOutcome, CleanupResourceRecord, CompiledEventV2,
    CompiledPhaseIdentity, CompiledScenarioV2, IsolationPolicyV2, ScenarioScheduleRunRecord,
    ScenarioScheduleV2, ScheduleError, ScheduleEventResult, SchedulePhaseV2, ScheduleRepeatV2,
    ScheduleResource, ScheduleRunStatus, ScheduleTransport, COMPILER_SEMANTICS_VERSION,
    MAX_COMPILED_EVENTS, MAX_REPEAT_COUNT, SCHEDULE_SCHEMA_VERSION,
};
