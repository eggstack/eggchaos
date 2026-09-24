//! Deterministic bounded scenario v2 source language, compiler, and
//! portable replay identity.
//!
//! This module is the M026 deliverable. The runtime/control integration
//! in M027 reuses `compile_schedule` and `compiled_fingerprint`; the
//! qualifier M028 builds the golden corpus on top of this surface.
//!
//! The module is a pure control-plane addition. It does not touch the
//! stream or datagram engines and never addresses `ControlState`,
//! Tokio wall time, sockets, or runtime `run_id`s.

mod compiler;
mod error;
mod fingerprint;
mod source;

#[cfg(test)]
mod property_tests;
#[cfg(test)]
mod tests;

pub use compiler::{
    compile_schedule, expanded_event_count, CompiledEventV2, CompiledPhaseIdentity,
    CompiledScenarioV2, COMPILER_SEMANTICS_VERSION,
};
pub use error::ScheduleError;
pub use fingerprint::{compiled_fingerprint, encode_compiled_for_fingerprint, fingerprint_hex};
pub use source::{
    CleanupPolicyV2, IsolationPolicyV2, ScenarioScheduleV2, SchedulePhaseV2, ScheduleRepeatV2,
    MAX_COMPILED_EVENTS, MAX_PHASES, MAX_PHASE_ACTIONS, MAX_PHASE_NAME_BYTES, MAX_REPEAT_COUNT,
    SCHEDULE_SCHEMA_VERSION,
};
