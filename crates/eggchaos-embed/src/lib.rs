//! Safe coarse embedding facade for eggchaos.
//!
//! ```text
//! foreign caller (e.g. Python)
//!   |
//! eggchaos-native (PyO3, managed objects only)
//!   |
//! eggchaos-embed (this crate: lifecycle + coarse control)
//!   |
//!   +--> eggchaos-server::operations (typed native operation authority)
//!   +--> eggchaos-protocol value/request types
//!   +--> eggchaos-server lifecycle + ControlState authority
//!   +--> eggchaos-experiment Scenario V2 authority
//! ```
//!
//! This crate owns lifecycle and delegates every state change to the
//! existing authorities. It never re-implements networking, policy
//! publication, RNG derivation, or schedule compilation. Every public
//! method delegates its conversion/state-call sequence to the typed
//! `eggchaos_server::operations` facade (M051); this module adds only
//! the blocking runtime, the start/shutdown lifecycle, and the
//! `EmbedError` mapping on top. Each embedded service owns one
//! private Tokio runtime on the calling thread's terms: every method
//! blocks the caller until the operation completes, so foreign
//! callers never observe Rust futures, `Arc`s, borrowed references,
//! or `tokio::time::Instant`.
//!
//! Blocking contract: do not call these methods from inside an async
//! context running on the embedded runtime (the runtime is private, so
//! this only happens if the caller blocks its own executor thread while
//! another task on the same thread awaits the facade — use a dedicated
//! thread instead). Concurrent calls from multiple OS threads are safe.
#![deny(unsafe_code)]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use eggchaos_protocol::DatagramFaultSpecV1;
use eggchaos_protocol::{
    DatagramFaultPatchV1, DatagramFaultUpsertV1, ErrorEnvelopeV1, FaultPatchV1, FaultUpsertV1,
    HealthV1, NativeDatagramAssociationViewV1, NativeDatagramProxyPatchV1,
    NativeDatagramProxyRequestV1, NativeDatagramProxyViewV1, NativeFaultViewV1, NativeProxyPatchV1,
    NativeProxyRequestV1, NativeProxyViewV1, RuntimeConfigV1, ScenarioRunV1, ScenarioScheduleV2Dto,
    ScenarioV1, ScheduleCompileV2, ScheduleRunV2, ScheduleValidateV2, VersionV1,
};
use eggchaos_server::{
    apply_datagram_fault_patch, apply_datagram_fault_upsert, apply_datagram_proxy_patch,
    apply_datagram_proxy_request, apply_proxy_patch, apply_proxy_request, apply_runtime_config,
    apply_scenario_v1, apply_scenario_v2_dto, apply_stream_fault_patch, apply_stream_fault_upsert,
    cancel_scenario_run, compile_scenario_v2_dto, get_connection, get_scenario_run,
    kill_connection, kill_datagram_association, reset_service, validate_scenario_v2_dto,
    ControlState, RuntimeLimitApply, ServiceBuilder,
};
use thiserror::Error;

/// Coarse embedding errors aligned with the remote client's categories.
#[derive(Debug, Error, Clone)]
pub enum EmbedError {
    /// Request or resulting state is invalid (includes wire validation).
    #[error("validation: {0}")]
    Validation(String),
    /// Named resource does not exist.
    #[error("not found: {0}")]
    NotFound(String),
    /// Duplicate identity or generation conflict.
    #[error("conflict: {0}")]
    Conflict(String),
    /// Capability the facade does not expose.
    #[error("unsupported: {0}")]
    Unsupported(String),
    /// Service lifecycle misuse (use after close, double start, bind failure).
    #[error("lifecycle: {0}")]
    Lifecycle(String),
    /// Listener bind/connect failure.
    #[error("bind: {0}")]
    Bind(String),
    /// Internal failure (never a panic; never a secret).
    #[error("internal: {0}")]
    Internal(String),
}

impl From<eggchaos_server::ControlError> for EmbedError {
    fn from(error: eggchaos_server::ControlError) -> Self {
        match error {
            eggchaos_server::ControlError::NotFound(detail) => Self::NotFound(detail),
            eggchaos_server::ControlError::Conflict(detail) => Self::Conflict(detail),
            eggchaos_server::ControlError::Invalid(detail) => Self::Validation(detail),
            eggchaos_server::ControlError::BindFailed { proxy, reason } => {
                Self::Bind(format!("{proxy}: {reason}"))
            }
            eggchaos_server::ControlError::RestartFailed { proxy, reason } => {
                Self::Lifecycle(format!("{proxy}: {reason}"))
            }
        }
    }
}

impl From<eggchaos_server::DatagramRuntimeError> for EmbedError {
    fn from(error: eggchaos_server::DatagramRuntimeError) -> Self {
        match error {
            eggchaos_server::DatagramRuntimeError::NotFound(detail) => Self::NotFound(detail),
            eggchaos_server::DatagramRuntimeError::Duplicate(detail) => Self::Conflict(detail),
            eggchaos_server::DatagramRuntimeError::Conflict(detail) => Self::Conflict(detail),
            eggchaos_server::DatagramRuntimeError::Bind(error) => Self::Bind(error.to_string()),
            eggchaos_server::DatagramRuntimeError::Invalid(detail) => Self::Validation(detail),
            eggchaos_server::DatagramRuntimeError::AssociationLimit => {
                Self::Validation("datagram runtime has reached its association limit".into())
            }
        }
    }
}

impl From<eggchaos_server::EggchaosError> for EmbedError {
    fn from(error: eggchaos_server::EggchaosError) -> Self {
        match error {
            eggchaos_server::EggchaosError::Control(inner) => Self::from(inner),
            eggchaos_server::EggchaosError::Io { source, .. } => {
                Self::Lifecycle(source.to_string())
            }
            eggchaos_server::EggchaosError::Join(detail) => Self::Lifecycle(detail),
            eggchaos_server::EggchaosError::InvalidProxy(detail) => Self::Validation(detail),
            eggchaos_server::EggchaosError::InvalidPlan(detail) => {
                Self::Validation(detail.to_string())
            }
            eggchaos_server::EggchaosError::DuplicateProxy(detail) => Self::Conflict(detail),
        }
    }
}

/// Options for [`EmbeddedService::start`].
#[derive(Debug, Clone, Default)]
pub struct EmbedOptions {
    /// Deterministic service seed namespace.
    pub seed: u64,
    /// Wire runtime configuration (bounds only; listeners come from proxies).
    pub runtime: RuntimeConfigV1,
}

/// A scenario run view covering both run families.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "family", rename_all = "kebab-case")]
pub enum ScenarioRunView {
    /// Scenario V1 run record.
    V1(ScenarioRunV1),
    /// Scenario V2 schedule run record.
    V2(ScheduleRunV2),
}

/// An embedded eggchaos service: owned runtime plus control authority.
///
/// All methods block the calling thread. `shutdown` is idempotent;
/// dropping without shutdown initiates shutdown without joining (the
/// owned runtime is then released, aborting supervised tasks — never
/// detaching them into the caller's process state).
pub struct EmbeddedService {
    runtime: tokio::runtime::Runtime,
    control: ControlState,
    closed: Arc<AtomicBool>,
}

impl EmbeddedService {
    /// Start an embedded service with no proxies.
    pub fn start(options: EmbedOptions) -> Result<Self, EmbedError> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .thread_name("eggchaos-embed")
            .build()
            .map_err(|error| EmbedError::Lifecycle(error.to_string()))?;
        let RuntimeLimitApply {
            admission,
            datagram,
            relay_buffer,
            termination_grace,
        } = apply_runtime_config(options.runtime).map_err(EmbedError::from)?;
        let service = ServiceBuilder::new(options.seed)
            .limits(admission)
            .relay_buffer(relay_buffer.into())
            .termination_grace(termination_grace)
            .datagram_limits(datagram)
            .build()
            .map_err(EmbedError::from)?;
        let control = runtime
            .block_on(service.start())
            .map_err(EmbedError::from)?
            .control_state();
        Ok(Self {
            runtime,
            control,
            closed: Arc::new(AtomicBool::new(false)),
        })
    }

    fn block_on<F, T>(&self, future: F) -> Result<T, EmbedError>
    where
        F: std::future::Future<Output = Result<T, EmbedError>>,
    {
        if self.closed.load(Ordering::SeqCst) {
            return Err(EmbedError::Lifecycle("service is closed".into()));
        }
        self.runtime.block_on(future)
    }

    /// Whether shutdown has completed or been requested.
    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }

    /// Global configuration generation (synchronous snapshot read).
    ///
    /// Reads the live generation even after close (snapshots stay
    /// observable); use [`Self::is_closed`] to gate lifecycle.
    pub fn generation(&self) -> u64 {
        self.control.generation()
    }

    /// Share the underlying control authority (advanced Rust embedding).
    ///
    /// The returned handle aliases this service's state; prefer the
    /// coarse methods above unless an existing `ControlState` consumer
    /// must attach to the embedded runtime.
    pub fn control_state(&self) -> ControlState {
        self.control.clone()
    }

    /// Shut down listeners and join supervised tasks. Idempotent.
    pub fn shutdown(&self) {
        if self.closed.swap(true, Ordering::SeqCst) {
            return;
        }
        self.control.initiate_shutdown();
        self.runtime.block_on(self.control.shutdown_and_join());
    }

    /// Service liveness and global generation.
    pub fn health(&self) -> Result<HealthV1, EmbedError> {
        self.block_on(async {
            Ok(HealthV1 {
                running: !self.closed.load(Ordering::SeqCst),
                generation: self.control.generation(),
            })
        })
    }

    /// Service and native API version.
    pub fn version(&self) -> Result<VersionV1, EmbedError> {
        self.block_on(async {
            Ok(VersionV1 {
                version: eggchaos_server::VERSION.to_owned(),
                api: "v1".to_owned(),
            })
        })
    }

    /// Prometheus text metrics (never JSON).
    pub fn metrics_text(&self) -> Result<String, EmbedError> {
        self.block_on(async { Ok(self.control.metrics_text().await) })
    }

    /// Reset stream and datagram state to configured definitions.
    pub fn reset(&self) -> Result<eggchaos_server::ResetReport, EmbedError> {
        self.block_on(async { reset_service(&self.control).await.map_err(EmbedError::from) })
    }

    // ------------------------------------------------------------ stream proxies

    /// Create a stream proxy; returns the view plus the generation.
    pub fn create_proxy(
        &self,
        request: NativeProxyRequestV1,
    ) -> Result<(NativeProxyViewV1, u64), EmbedError> {
        request.validate().map_err(EmbedError::Validation)?;
        self.block_on(async {
            let outcome = apply_proxy_request(&self.control, request)
                .await
                .map_err(EmbedError::from)?;
            Ok((outcome.proxy, outcome.generation))
        })
    }

    /// List stream proxy views.
    pub fn list_proxies(&self) -> Result<Vec<NativeProxyViewV1>, EmbedError> {
        self.block_on(async {
            Ok(self
                .control
                .list()
                .await
                .into_iter()
                .map(NativeProxyViewV1::from)
                .collect())
        })
    }

    /// Get one stream proxy view.
    pub fn get_proxy(&self, name: &str) -> Result<NativeProxyViewV1, EmbedError> {
        self.block_on(async {
            self.control
                .get(name)
                .await
                .map(NativeProxyViewV1::from)
                .ok_or_else(|| EmbedError::NotFound(name.to_owned()))
        })
    }

    /// Update a stream proxy; returns the view plus the generation.
    pub fn patch_proxy(
        &self,
        name: &str,
        patch: NativeProxyPatchV1,
    ) -> Result<(NativeProxyViewV1, u64), EmbedError> {
        self.block_on(async {
            patch.validate().map_err(EmbedError::Validation)?;
            let outcome = apply_proxy_patch(&self.control, name, patch)
                .await
                .map_err(EmbedError::from)?;
            Ok((outcome.proxy, outcome.generation))
        })
    }

    /// Delete a stream proxy; returns the generation.
    pub fn delete_proxy(&self, name: &str) -> Result<u64, EmbedError> {
        self.block_on(async {
            self.control
                .delete_proxy(name)
                .await
                .map_err(EmbedError::from)
        })
    }

    // ------------------------------------------------------------ stream faults

    /// Add a stream fault; returns direction, view, and generation.
    pub fn add_fault(
        &self,
        proxy: &str,
        upsert: FaultUpsertV1,
    ) -> Result<(eggchaos_core::Direction, NativeFaultViewV1, u64), EmbedError> {
        upsert.validate().map_err(EmbedError::Validation)?;
        self.block_on(async {
            let outcome = apply_stream_fault_upsert(&self.control, proxy, upsert)
                .await
                .map_err(EmbedError::from)?;
            Ok((outcome.direction, outcome.fault, outcome.generation))
        })
    }

    /// List stream fault views by direction.
    pub fn list_faults(
        &self,
        proxy: &str,
    ) -> Result<(Vec<NativeFaultViewV1>, Vec<NativeFaultViewV1>), EmbedError> {
        self.block_on(async {
            let (upstream, downstream) = self
                .control
                .list_faults(proxy)
                .await
                .ok_or_else(|| EmbedError::NotFound(proxy.to_owned()))?;
            Ok((
                upstream.iter().map(NativeFaultViewV1::from).collect(),
                downstream.iter().map(NativeFaultViewV1::from).collect(),
            ))
        })
    }

    /// Get one stream fault view plus its direction.
    pub fn get_fault(
        &self,
        proxy: &str,
        id: &str,
    ) -> Result<(eggchaos_core::Direction, NativeFaultViewV1), EmbedError> {
        self.block_on(async {
            let (direction, fault) = self
                .control
                .get_fault(proxy, id)
                .await
                .ok_or_else(|| EmbedError::NotFound(format!("fault {id} on {proxy}")))?;
            Ok((direction, NativeFaultViewV1::from(&fault)))
        })
    }

    /// Update a stream fault; returns direction, view, and generation.
    pub fn patch_fault(
        &self,
        proxy: &str,
        id: &str,
        patch: FaultPatchV1,
    ) -> Result<(eggchaos_core::Direction, NativeFaultViewV1, u64), EmbedError> {
        patch.validate().map_err(EmbedError::Validation)?;
        self.block_on(async {
            let outcome = apply_stream_fault_patch(&self.control, proxy, id, patch)
                .await
                .map_err(EmbedError::from)?;
            Ok((outcome.direction, outcome.fault, outcome.generation))
        })
    }

    /// Remove a stream fault; returns the generation.
    pub fn remove_fault(&self, proxy: &str, id: &str) -> Result<u64, EmbedError> {
        self.block_on(async {
            self.control
                .remove_fault(proxy, id)
                .await
                .map_err(EmbedError::from)
        })
    }

    // ------------------------------------------------------------ connections

    /// Snapshot active stream connections.
    pub fn connections(&self) -> Result<Vec<eggchaos_server::ConnectionSnapshot>, EmbedError> {
        self.block_on(async { Ok(self.control.connections().await) })
    }

    /// Snapshot one active stream connection.
    pub fn get_connection(
        &self,
        id: u64,
    ) -> Result<eggchaos_server::ConnectionSnapshot, EmbedError> {
        self.block_on(async {
            get_connection(&self.control, id)
                .await
                .ok_or_else(|| EmbedError::NotFound(format!("connection {id}")))
        })
    }

    /// Terminate one active stream connection.
    pub fn kill_connection(&self, id: u64) -> Result<bool, EmbedError> {
        self.block_on(async {
            if kill_connection(&self.control, id).await {
                Ok(true)
            } else {
                Err(EmbedError::NotFound(format!("connection {id}")))
            }
        })
    }

    /// Snapshot bounded closed-connection history.
    pub fn history(&self) -> Result<Vec<eggchaos_server::ClosedConnection>, EmbedError> {
        self.block_on(async { Ok(self.control.history().await) })
    }

    // ------------------------------------------------------------ scenarios

    /// Apply a Scenario V1 document; returns the run record view.
    pub fn apply_scenario_v1(&self, scenario: ScenarioV1) -> Result<ScenarioRunV1, EmbedError> {
        scenario.validate().map_err(EmbedError::Validation)?;
        self.block_on(async {
            apply_scenario_v1(&self.control, scenario)
                .await
                .map_err(EmbedError::from)
        })
    }

    /// Validate a Scenario V2 schedule without creating a run.
    pub fn validate_schedule_v2(
        &self,
        schedule: ScenarioScheduleV2Dto,
    ) -> Result<ScheduleValidateV2, EmbedError> {
        // Pure validation: observable even after close.
        validate_scenario_v2_dto(schedule).map_err(EmbedError::from)
    }

    /// Compile a Scenario V2 schedule without creating a run.
    pub fn compile_schedule_v2(
        &self,
        schedule: ScenarioScheduleV2Dto,
    ) -> Result<ScheduleCompileV2, EmbedError> {
        // Pure compilation: observable even after close.
        compile_scenario_v2_dto(schedule).map_err(EmbedError::from)
    }

    /// Apply a Scenario V2 schedule; returns the run view.
    pub fn apply_schedule_v2(
        &self,
        schedule: ScenarioScheduleV2Dto,
    ) -> Result<ScheduleRunV2, EmbedError> {
        self.block_on(async {
            apply_scenario_v2_dto(&self.control, schedule)
                .await
                .map_err(EmbedError::from)
        })
    }

    /// Get a Scenario V1 or V2 run record.
    pub fn get_scenario(&self, run_id: u64) -> Result<ScenarioRunView, EmbedError> {
        self.block_on(async {
            get_scenario_run(&self.control, run_id)
                .await
                .map(|record| match record {
                    eggchaos_server::ScenarioRunLookup::V1(record) => ScenarioRunView::V1(record),
                    eggchaos_server::ScenarioRunLookup::V2(record) => ScenarioRunView::V2(record),
                })
                .ok_or_else(|| EmbedError::NotFound(format!("scenario run {run_id}")))
        })
    }

    /// Cancel a Scenario V1 or V2 run.
    pub fn cancel_scenario(&self, run_id: u64) -> Result<ScenarioRunView, EmbedError> {
        self.block_on(async {
            cancel_scenario_run(&self.control, run_id)
                .await
                .map(|record| match record {
                    eggchaos_server::ScenarioRunLookup::V1(record) => ScenarioRunView::V1(record),
                    eggchaos_server::ScenarioRunLookup::V2(record) => ScenarioRunView::V2(record),
                })
                .ok_or_else(|| EmbedError::NotFound(format!("scenario run {run_id}")))
        })
    }

    // ------------------------------------------------------------ datagrams

    /// Create a datagram proxy; returns the view plus the generation.
    pub fn create_datagram_proxy(
        &self,
        request: NativeDatagramProxyRequestV1,
    ) -> Result<(NativeDatagramProxyViewV1, u64), EmbedError> {
        request.validate().map_err(EmbedError::Validation)?;
        self.block_on(async {
            let outcome = apply_datagram_proxy_request(&self.control, request)
                .await
                .map_err(EmbedError::from)?;
            Ok((outcome.proxy, outcome.generation))
        })
    }

    /// List datagram proxy views.
    pub fn list_datagram_proxies(&self) -> Result<Vec<NativeDatagramProxyViewV1>, EmbedError> {
        self.block_on(async {
            Ok(self
                .control
                .datagram_proxies()
                .await
                .into_iter()
                .map(NativeDatagramProxyViewV1::from)
                .collect())
        })
    }

    /// Get one datagram proxy view.
    pub fn get_datagram_proxy(&self, name: &str) -> Result<NativeDatagramProxyViewV1, EmbedError> {
        self.block_on(async {
            self.control
                .get_datagram_proxy(name)
                .await
                .map(NativeDatagramProxyViewV1::from)
                .ok_or_else(|| EmbedError::NotFound(name.to_owned()))
        })
    }

    /// Update a datagram proxy; returns the view plus the generation.
    pub fn patch_datagram_proxy(
        &self,
        name: &str,
        patch: NativeDatagramProxyPatchV1,
    ) -> Result<(NativeDatagramProxyViewV1, u64), EmbedError> {
        patch.validate().map_err(EmbedError::Validation)?;
        self.block_on(async {
            let outcome = apply_datagram_proxy_patch(&self.control, name, patch)
                .await
                .map_err(EmbedError::from)?;
            Ok((outcome.proxy, outcome.generation))
        })
    }

    /// Delete a datagram proxy; returns the generation.
    pub fn delete_datagram_proxy(&self, name: &str) -> Result<u64, EmbedError> {
        self.block_on(async {
            self.control
                .delete_datagram_proxy(name)
                .await
                .map_err(EmbedError::from)
        })
    }

    /// Add a datagram fault; returns direction, view, and generation.
    ///
    /// Mutation semantics (duplicate/cross-direction conflicts, plan
    /// reconstruction, generation-guarded publication) are owned by the
    /// shared [`ControlState`] authority; this method only delegates
    /// through the typed operation facade (M051).
    pub fn add_datagram_fault(
        &self,
        proxy: &str,
        upsert: DatagramFaultUpsertV1,
    ) -> Result<(eggchaos_core::Direction, DatagramFaultSpecV1, u64), EmbedError> {
        upsert
            .fault
            .clone()
            .into_core()
            .map_err(EmbedError::Validation)?;
        self.block_on(async {
            let outcome = apply_datagram_fault_upsert(&self.control, proxy, upsert)
                .await
                .map_err(EmbedError::from)?;
            Ok((outcome.direction, outcome.fault, outcome.generation))
        })
    }

    /// Get one datagram fault view plus its direction.
    pub fn get_datagram_fault(
        &self,
        proxy: &str,
        id: &str,
    ) -> Result<(eggchaos_core::Direction, DatagramFaultSpecV1), EmbedError> {
        self.block_on(async {
            let (direction, fault) = self
                .control
                .get_datagram_fault(proxy, id)
                .await
                .ok_or_else(|| EmbedError::NotFound(format!("fault {id} on {proxy}")))?;
            Ok((direction, DatagramFaultSpecV1::from(fault)))
        })
    }

    /// List datagram fault views by direction.
    pub fn list_datagram_faults(
        &self,
        proxy: &str,
    ) -> Result<(Vec<DatagramFaultSpecV1>, Vec<DatagramFaultSpecV1>), EmbedError> {
        self.block_on(async {
            let (upstream, downstream) = self
                .control
                .list_datagram_faults(proxy)
                .await
                .ok_or_else(|| EmbedError::NotFound(proxy.to_owned()))?;
            Ok((
                upstream
                    .into_iter()
                    .map(DatagramFaultSpecV1::from)
                    .collect(),
                downstream
                    .into_iter()
                    .map(DatagramFaultSpecV1::from)
                    .collect(),
            ))
        })
    }

    /// Update a datagram fault; returns direction, view, and generation.
    pub fn patch_datagram_fault(
        &self,
        proxy: &str,
        id: &str,
        patch: DatagramFaultPatchV1,
    ) -> Result<(eggchaos_core::Direction, DatagramFaultSpecV1, u64), EmbedError> {
        patch.validate().map_err(EmbedError::Validation)?;
        self.block_on(async {
            let outcome = apply_datagram_fault_patch(&self.control, proxy, id, patch)
                .await
                .map_err(EmbedError::from)?;
            Ok((outcome.direction, outcome.fault, outcome.generation))
        })
    }

    /// Remove a datagram fault; returns the generation.
    pub fn remove_datagram_fault(
        &self,
        proxy: &str,
        id: &str,
    ) -> Result<(eggchaos_core::Direction, u64), EmbedError> {
        self.block_on(async {
            self.control
                .remove_datagram_fault(proxy, id)
                .await
                .map_err(EmbedError::from)
        })
    }

    /// Snapshot datagram association views.
    pub fn datagram_associations(
        &self,
    ) -> Result<Vec<NativeDatagramAssociationViewV1>, EmbedError> {
        self.block_on(async {
            Ok(self
                .control
                .datagram_associations()
                .await
                .into_iter()
                .map(NativeDatagramAssociationViewV1::from)
                .collect())
        })
    }

    /// Terminate one datagram association.
    pub fn kill_datagram_association(&self, id: u64) -> Result<bool, EmbedError> {
        self.block_on(async {
            if kill_datagram_association(&self.control, id).await {
                Ok(true)
            } else {
                Err(EmbedError::NotFound(format!("datagram association {id}")))
            }
        })
    }

    /// Render the shared native error envelope for an embed error.
    pub fn error_envelope(error: &EmbedError) -> ErrorEnvelopeV1 {
        // Codes stay within the native wire vocabulary
        // (`not_found`, `conflict`, `invalid`, `bind_failed`,
        // `restart_failed`, ...): lifecycle/internal map to the 500-class
        // `restart_failed`, unsupported maps to `invalid`.
        let (code, message) = match error {
            EmbedError::Validation(detail) => ("invalid", detail.clone()),
            EmbedError::NotFound(detail) => ("not_found", detail.clone()),
            EmbedError::Conflict(detail) => ("conflict", detail.clone()),
            EmbedError::Unsupported(detail) => ("invalid", detail.clone()),
            EmbedError::Lifecycle(detail) => ("restart_failed", detail.clone()),
            EmbedError::Bind(detail) => ("bind_failed", detail.clone()),
            EmbedError::Internal(detail) => ("restart_failed", detail.clone()),
        };
        ErrorEnvelopeV1::new(code, message)
    }
}

impl Drop for EmbeddedService {
    fn drop(&mut self) {
        // Best-effort: signal shutdown without joining. Explicit `shutdown`
        // joins supervised tasks; dropping the owned runtime afterwards
        // aborts (never detaches) any remainder.
        if !self.closed.swap(true, Ordering::SeqCst) {
            self.control.initiate_shutdown();
        }
    }
}
