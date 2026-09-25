//! Deterministic bounded scenario v2 source language, compiler, portable
//! replay identity, and shared schedule driver.
//!
//! The pure semantic authority (source language, compiler, fingerprint,
//! run evidence) and the schedule driver now live in
//! `eggchaos_experiment`, the consumer-neutral crate both the server
//! and embedded harnesses build on. This module keeps the server's
//! `ControlState` driver wiring (`runtime`), its test suites, and
//! source-compatible re-exports of every previously public symbol.
//!
//! The module remains a pure control-plane addition on top of the
//! stream/datagram policy publication machinery. The driver never
//! addresses sockets or runtime `run_id`s.

pub(crate) mod runtime;

#[cfg(test)]
mod conformance_tests;
#[cfg(test)]
mod property_tests;
#[cfg(test)]
mod runtime_tests;
#[cfg(test)]
mod tests;

pub use eggchaos_experiment::{
    compile_schedule, compiled_fingerprint, encode_compiled_for_fingerprint, expanded_event_count,
    fingerprint_hex, CleanupOutcome, CleanupPolicyV2, CleanupResourceOutcome,
    CleanupResourceRecord, CompiledEventV2, CompiledPhaseIdentity, CompiledScenarioV2,
    IsolationPolicyV2, ScenarioScheduleRunRecord, ScenarioScheduleV2, ScheduleError,
    ScheduleEventResult, SchedulePhaseV2, ScheduleRepeatV2, ScheduleResource, ScheduleRunStatus,
    ScheduleTransport, COMPILER_SEMANTICS_VERSION, MAX_COMPILED_EVENTS, MAX_PHASES,
    MAX_PHASE_ACTIONS, MAX_PHASE_NAME_BYTES, MAX_REPEAT_COUNT, SCHEDULE_SCHEMA_VERSION,
};
