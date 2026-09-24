//! Scenario v2 source language.
//!
//! The source model is a piecewise-constant fault schedule with named,
//! sequential phases of explicit duration and finite repetition of a
//! bounded phase group. It compiles to a flat `CompiledScenarioV2` event
//! tape with absolute, monotonic offsets and stable per-event indices.

use serde::{Deserialize, Serialize};

use crate::ScenarioAction;

use super::error::ScheduleError;

/// Maximum compiled-event count. Matches the existing v1 ceiling.
pub const MAX_COMPILED_EVENTS: usize = 1024;
/// Maximum number of top-level phases or repeat-block phases.
pub const MAX_PHASES: usize = 256;
/// Maximum number of actions a single phase may carry.
pub const MAX_PHASE_ACTIONS: usize = 64;
/// Maximum finite repeat count of the repeat block (one level).
pub const MAX_REPEAT_COUNT: u32 = 64;
/// Maximum byte length of an optional phase name.
pub const MAX_PHASE_NAME_BYTES: usize = 128;

/// Schedule schema version. Anything else is rejected by `compile_schedule`.
pub const SCHEDULE_SCHEMA_VERSION: u32 = 2;

/// Isolation mode for a v2 run.
///
/// `Strict` snapshots initial directional state and refuses to silently
/// incorporate or overwrite an external mutation. `Live` retains the v1
/// interactive rule: each event builds from the currently live plan at
/// fire time and uses an expected-generation guard for publication.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum IsolationPolicyV2 {
    /// Per-resource expected-generation ownership; fail on external conflict.
    Strict,
    /// V1-style interactive current-state-at-fire-time model.
    Live,
}

impl Default for IsolationPolicyV2 {
    fn default() -> Self {
        Self::Strict
    }
}

/// Cleanup policy for a v2 run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CleanupPolicyV2 {
    /// Restore the originally snapshotted directional plans.
    RestoreInitial,
    /// Leave the last successfully published plans in place.
    Leave,
}

impl Default for CleanupPolicyV2 {
    fn default() -> Self {
        Self::RestoreInitial
    }
}

/// One named sequential phase of the schedule.
///
/// The phase applies its `actions` at the phase start in source order,
/// then advances the schedule cursor by `duration_ns`. Zero-duration
/// phases are allowed; their actions keep their source order in compiled
/// indices.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SchedulePhaseV2 {
    /// Optional bounded human-readable label. Used as presentation evidence
    /// only; it does NOT participate in the schedule fingerprint.
    #[serde(default)]
    pub name: Option<String>,
    /// Phase duration in nanoseconds. Added to the schedule cursor at the
    /// end of the phase. May be zero.
    pub duration_ns: u64,
    /// Actions to apply at phase start, in source order. Must be non-empty.
    pub actions: Vec<ScenarioAction>,
}

/// Optional one-level finite repetition of a bounded phase group.
///
/// Empty schedules without this block are rejected by the compiler.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScheduleRepeatV2 {
    /// Number of repetitions. Must be in `1..=MAX_REPEAT_COUNT`.
    pub count: u32,
    /// Phases inside the repeat block. Must be non-empty and bounded.
    pub phases: Vec<SchedulePhaseV2>,
}

/// ScenarioScheduleV2 source document.
///
/// The schema version is the literal `2` (compared exactly) and the
/// runtime must reject any other value. All randomness derives from
/// `(seed, execution_key, schedule_fingerprint, compiled_event_index)`;
/// the daemon run id never participates in v2 derivation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScenarioScheduleV2 {
    /// Document schema version. Must equal `SCHEDULE_SCHEMA_VERSION`.
    pub version: u32,
    /// Explicit run seed (mixes into v2 policy namespaces).
    pub seed: u64,
    /// Explicit execution key (selects another deterministic realization).
    pub execution_key: u64,
    /// Isolation policy for the run. Defaults to strict.
    #[serde(default)]
    pub isolation: IsolationPolicyV2,
    /// Cleanup policy for the run. Defaults to `restore-initial`.
    #[serde(default)]
    pub cleanup: CleanupPolicyV2,
    /// Ordered top-level phases. May be empty only when `repeat` provides
    /// the entire schedule.
    #[serde(default)]
    pub phases: Vec<SchedulePhaseV2>,
    /// Optional single-level finite repeat block.
    #[serde(default)]
    pub repeat: Option<ScheduleRepeatV2>,
}

impl ScenarioScheduleV2 {
    /// Reject obvious source-level structural errors before expansion.
    ///
    /// This is intentionally a pure, infallible-shape validation; the actual
    /// action validation (plan legality) still belongs in the compiler because
    /// that authority is shared with the runtime.
    pub fn check_structure(&self) -> Result<(), ScheduleError> {
        if self.version != SCHEDULE_SCHEMA_VERSION {
            return Err(ScheduleError::UnsupportedVersion(self.version));
        }
        if self.phases.len() > MAX_PHASES {
            return Err(ScheduleError::TooManyPhases {
                got: self.phases.len(),
            });
        }
        if let Some(repeat) = &self.repeat {
            if repeat.count == 0 || repeat.count > MAX_REPEAT_COUNT {
                return Err(ScheduleError::RepeatOutOfRange { got: repeat.count });
            }
            if repeat.phases.len() > MAX_PHASES {
                return Err(ScheduleError::TooManyRepeatPhases {
                    got: repeat.phases.len(),
                });
            }
        }
        let mut any_top_action = false;
        for phase in &self.phases {
            if phase.actions.is_empty() {
                return Err(ScheduleError::EmptyPhase);
            }
            if phase.actions.len() > MAX_PHASE_ACTIONS {
                return Err(ScheduleError::TooManyPhaseActions {
                    got: phase.actions.len(),
                });
            }
            if matches!(&phase.name, Some(name) if name.is_empty() || name.len() > MAX_PHASE_NAME_BYTES)
            {
                return Err(ScheduleError::InvalidPhaseName);
            }
            any_top_action = true;
        }
        let mut any_repeat_action = false;
        if let Some(repeat) = &self.repeat {
            if repeat.phases.is_empty() {
                return Err(ScheduleError::EmptySchedule);
            }
            for phase in &repeat.phases {
                if phase.actions.is_empty() {
                    return Err(ScheduleError::EmptyPhase);
                }
                if phase.actions.len() > MAX_PHASE_ACTIONS {
                    return Err(ScheduleError::TooManyPhaseActions {
                        got: phase.actions.len(),
                    });
                }
                if matches!(&phase.name, Some(name) if name.is_empty() || name.len() > MAX_PHASE_NAME_BYTES)
                {
                    return Err(ScheduleError::InvalidPhaseName);
                }
                any_repeat_action = true;
            }
        }
        if !any_top_action && !any_repeat_action {
            return Err(ScheduleError::EmptySchedule);
        }
        Ok(())
    }

    /// All phases, flattened across the top-level list and the optional
    /// repeat block. Phase identity in the compiled tape is a small
    /// two-bit enum (Top / Repeat{iteration}) so phase names and indices
    /// remain unambiguous.
    pub(crate) fn flattened_phases(&self) -> Vec<SchedulePhaseV2> {
        let mut out = Vec::with_capacity(self.phases.len());
        out.extend(self.phases.iter().cloned());
        if let Some(repeat) = &self.repeat {
            for _ in 0..repeat.count {
                for phase in &repeat.phases {
                    out.push(phase.clone());
                }
            }
        }
        out
    }
}
