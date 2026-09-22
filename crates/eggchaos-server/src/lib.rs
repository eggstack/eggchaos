//! Fixed-target TCP runtime, bounded connection registry, and native control
//! model for eggchaos.
#![deny(unsafe_code)]

mod admin;
mod config;
mod runtime;
mod scenario;

pub use admin::{AdminConfig, AdminError, AdminHandle, NativeAdmin};
pub use config::{
    AdminFileConfig, FaultFileConfig, NativeConfig, NativeConfigError, ProxyFileConfig,
};
pub use runtime::{
    AdmissionLimits, ClosedConnection, ConnectionOutcome, ConnectionSnapshot, ConnectionState,
    ControlError, ControlState, EggchaosError, EggchaosService, FaultPatch, FaultUpsert,
    ProxyPatch, ProxySpec, ProxyView, ResetReport, ResetResult, ResettableTcpStream, RuntimeParams,
    ServiceBuilder, ServiceHandle, TcpResetHandle, VERSION,
};
pub use scenario::{apply_scenario, Scenario, ScenarioAction, ScenarioEvent, ScenarioReport};
