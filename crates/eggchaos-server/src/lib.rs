//! Fixed-target TCP runtime, bounded connection registry, and native control
//! model for eggchaos.
#![deny(unsafe_code)]

mod admin;
mod config;
mod runtime;
pub mod scenario;

pub use admin::{AdminConfig, AdminError, AdminHandle, NativeAdmin};
pub use config::{
    AdminFileConfig, FaultFileConfig, NativeConfig, NativeConfigError, ProxyFileConfig,
};
pub use eggchaos_core::{ActiveFault, FAULT_TYPE_NAMES};
pub use runtime::{
    AdmissionLimits, ClosedConnection, ConnectionEvidence, ConnectionOutcome, ConnectionSnapshot,
    ConnectionState, ControlError, ControlState, DirectionBytes, EggchaosError, EggchaosService,
    ExpectedPublish, FaultPatch, FaultUpsert, MetricTables, MetricsCounters, PerProxyMetrics,
    ProxyPatch, ProxySpec, ProxyView, ResetReport, ResetResult, ResettableTcpStream, RuntimeParams,
    ServiceBuilder, ServiceHandle, TcpResetHandle, MAX_METRIC_ACTIVATIONS, MAX_METRIC_PROXIES,
    OUTCOME_CLASS_NAMES, VERSION,
};
pub use scenario::{
    drive_scenario_run, validate_scenario, Scenario, ScenarioAction, ScenarioEvent,
    ScenarioEventResult, ScenarioRunRecord, ScenarioRunStatus,
};
