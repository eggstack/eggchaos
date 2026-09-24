//! Pure scenario-v2 compiler.
//!
//! `compile_schedule` is a deterministic function of `(source,
//! COMPILER_SEMANTICS_VERSION)`. It must not access `ControlState`, Tokio
//! wall time, sockets, or daemon `run_id`. The output is an immutable
//! `CompiledScenarioV2` event tape the runtime then plays through the
//! existing scenario supervisor in M027.

use crate::ScenarioAction;

use super::error::ScheduleError;
use super::source::{
    CleanupPolicyV2, IsolationPolicyV2, ScenarioScheduleV2, SchedulePhaseV2, MAX_COMPILED_EVENTS,
    MAX_PHASES, MAX_PHASE_ACTIONS, MAX_REPEAT_COUNT,
};

/// Frozen compiler semantics version. A change requires:
///
/// - a new `COMPILER_SEMANTICS_VERSION` value,
/// - a regeneration of every documented golden corpus digest, and
/// - a registered bug or ADR justifying the change.
///
/// The version participates in the schedule fingerprint domain prefix so
/// two compilers cannot accidentally produce the same digest.
pub const COMPILER_SEMANTICS_VERSION: u32 = 1;

/// Phase identity inside the compiled tape. Stable, two-bit classification
/// that the runtime can switch on without parsing human labels.
#[derive(Debug, Clone, Copy, PartialEq, Hash)]
pub enum CompiledPhaseIdentity {
    /// Top-level phase index.
    Top { index: u32 },
    /// Iteration of the repeat block, then phase index inside the block.
    Repeat {
        /// 1-based repeat iteration.
        iteration: u32,
        /// Zero-based phase index inside the repeat block.
        index: u32,
    },
}

/// One expanded event in the compiled tape.
///
/// Equal-offset events preserve source order; `compiled_index` is the
/// stable zero-based event index across the entire schedule.
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledEventV2 {
    /// Zero-based stable index across the entire compiled tape.
    pub compiled_index: u32,
    /// Phase identity for run evidence.
    pub phase: CompiledPhaseIdentity,
    /// Absolute offset from epoch in nanoseconds.
    pub offset_ns: u64,
    /// Action to apply at this offset.
    pub action: ScenarioAction,
}

/// Frozen, inspectable compile result. Immutable after construction.
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledScenarioV2 {
    /// Frozen compiler semantics version (see `COMPILER_SEMANTICS_VERSION`).
    pub compiler_semantics_version: u32,
    /// Schedule seed.
    pub seed: u64,
    /// Schedule execution key.
    pub execution_key: u64,
    /// Isolation policy.
    pub isolation: IsolationPolicyV2,
    /// Cleanup policy.
    pub cleanup: CleanupPolicyV2,
    /// Ordered, immutable event tape.
    pub events: Vec<CompiledEventV2>,
}

/// Validate that `count` fits the structural ceiling before iterating.
fn check_event_count(count: usize) -> Result<(), ScheduleError> {
    if count > MAX_COMPILED_EVENTS {
        return Err(ScheduleError::TooManyCompiledEvents(count));
    }
    Ok(())
}

/// Compute the absolute schedule cursor for a single phase.
fn phase_target_offset(running: &mut u64, duration_ns: u64) -> Result<(), ScheduleError> {
    *running = running
        .checked_add(duration_ns)
        .ok_or(ScheduleError::OffsetOverflow)?;
    Ok(())
}

/// Deterministically compile a source schedule into a flat event tape.
///
/// The compiler is pure for a fixed `(source, compiler_version)` pair.
/// Failures are structural — the result is either a complete compile or
/// no compile; partial-event emission followed by truncation is forbidden.
pub fn compile_schedule(source: &ScenarioScheduleV2) -> Result<CompiledScenarioV2, ScheduleError> {
    source.check_structure()?;

    // First, count the expanded events to fail fast before allocation.
    let mut total: usize = 0;
    for phase in source.flattened_phases() {
        total =
            total
                .checked_add(phase.actions.len())
                .ok_or(ScheduleError::TooManyCompiledEvents(
                    MAX_COMPILED_EVENTS + 1,
                ))?;
    }
    check_event_count(total)?;

    let mut events: Vec<CompiledEventV2> = Vec::with_capacity(total);
    let mut cursor_ns: u64 = 0;
    let mut compiled_index: u32 = 0;

    // Validate each phase's actions via FaultPlan/DatagramPlan BEFORE
    // committing them to the compiled tape. This keeps the validation
    // authority shared with the runtime and aborts atomically.
    for (top_idx, phase) in source.phases.iter().enumerate() {
        validate_phase_actions(phase)?;
        for action in &phase.actions {
            events.push(CompiledEventV2 {
                compiled_index,
                phase: CompiledPhaseIdentity::Top {
                    index: top_idx as u32,
                },
                offset_ns: cursor_ns,
                action: action.clone(),
            });
            compiled_index += 1;
        }
        phase_target_offset(&mut cursor_ns, phase.duration_ns)?;
    }

    if let Some(repeat) = &source.repeat {
        if repeat.count > MAX_REPEAT_COUNT {
            return Err(ScheduleError::RepeatOutOfRange { got: repeat.count });
        }
        if repeat.phases.len() > MAX_PHASES {
            return Err(ScheduleError::TooManyRepeatPhases {
                got: repeat.phases.len(),
            });
        }
        for phase in &repeat.phases {
            validate_phase_actions(phase)?;
        }
        for iteration in 0..repeat.count {
            for (phase_idx, phase) in repeat.phases.iter().enumerate() {
                for action in &phase.actions {
                    events.push(CompiledEventV2 {
                        compiled_index,
                        phase: CompiledPhaseIdentity::Repeat {
                            iteration: iteration + 1,
                            index: phase_idx as u32,
                        },
                        offset_ns: cursor_ns,
                        action: action.clone(),
                    });
                    compiled_index += 1;
                }
                phase_target_offset(&mut cursor_ns, phase.duration_ns)?;
            }
        }
    }

    if events.len() > MAX_COMPILED_EVENTS {
        return Err(ScheduleError::TooManyCompiledEvents(events.len()));
    }

    Ok(CompiledScenarioV2 {
        compiler_semantics_version: COMPILER_SEMANTICS_VERSION,
        seed: source.seed,
        execution_key: source.execution_key,
        isolation: source.isolation,
        cleanup: source.cleanup,
        events,
    })
}

fn validate_phase_actions(phase: &SchedulePhaseV2) -> Result<(), ScheduleError> {
    if phase.actions.len() > MAX_PHASE_ACTIONS {
        return Err(ScheduleError::TooManyPhaseActions {
            got: phase.actions.len(),
        });
    }
    for action in &phase.actions {
        match action {
            ScenarioAction::SetPlan { faults, .. } => {
                eggchaos_core::FaultPlan::new(faults.clone())
                    .map_err(|error| ScheduleError::InvalidStreamPlan(error.to_string()))?;
            }
            ScenarioAction::SetDatagramPlan { faults, .. } => {
                eggchaos_core::DatagramPlan::new(faults.clone())
                    .map_err(|error| ScheduleError::InvalidDatagramPlan(error.to_string()))?;
            }
            _ => {}
        }
    }
    Ok(())
}

/// Convenience: total event count the compiler will emit (or error from).
pub fn expanded_event_count(source: &ScenarioScheduleV2) -> usize {
    let mut count = 0usize;
    for phase in source.flattened_phases() {
        count = count.saturating_add(phase.actions.len());
    }
    count
}
