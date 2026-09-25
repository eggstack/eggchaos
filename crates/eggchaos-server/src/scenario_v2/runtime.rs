//! Server wiring for the shared v2 schedule driver.
//!
//! The schedule-execution authority is `eggchaos_experiment::drive`,
//! shared with embedded harnesses. This module adapts the server's
//! `ControlState` to the consumer-neutral [`PolicyTarget`] contract
//! and translates the generic outcome back into the server's run
//! record. There is no second generation store and no divergent
//! scheduling semantic: strict/live ownership, absolute deadlines,
//! and CAS-safe cleanup behave exactly as the shared driver defines.

use eggchaos_experiment::PolicyTarget;
use eggchaos_experiment::{
    CompiledScenarioV2, EpochGate, EventSink, PreparedExperiment, PublishReceipt,
    ScheduleEventResult, ScheduleResource, ScheduleRunStatus, ScheduleTransport,
    TargetCapabilities, TargetError, TargetPlan, TargetSnapshot,
};

use crate::runtime::ControlError;
use crate::ControlState;

/// `ControlState` adapted to the shared [`PolicyTarget`] contract.
///
/// Delegates to the existing publication methods, so canonical proxy
/// state, global generations, and expected-generation guards stay in
/// the one `ControlState` authority.
#[derive(Clone)]
struct ControlStateTarget {
    state: ControlState,
}

impl PolicyTarget for ControlStateTarget {
    fn capabilities(&self) -> TargetCapabilities {
        TargetCapabilities {
            stream: true,
            datagram: true,
        }
    }

    async fn snapshot(&self, resource: &ScheduleResource) -> Result<TargetSnapshot, TargetError> {
        match resource.transport {
            ScheduleTransport::Stream => {
                let Some((upstream, downstream)) =
                    self.state.snapshot_policies(&resource.proxy).await
                else {
                    return Err(TargetError::missing(format!(
                        "stream proxy {} not found",
                        resource.proxy
                    )));
                };
                let (generation, plan) = match resource.direction {
                    eggchaos_core::Direction::Upstream => {
                        (upstream.generation, (*upstream.plan).clone())
                    }
                    eggchaos_core::Direction::Downstream => {
                        (downstream.generation, (*downstream.plan).clone())
                    }
                };
                Ok(TargetSnapshot {
                    generation,
                    plan: TargetPlan::Stream(plan),
                })
            }
            ScheduleTransport::Datagram => {
                let (plan, generation, _) = self
                    .state
                    .get_datagram_plan(&resource.proxy, resource.direction)
                    .await
                    .map_err(|_| {
                        TargetError::missing(format!("datagram proxy {} not found", resource.proxy))
                    })?;
                Ok(TargetSnapshot {
                    generation,
                    plan: TargetPlan::Datagram(plan),
                })
            }
        }
    }

    async fn current_generation(&self, resource: &ScheduleResource) -> Result<u64, TargetError> {
        match resource.transport {
            ScheduleTransport::Stream => {
                let Some((upstream, downstream)) =
                    self.state.current_generations(&resource.proxy).await
                else {
                    return Err(TargetError::missing(format!(
                        "stream proxy {} not found",
                        resource.proxy
                    )));
                };
                Ok(match resource.direction {
                    eggchaos_core::Direction::Upstream => upstream,
                    eggchaos_core::Direction::Downstream => downstream,
                })
            }
            ScheduleTransport::Datagram => {
                let Some((upstream, downstream)) = self
                    .state
                    .datagram_current_generations(&resource.proxy)
                    .await
                else {
                    return Err(TargetError::missing(format!(
                        "datagram proxy {} not found",
                        resource.proxy
                    )));
                };
                Ok(match resource.direction {
                    eggchaos_core::Direction::Upstream => upstream,
                    eggchaos_core::Direction::Downstream => downstream,
                })
            }
        }
    }

    async fn publish(
        &self,
        resource: &ScheduleResource,
        expected: u64,
        plan: TargetPlan,
        seed_namespace: u64,
    ) -> Result<PublishReceipt, TargetError> {
        match (&plan, resource.transport) {
            (TargetPlan::Stream(plan), ScheduleTransport::Stream) => {
                match self
                    .state
                    .publish_direction_expected(
                        &resource.proxy,
                        resource.direction,
                        plan.clone(),
                        seed_namespace,
                        expected,
                    )
                    .await
                {
                    Ok((_, generation)) => Ok(PublishReceipt { generation }),
                    Err(error) => Err(map_publish_error(self, resource, expected, error).await),
                }
            }
            (TargetPlan::Datagram(plan), ScheduleTransport::Datagram) => {
                match self
                    .state
                    .publish_datagram_plan(
                        &resource.proxy,
                        resource.direction,
                        plan.clone(),
                        seed_namespace,
                        Some(expected),
                    )
                    .await
                {
                    Ok(generation) => Ok(PublishReceipt { generation }),
                    Err(error) => Err(map_publish_error(self, resource, expected, error).await),
                }
            }
            _ => Err(TargetError::unsupported("plan/transport mismatch")),
        }
    }

    fn global_generation(&self) -> u64 {
        self.state.generation()
    }
}

/// Map a `ControlError` publication failure onto the bounded target
/// categories. Conflicts re-read the live generation for an exact
/// `expected/found` pair.
async fn map_publish_error(
    target: &ControlStateTarget,
    resource: &ScheduleResource,
    expected: u64,
    error: ControlError,
) -> TargetError {
    match error {
        ControlError::NotFound(_) => {
            TargetError::missing(format!("proxy {} not found", resource.proxy))
        }
        ControlError::Invalid(detail) => TargetError::invalid(detail),
        ControlError::Conflict(_) => {
            let found = target
                .current_generation(resource)
                .await
                .unwrap_or(expected);
            TargetError::GenerationConflict { expected, found }
        }
        ControlError::BindFailed { reason, .. } | ControlError::RestartFailed { reason, .. } => {
            TargetError::internal(reason)
        }
    }
}

/// Run-record sink: appends each applied event to the server's bounded
/// run record in compiled order, preserving per-event visibility on
/// failure and cancellation.
struct RunRecordSink {
    state: ControlState,
    run_id: u64,
}

impl EventSink for RunRecordSink {
    async fn record(&mut self, event: ScheduleEventResult) {
        self.state
            .append_schedule_v2_event(self.run_id, event)
            .await;
    }
}

/// Drive one owned v2 schedule run to completion, cancellation, or
/// fail-fast failure through the shared experiment driver.
///
/// Preparation (compile, capability validation, initial snapshots)
/// publishes nothing. The epoch is captured at run entry, preserving
/// the historical run-entry timing anchor. Per-event evidence
/// accumulates in the run record through `append_schedule_v2_event`;
/// the record remains the single source of truth.
pub(crate) async fn drive_schedule_v2_run(
    state: ControlState,
    run_id: u64,
    compiled: CompiledScenarioV2,
    token: tokio_util::sync::CancellationToken,
) {
    state
        .update_schedule_v2_run(run_id, |record| {
            record.status = ScheduleRunStatus::Running;
        })
        .await;

    let target = ControlStateTarget {
        state: state.clone(),
    };
    // Preparation publishes nothing; on failure the run ends Failed
    // with an empty cleanup over the schedule's own cleanup policy,
    // mirroring the previous driver exactly.
    let cleanup_policy = compiled.cleanup;
    let prepared = match PreparedExperiment::prepare_compiled(compiled, target).await {
        Ok(prepared) => prepared,
        Err(error) => {
            let failure = error.to_string();
            state
                .update_schedule_v2_run(run_id, |record| {
                    record.status = ScheduleRunStatus::Failed;
                    record.failure = Some(failure.clone());
                })
                .await;
            state
                .update_schedule_v2_run(run_id, |record| {
                    record.cleanup = Some(eggchaos_experiment::CleanupOutcome {
                        policy: cleanup_policy,
                        resources: Vec::new(),
                    });
                })
                .await;
            state.remove_schedule_v2_token(run_id).await;
            return;
        }
    };

    let gate = EpochGate::started();
    let epoch = gate.epoch().expect("started gate holds an epoch");
    let mut sink = RunRecordSink {
        state: state.clone(),
        run_id,
    };
    let outcome = prepared.run_from_epoch(epoch, token, &mut sink).await;

    state
        .update_schedule_v2_run(run_id, |record| {
            record.status = outcome.status;
            record.applied = outcome.applied;
            if record.status == ScheduleRunStatus::Failed {
                record.failure.clone_from(&outcome.failure);
            }
        })
        .await;
    state
        .update_schedule_v2_run(run_id, |record| {
            record.cleanup = Some(outcome.cleanup.clone());
        })
        .await;
    state.remove_schedule_v2_token(run_id).await;
}
