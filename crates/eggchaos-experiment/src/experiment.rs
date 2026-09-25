//! Prepare/arm/start experiment lifecycle over [`crate::PolicyTarget`].
//!
//! ```text
//! source
//!   -> compiled (validate, expand, fingerprint)
//!   -> prepared (capability check, touched-resource resolution, initial
//!      snapshots; no publication, no clock)
//!   -> armed (epoch gate shared with the caller)
//!   -> running (one captured epoch; deadlines = epoch + offset)
//!   -> completed | failed | cancelled
//!   -> cleanup complete
//! ```
//!
//! Cancellation before start publishes nothing. Cancellation while
//! sleeping wakes promptly. Cancellation after publications runs the
//! selected cleanup. The driver future is owned by the caller's run
//! handle; dropping the gate or the prepared experiment before start
//! publishes nothing and spawns nothing.

use std::collections::BTreeMap;
use std::sync::Arc;

use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

use crate::compiler::{compile_schedule, CompiledScenarioV2};
use crate::driver::{self, DriveOutcome, EventSink, OwnedResource};
use crate::error::ScheduleError;
use crate::fingerprint::{compiled_fingerprint, fingerprint_hex};
use crate::gate::{EpochGate, EpochWaiter};
use crate::run::{CleanupOutcome, ScheduleRunStatus};
use crate::source::ScenarioScheduleV2;
use crate::target::{PolicyTarget, TargetError};

/// Maximum byte length of a caller integration identity.
pub const MAX_EXPERIMENT_IDENTITY_BYTES: usize = 128;

/// Prepare/start failure.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ExperimentError {
    /// Source failed validation or compilation.
    #[error(transparent)]
    Schedule(#[from] ScheduleError),
    /// Target capability/snapshot failure during preparation.
    #[error(transparent)]
    Target(#[from] TargetError),
    /// Caller integration identity exceeds
    /// [`MAX_EXPERIMENT_IDENTITY_BYTES`] bytes.
    #[error("integration identity exceeds 128 bytes")]
    IdentityTooLong,
}

/// Bounded experiment evidence sufficient to correlate a run.
///
/// Portable reports use relative offsets and stable identity fields.
/// The captured epoch is process-local (a Tokio monotonic `Instant`)
/// and exposed in debug form only; it must never be serialized as a
/// portable timestamp. No payload capture.
#[derive(Debug, Clone)]
pub struct ExperimentEvidence {
    /// Frozen compiler semantics version.
    pub compiler_semantics_version: u32,
    /// Lowercase-hex SHA-256 schedule fingerprint.
    pub schedule_fingerprint: String,
    /// Schedule seed.
    pub seed: u64,
    /// Schedule execution key.
    pub execution_key: u64,
    /// Caller integration identity, if configured.
    pub integration_id: Arc<str>,
    /// Number of events successfully applied.
    pub applied: usize,
    /// Terminal run status.
    pub status: ScheduleRunStatus,
    /// Failure detail for `Failed` runs.
    pub failure: Option<String>,
    /// Post-run cleanup outcome.
    pub cleanup: CleanupOutcome,
}

/// Terminal outcome of one experiment run.
#[derive(Debug, Clone)]
pub struct ExperimentOutcome {
    /// Terminal run status.
    pub status: ScheduleRunStatus,
    /// Failure detail for `Failed` runs.
    pub failure: Option<String>,
    /// Post-run cleanup outcome.
    pub cleanup: CleanupOutcome,
    /// Number of events successfully applied.
    pub applied: usize,
    /// Bounded correlation evidence.
    pub evidence: ExperimentEvidence,
}

/// A compiled, prepared experiment: validated, capability-checked, and
/// snapshotted, with no publication and no clock started.
pub struct PreparedExperiment<T> {
    target: T,
    compiled: CompiledScenarioV2,
    fingerprint: [u8; 32],
    initial: BTreeMap<String, OwnedResource>,
    integration_id: Arc<str>,
}

impl<T> std::fmt::Debug for PreparedExperiment<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreparedExperiment")
            .field("fingerprint", &fingerprint_hex(&self.fingerprint))
            .field("events", &self.compiled.events.len())
            .field("touched_resources", &self.initial.len())
            .finish_non_exhaustive()
    }
}

impl<T> PreparedExperiment<T> {
    /// Borrow the compiled schedule.
    pub fn compiled(&self) -> &CompiledScenarioV2 {
        &self.compiled
    }

    /// Schedule fingerprint (32-byte SHA-256).
    pub const fn fingerprint(&self) -> &[u8; 32] {
        &self.fingerprint
    }

    /// Lowercase-hex schedule fingerprint.
    pub fn fingerprint_hex(&self) -> String {
        fingerprint_hex(&self.fingerprint)
    }

    /// Number of touched resources resolved during preparation.
    pub fn touched_resource_count(&self) -> usize {
        self.initial.len()
    }
}

impl<T: PolicyTarget> PreparedExperiment<T> {
    /// Compile, validate target capabilities, resolve touched
    /// resources, and capture initial snapshots. No policy event is
    /// published and no schedule deadline begins.
    pub async fn prepare(source: &ScenarioScheduleV2, target: T) -> Result<Self, ExperimentError> {
        let compiled = compile_schedule(source)?;
        Self::prepare_compiled(compiled, target).await
    }

    /// Prepare from an already-compiled schedule (same checks minus
    /// compilation).
    pub async fn prepare_compiled(
        compiled: CompiledScenarioV2,
        target: T,
    ) -> Result<Self, ExperimentError> {
        let fingerprint = compiled_fingerprint(&compiled);
        let initial = driver::prepare_initial(&target, &compiled).await?;
        Ok(Self {
            target,
            compiled,
            fingerprint,
            initial,
            integration_id: Arc::from(""),
        })
    }

    /// Set the caller integration identity reported in evidence.
    ///
    /// # Errors
    ///
    /// Returns [`ExperimentError::IdentityTooLong`] when the identity
    /// exceeds [`MAX_EXPERIMENT_IDENTITY_BYTES`] bytes.
    pub fn with_integration_id(
        mut self,
        integration_id: impl Into<String>,
    ) -> Result<Self, ExperimentError> {
        let integration_id = integration_id.into();
        if integration_id.len() > MAX_EXPERIMENT_IDENTITY_BYTES {
            return Err(ExperimentError::IdentityTooLong);
        }
        self.integration_id = Arc::from(integration_id);
        Ok(self)
    }

    /// Run the prepared schedule from the shared epoch.
    ///
    /// The driver first awaits the epoch from `waiter`, so no event
    /// executes before the caller releases the gate. The returned
    /// future is owned by the caller: dropping it before completion
    /// cancels the wait (no publication happens without the epoch),
    /// and cancelling `token` wakes deadline sleeps promptly.
    pub async fn run<S: EventSink>(
        self,
        waiter: EpochWaiter,
        token: CancellationToken,
        sink: &mut S,
    ) -> ExperimentOutcome {
        let epoch = waiter.await_epoch().await;
        self.run_from_epoch(epoch, token, sink).await
    }

    /// Run from an already-captured epoch (e.g. `EpochGate::started`).
    pub async fn run_from_epoch<S: EventSink>(
        self,
        epoch: Instant,
        token: CancellationToken,
        sink: &mut S,
    ) -> ExperimentOutcome {
        let outcome: DriveOutcome = driver::drive(
            &self.target,
            &self.compiled,
            self.fingerprint,
            self.initial,
            epoch,
            &token,
            sink,
        )
        .await;
        let evidence = ExperimentEvidence {
            compiler_semantics_version: self.compiled.compiler_semantics_version,
            schedule_fingerprint: fingerprint_hex(&self.fingerprint),
            seed: self.compiled.seed,
            execution_key: self.compiled.execution_key,
            integration_id: self.integration_id.clone(),
            applied: outcome.applied,
            status: outcome.status,
            failure: outcome.failure.clone(),
            cleanup: outcome.cleanup.clone(),
        };
        ExperimentOutcome {
            status: outcome.status,
            failure: outcome.failure,
            cleanup: outcome.cleanup,
            applied: outcome.applied,
            evidence,
        }
    }

    /// Convenience: create a gate, run the schedule, and drive a
    /// caller workload from the same epoch.
    ///
    /// The gate starts before either side runs, so the schedule and
    /// the workload observe the identical captured epoch. Returns the
    /// experiment outcome plus the workload output.
    pub async fn run_with_workload<S, F, W>(
        self,
        token: CancellationToken,
        sink: &mut S,
        workload: impl FnOnce(Instant) -> F,
    ) -> (ExperimentOutcome, W)
    where
        S: EventSink,
        F: std::future::Future<Output = W>,
    {
        let gate = EpochGate::new();
        let waiter = gate.waiter();
        let epoch = gate.start();
        let (outcome, output) =
            tokio::join!(self.run_from_epoch(epoch, token, sink), workload(epoch),);
        let _ = waiter;
        (outcome, output)
    }
}
