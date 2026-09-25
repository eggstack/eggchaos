//! Safe coarse embedding facade for eggchaos.
//!
//! ```text
//! foreign caller (e.g. Python)
//!   |
//! eggchaos-native (PyO3, managed objects only)
//!   |
//! eggchaos-embed (this crate: lifecycle + coarse control)
//!   |
//!   +--> eggchaos-protocol value/request types
//!   +--> eggchaos-server lifecycle + ControlState authority
//!   +--> eggchaos-experiment Scenario V2 authority
//! ```
//!
//! This crate owns lifecycle and delegates every state change to the
//! existing authorities. It never re-implements networking, policy
//! publication, RNG derivation, or schedule compilation. Each embedded
//! service owns one private Tokio runtime on the calling thread's terms:
//! every method blocks the caller until the operation completes, so
//! foreign callers never observe Rust futures, `Arc`s, borrowed
//! references, or `tokio::time::Instant`.
//!
//! Blocking contract: do not call these methods from inside an async
//! context running on the embedded runtime (the runtime is private, so
//! this only happens if the caller blocks its own executor thread while
//! another task on the same thread awaits the facade — use a dedicated
//! thread instead). Concurrent calls from multiple OS threads are safe.
#![deny(unsafe_code)]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use eggchaos_protocol::{
    DatagramFaultPatchV1, DatagramFaultSpecV1, DatagramFaultUpsertV1, ErrorEnvelopeV1,
    FaultPatchV1, FaultUpsertV1, HealthV1, NativeDatagramAssociationViewV1,
    NativeDatagramProxyPatchV1, NativeDatagramProxyRequestV1, NativeDatagramProxyViewV1,
    NativeFaultViewV1, NativeProxyPatchV1, NativeProxyRequestV1, NativeProxyViewV1,
    RuntimeConfigV1, ScenarioRunV1, ScenarioScheduleV2Dto, ScenarioV1, ScheduleCompileV2,
    ScheduleRunV2, ScheduleValidateV2, VersionV1,
};
use eggchaos_server::{
    compile_schedule, datagram_fault_patch_into_parts, datagram_proxy_request_into_spec,
    fault_patch_into_runtime, fault_upsert_into_runtime, proxy_request_into_spec,
    runtime_admission_limits, runtime_datagram_limits, scenario_v1_into_runtime, ControlState,
    ServiceBuilder,
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
        let service = ServiceBuilder::new(options.seed)
            .limits(runtime_admission_limits(options.runtime).map_err(EmbedError::Validation)?)
            .relay_buffer(
                options
                    .runtime
                    .relay_buffer()
                    .map_err(EmbedError::Validation)?,
            )
            .termination_grace(
                options
                    .runtime
                    .termination_grace()
                    .map_err(EmbedError::Validation)?,
            )
            .datagram_limits(
                runtime_datagram_limits(options.runtime.datagram)
                    .map_err(EmbedError::Validation)?,
            )
            .build()
            .map_err(|error| EmbedError::Validation(error.to_string()))?;
        let control = runtime
            .block_on(service.start())
            .map_err(|error| EmbedError::Lifecycle(error.to_string()))?
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
                running: true,
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
        self.block_on(async { self.control.reset().await.map_err(EmbedError::from) })
    }

    // ------------------------------------------------------------ stream proxies

    /// Create a stream proxy; returns the view plus the generation.
    pub fn create_proxy(
        &self,
        request: NativeProxyRequestV1,
    ) -> Result<(NativeProxyViewV1, u64), EmbedError> {
        self.block_on(async {
            let spec = proxy_request_into_spec(request).map_err(EmbedError::Validation)?;
            let (view, generation) = self
                .control
                .create_proxy(spec)
                .await
                .map_err(EmbedError::from)?;
            Ok((NativeProxyViewV1::from(view), generation))
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
            let (view, generation) = self
                .control
                .update_proxy(name, patch.into())
                .await
                .map_err(EmbedError::from)?;
            Ok((NativeProxyViewV1::from(view), generation))
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
        self.block_on(async {
            let upsert = fault_upsert_into_runtime(upsert).map_err(EmbedError::Validation)?;
            let (direction, fault, generation) = self
                .control
                .add_fault(proxy, upsert)
                .await
                .map_err(EmbedError::from)?;
            Ok((direction, NativeFaultViewV1::from(&fault), generation))
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
        self.block_on(async {
            let patch = fault_patch_into_runtime(patch).map_err(EmbedError::Validation)?;
            if patch.probability.is_none() && patch.kind.is_none() {
                return Err(EmbedError::Validation(
                    "fault patch must include probability or kind".into(),
                ));
            }
            let (direction, fault, generation) = self
                .control
                .update_fault(proxy, id, patch)
                .await
                .map_err(EmbedError::from)?;
            Ok((direction, NativeFaultViewV1::from(&fault), generation))
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
            self.control
                .get_connection(id)
                .await
                .ok_or_else(|| EmbedError::NotFound(format!("connection {id}")))
        })
    }

    /// Terminate one active stream connection.
    pub fn kill_connection(&self, id: u64) -> Result<bool, EmbedError> {
        self.block_on(async { Ok(self.control.kill(id).await) })
    }

    /// Snapshot bounded closed-connection history.
    pub fn history(&self) -> Result<Vec<eggchaos_server::ClosedConnection>, EmbedError> {
        self.block_on(async { Ok(self.control.history().await) })
    }

    // ------------------------------------------------------------ scenarios

    /// Apply a Scenario V1 document; returns the run record view.
    pub fn apply_scenario_v1(&self, scenario: ScenarioV1) -> Result<ScenarioRunV1, EmbedError> {
        self.block_on(async {
            let scenario = scenario_v1_into_runtime(scenario).map_err(EmbedError::Validation)?;
            let record = self
                .control
                .start_scenario(scenario)
                .await
                .map_err(|error| EmbedError::Validation(error.to_string()))?;
            Ok(ScenarioRunV1::from(record))
        })
    }

    /// Validate a Scenario V2 schedule without creating a run.
    pub fn validate_schedule_v2(
        &self,
        schedule: ScenarioScheduleV2Dto,
    ) -> Result<ScheduleValidateV2, EmbedError> {
        let source = schedule.into_internal().map_err(EmbedError::Validation)?;
        let compiled =
            compile_schedule(&source).map_err(|error| EmbedError::Validation(error.to_string()))?;
        Ok(ScheduleValidateV2::from_compiled(&compiled))
    }

    /// Compile a Scenario V2 schedule without creating a run.
    pub fn compile_schedule_v2(
        &self,
        schedule: ScenarioScheduleV2Dto,
    ) -> Result<ScheduleCompileV2, EmbedError> {
        let source = schedule.into_internal().map_err(EmbedError::Validation)?;
        let compiled =
            compile_schedule(&source).map_err(|error| EmbedError::Validation(error.to_string()))?;
        Ok(ScheduleCompileV2::from_compiled(&compiled))
    }

    /// Apply a Scenario V2 schedule; returns the run view.
    pub fn apply_schedule_v2(
        &self,
        schedule: ScenarioScheduleV2Dto,
    ) -> Result<ScheduleRunV2, EmbedError> {
        self.block_on(async {
            let source = schedule.into_internal().map_err(EmbedError::Validation)?;
            let record = self
                .control
                .start_schedule_v2(source)
                .await
                .map_err(|error| EmbedError::Validation(error.to_string()))?;
            Ok(ScheduleRunV2::from(record))
        })
    }

    /// Get a Scenario V1 or V2 run record.
    pub fn get_scenario(&self, run_id: u64) -> Result<ScenarioRunView, EmbedError> {
        self.block_on(async {
            if let Some(record) = self.control.get_scenario(run_id).await {
                return Ok(ScenarioRunView::V1(ScenarioRunV1::from(record)));
            }
            if let Some(record) = self.control.get_schedule_v2(run_id).await {
                return Ok(ScenarioRunView::V2(ScheduleRunV2::from(record)));
            }
            Err(EmbedError::NotFound(format!("scenario run {run_id}")))
        })
    }

    /// Cancel a Scenario V1 or V2 run.
    pub fn cancel_scenario(&self, run_id: u64) -> Result<ScenarioRunView, EmbedError> {
        self.block_on(async {
            if let Some(record) = self.control.cancel_scenario(run_id).await {
                return Ok(ScenarioRunView::V1(ScenarioRunV1::from(record)));
            }
            if let Some(record) = self.control.cancel_schedule_v2(run_id).await {
                return Ok(ScenarioRunView::V2(ScheduleRunV2::from(record)));
            }
            Err(EmbedError::NotFound(format!("scenario run {run_id}")))
        })
    }

    // ------------------------------------------------------------ datagrams

    /// Create a datagram proxy; returns the view plus the generation.
    pub fn create_datagram_proxy(
        &self,
        request: NativeDatagramProxyRequestV1,
    ) -> Result<(NativeDatagramProxyViewV1, u64), EmbedError> {
        self.block_on(async {
            let spec = datagram_proxy_request_into_spec(request).map_err(EmbedError::Validation)?;
            let (view, generation) = self
                .control
                .create_datagram_proxy(spec)
                .await
                .map_err(EmbedError::from)?;
            Ok((NativeDatagramProxyViewV1::from(view), generation))
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
        self.block_on(async {
            if patch.enabled.is_none()
                && patch.listen.is_none()
                && patch.upstream.is_none()
                && patch.max_associations.is_none()
                && patch.association_idle_timeout_ms.is_none()
            {
                return Err(EmbedError::Validation("empty datagram proxy patch".into()));
            }
            let (view, generation) = self
                .control
                .update_datagram_proxy(
                    name,
                    patch.listen,
                    patch.upstream,
                    patch.max_associations,
                    patch.association_idle_timeout_ms,
                    patch.enabled,
                )
                .await
                .map_err(EmbedError::from)?;
            Ok((NativeDatagramProxyViewV1::from(view), generation))
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
    pub fn add_datagram_fault(
        &self,
        proxy: &str,
        upsert: DatagramFaultUpsertV1,
    ) -> Result<
        (
            eggchaos_core::Direction,
            eggchaos_protocol::DatagramFaultSpecV1,
            u64,
        ),
        EmbedError,
    > {
        self.block_on(async {
            let direction = upsert.direction;
            let fault = upsert.fault.into_core().map_err(EmbedError::Validation)?;
            let (plan, generation, seed) = self
                .control
                .get_datagram_plan(proxy, direction)
                .await
                .map_err(EmbedError::from)?;
            if plan.faults().iter().any(|existing| existing.id == fault.id) {
                return Err(EmbedError::Conflict("datagram fault already exists".into()));
            }
            let other = match direction {
                eggchaos_core::Direction::Upstream => eggchaos_core::Direction::Downstream,
                eggchaos_core::Direction::Downstream => eggchaos_core::Direction::Upstream,
            };
            if self
                .control
                .get_datagram_plan(proxy, other)
                .await
                .is_ok_and(|(plan, _, _)| {
                    plan.faults().iter().any(|existing| existing.id == fault.id)
                })
            {
                return Err(EmbedError::Conflict(
                    "datagram fault id must be unique across directions for path lookup".into(),
                ));
            }
            let mut faults = plan.faults().to_vec();
            faults.push(fault.clone());
            let plan = eggchaos_core::DatagramPlan::new(faults)
                .map_err(|error| EmbedError::Validation(error.to_string()))?;
            let next = self
                .control
                .publish_datagram_plan(proxy, direction, plan, seed, Some(generation))
                .await
                .map_err(EmbedError::from)?;
            Ok((direction, DatagramFaultSpecV1::from(fault), next))
        })
    }

    /// Get one datagram fault view plus its direction.
    pub fn get_datagram_fault(
        &self,
        proxy: &str,
        id: &str,
    ) -> Result<
        (
            eggchaos_core::Direction,
            eggchaos_protocol::DatagramFaultSpecV1,
        ),
        EmbedError,
    > {
        self.block_on(async {
            for direction in [
                eggchaos_core::Direction::Upstream,
                eggchaos_core::Direction::Downstream,
            ] {
                if let Ok((plan, _, _)) = self.control.get_datagram_plan(proxy, direction).await {
                    if let Some(fault) = plan.faults().iter().find(|fault| fault.id.as_str() == id)
                    {
                        return Ok((
                            direction,
                            eggchaos_protocol::DatagramFaultSpecV1::from(fault.clone()),
                        ));
                    }
                }
            }
            Err(EmbedError::NotFound(format!("fault {id} on {proxy}")))
        })
    }

    /// Update a datagram fault; returns direction, view, and generation.
    pub fn patch_datagram_fault(
        &self,
        proxy: &str,
        id: &str,
        patch: DatagramFaultPatchV1,
    ) -> Result<
        (
            eggchaos_core::Direction,
            eggchaos_protocol::DatagramFaultSpecV1,
            u64,
        ),
        EmbedError,
    > {
        self.block_on(async {
            let (probability, kind) =
                datagram_fault_patch_into_parts(patch).map_err(EmbedError::Validation)?;
            if probability.is_none() && kind.is_none() {
                return Err(EmbedError::Validation(
                    "fault patch must include probability or kind".into(),
                ));
            }
            for direction in [
                eggchaos_core::Direction::Upstream,
                eggchaos_core::Direction::Downstream,
            ] {
                let Ok((plan, generation, seed)) =
                    self.control.get_datagram_plan(proxy, direction).await
                else {
                    continue;
                };
                let Some(mut fault) = plan
                    .faults()
                    .iter()
                    .find(|fault| fault.id.as_str() == id)
                    .cloned()
                else {
                    continue;
                };
                if let Some(probability) = probability {
                    fault.probability = eggchaos_core::Probability::new(probability)
                        .map_err(|error| EmbedError::Validation(error.to_string()))?;
                }
                if let Some(kind) = kind.clone() {
                    fault.kind = kind;
                }
                let mut faults = plan.faults().to_vec();
                if let Some(existing) = faults
                    .iter_mut()
                    .find(|candidate| candidate.id.as_str() == id)
                {
                    *existing = fault.clone();
                }
                let updated = eggchaos_core::DatagramPlan::new(faults)
                    .map_err(|error| EmbedError::Validation(error.to_string()))?;
                let next = self
                    .control
                    .publish_datagram_plan(proxy, direction, updated, seed, Some(generation))
                    .await
                    .map_err(EmbedError::from)?;
                return Ok((
                    direction,
                    eggchaos_protocol::DatagramFaultSpecV1::from(fault),
                    next,
                ));
            }
            Err(EmbedError::NotFound(format!("fault {id} on {proxy}")))
        })
    }

    /// Remove a datagram fault; returns the generation.
    pub fn remove_datagram_fault(&self, proxy: &str, id: &str) -> Result<u64, EmbedError> {
        self.block_on(async {
            for direction in [
                eggchaos_core::Direction::Upstream,
                eggchaos_core::Direction::Downstream,
            ] {
                let Ok((plan, generation, seed)) =
                    self.control.get_datagram_plan(proxy, direction).await
                else {
                    continue;
                };
                if plan.faults().iter().any(|fault| fault.id.as_str() == id) {
                    let faults = plan
                        .faults()
                        .iter()
                        .filter(|fault| fault.id.as_str() != id)
                        .cloned()
                        .collect();
                    let plan = eggchaos_core::DatagramPlan::new(faults)
                        .map_err(|error| EmbedError::Validation(error.to_string()))?;
                    return self
                        .control
                        .publish_datagram_plan(proxy, direction, plan, seed, Some(generation))
                        .await
                        .map_err(EmbedError::from);
                }
            }
            Err(EmbedError::NotFound(format!("fault {id} on {proxy}")))
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
        self.block_on(async { Ok(self.control.kill_datagram_association(id).await) })
    }

    /// Render the shared native error envelope for an embed error.
    pub fn error_envelope(error: &EmbedError) -> ErrorEnvelopeV1 {
        let (code, message) = match error {
            EmbedError::Validation(detail) => ("invalid", detail.clone()),
            EmbedError::NotFound(detail) => ("not_found", detail.clone()),
            EmbedError::Conflict(detail) => ("conflict", detail.clone()),
            EmbedError::Unsupported(detail) => ("unsupported", detail.clone()),
            EmbedError::Lifecycle(detail) => ("lifecycle", detail.clone()),
            EmbedError::Bind(detail) => ("bind_failed", detail.clone()),
            EmbedError::Internal(detail) => ("internal", detail.clone()),
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
