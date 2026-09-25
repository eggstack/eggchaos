//! Consumer-neutral deterministic experiment harness for eggchaos.
//!
//! This crate is the single authority for Scenario V2 schedule
//! semantics (source language, deterministic compiler, SHA-256
//! fingerprint, run_id-independent namespaces) and the reusable
//! prepare/arm/start experiment lifecycle over a narrow
//! [`PolicyTarget`] abstraction.
//!
//! ```text
//! eggchaos-core
//!       ^
//!       |
//! eggchaos-experiment (this crate: semantics + driver + epoch gate)
//!    ^            ^
//!    |            |
//! eggchaos-server   downstream Rust harnesses
//!    |
//! ControlState adapter
//! ```
//!
//! The crate depends only on `eggchaos-core` plus narrow async
//! primitives. It never depends on `eggchaos-server`, EggServe, CLI
//! parsing, Toxiproxy, EggReplay, or EggProbe.
//!
//! The shared-epoch contract synchronizes the schedule clock only:
//! every deadline is `epoch + compiled_offset` for both the schedule
//! driver and the caller workload. Kernel packet transmission, task
//! wake-up latency, and application processing remain observational.
#![forbid(unsafe_code)]

mod action;
mod compiler;
mod driver;
mod error;
mod experiment;
mod fingerprint;
mod gate;
mod run;
mod source;
mod stream_target;
mod target;

pub use action::ScenarioAction;
pub use compiler::{
    compile_schedule, expanded_event_count, CompiledEventV2, CompiledPhaseIdentity,
    CompiledScenarioV2, COMPILER_SEMANTICS_VERSION,
};
pub use driver::{drive, prepare_initial, DriveOutcome, EventSink, OwnedResource};
pub use error::ScheduleError;
pub use experiment::{
    ExperimentError, ExperimentEvidence, ExperimentOutcome, PreparedExperiment,
    MAX_EXPERIMENT_IDENTITY_BYTES,
};
pub use fingerprint::{compiled_fingerprint, encode_compiled_for_fingerprint, fingerprint_hex};
pub use gate::{EpochGate, EpochWaiter};
pub use run::{
    CleanupOutcome, CleanupResourceOutcome, CleanupResourceRecord, ScenarioScheduleRunRecord,
    ScheduleEventResult, ScheduleResource, ScheduleRunStatus, ScheduleTransport,
};
pub use source::{
    CleanupPolicyV2, IsolationPolicyV2, ScenarioScheduleV2, SchedulePhaseV2, ScheduleRepeatV2,
    MAX_COMPILED_EVENTS, MAX_PHASES, MAX_PHASE_ACTIONS, MAX_PHASE_NAME_BYTES, MAX_REPEAT_COUNT,
    SCHEDULE_SCHEMA_VERSION,
};
pub use stream_target::{empty_stream_plan, stream_target_from_policies, StreamPolicyTarget};
pub use target::{
    action_resource, resource_key, PolicyTarget, PublishReceipt, TargetCapabilities, TargetError,
    TargetPlan, TargetSnapshot, MAX_RESOURCE_NAME_BYTES, MAX_TARGET_LABEL_BYTES,
};

#[cfg(test)]
mod tests;
