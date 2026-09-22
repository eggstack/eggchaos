//! Fixed-target TCP runtime, bounded connection registry, and native control
//! model for eggchaos.
#![deny(unsafe_code)]

mod admin;
mod config;
mod runtime;
mod scenario;

pub use admin::{AdminConfig, AdminError, AdminHandle, ControlState, NativeAdmin};
pub use config::{
    AdminFileConfig, FaultFileConfig, NativeConfig, NativeConfigError, ProxyFileConfig,
};
pub use runtime::{
    AdmissionLimits, ConnectionSnapshot, ConnectionState, EggchaosError, EggchaosService,
    ProxySpec, ServiceBuilder, ServiceHandle, VERSION,
};
pub use scenario::{apply_scenario, Scenario, ScenarioAction, ScenarioEvent, ScenarioReport};
