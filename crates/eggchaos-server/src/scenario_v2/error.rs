//! Scenario v2 source/compiler/fingerprint errors.
//!
//! The v2 model is a control-plane scheduler above the existing stream and
//! datagram fault engines. It does not change the engines themselves, so
//! schedule validation can fail outright (no run, no side effects) and
//! compilation failures are unrelated to data-plane correctness.

use thiserror::Error;

/// Schema/identifier/bounds failure for a v2 source document.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ScheduleError {
    /// Source `version` was not the expected schema value (`2`).
    #[error("unsupported schedule version: {0}")]
    UnsupportedVersion(u32),
    /// Compiled-event ceiling would be exceeded.
    #[error("compiled event count exceeds ceiling: have {0}, ceiling {ceiling}", ceiling = super::MAX_COMPILED_EVENTS)]
    TooManyCompiledEvents(usize),
    /// Repeat count outside the structural bound.
    #[error("repeat count must be in 1..={max} (got {got})", max = super::MAX_REPEAT_COUNT)]
    RepeatOutOfRange {
        /// Requested repetition count.
        got: u32,
    },
    /// Source phase count exceeds the structural bound.
    #[error("phase count must be at most {max} (got {got})", max = super::MAX_PHASES)]
    TooManyPhases {
        /// Number of phases in the source.
        got: usize,
    },
    /// Repeat-block phase count exceeds the structural bound.
    #[error("repeat-block phases must be at most {max} (got {got})", max = super::MAX_PHASES)]
    TooManyRepeatPhases {
        /// Number of phases inside `repeat`.
        got: usize,
    },
    /// Per-phase action count exceeds the structural bound.
    #[error("phase actions must be at most {max} per phase (got {got})", max = super::MAX_PHASE_ACTIONS)]
    TooManyPhaseActions {
        /// Number of actions inside one phase.
        got: usize,
    },
    /// Phase `name` was either empty or longer than 128 bytes.
    #[error("phase name must be 1..=128 bytes when provided")]
    InvalidPhaseName,
    /// Phase `duration_ns` plus running sum would overflow u64.
    #[error("phase offset overflows u64")]
    OffsetOverflow,
    /// A phase had no actions, and empty phases are not allowed in v2.
    #[error("phase has no actions")]
    EmptyPhase,
    /// Both `phases` and `repeat.phases` were empty.
    #[error("schedule has no phases and no repeat phases")]
    EmptySchedule,
    /// A `FaultPlan::new` validation failed inside a scenario action.
    #[error("invalid stream plan: {0}")]
    InvalidStreamPlan(String),
    /// A `DatagramPlan::new` validation failed inside a scenario action.
    #[error("invalid datagram plan: {0}")]
    InvalidDatagramPlan(String),
}
