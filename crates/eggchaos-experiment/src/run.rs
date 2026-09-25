//! Scenario schedule v2 run record, status, and evidence types.
//!
//! These types are the bounded evidence contract for a v2 schedule
//! run. They deliberately reuse the v1 status vocabulary where it
//! matches (Pending, Running, Completed, Failed, Cancelling,
//! Cancelled) and add v2-only fields: schedule fingerprint, isolation
//! policy, cleanup policy, scheduled vs applied timing, per-event
//! target generations, and post-run cleanup outcome.
//!
//! Nothing in this file executes; the driver lives in
//! [`crate::driver`]. These types stay JSON/TOML-free so consumers can
//! serialize them through their own explicit wire DTOs.

use eggchaos_core::Direction;
use serde::{Deserialize, Serialize};

use super::compiler::CompiledPhaseIdentity;
use crate::{CleanupPolicyV2, IsolationPolicyV2};

/// Transport that a v2 schedule event targeted. Stream and datagram
/// faults have separate publication paths and so are recorded
/// separately per event for evidence purposes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ScheduleTransport {
    /// Stream TCP fault plan.
    Stream,
    /// Datagram UDP fault plan.
    Datagram,
}

/// V2 schedule run lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ScheduleRunStatus {
    /// Validated, waiting for the first event.
    Pending,
    /// Applying events.
    Running,
    /// Cancellation requested; driver unwinding.
    Cancelling,
    /// Cancelled before completion.
    Cancelled,
    /// All events applied (or schedule was empty).
    Completed,
    /// Stopped early by a failed event (fail-fast).
    Failed,
}

/// Per-resource identifier touched by a v2 schedule. Combined with
/// `ScheduleTransport`, it identifies exactly the authority
/// publication will go through.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ScheduleResource {
    /// Target proxy or experiment resource name.
    pub proxy: String,
    /// Target direction.
    pub direction: Direction,
    /// Transport the action targets.
    pub transport: ScheduleTransport,
}

/// Per-event evidence produced by the v2 driver. Carries the
/// scheduled/applied timing split required by ADR 004.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScheduleEventResult {
    /// Stable zero-based compiled index.
    pub compiled_index: u32,
    /// Phase identity for run evidence.
    pub phase: CompiledPhaseIdentity,
    /// Scheduled absolute offset from the run epoch in nanoseconds.
    pub scheduled_offset_ns: u64,
    /// Actual monotonic elapsed application time in nanoseconds,
    /// measured from the same epoch captured at run entry.
    pub applied_elapsed_ns: u64,
    /// `max(applied_elapsed - scheduled_offset, 0)` in nanoseconds.
    pub late_by_ns: u64,
    /// Action summary string (`set-plan`, `remove-fault`,
    /// `set-datagram-plan`, `remove-datagram-fault`).
    pub action: String,
    /// Target resource.
    pub resource: ScheduleResource,
    /// Resulting upstream generation (or latest known upstream gen).
    pub upstream_generation: u64,
    /// Resulting downstream generation (or latest known downstream
    /// gen).
    pub downstream_generation: u64,
    /// Global configuration generation after the action.
    pub global_generation: u64,
}

/// Cleanup outcome for one touched resource. Conflicts are recorded
/// distinctly from failures so the run's own outcome (Completed /
/// Failed / Cancelled) stays separable from cleanup-side conflicts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CleanupResourceOutcome {
    /// Plan restoration published successfully.
    Restored,
    /// Generation moved externally; cleanup did not overwrite.
    Conflict,
    /// Target was missing during cleanup (already removed/deleted).
    Missing,
    /// Cleanup not requested (leave policy).
    NotRequested,
}

/// Cleanup summary for the whole run. Captures per-resource outcomes
/// separately so cleanup of one resource does not lose information
/// because of a conflict on another.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CleanupOutcome {
    /// Whether cleanup was requested (`restore-initial`) or skipped
    /// (`leave`).
    pub policy: CleanupPolicyV2,
    /// Per-resource cleanup outcome.
    pub resources: Vec<CleanupResourceRecord>,
}

/// Cleanup record for one touched resource.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CleanupResourceRecord {
    /// Resource the cleanup attempted to restore (or skipped).
    pub resource: ScheduleResource,
    /// Per-resource outcome.
    pub outcome: CleanupResourceOutcome,
}

/// Bounded v2 run record. The field shape is fixed; new fields
/// require a wire-format bump so existing fixtures stay stable.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScenarioScheduleRunRecord {
    /// Stable run identifier.
    pub run_id: u64,
    /// Schedule seed.
    pub seed: u64,
    /// Schedule execution key.
    pub execution_key: u64,
    /// 32-byte SHA-256 schedule fingerprint.
    pub schedule_fingerprint: [u8; 32],
    /// Frozen compiler semantics version. See
    /// [`crate::COMPILER_SEMANTICS_VERSION`].
    pub compiler_semantics_version: u32,
    /// Isolation mode.
    pub isolation: IsolationPolicyV2,
    /// Cleanup mode.
    pub cleanup_policy: CleanupPolicyV2,
    /// Lifecycle status.
    pub status: ScheduleRunStatus,
    /// Number of events successfully applied.
    pub applied: usize,
    /// Failure detail for `Failed` runs.
    pub failure: Option<String>,
    /// Per-event evidence.
    pub events: Vec<ScheduleEventResult>,
    /// Optional cleanup outcome.
    pub cleanup: Option<CleanupOutcome>,
}
