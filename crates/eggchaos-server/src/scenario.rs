use std::time::Duration;

use eggchaos_core::{derive_policy_seed, Direction, FaultId, FaultPlan, FaultSpec};
use serde::{Deserialize, Serialize};
use tokio::time::sleep;

use crate::{ControlState, EggchaosError};

/// Deterministic bounded scenario document.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Scenario {
    /// Scenario schema version.
    pub version: u32,
    /// Explicit run seed recorded in evidence and mixed into published
    /// policy seed namespaces.
    pub seed: u64,
    /// Relative control events.
    pub events: Vec<ScenarioEvent>,
}

/// One monotonic-time scenario event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioEvent {
    /// Milliseconds from scenario start.
    pub at_ms: u64,
    /// Control action.
    pub action: ScenarioAction,
}

/// Supported scenario v1 actions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ScenarioAction {
    /// Replace one directional plan as a barrier generation.
    SetPlan {
        proxy: String,
        direction: Direction,
        faults: Vec<FaultSpec>,
    },
    /// Remove one fault from a directional plan.
    RemoveFault {
        proxy: String,
        direction: Direction,
        id: String,
    },
}

/// Observable scenario run lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScenarioRunStatus {
    /// Validated, waiting for its first event.
    Pending,
    /// Applying events.
    Running,
    /// Cancellation requested; the driver is unwinding.
    Cancelling,
    /// Cancelled before completion.
    Cancelled,
    /// All events applied.
    Completed,
    /// Stopped early by a failed event (fail-fast).
    Failed,
}

/// Evidence for one applied scenario event (no payloads).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioEventResult {
    /// Event index in the scenario document.
    pub index: usize,
    /// Milliseconds from scenario start.
    pub at_ms: u64,
    /// Action summary (`set-plan` or `remove-fault`).
    pub action: String,
    /// Target proxy.
    pub proxy: String,
    /// Target direction.
    pub direction: Direction,
    /// Global configuration generation after the event.
    pub global_generation: u64,
    /// Upstream policy generation after the event.
    pub upstream_generation: u64,
    /// Downstream policy generation after the event.
    pub downstream_generation: u64,
}

/// Observable scenario run record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioRunRecord {
    /// Stable run identifier.
    pub run_id: u64,
    /// Scenario seed driving published policy namespaces.
    pub seed: u64,
    /// Current lifecycle status.
    pub status: ScenarioRunStatus,
    /// Number of applied events.
    pub applied: usize,
    /// Failure detail for `Failed` runs.
    pub failure: Option<String>,
    /// Per-event generation trail.
    pub trail: Vec<ScenarioEventResult>,
}

/// Validate a scenario document entirely before a run begins.
pub async fn validate_scenario(
    state: &ControlState,
    scenario: &Scenario,
) -> Result<(), EggchaosError> {
    if scenario.version != 1 {
        return Err(EggchaosError::InvalidProxy(
            "unsupported scenario version".into(),
        ));
    }
    if scenario.events.len() > 1024 {
        return Err(EggchaosError::InvalidProxy(
            "too many scenario events".into(),
        ));
    }
    let mut previous = 0;
    for event in &scenario.events {
        if event.at_ms < previous {
            return Err(EggchaosError::InvalidProxy(
                "scenario events must be ordered".into(),
            ));
        }
        previous = event.at_ms;
        validate_action(state, &event.action).await?;
    }
    Ok(())
}

async fn validate_action(
    state: &ControlState,
    action: &ScenarioAction,
) -> Result<(), EggchaosError> {
    let (proxy, direction, id) = match action {
        ScenarioAction::SetPlan {
            proxy,
            direction,
            faults,
        } => {
            FaultPlan::new(faults.clone()).map_err(EggchaosError::InvalidPlan)?;
            (proxy, *direction, None)
        }
        ScenarioAction::RemoveFault {
            proxy,
            direction,
            id,
        } => {
            FaultId::new(id.clone()).map_err(|error| {
                EggchaosError::InvalidProxy(format!("invalid fault id: {error}"))
            })?;
            (proxy, *direction, Some(id.clone()))
        }
    };
    let Some((upstream, downstream)) = state.snapshot_policies(proxy).await else {
        return Err(EggchaosError::InvalidProxy(
            "scenario proxy not found".into(),
        ));
    };
    if let Some(id) = id {
        let base = match direction {
            Direction::Upstream => upstream,
            Direction::Downstream => downstream,
        };
        if base.plan.get(&id).is_none() {
            return Err(EggchaosError::InvalidProxy(format!(
                "scenario fault {id} not present on {proxy}"
            )));
        }
    }
    Ok(())
}

/// Drive one owned scenario run to completion, cancellation, or fail-fast
/// failure. Every event applies to the currently live plans (never a
/// stale snapshot) and publishes with an expected-generation guard, so a
/// concurrent manual publication fails the run instead of being silently
/// overwritten.
pub async fn drive_scenario_run(
    state: ControlState,
    run_id: u64,
    scenario: Scenario,
    token: tokio_util::sync::CancellationToken,
) {
    state
        .update_scenario_run(run_id, |record| {
            record.status = ScenarioRunStatus::Running;
        })
        .await;
    let mut previous = 0;
    for (index, event) in scenario.events.iter().enumerate() {
        let wait = Duration::from_millis(event.at_ms.saturating_sub(previous));
        tokio::select! {
            biased;
            () = token.cancelled() => {
                state
                    .update_scenario_run(run_id, |record| {
                        record.status = ScenarioRunStatus::Cancelled;
                    })
                    .await;
                state.remove_scenario_token(run_id).await;
                return;
            }
            () = sleep(wait) => {}
        }
        previous = event.at_ms;
        let (proxy, direction) = match &event.action {
            ScenarioAction::SetPlan {
                proxy, direction, ..
            }
            | ScenarioAction::RemoveFault {
                proxy, direction, ..
            } => (proxy.clone(), *direction),
        };
        let Some((base_upstream, base_downstream)) = state.snapshot_policies(&proxy).await else {
            fail_run(&state, run_id, format!("scenario proxy {proxy} not found")).await;
            return;
        };
        let mut upstream = (*base_upstream.plan).clone();
        let mut downstream = (*base_downstream.plan).clone();
        let action_summary = match &event.action {
            ScenarioAction::SetPlan { faults, .. } => {
                let next = match FaultPlan::new(faults.clone()) {
                    Ok(next) => next,
                    Err(error) => {
                        fail_run(&state, run_id, format!("scenario plan invalid: {error}")).await;
                        return;
                    }
                };
                match direction {
                    Direction::Upstream => upstream = next,
                    Direction::Downstream => downstream = next,
                }
                "set-plan"
            }
            ScenarioAction::RemoveFault { id, .. } => {
                let base = match direction {
                    Direction::Upstream => &upstream,
                    Direction::Downstream => &downstream,
                };
                if base.get(id).is_none() {
                    fail_run(
                        &state,
                        run_id,
                        format!("scenario fault {id} not present on {proxy}"),
                    )
                    .await;
                    return;
                }
                match direction {
                    Direction::Upstream => upstream = upstream.without_fault(id),
                    Direction::Downstream => downstream = downstream.without_fault(id),
                }
                "remove-fault"
            }
        };
        // The scenario seed participates in deterministic engine decisions
        // through the published seed namespace, derived purely from
        // (seed, run, event) with no scheduling input. Only the target
        // direction publishes, so the other direction keeps its plan,
        // generation, and namespace.
        let namespace = derive_policy_seed(scenario.seed, run_id, index as u64);
        let published = state
            .publish_direction_expected(
                &proxy,
                direction,
                match direction {
                    Direction::Upstream => upstream,
                    Direction::Downstream => downstream,
                },
                namespace,
                match direction {
                    Direction::Upstream => base_upstream.generation,
                    Direction::Downstream => base_downstream.generation,
                },
            )
            .await;
        let (global, published_generation) = match published {
            Ok(generations) => generations,
            Err(error) => {
                fail_run(&state, run_id, format!("scenario event failed: {error}")).await;
                return;
            }
        };
        let (upstream_generation, downstream_generation) = match direction {
            Direction::Upstream => (published_generation, base_downstream.generation),
            Direction::Downstream => (base_upstream.generation, published_generation),
        };
        let trail = ScenarioEventResult {
            index,
            at_ms: event.at_ms,
            action: action_summary.to_owned(),
            proxy,
            direction,
            global_generation: global,
            upstream_generation,
            downstream_generation,
        };
        state
            .update_scenario_run(run_id, |record| {
                record.applied += 1;
                record.trail.push(trail);
            })
            .await;
    }
    state
        .update_scenario_run(run_id, |record| {
            record.status = ScenarioRunStatus::Completed;
        })
        .await;
    state.remove_scenario_token(run_id).await;
}

async fn fail_run(state: &ControlState, run_id: u64, message: String) {
    state
        .update_scenario_run(run_id, |record| {
            record.status = ScenarioRunStatus::Failed;
            record.failure = Some(message);
        })
        .await;
    state.remove_scenario_token(run_id).await;
}

/// Keep the ID type in the scenario API documentation without duplicating a
/// second parser for fault IDs.
#[allow(dead_code)]
fn _fault_id(value: String) -> Result<FaultId, eggchaos_core::ValidationError> {
    FaultId::new(value)
}
